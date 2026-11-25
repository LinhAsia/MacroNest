#![cfg(windows)]
#![allow(dead_code)]

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use iced_x86::{
    Decoder, DecoderOptions, FlowControl, Formatter, Instruction, InstructionInfoFactory,
    IntelFormatter, OpAccess, OpKind, Register,
};
use parking_lot::Mutex;

use crate::{
    android_bridge::{AndroidMemoryModule, AndroidMemoryRegion},
    memory_debugger::debugger::WatchEvent,
    process_memory::read_memory_bytes,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const REMOTE_MN_WATCH: &str = "/data/local/tmp/mn_watch";
const MN_WATCH_ELF: &[u8] = include_bytes!("mn_watch/mn_watch.bin");

#[derive(Clone, Debug)]
pub struct CapturedAndroidInstrMeta {
    pub inst_gva: usize,
    pub asm_text: String,
    pub module_name: String,
    pub module_offset: usize,
    pub aob_signature: String,
    pub aob_target_offset: usize,
    pub writes: bool,
}

static CAPTURED_META: OnceLock<Mutex<HashMap<usize, CapturedAndroidInstrMeta>>> = OnceLock::new();
static CAPTURED_GUEST_VALUES: OnceLock<Mutex<HashMap<usize, [u8; 8]>>> = OnceLock::new();

fn meta_map() -> &'static Mutex<HashMap<usize, CapturedAndroidInstrMeta>> {
    CAPTURED_META.get_or_init(|| Mutex::new(HashMap::new()))
}

fn guest_val_map() -> &'static Mutex<HashMap<usize, [u8; 8]>> {
    CAPTURED_GUEST_VALUES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn lookup_captured_instruction(addr: usize) -> Option<CapturedAndroidInstrMeta> {
    meta_map().lock().get(&addr).cloned()
}

pub fn lookup_captured_guest_bytes(addr: usize) -> Option<[u8; 8]> {
    guest_val_map().lock().get(&addr).copied()
}

fn store_captured_instruction(meta: CapturedAndroidInstrMeta) {
    let mut m = meta_map().lock();
    if m.len() >= 512 {
        m.clear();
    }
    m.insert(meta.inst_gva, meta);
}

fn store_captured_guest_value(addr: usize, val: u64) {
    let mut m = guest_val_map().lock();
    if m.len() >= 2048 {
        m.clear();
    }
    m.insert(addr, val.to_le_bytes());
}

fn adb_run(adb: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(adb);
    cmd.creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd.output().map_err(|e| format!("adb failed: {e}"))?;
    let mut s = String::from_utf8_lossy(&out.stdout).to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() && s.trim().is_empty() {
        s = err.to_string();
    }
    Ok(s)
}

fn adb_shell_root(adb: &Path, target: &str, shell_cmd: &str) -> Result<String, String> {
    let wrapped = format!(
        "if [ \"$(id -u)\" = \"0\" ]; then {shell_cmd}; else su -c '{shell_cmd}' 2>/dev/null || su 0 sh -c '{shell_cmd}'; fi"
    );
    adb_run(adb, &["-s", target, "shell", &wrapped])
}

fn spawn_adb_root(adb: &Path, target: &str, shell_cmd: &str) -> Result<Child, String> {
    let wrapped = format!(
        "if [ \"$(id -u)\" = \"0\" ]; then {shell_cmd}; else su -c '{shell_cmd}' 2>/dev/null || su 0 sh -c '{shell_cmd}'; fi"
    );
    let mut cmd = Command::new(adb);
    cmd.creation_flags(CREATE_NO_WINDOW)
        .args(["-s", target, "shell", &wrapped])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.spawn().map_err(|e| format!("Failed to spawn adb mn_watch: {e}"))
}

pub fn ensure_mn_watch_deployed(adb: &Path, target: &str) -> Result<String, String> {
    let size_cmd = format!(
        "stat -c %s {REMOTE_MN_WATCH} 2>/dev/null || wc -c < {REMOTE_MN_WATCH} 2>/dev/null || echo 0"
    );
    let remote_sz = adb_run(adb, &["-s", target, "shell", &size_cmd])
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(0);

    if remote_sz != MN_WATCH_ELF.len() {
        let local_tmp = std::env::temp_dir().join("mn_watch_elf.bin");
        std::fs::write(&local_tmp, MN_WATCH_ELF)
            .map_err(|e| format!("Failed to stage mn_watch helper: {e}"))?;
        let local_str = local_tmp.to_string_lossy().to_string();
        let push_out = adb_run(adb, &["-s", target, "push", &local_str, REMOTE_MN_WATCH])?;
        if push_out.to_lowercase().contains("error") || push_out.to_lowercase().contains("failed") {
            return Err(format!("adb push mn_watch failed: {push_out}"));
        }
        let _ = adb_run(
            adb,
            &["-s", target, "shell", &format!("chmod 755 {REMOTE_MN_WATCH}")],
        );
    }

    Ok(REMOTE_MN_WATCH.to_string())
}

fn parse_hex_u64(s: &str) -> Option<u64> {
    let t = s.trim();
    let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u64::from_str_radix(hex, 16).ok()
}

pub fn parse_hex_bytes(hex: &str) -> Option<Vec<u8>> {
    let clean: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    for i in (0..clean.len()).step_by(2) {
        let b = u8::from_str_radix(&clean[i..i + 2], 16).ok()?;
        out.push(b);
    }
    Some(out)
}

fn format_instr_intel(instr: &Instruction) -> String {
    let mut fmt = IntelFormatter::new();
    let mut s = String::new();
    fmt.format(instr, &mut s);
    s
}

/// Given `buf64` covering `[ip_after - 48 .. ip_after + 16]` (`ip_after` at index 48),
/// synchronizes forward to find the exact memory-access instruction ending at `ip_after`
/// (or starting at `ip_after` for `rep` string instructions).
pub fn decode_access_instr_before(
    ip_after: usize,
    buf64: &[u8],
) -> Option<(usize, usize, String, Instruction, bool)> {
    if buf64.len() < 48 {
        return None;
    }
    let buf_base = ip_after.checked_sub(48)?;
    let mut best: Option<(usize, usize, usize, String, Instruction, bool)> = None;
    let mut factory = InstructionInfoFactory::new();

    for start_off in 0..48usize {
        let start_ip = buf_base + start_off;
        let mut decoder = Decoder::with_ip(64, &buf64[start_off..], start_ip as u64, DecoderOptions::NONE);
        let mut chain_len = 0usize;

        while decoder.can_decode() {
            let instr = decoder.decode();
            if instr.is_invalid() {
                break;
            }
            let addr = instr.ip() as usize;
            let len = instr.len();
            if addr + len > ip_after + 15 {
                break;
            }
            chain_len += 1;

            let is_match = (addr + len == ip_after) || (addr == ip_after && (instr.has_rep_prefix() || instr.has_repne_prefix()));
            if is_match {
                let has_mem = (0..instr.op_count()).any(|k| instr.op_kind(k) == OpKind::Memory);
                if has_mem {
                    let writes = factory.info(&instr).used_memory().iter().any(|m| {
                        matches!(
                            m.access(),
                            OpAccess::Write | OpAccess::ReadWrite | OpAccess::CondWrite
                        )
                    });
                    // Prefer longer synchronized instruction chains ending cleanly at ip_after
                    if best.as_ref().is_none_or(|(b_chain, ..)| chain_len > *b_chain) {
                        best = Some((chain_len, addr, len, format_instr_intel(&instr), instr, writes));
                    }
                }
                break;
            }
            if addr + len > ip_after {
                break;
            }
        }
    }

    best.map(|(_, addr, len, text, instr, writes)| (addr, len, text, instr, writes))
}

/// Builds a restart-resilient wildcarded AOB signature around `inst_ip` inside `buf64`
/// (`buf_base = ip_after - 48`). Preserves struct field offsets (disp32 on memory ops)
/// while wildcarding ASLR immediates and branch/RIP displacements.
pub fn generate_aob_from_code64(buf64: &[u8], buf_base: usize, inst_ip: usize) -> (String, usize) {
    let rel_target = inst_ip.saturating_sub(buf_base);
    if rel_target >= buf64.len() {
        return (String::new(), 0);
    }

    // Find a valid instruction start <= 16 bytes before inst_ip that reaches inst_ip cleanly
    let min_start = rel_target.saturating_sub(16);
    let mut chosen_start = rel_target;
    for cand_off in min_start..=rel_target {
        let mut dec = Decoder::with_ip(64, &buf64[cand_off..], (buf_base + cand_off) as u64, DecoderOptions::NONE);
        while dec.can_decode() {
            let i = dec.decode();
            if i.is_invalid() {
                break;
            }
            if i.ip() as usize == inst_ip {
                chosen_start = cand_off;
                break;
            }
            if (i.ip() as usize) > inst_ip {
                break;
            }
        }
        if chosen_start != rel_target {
            break;
        }
    }

    let mut dec = Decoder::with_ip(
        64,
        &buf64[chosen_start..],
        (buf_base + chosen_start) as u64,
        DecoderOptions::NONE,
    );
    let mut tokens = Vec::new();
    let mut total_bytes = 0usize;
    let mut seen_target = false;

    while dec.can_decode() && total_bytes < 32 {
        let instr = dec.decode();
        if instr.is_invalid() || instr.code() == iced_x86::Code::Int3 {
            break;
        }
        let off = (instr.ip() as usize).saturating_sub(buf_base);
        let len = instr.len();
        if off + len > buf64.len() {
            break;
        }
        if instr.ip() as usize == inst_ip {
            seen_target = true;
        }
        let raw = &buf64[off..off + len];
        let co = dec.get_constant_offsets(&instr);
        let mut wild = vec![false; len];

        let is_branch = matches!(
            instr.flow_control(),
            FlowControl::UnconditionalBranch
                | FlowControl::ConditionalBranch
                | FlowControl::Call
        );
        if is_branch || instr.is_ip_rel_memory_operand() {
            if co.has_displacement() && co.displacement_size() >= 4 {
                let d0 = co.displacement_offset();
                for k in d0..(d0 + co.displacement_size()).min(len) {
                    wild[k] = true;
                }
            }
        }
        if co.has_immediate() && co.immediate_size() >= 4 {
            let i0 = co.immediate_offset();
            let sz = co.immediate_size();
            let val = match sz {
                4 => u32::from_le_bytes(raw[i0..i0 + 4].try_into().unwrap_or_default()) as u64,
                8 => u64::from_le_bytes(raw[i0..i0 + 8].try_into().unwrap_or_default()),
                _ => 0,
            };
            // Wildcard large ASLR / guest addresses while preserving small constants like 0 or 1
            if val >= 0x10000 {
                for k in i0..(i0 + sz).min(len) {
                    wild[k] = true;
                }
            }
        }

        for k in 0..len {
            if wild[k] {
                tokens.push("??".to_string());
            } else {
                tokens.push(format!("{:02X}", raw[k]));
            }
        }
        total_bytes += len;
        if seen_target && total_bytes >= (rel_target - chosen_start) + len + 10 {
            break;
        }
        if is_branch {
            break;
        }
    }

    (tokens.join(" "), rel_target.saturating_sub(chosen_start))
}

/// Maps an x86_64 `iced_x86::Register` to `mn_watch`'s `sample_regs_user` index (`0..17`).
/// Mask `0x00FF01FF` order: RAX=0..RIP=8, R8=9..R15=16.
pub fn register_to_perf_idx(reg: Register) -> Option<i32> {
    let idx = match reg.full_register() {
        Register::RAX => 0,
        Register::RBX => 1,
        Register::RCX => 2,
        Register::RDX => 3,
        Register::RSI => 4,
        Register::RDI => 5,
        Register::RBP => 6,
        Register::RSP => 7,
        Register::RIP => 8,
        Register::R8 => 9,
        Register::R9 => 10,
        Register::R10 => 11,
        Register::R11 => 12,
        Register::R12 => 13,
        Register::R13 => 14,
        Register::R14 => 15,
        Register::R15 => 16,
        _ => return None,
    };
    Some(idx)
}

fn resolve_target_gva(
    host_pid: u32,
    android_pid: u32,
    target_gva_hint: usize,
    target_hva: usize,
    delta: isize,
    all_regions: &[AndroidMemoryRegion],
    adb: &Path,
    adb_target: &str,
) -> Result<usize, String> {
    if delta != 0
        && target_gva_hint != target_hva
        && (all_regions.is_empty()
            || all_regions
                .iter()
                .any(|r| target_gva_hint >= r.start && target_gva_hint < r.end))
    {
        return Ok(target_gva_hint);
    }

    // Fallback: match 48 bytes around target_hva (wildcarding the 4 mutating value bytes) in guest RAM
    let page_base = target_hva & !0xFFF;
    let page_off = target_hva & 0xFFF;
    let win_off = page_off.saturating_sub(16).min(4096 - 48);
    let delta_in_win = page_off - win_off;

    let bytes = read_memory_bytes(host_pid, page_base + win_off, 48)
        .map_err(|e| format!("Cannot snapshot host target page 0x{target_hva:X}: {e}"))?;
    let mut pat = String::with_capacity(48 * 2);
    for (idx, &b) in bytes.iter().enumerate() {
        if idx >= delta_in_win && idx < delta_in_win + 4 {
            pat.push_str("??");
        } else {
            use std::fmt::Write;
            let _ = write!(&mut pat, "{b:02X}");
        }
    }

    let cmd = format!(
        "{REMOTE_MN_WATCH} find {android_pid} 0x{win_off:x} 0x{delta_in_win:x} \"{pat}\""
    );
    let out = adb_shell_root(adb, adb_target, &cmd)?;
    for line in out.lines() {
        if let Some(rest) = line.trim().strip_prefix("FOUND ") {
            if let Some(gva) = parse_hex_u64(rest) {
                return Ok(gva as usize);
            }
        }
    }
    Err(format!(
        "Could not translate HVA 0x{target_hva:X} to Android guest address (try syncing Android Bridge Delta)"
    ))
}

fn describe_guest_location(
    inst_gva: usize,
    ctx_pc: usize,
    modules: &[AndroidMemoryModule],
) -> (String, String, usize) {
    if let Some(m) = modules
        .iter()
        .find(|m| inst_gva >= m.base_address && inst_gva < m.end_address)
    {
        let off = inst_gva - m.base_address;
        return (
            format!("{}+0x{off:X}", m.name),
            format!("Android::{}", m.name),
            off,
        );
    }
    if ctx_pc != 0 {
        if let Some(m) = modules
            .iter()
            .find(|m| ctx_pc >= m.base_address && ctx_pc < m.end_address)
        {
            let off = ctx_pc - m.base_address;
            return (
                format!("Houdini JIT ({}+0x{off:X})", m.name),
                "[houdini_jit]".to_string(),
                inst_gva,
            );
        }
    }
    (
        "Houdini JIT".to_string(),
        "[houdini_jit]".to_string(),
        inst_gva,
    )
}

/// Real-time kernel hardware watchpoint (`perf_event_open`) for `InstructionWatchDialog`
/// on Android emulators (supports both x86_64 and ARM64/libhoudini games, zero `ptrace`).
pub struct AndroidPerfInstructionWatch {
    stop: Arc<AtomicBool>,
    stdin_tx: Arc<Mutex<Option<std::process::ChildStdin>>>,
    handle: Option<JoinHandle<()>>,
}

impl AndroidPerfInstructionWatch {
    pub fn start(
        host_pid: u32,
        android_pid: u32,
        adb_path: PathBuf,
        adb_target: String,
        watch_targets: Vec<(usize, usize)>, // (gva_hint, hva)
        writes_only: bool,
        tx: mpsc::Sender<WatchEvent>,
        modules: Vec<AndroidMemoryModule>,
        all_regions: Vec<AndroidMemoryRegion>,
        delta: isize,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stdin_tx: Arc<Mutex<Option<std::process::ChildStdin>>> = Arc::new(Mutex::new(None));
        let stop_clone = Arc::clone(&stop);
        let stdin_clone = Arc::clone(&stdin_tx);

        let handle = thread::spawn(move || {
            let _ = tx.send(WatchEvent::Started {
                armed_threads: 1,
                total_threads: 1,
            });

            if let Err(err) = ensure_mn_watch_deployed(&adb_path, &adb_target) {
                let _ = tx.send(WatchEvent::Error(err));
                let _ = tx.send(WatchEvent::Stopped);
                return;
            }

            let Some(&(target_gva_hint, target_hva)) = watch_targets.first() else {
                let _ = tx.send(WatchEvent::Error("No target address".to_string()));
                let _ = tx.send(WatchEvent::Stopped);
                return;
            };

            let target_gva = match resolve_target_gva(
                host_pid,
                android_pid,
                target_gva_hint,
                target_hva,
                delta,
                &all_regions,
                &adb_path,
                &adb_target,
            ) {
                Ok(g) => g,
                Err(err) => {
                    let _ = tx.send(WatchEvent::Status(err));
                    target_gva_hint
                }
            };

            let bp_type = if writes_only { 2 } else { 3 };
            let cmd = format!(
                "{REMOTE_MN_WATCH} bp {android_pid} 0x{target_gva:x} {bp_type} 4"
            );
            let mut child = match spawn_adb_root(&adb_path, &adb_target, &cmd) {
                Ok(c) => c,
                Err(err) => {
                    let _ = tx.send(WatchEvent::Error(err));
                    let _ = tx.send(WatchEvent::Stopped);
                    return;
                }
            };

            *stdin_clone.lock() = child.stdin.take();
            let Some(stdout) = child.stdout.take() else {
                let _ = child.kill();
                let _ = tx.send(WatchEvent::Error("Missing mn_watch stdout".to_string()));
                let _ = tx.send(WatchEvent::Stopped);
                return;
            };

            // Background value mutation poller on host HVA so live Diff/Writes summary still updates
            let mut_stop = Arc::clone(&stop_clone);
            let mut_tx = tx.clone();
            let mut_mods = modules.clone();
            let mut_regions = all_regions.clone();
            let mut_poller = thread::spawn(move || {
                let mut prev_bytes = read_memory_bytes(host_pid, target_hva, 8).ok();
                let mut count = 0usize;
                while !mut_stop.load(Ordering::Acquire) {
                    if let Ok(cur) = read_memory_bytes(host_pid, target_hva, 8) {
                        if let Some(prev) = &prev_bytes {
                            if prev != &cur {
                                let prev_u32 =
                                    u32::from_le_bytes(prev[..4].try_into().unwrap_or_default());
                                let cur_u32 =
                                    u32::from_le_bytes(cur[..4].try_into().unwrap_or_default());
                                let prev_f32 =
                                    f32::from_le_bytes(prev[..4].try_into().unwrap_or_default());
                                let cur_f32 =
                                    f32::from_le_bytes(cur[..4].try_into().unwrap_or_default());
                                let is_float = (cur_f32 - prev_f32).abs() > 0.0001
                                    && cur_f32.is_finite()
                                    && prev_f32.is_finite()
                                    && cur_f32.abs() < 1e7
                                    && prev_f32.abs() < 1e7;
                                let summary = if is_float {
                                    format!(
                                        "Write: {:.2} -> {:.2} (Diff: {:.2})",
                                        prev_f32,
                                        cur_f32,
                                        cur_f32 - prev_f32
                                    )
                                } else {
                                    let d = cur_u32 as i64 - prev_u32 as i64;
                                    format!("Write: {prev_u32} -> {cur_u32} (Diff: {d:+})")
                                };
                                let mod_desc = mut_mods
                                    .iter()
                                    .find(|m| target_gva >= m.base_address && target_gva < m.end_address)
                                    .map(|m| format!("{}+0x{:X}", m.name, target_gva - m.base_address))
                                    .or_else(|| {
                                        mut_regions
                                            .iter()
                                            .find(|r| target_gva >= r.start && target_gva < r.end)
                                            .map(|r| format!("{}+0x{:X}", r.pathname, target_gva - r.start))
                                    })
                                    .unwrap_or_else(|| "Guest Heap".to_string());
                                count += 1;
                                let details = format!(
                                    "Target GVA: 0x{target_gva:X} (Host HVA: 0x{target_hva:X})\nRegion: {mod_desc}\nOld: {prev_u32} (f32: {prev_f32:.2})\nNew: {cur_u32} (f32: {cur_f32:.2})"
                                );
                                let _ = mut_tx.send(WatchEvent::ValueMutation {
                                    target_address: target_hva,
                                    summary,
                                    details,
                                    count,
                                });
                            }
                        }
                        prev_bytes = Some(cur);
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            });

            let mut total_hits_by_ip: HashMap<usize, u64> = HashMap::new();
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if stop_clone.load(Ordering::Acquire) {
                    break;
                }
                let t = line.trim();
                if let Some(n_str) = t.strip_prefix("ARMED ") {
                    let armed_n = n_str.trim().parse::<usize>().unwrap_or(0);
                    if armed_n == 0 {
                        let _ = tx.send(WatchEvent::Status(format!(
                            "Kernel HW Watchpoint armed 0 threads (check root/PID {android_pid})"
                        )));
                    } else {
                        let mode = if writes_only { "Write" } else { "Access" };
                        let _ = tx.send(WatchEvent::Status(format!(
                            "Kernel HW {mode} Watchpoint active ({armed_n} threads) — GVA 0x{target_gva:X}"
                        )));
                    }
                } else if let Some(rest) = t.strip_prefix("HIT ") {
                    // HIT <ip_hex> <count> <rbp_hex> <ctx_pc_hex> <128hex>
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    if parts.len() >= 5 {
                        let Some(ip_after) = parse_hex_u64(parts[0]).map(|v| v as usize) else {
                            continue;
                        };
                        let delta_hits = parts[1].parse::<u64>().unwrap_or(1);
                        let ctx_pc = parse_hex_u64(parts[3]).unwrap_or(0) as usize;
                        let Some(code64) = parse_hex_bytes(parts[4]) else {
                            continue;
                        };

                        let (inst_gva, _inst_len, asm_text, writes) =
                            match decode_access_instr_before(ip_after, &code64) {
                                Some((addr, len, asm, _, w)) => (addr, len, asm, w),
                                None => (ip_after, 1, format!("op @ 0x{ip_after:X}"), writes_only),
                            };
                        let buf_base = ip_after.saturating_sub(48);
                        let (aob_signature, aob_target_offset) =
                            generate_aob_from_code64(&code64, buf_base, inst_gva);
                        let (loc_desc, mod_name, mod_off) =
                            describe_guest_location(inst_gva, ctx_pc, &modules);

                        store_captured_instruction(CapturedAndroidInstrMeta {
                            inst_gva,
                            asm_text: asm_text.clone(),
                            module_name: mod_name,
                            module_offset: mod_off,
                            aob_signature,
                            aob_target_offset,
                            writes,
                        });

                        let total = total_hits_by_ip.entry(inst_gva).or_insert(0);
                        *total += delta_hits;
                        let details = format!(
                            "Kernel HW Watchpoint Captured Instruction!\nInstruction: {asm_text}\nLocation: {loc_desc} (Guest IP: 0x{inst_gva:X})\nWatched GVA: 0x{target_gva:X}\nLive Hits: {}\nClick 'Add to code list' to track values after game restarts!",
                            *total
                        );
                        let emit_n = delta_hits.min(16);
                        for _ in 0..emit_n {
                            let _ = tx.send(WatchEvent::AddressHit {
                                instruction_address: inst_gva,
                                instruction: format!("{asm_text}  ({loc_desc})"),
                                details: details.clone(),
                            });
                        }
                    }
                }
            }

            stop_clone.store(true, Ordering::Release);
            let _ = mut_poller.join();
            let _ = child.kill();
            let _ = child.wait();
            let _ = tx.send(WatchEvent::Stopped);
        });

        Self {
            stop,
            stdin_tx,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(mut stdin) = self.stdin_tx.lock().take() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for AndroidPerfInstructionWatch {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Real-time kernel `HW_BREAKPOINT_X` watcher for `CodeAccessDialog` ("Watch Writes / Find accessed")
/// supporting both `x86_64` `.so` instructions and `ARM64`/`libhoudini` JIT instructions across restarts.
pub struct AndroidPerfCodeAccessWatch {
    stop: Arc<AtomicBool>,
    stdin_tx: Arc<Mutex<Option<std::process::ChildStdin>>>,
    handle: Option<JoinHandle<()>>,
}

impl AndroidPerfCodeAccessWatch {
    pub fn start(
        android_pid: u32,
        adb_path: PathBuf,
        adb_target: String,
        initial_inst_gva: usize,
        module_name: String,
        aob_signature: String,
        aob_target_offset: usize,
        tx: mpsc::Sender<WatchEvent>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stdin_tx: Arc<Mutex<Option<std::process::ChildStdin>>> = Arc::new(Mutex::new(None));
        let stop_clone = Arc::clone(&stop);
        let stdin_clone = Arc::clone(&stdin_tx);

        let handle = thread::spawn(move || {
            let _ = tx.send(WatchEvent::Started {
                armed_threads: 1,
                total_threads: 1,
            });

            if let Err(err) = ensure_mn_watch_deployed(&adb_path, &adb_target) {
                let _ = tx.send(WatchEvent::Error(err));
                let _ = tx.send(WatchEvent::Stopped);
                return;
            }

            // 1. Resolve live inst_gva (via direct read if valid, or via AOB scan in rwx/rx guest memory)
            let is_houdini_jit = module_name == "[houdini_jit]" || module_name == "[dynamic]";
            let mut inst_gva = 0usize;
            if !is_houdini_jit && initial_inst_gva > 0x10000 {
                let r_cmd = format!("{REMOTE_MN_WATCH} read {android_pid} 0x{initial_inst_gva:x} 32");
                if let Ok(out) = adb_shell_root(&adb_path, &adb_target, &r_cmd) {
                    if let Some(hex) = out.lines().find_map(|l| l.trim().strip_prefix("BYTES ")) {
                        if let Some(bytes) = parse_hex_bytes(hex) {
                            let mut dec = Decoder::with_ip(64, &bytes, initial_inst_gva as u64, DecoderOptions::NONE);
                            let instr = dec.decode();
                            if !instr.is_invalid()
                                && (0..instr.op_count()).any(|k| instr.op_kind(k) == OpKind::Memory)
                            {
                                inst_gva = initial_inst_gva;
                            }
                        }
                    }
                }
            }

            if inst_gva == 0 && !aob_signature.is_empty() {
                let compact_aob: String = aob_signature.split_whitespace().collect();
                let perm = if is_houdini_jit { 'w' } else { 'x' };
                let _ = tx.send(WatchEvent::Status(
                    "Locating instruction via AOB in live game memory (perform the action in game)..."
                        .to_string(),
                ));
                while !stop_clone.load(Ordering::Acquire) {
                    let a_cmd = format!(
                        "{REMOTE_MN_WATCH} aob {android_pid} {perm} \"{compact_aob}\""
                    );
                    if let Ok(out) = adb_shell_root(&adb_path, &adb_target, &a_cmd) {
                        if let Some(addr_str) = out.lines().find_map(|l| l.trim().strip_prefix("AOB ")) {
                            if let Some(base) = parse_hex_u64(addr_str) {
                                inst_gva = base as usize + aob_target_offset;
                                break;
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(450));
                }
            }

            if stop_clone.load(Ordering::Acquire) {
                let _ = tx.send(WatchEvent::Stopped);
                return;
            }
            if inst_gva == 0 {
                let _ = tx.send(WatchEvent::Error(
                    "Could not resolve instruction in guest memory".to_string(),
                ));
                let _ = tx.send(WatchEvent::Stopped);
                return;
            }

            // 2. Decode memory operand at inst_gva so mn_watch computes [base + index*scale + disp]
            let r_cmd = format!("{REMOTE_MN_WATCH} read {android_pid} 0x{inst_gva:x} 32");
            let bytes = match adb_shell_root(&adb_path, &adb_target, &r_cmd)
                .ok()
                .and_then(|out| {
                    out.lines()
                        .find_map(|l| l.trim().strip_prefix("BYTES "))
                        .and_then(parse_hex_bytes)
                }) {
                Some(b) => b,
                None => {
                    let _ = tx.send(WatchEvent::Error(format!(
                        "Cannot read guest instruction bytes at 0x{inst_gva:X}"
                    )));
                    let _ = tx.send(WatchEvent::Stopped);
                    return;
                }
            };

            let mut dec = Decoder::with_ip(64, &bytes, inst_gva as u64, DecoderOptions::NONE);
            let instr = dec.decode();
            if instr.is_invalid()
                || !(0..instr.op_count()).any(|k| instr.op_kind(k) == OpKind::Memory)
            {
                let _ = tx.send(WatchEvent::Error(format!(
                    "Instruction at 0x{inst_gva:X} has no memory operand"
                )));
                let _ = tx.send(WatchEvent::Stopped);
                return;
            }

            let is_rip = if instr.is_ip_rel_memory_operand() { 1 } else { 0 };
            let (breg, ireg, scale, disp) = if is_rip != 0 {
                let abs = instr.ip_rel_memory_address() as i64;
                (-1, -1, 1, abs.wrapping_sub(inst_gva as i64))
            } else {
                let b = if instr.memory_base() == Register::None {
                    -1
                } else {
                    register_to_perf_idx(instr.memory_base()).unwrap_or(-1)
                };
                let idx = if instr.memory_index() == Register::None {
                    -1
                } else {
                    register_to_perf_idx(instr.memory_index()).unwrap_or(-1)
                };
                (
                    b,
                    idx,
                    instr.memory_index_scale(),
                    instr.memory_displacement64() as i64,
                )
            };

            let cmd = format!(
                "{REMOTE_MN_WATCH} bp {android_pid} 0x{inst_gva:x} 4 8 {breg} {ireg} {scale} {disp} {is_rip}"
            );
            let mut child = match spawn_adb_root(&adb_path, &adb_target, &cmd) {
                Ok(c) => c,
                Err(err) => {
                    let _ = tx.send(WatchEvent::Error(err));
                    let _ = tx.send(WatchEvent::Stopped);
                    return;
                }
            };
            *stdin_clone.lock() = child.stdin.take();
            let Some(stdout) = child.stdout.take() else {
                let _ = child.kill();
                let _ = tx.send(WatchEvent::Error("Missing mn_watch stdout".to_string()));
                let _ = tx.send(WatchEvent::Stopped);
                return;
            };

            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if stop_clone.load(Ordering::Acquire) {
                    break;
                }
                let t = line.trim();
                if let Some(n_str) = t.strip_prefix("ARMED ") {
                    let armed_n = n_str.trim().parse::<usize>().unwrap_or(0);
                    let asm = format_instr_intel(&instr);
                    let _ = tx.send(WatchEvent::Status(format!(
                        "Kernel HW Execute Breakpoint active ({armed_n} threads) — {asm} @ 0x{inst_gva:X}"
                    )));
                } else if let Some(rest) = t.strip_prefix("ACC ") {
                    // ACC <data_addr_hex> <count> <val64_hex>
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let Some(data_addr) = parse_hex_u64(parts[0]).map(|v| v as usize) else {
                            continue;
                        };
                        let cnt = parts[1].parse::<u64>().unwrap_or(1).min(16);
                        if parts.len() >= 3 {
                            if let Some(v64) = parse_hex_u64(parts[2]) {
                                store_captured_guest_value(data_addr, v64);
                            }
                        }
                        for _ in 0..cnt {
                            let _ = tx.send(WatchEvent::AccessHit {
                                instruction_address: inst_gva,
                                data_address: data_addr,
                            });
                        }
                    }
                }
            }

            let _ = child.kill();
            let _ = child.wait();
            let _ = tx.send(WatchEvent::Stopped);
        });

        Self {
            stop,
            stdin_tx,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(mut stdin) = self.stdin_tx.lock().take() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for AndroidPerfCodeAccessWatch {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_access_instr_before_houdini_jit_write() {
        // Real bytes captured from libhoudini JIT (`vmovd [r13+2E0h], xmm1` ending at offset 48)
        let buf64 = parse_hex_bytes(
            "0008032100ba0000803fc5f96ecac4c1796e95e0020000c5f96ffaf30f58f9660fefc9f30f11f9c4c1797e8de002000048c785b800000000000000458d6424ff",
        )
        .unwrap();
        assert_eq!(buf64.len(), 64);
        let ip_after = 0x2508_A04Busize;
        let (inst_ip, len, asm, instr, writes) =
            decode_access_instr_before(ip_after, &buf64).expect("should decode vmovd");
        assert_eq!(inst_ip, 0x2508_A042);
        assert_eq!(len, 9);
        assert!(asm.contains("r13+2E0h"), "{asm}");
        assert_eq!(instr.memory_base(), Register::R13);
        assert_eq!(instr.memory_displacement64(), 0x2E0);
        assert!(writes);

        let (aob, off) = generate_aob_from_code64(&buf64, ip_after - 48, inst_ip);
        assert!(!aob.is_empty());
        assert!(aob.contains("C4 C1 79 7E 8D E0 02 00 00"), "{aob}");
        assert_eq!(off + 9 <= aob.split_whitespace().count(), true);
    }

    #[test]
    fn test_register_to_perf_idx() {
        assert_eq!(register_to_perf_idx(Register::RAX), Some(0));
        assert_eq!(register_to_perf_idx(Register::RBP), Some(6));
        assert_eq!(register_to_perf_idx(Register::R8), Some(9));
        assert_eq!(register_to_perf_idx(Register::R13), Some(14));
        assert_eq!(register_to_perf_idx(Register::R15), Some(16));
    }
}
