//! x64 hardware watchpoints for an explicitly selected offline process.

use super::memory::Process;
use crate::model::MemoryDebuggerArchitecture;
use iced_x86::{
    Decoder, DecoderOptions, Formatter, Instruction, InstructionInfoFactory, IntelFormatter,
    OpAccess, OpKind, Register,
};
use std::{
    collections::HashMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, DBG_CONTINUE, DBG_EXCEPTION_NOT_HANDLED, EXCEPTION_BREAKPOINT,
        EXCEPTION_SINGLE_STEP, HANDLE, INVALID_HANDLE_VALUE, STATUS_WX86_BREAKPOINT,
        STATUS_WX86_SINGLE_STEP,
    },
    System::{
        Diagnostics::Debug::{
            CONTEXT, CONTEXT_CONTROL_AMD64, CONTEXT_DEBUG_REGISTERS_AMD64, CONTEXT_INTEGER_AMD64,
            CREATE_PROCESS_DEBUG_EVENT, CREATE_THREAD_DEBUG_EVENT, ContinueDebugEvent, DEBUG_EVENT,
            DebugActiveProcess, DebugActiveProcessStop, DebugSetProcessKillOnExit,
            EXCEPTION_DEBUG_EVENT, EXIT_PROCESS_DEBUG_EVENT, EXIT_THREAD_DEBUG_EVENT,
            GetThreadContext, SetThreadContext, WOW64_CONTEXT, WOW64_CONTEXT_CONTROL,
            WOW64_CONTEXT_DEBUG_REGISTERS, WOW64_CONTEXT_INTEGER, WaitForDebugEvent,
            Wow64GetThreadContext, Wow64SetThreadContext,
        },
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
            PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
            TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
        },
        Threading::{
            IsWow64Process, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, ResumeThread,
            SuspendThread,
        },
    },
};

pub fn list_processes() -> io::Result<Vec<(u32, String)>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut processes = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..PROCESSENTRY32W::default()
    };
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        let end = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        if entry.th32ProcessID != 0 {
            processes.push((
                entry.th32ProcessID,
                String::from_utf16_lossy(&entry.szExeFile[..end]),
            ));
        }
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe { CloseHandle(snapshot) };
    processes.sort_by(|left, right| left.1.to_ascii_lowercase().cmp(&right.1.to_ascii_lowercase()).then(left.0.cmp(&right.0)));
    Ok(processes)
}

pub fn module_offset_for_address(pid: u32, address: usize) -> io::Result<(String, usize)> {
    for (name, base, size) in process_modules(pid)? {
        if (base..base.saturating_add(size)).contains(&address) {
            return Ok((name, address - base));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "instruction is not inside a loaded module",
    ))
}

pub fn resolve_module_offset(pid: u32, module: &str, offset: usize) -> io::Result<usize> {
    process_modules(pid)?
        .into_iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(module))
        .and_then(|(_, base, size)| (offset < size).then_some(base + offset))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "module is not loaded"))
}

pub fn process_modules(pid: u32) -> io::Result<Vec<(String, usize, usize)>> {
    let snapshot =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut modules = Vec::new();
    let mut entry = MODULEENTRY32W {
        dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
        ..MODULEENTRY32W::default()
    };
    let mut ok = unsafe { Module32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        let end = entry
            .szModule
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szModule.len());
        modules.push((
            String::from_utf16_lossy(&entry.szModule[..end]),
            entry.modBaseAddr as usize,
            entry.modBaseSize as usize,
        ));
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        ok = unsafe { Module32NextW(snapshot, &mut entry) };
    }
    unsafe { CloseHandle(snapshot) };
    Ok(modules)
}

const ERROR_SEM_TIMEOUT: i32 = 121;
// ponytail: bound runaway debugger traffic; switch to sampled/VEH capture before raising this again.
const MAX_ACCESS_HITS: usize = 10_000;
const RESUME_FLAG: u32 = 1 << 16;

#[repr(C, align(16))]
struct AlignedContext(CONTEXT);

impl Default for AlignedContext {
    fn default() -> Self {
        Self(CONTEXT::default())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetArchitecture {
    X86,
    X64,
}

#[derive(Debug)]
pub enum WatchEvent {
    Started,
    AddressHit {
        instruction_address: usize,
        instruction: String,
        data_address: usize,
        details: String,
        likely_stack_copy: bool,
    },
    AccessHit {
        data_address: usize,
    },
    CaptureLimitReached(usize),
    Error(String),
    Stopped,
}

struct WatchSession {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl WatchSession {
    fn start<F>(
        pid: u32,
        kind: WatchKind,
        requested_architecture: MemoryDebuggerArchitecture,
        notify: F,
    ) -> io::Result<Self>
    where
        F: Fn(WatchEvent) + Send + 'static,
    {
        let architecture = target_architecture(pid, requested_architecture)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || watch_loop(pid, kind, architecture, worker_stop, notify));
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for WatchSession {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct WriteWatch(WatchSession);

impl WriteWatch {
    pub fn start<F>(pid: u32, address: usize, architecture: MemoryDebuggerArchitecture, notify: F) -> io::Result<Self>
    where
        F: Fn(WatchEvent) + Send + 'static,
    {
        if address % 4 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "hardware watchpoint i32 requires a 4-byte aligned address",
            ));
        }
        WatchSession::start(pid, WatchKind::Write { address }, architecture, notify).map(Self)
    }

    pub fn stop(&mut self) {
        self.0.stop();
    }
}

pub struct AddressAccessWatch(WatchSession);

impl AddressAccessWatch {
    pub fn start<F>(pid: u32, address: usize, architecture: MemoryDebuggerArchitecture, notify: F) -> io::Result<Self>
    where
        F: Fn(WatchEvent) + Send + 'static,
    {
        if address % 4 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "hardware i32 access watchpoint requires a 4-byte aligned address",
            ));
        }
        WatchSession::start(pid, WatchKind::ReadWrite { address }, architecture, notify).map(Self)
    }

    pub fn stop(&mut self) {
        self.0.stop();
    }
}

pub struct AccessWatch(WatchSession);

impl AccessWatch {
    pub fn start<F>(pid: u32, instruction_address: usize, architecture: MemoryDebuggerArchitecture, notify: F) -> io::Result<Self>
    where
        F: Fn(WatchEvent) + Send + 'static,
    {
        let target = target_architecture(pid, architecture)?;
        let instruction = decode_at(&Process::open(pid)?, instruction_address, target)?;
        WatchSession::start(
            pid,
            WatchKind::Execute {
                address: instruction_address,
                instruction,
            },
            architecture,
            notify,
        )
        .map(Self)
    }

    pub fn stop(&mut self) {
        self.0.stop();
    }
}

enum WatchKind {
    Write {
        address: usize,
    },
    ReadWrite {
        address: usize,
    },
    Execute {
        address: usize,
        instruction: Instruction,
    },
}

impl WatchKind {
    fn address(&self) -> usize {
        match self {
            Self::Write { address }
            | Self::ReadWrite { address }
            | Self::Execute { address, .. } => *address,
        }
    }

    fn dr7(&self) -> u64 {
        match self {
            Self::Write { .. } => write_breakpoint_dr7(),
            Self::ReadWrite { .. } => read_write_breakpoint_dr7(),
            Self::Execute { .. } => 1,
        }
    }
}

fn target_architecture(pid: u32, requested: MemoryDebuggerArchitecture) -> io::Result<TargetArchitecture> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut wow64 = 0;
    let result = unsafe { IsWow64Process(process, &mut wow64) };
    unsafe { CloseHandle(process) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    let detected = if wow64 != 0 { TargetArchitecture::X86 } else { TargetArchitecture::X64 };
    match requested {
        MemoryDebuggerArchitecture::Auto => Ok(detected),
        MemoryDebuggerArchitecture::X86 if detected == TargetArchitecture::X86 => Ok(detected),
        MemoryDebuggerArchitecture::X64 if detected == TargetArchitecture::X64 => Ok(detected),
        MemoryDebuggerArchitecture::X86 => Err(io::Error::new(io::ErrorKind::InvalidInput, "selected 32-bit debugger for a 64-bit process")),
        MemoryDebuggerArchitecture::X64 => Err(io::Error::new(io::ErrorKind::InvalidInput, "selected 64-bit debugger for a 32-bit process")),
    }
}

fn watch_loop<F>(pid: u32, kind: WatchKind, architecture: TargetArchitecture, stop: Arc<AtomicBool>, notify: F)
where
    F: Fn(WatchEvent),
{
    let process = match Process::open(pid) {
        Ok(process) => process,
        Err(error) => {
            notify(WatchEvent::Error(error.to_string()));
            return;
        }
    };
    if unsafe { DebugActiveProcess(pid) } == 0 {
        notify(WatchEvent::Error(io::Error::last_os_error().to_string()));
        return;
    }
    if unsafe { DebugSetProcessKillOnExit(0) } == 0 {
        let error = io::Error::last_os_error();
        unsafe { DebugActiveProcessStop(pid) };
        notify(WatchEvent::Error(format!(
            "unable to protect the target process from debugger shutdown: {error}"
        )));
        return;
    }
    notify(WatchEvent::Started);
    let mut first_breakpoint = true;
    let mut threads = HashMap::new();
    let mut access_hits = 0usize;
    let mut capture_limit_reached = false;
    while !stop.load(Ordering::Acquire) {
        let mut event = DEBUG_EVENT::default();
        if unsafe { WaitForDebugEvent(&mut event, 100) } == 0 {
            if io::Error::last_os_error().raw_os_error() == Some(ERROR_SEM_TIMEOUT) {
                continue;
            }
            notify(WatchEvent::Error(io::Error::last_os_error().to_string()));
            break;
        }

        let mut status = DBG_CONTINUE;
        match event.dwDebugEventCode {
            CREATE_PROCESS_DEBUG_EVENT => unsafe {
                let info = event.u.CreateProcessInfo;
                if let Err(error) = arm_thread(info.hThread, &kind, architecture) {
                    if error.raw_os_error() != Some(87) {
                        notify(WatchEvent::Error(error.to_string()));
                        stop.store(true, Ordering::Release);
                    }
                }
                close_if_valid(info.hFile);
                close_if_valid(info.hProcess);
                threads.insert(event.dwThreadId, info.hThread);
            },
            CREATE_THREAD_DEBUG_EVENT => unsafe {
                let thread = event.u.CreateThread.hThread;
                if let Err(error) = arm_thread(thread, &kind, architecture) {
                    if error.raw_os_error() != Some(87) {
                        notify(WatchEvent::Error(error.to_string()));
                        stop.store(true, Ordering::Release);
                    }
                }
                threads.insert(event.dwThreadId, thread);
            },
            EXCEPTION_DEBUG_EVENT => unsafe {
                let exception = event.u.Exception.ExceptionRecord.ExceptionCode;
                if exception == EXCEPTION_SINGLE_STEP || exception == STATUS_WX86_SINGLE_STEP {
                    if let Some(context) = threads.get(&event.dwThreadId).and_then(|&thread| {
                        read_hit(
                            thread,
                            kind.address(),
                            matches!(kind, WatchKind::Execute { .. }),
                            architecture,
                        )
                    }) {
                        let context = context.as_amd64();
                        match &kind {
                            WatchKind::Write { address } | WatchKind::ReadWrite { address } => {
                                let read_write = matches!(kind, WatchKind::ReadWrite { .. });
                                if !capture_limit_reached
                                    && let Some((instruction_address, decoded, instruction)) =
                                    decode_previous_access(
                                        &process,
                                        &context,
                                        *address,
                                        !read_write,
                                        architecture,
                                    )
                                {
                                    notify(WatchEvent::AddressHit {
                                        instruction_address,
                                        instruction,
                                        data_address: *address,
                                        details: format_hit_details(
                                            &process,
                                            &decoded,
                                            &context,
                                            *address,
                                            if read_write { "truy cáº­p" } else { "ghi" },
                                            architecture,
                                        ),
                                        likely_stack_copy: address.abs_diff(context.Rsp as usize)
                                            < 8 * 1024 * 1024,
                                    });
                                    if read_write {
                                        access_hits += 1;
                                        if access_hits >= MAX_ACCESS_HITS {
                                            capture_limit_reached = true;
                                            notify(WatchEvent::CaptureLimitReached(MAX_ACCESS_HITS));
                                        }
                                    }
                                }
                            }
                            WatchKind::Execute { instruction, .. } => {
                                if !capture_limit_reached
                                    && let Some(data_address) = effective_address(instruction, &context)
                                {
                                    notify(WatchEvent::AccessHit { data_address });
                                    access_hits += 1;
                                    if access_hits >= MAX_ACCESS_HITS {
                                        capture_limit_reached = true;
                                        notify(WatchEvent::CaptureLimitReached(MAX_ACCESS_HITS));
                                    }
                                }
                            }
                        }
                    } else {
                        // This debug loop owns an active hardware breakpoint. WOW64 can report
                        // STATUS_SINGLE_STEP before its DR6 state is visible through the native
                        // context API; never leak that debugger-owned exception into the game.
                        status = DBG_CONTINUE;
                    }
                } else if (exception == EXCEPTION_BREAKPOINT
                    || exception == STATUS_WX86_BREAKPOINT)
                    && first_breakpoint
                {
                    first_breakpoint = false;
                    // The attach breakpoint can replace debug-register state installed during the
                    // synthetic CREATE_THREAD events. Re-arm every existing game thread after it.
                    for &thread in threads.values() {
                        if let Err(error) = arm_thread(thread, &kind, architecture) {
                            if error.raw_os_error() != Some(87) {
                                notify(WatchEvent::Error(format!(
                                    "unable to arm an existing game thread: {error}"
                                )));
                                stop.store(true, Ordering::Release);
                                break;
                            }
                        }
                    }
                } else {
                    status = DBG_EXCEPTION_NOT_HANDLED;
                }
            },
            EXIT_THREAD_DEBUG_EVENT => {
                if let Some(thread) = threads.remove(&event.dwThreadId) {
                    unsafe { close_if_valid(thread) };
                }
            }
            EXIT_PROCESS_DEBUG_EVENT => stop.store(true, Ordering::Release),
            _ => {}
        }
        unsafe { ContinueDebugEvent(event.dwProcessId, event.dwThreadId, status) };
    }
    for (_, thread) in threads {
        unsafe {
            if SuspendThread(thread) != u32::MAX {
                disarm_thread(thread, architecture);
                ResumeThread(thread);
            }
            close_if_valid(thread);
        }
    }
    unsafe { DebugActiveProcessStop(pid) };
    notify(WatchEvent::Stopped);
}

unsafe fn close_if_valid(handle: HANDLE) {
    if !handle.is_null() {
        unsafe { CloseHandle(handle) };
    }
}

unsafe fn arm_thread(thread: HANDLE, kind: &WatchKind, architecture: TargetArchitecture) -> io::Result<()> {
    if architecture == TargetArchitecture::X86 {
        let mut context = WOW64_CONTEXT {
            ContextFlags: WOW64_CONTEXT_DEBUG_REGISTERS | WOW64_CONTEXT_CONTROL,
            ..WOW64_CONTEXT::default()
        };
        if unsafe { Wow64GetThreadContext(thread, &mut context) } == 0 {
            return Err(io::Error::last_os_error());
        }
        context.Dr0 = kind.address() as u32;
        context.Dr6 = 0;
        context.Dr7 &= !0xF0003;
        context.Dr7 |= kind.dr7() as u32;
        return if unsafe { Wow64SetThreadContext(thread, &context) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        };
    }
    let mut aligned = AlignedContext::default();
    let context = &mut aligned.0;
    context.ContextFlags = CONTEXT_DEBUG_REGISTERS_AMD64 | CONTEXT_CONTROL_AMD64;
    if unsafe { GetThreadContext(thread, context) } == 0 {
        return Err(io::Error::last_os_error());
    }
    context.Dr0 = kind.address() as u64;
    context.Dr6 = 0;
    context.Dr7 &= !0xF0003;
    context.Dr7 |= kind.dr7();
    if unsafe { SetThreadContext(thread, context) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

enum CapturedContext {
    X86(WOW64_CONTEXT),
    X64(CONTEXT),
}

impl CapturedContext {
    fn as_amd64(&self) -> CONTEXT {
        match self {
            Self::X64(context) => *context,
            Self::X86(context) => CONTEXT {
                Rax: context.Eax as u64,
                Rbx: context.Ebx as u64,
                Rcx: context.Ecx as u64,
                Rdx: context.Edx as u64,
                Rsi: context.Esi as u64,
                Rdi: context.Edi as u64,
                Rbp: context.Ebp as u64,
                Rsp: context.Esp as u64,
                Rip: context.Eip as u64,
                EFlags: context.EFlags,
                Dr0: context.Dr0 as u64,
                Dr6: context.Dr6 as u64,
                Dr7: context.Dr7 as u64,
                ..CONTEXT::default()
            },
        }
    }
}

fn read_hit(thread: HANDLE, address: usize, execution_breakpoint: bool, architecture: TargetArchitecture) -> Option<CapturedContext> {
    if architecture == TargetArchitecture::X86 {
        let mut context = WOW64_CONTEXT {
            ContextFlags: WOW64_CONTEXT_DEBUG_REGISTERS
                | WOW64_CONTEXT_CONTROL
                | WOW64_CONTEXT_INTEGER,
            ..WOW64_CONTEXT::default()
        };
        let hit = unsafe { Wow64GetThreadContext(thread, &mut context) } != 0
            && (context.Dr6 & 1 != 0 || context.Dr0 == address as u32);
        if hit {
            context.Dr6 = 0;
            if execution_breakpoint {
                context.EFlags |= RESUME_FLAG;
            }
            unsafe { Wow64SetThreadContext(thread, &context) };
        }
        return hit.then_some(CapturedContext::X86(context));
    }
    let mut aligned = AlignedContext::default();
    let context = &mut aligned.0;
    context.ContextFlags =
        CONTEXT_DEBUG_REGISTERS_AMD64 | CONTEXT_CONTROL_AMD64 | CONTEXT_INTEGER_AMD64;
    let hit = unsafe { GetThreadContext(thread, context) } != 0
        && (context.Dr6 & 1 != 0 || context.Dr0 == address as u64);
    let captured = hit.then_some(*context);
    if hit {
        context.Dr6 = 0;
        if execution_breakpoint {
            context.EFlags |= RESUME_FLAG;
        }
        unsafe { SetThreadContext(thread, context) };
    }
    captured.map(CapturedContext::X64)
}

fn decode_at(process: &Process, address: usize, architecture: TargetArchitecture) -> io::Result<Instruction> {
    let mut bytes = [0u8; 15];
    let read = process.read(address, &mut bytes)?;
    let mut decoder = Decoder::with_ip(
        if architecture == TargetArchitecture::X86 { 32 } else { 64 },
        &bytes[..read],
        address as u64,
        DecoderOptions::NONE,
    );
    let instruction = decoder.decode();
    if instruction.is_invalid() {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid x64 instruction",
        ))
    } else {
        Ok(instruction)
    }
}

fn decode_previous_access(
    process: &Process,
    context: &CONTEXT,
    data_address: usize,
    writes_only: bool,
    architecture: TargetArchitecture,
) -> Option<(usize, Instruction, String)> {
    let next_ip = context.Rip as usize;
    for length in 1..=15usize {
        let start = next_ip.checked_sub(length)?;
        let mut bytes = [0u8; 15];
        let Ok(read) = process.read(start, &mut bytes[..length]) else {
            continue;
        };
        if read != length {
            continue;
        }
        let mut decoder = Decoder::with_ip(
            if architecture == TargetArchitecture::X86 { 32 } else { 64 },
            &bytes[..length],
            start as u64,
            DecoderOptions::NONE,
        );
        let instruction = decoder.decode();
        if !instruction.is_invalid()
            && instruction.len() == length
            && instruction.next_ip() as usize == next_ip
            && (!writes_only || writes_memory(&instruction))
            && effective_address(&instruction, context) == Some(data_address)
        {
            let mut formatter = IntelFormatter::new();
            let mut text = String::new();
            formatter.format(&instruction, &mut text);
            return Some((start, instruction, text));
        }
    }
    None
}

fn writes_memory(instruction: &Instruction) -> bool {
    let mut factory = InstructionInfoFactory::new();
    factory
        .info(instruction)
        .used_memory()
        .iter()
        .any(|memory| {
            matches!(
                memory.access(),
                OpAccess::Write
                    | OpAccess::CondWrite
                    | OpAccess::ReadWrite
                    | OpAccess::ReadCondWrite
            )
        })
}

pub fn instruction_writes_memory(pid: u32, address: usize) -> io::Result<bool> {
    let architecture = target_architecture(pid, MemoryDebuggerArchitecture::Auto)?;
    Ok(writes_memory(&decode_at(&Process::open(pid)?, address, architecture)?))
}

pub fn disassemble_from(
    pid: u32,
    address: usize,
    configured: MemoryDebuggerArchitecture,
    count: usize,
) -> io::Result<Vec<(usize, String, String)>> {
    let architecture = target_architecture(pid, configured)?;
    let process = Process::open(pid)?;
    let mut current = address;
    let mut lines = Vec::with_capacity(count);
    for _ in 0..count {
        let instruction = decode_at(&process, current, architecture)?;
        let mut encoded = [0u8; 15];
        let read = process.read(current, &mut encoded[..instruction.len()])?;
        let bytes = encoded[..read]
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut formatter = IntelFormatter::new();
        let mut assembly = String::new();
        formatter.format(&instruction, &mut assembly);
        lines.push((current, bytes, assembly));
        let next = instruction.next_ip() as usize;
        if next <= current {
            break;
        }
        current = next;
    }
    Ok(lines)
}

fn format_hit_details(
    process: &Process,
    instruction: &Instruction,
    context: &CONTEXT,
    data_address: usize,
    action: &str,
    architecture: TargetArchitecture,
) -> String {
    let action = if action == "ghi" { "write" } else { "access" };
    let mut text = format!(
        "DISASSEMBLY (<< marks the {action} instruction; following lines are subsequent code)\r\n"
    );
    let mut current = *instruction;
    for index in 0..6 {
        let mut bytes = [0u8; 15];
        let read = process
            .read(current.ip() as usize, &mut bytes[..current.len()])
            .unwrap_or(0);
        let encoded = bytes[..read]
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut formatter = IntelFormatter::new();
        let mut assembly = String::new();
        formatter.format(&current, &mut assembly);
        text.push_str(&format!(
            "0x{:016X}  {:<32}  {}{}\r\n",
            current.ip(),
            encoded,
            assembly,
            if index == 0 { "  <<" } else { "" }
        ));
        let Ok(next) = decode_at(process, current.next_ip() as usize, architecture) else {
            break;
        };
        current = next;
    }
    text.push_str(&format!(
        "\r\nREGISTER SNAPSHOT AFTER MOST RECENT {}\r\n\
RAX={:016X}  RBX={:016X}\r\n\
RCX={:016X}  RDX={:016X}\r\n\
RSI={:016X}  RDI={:016X}\r\n\
RBP={:016X}  RSP={:016X}\r\n\
R8 ={:016X}  R9 ={:016X}\r\n\
R10={:016X}  R11={:016X}\r\n\
R12={:016X}  R13={:016X}\r\n\
R14={:016X}  R15={:016X}\r\n\
RIP(after instruction)={:016X}  RFLAGS={:08X}\r\n\
ACTUAL DATA ADDRESS=0x{:016X}\r\n",
        action.to_uppercase(),
        context.Rax,
        context.Rbx,
        context.Rcx,
        context.Rdx,
        context.Rsi,
        context.Rdi,
        context.Rbp,
        context.Rsp,
        context.R8,
        context.R9,
        context.R10,
        context.R11,
        context.R12,
        context.R13,
        context.R14,
        context.R15,
        context.Rip,
        context.EFlags,
        data_address,
    ));
    text
}

fn format_hit_details_legacy(
    process: &Process,
    instruction: &Instruction,
    context: &CONTEXT,
    data_address: usize,
    action: &str,
) -> String {
    let mut text = format!(
        "DISASSEMBLY (dÃ²ng << lÃ  instruction {action}; cÃ¡c dÃ²ng sau lÃ  code káº¿ tiáº¿p)\r\n"
    );
    let mut current = *instruction;
    for index in 0..6 {
        let mut bytes = [0u8; 15];
        let read = process
            .read(current.ip() as usize, &mut bytes[..current.len()])
            .unwrap_or(0);
        let encoded = bytes[..read]
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut formatter = IntelFormatter::new();
        let mut assembly = String::new();
        formatter.format(&current, &mut assembly);
        text.push_str(&format!(
            "0x{:016X}  {:<32}  {}{}\r\n",
            current.ip(),
            encoded,
            assembly,
            if index == 0 { "  <<" } else { "" }
        ));
        let Ok(next) = decode_at(process, current.next_ip() as usize, TargetArchitecture::X64) else {
            break;
        };
        current = next;
    }
    text.push_str(&format!(
        "\r\nSNAPSHOT SAU Láº¦N {} Gáº¦N NHáº¤T\r\n\
RAX={:016X}  RBX={:016X}\r\n\
RCX={:016X}  RDX={:016X}\r\n\
RSI={:016X}  RDI={:016X}\r\n\
RBP={:016X}  RSP={:016X}\r\n\
R8 ={:016X}  R9 ={:016X}\r\n\
R10={:016X}  R11={:016X}\r\n\
R12={:016X}  R13={:016X}\r\n\
R14={:016X}  R15={:016X}\r\n\
RIP(sau lá»‡nh)={:016X}  RFLAGS={:08X}\r\n\
Äá»ŠA CHá»ˆ DATA THá»°C Táº¾=0x{:016X}\r\n",
        action.to_uppercase(),
        context.Rax,
        context.Rbx,
        context.Rcx,
        context.Rdx,
        context.Rsi,
        context.Rdi,
        context.Rbp,
        context.Rsp,
        context.R8,
        context.R9,
        context.R10,
        context.R11,
        context.R12,
        context.R13,
        context.R14,
        context.R15,
        context.Rip,
        context.EFlags,
        data_address,
    ));
    text
}

fn effective_address(instruction: &Instruction, context: &CONTEXT) -> Option<usize> {
    let has_memory =
        (0..instruction.op_count()).any(|index| instruction.op_kind(index) == OpKind::Memory);
    if !has_memory {
        return None;
    }
    if instruction.is_ip_rel_memory_operand() {
        return Some(instruction.ip_rel_memory_address() as usize);
    }
    let base = register_value(instruction.memory_base(), context)?;
    let index = register_value(instruction.memory_index(), context)?;
    Some(
        base.wrapping_add(index.wrapping_mul(instruction.memory_index_scale() as u64))
            .wrapping_add(instruction.memory_displacement64()) as usize,
    )
}

fn register_value(register: Register, context: &CONTEXT) -> Option<u64> {
    Some(match register {
        Register::None => 0,
        Register::RAX => context.Rax,
        Register::RCX => context.Rcx,
        Register::RDX => context.Rdx,
        Register::RBX => context.Rbx,
        Register::RSP => context.Rsp,
        Register::RBP => context.Rbp,
        Register::RSI => context.Rsi,
        Register::RDI => context.Rdi,
        Register::R8 => context.R8,
        Register::R9 => context.R9,
        Register::R10 => context.R10,
        Register::R11 => context.R11,
        Register::R12 => context.R12,
        Register::R13 => context.R13,
        Register::R14 => context.R14,
        Register::R15 => context.R15,
        Register::EAX => context.Rax as u32 as u64,
        Register::ECX => context.Rcx as u32 as u64,
        Register::EDX => context.Rdx as u32 as u64,
        Register::EBX => context.Rbx as u32 as u64,
        Register::ESP => context.Rsp as u32 as u64,
        Register::EBP => context.Rbp as u32 as u64,
        Register::ESI => context.Rsi as u32 as u64,
        Register::EDI => context.Rdi as u32 as u64,
        Register::R8D => context.R8 as u32 as u64,
        Register::R9D => context.R9 as u32 as u64,
        Register::R10D => context.R10 as u32 as u64,
        Register::R11D => context.R11 as u32 as u64,
        Register::R12D => context.R12 as u32 as u64,
        Register::R13D => context.R13 as u32 as u64,
        Register::R14D => context.R14 as u32 as u64,
        Register::R15D => context.R15 as u32 as u64,
        _ => return None,
    })
}

unsafe fn disarm_thread(thread: HANDLE, architecture: TargetArchitecture) {
    if architecture == TargetArchitecture::X86 {
        let mut context = WOW64_CONTEXT {
            ContextFlags: WOW64_CONTEXT_DEBUG_REGISTERS,
            ..WOW64_CONTEXT::default()
        };
        if unsafe { Wow64GetThreadContext(thread, &mut context) } != 0 {
            context.Dr0 = 0;
            context.Dr6 = 0;
            context.Dr7 &= !0xF0003;
            unsafe { Wow64SetThreadContext(thread, &context) };
        }
    }
    let mut aligned = AlignedContext::default();
    let context = &mut aligned.0;
    context.ContextFlags = CONTEXT_DEBUG_REGISTERS_AMD64;
    if unsafe { GetThreadContext(thread, context) } != 0 {
        context.Dr0 = 0;
        context.Dr6 = 0;
        context.Dr7 &= !0xF0003;
        unsafe { SetThreadContext(thread, context) };
    }
}

const fn write_breakpoint_dr7() -> u64 {
    1 | (1 << 16) | (3 << 18)
}

const fn read_write_breakpoint_dr7() -> u64 {
    1 | (3 << 16) | (3 << 18)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        sync::{
            atomic::{AtomicI32, Ordering},
            mpsc,
        },
        time::Duration,
    };

    #[test]
    fn dr7_enables_local_four_byte_write_watchpoint_in_slot_zero() {
        let dr7 = write_breakpoint_dr7();
        assert_eq!(dr7 & 1, 1);
        assert_eq!((dr7 >> 16) & 0b11, 0b01);
        assert_eq!((dr7 >> 18) & 0b11, 0b11);
    }

    #[test]
    fn dr7_read_write_watchpoint_uses_the_access_mode() {
        let dr7 = read_write_breakpoint_dr7();
        assert_eq!(dr7 & 1, 1);
        assert_eq!((dr7 >> 16) & 0b11, 0b11);
        assert_eq!((dr7 >> 18) & 0b11, 0b11);
    }

    #[test]
    fn instruction_filter_rejects_memory_reads() {
        let mut write_decoder = Decoder::new(64, &[0x89, 0x44, 0x24, 0x20], DecoderOptions::NONE);
        let mut read_decoder = Decoder::new(64, &[0x8B, 0x44, 0x24, 0x20], DecoderOptions::NONE);
        assert!(writes_memory(&write_decoder.decode()));
        assert!(!writes_memory(&read_decoder.decode()));
    }

    #[test]
    fn watch_child() {
        if std::env::var_os("RAM_READER_WATCH_CHILD").is_none() {
            return;
        }
        let value = AtomicI32::new(0);
        println!("WATCH_ADDRESS={:X}", &value as *const AtomicI32 as usize);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
        thread::sleep(Duration::from_millis(700));
        for next in 1..40 {
            value.store(next, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn hardware_write_watch_reports_a_hit_from_owned_child() {
        let executable = std::env::current_exe().unwrap();
        let mut child = Command::new(executable)
            .args(["--exact", "debugger::tests::watch_child", "--nocapture"])
            .env("RAM_READER_WATCH_CHILD", "1")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        let address = loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(hex) = line.trim().strip_prefix("WATCH_ADDRESS=") {
                break usize::from_str_radix(hex, 16).unwrap();
            }
        };
        let (sender, receiver) = mpsc::channel();
        let mut watch = WriteWatch::start(child.id(), address, MemoryDebuggerArchitecture::Auto, move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
        let (ip, assembly, details) = loop {
            match receiver.recv_timeout(Duration::from_secs(4)).unwrap() {
                WatchEvent::AddressHit {
                    instruction_address,
                    instruction,
                    details,
                    ..
                } => break (instruction_address, instruction, details),
                WatchEvent::AccessHit { .. } => {}
                WatchEvent::Error(error) => panic!("watch failed: {error}"),
                WatchEvent::Stopped => panic!("watch stopped before a write"),
                WatchEvent::Started => {}
            }
        };
        assert_ne!(ip, 0);
        assert!(!assembly.is_empty());
        assert!(details.contains("RAX="));
        assert!(details.contains("<<"));
        watch.stop();
        let (sender, receiver) = mpsc::channel();
        let mut address_access_watch =
            AddressAccessWatch::start(child.id(), address, MemoryDebuggerArchitecture::Auto, move |event| {
                let _ = sender.send(event);
            })
            .unwrap();
        let access_details = loop {
            match receiver.recv_timeout(Duration::from_secs(4)).unwrap() {
                WatchEvent::AddressHit { details, .. } => break details,
                WatchEvent::Error(error) => panic!("address access watch failed: {error}"),
                WatchEvent::Stopped => panic!("address access watch stopped before an access"),
                _ => {}
            }
        };
        assert!(access_details.contains("TRUY Cáº¬P"));
        address_access_watch.stop();
        let (sender, receiver) = mpsc::channel();
        let mut access_watch = AccessWatch::start(child.id(), ip, MemoryDebuggerArchitecture::Auto, move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
        let accessed = loop {
            match receiver.recv_timeout(Duration::from_secs(4)).unwrap() {
                WatchEvent::AccessHit { data_address } => break data_address,
                WatchEvent::Error(error) => panic!("access watch failed: {error}"),
                WatchEvent::Stopped => panic!("access watch stopped before an access"),
                _ => {}
            }
        };
        assert_eq!(accessed, address);
        thread::sleep(Duration::from_millis(250));
        let extra_hits = receiver
            .try_iter()
            .filter(|event| matches!(event, WatchEvent::AccessHit { .. }))
            .count();
        assert!(
            extra_hits <= 20,
            "execute breakpoint retriggered without progress"
        );
        access_watch.stop();
        let _ = child.wait();
    }
}
