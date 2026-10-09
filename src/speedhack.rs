//! Speedhack module: hooks time functions (QueryPerformanceCounter, GetTickCount64)
//! in the target process to accelerate or decelerate game simulation time, identical to Cheat Engine's speedhack.

#![allow(dead_code)]

#[cfg(windows)]
mod imp {
    use std::io;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory},
            LibraryLoader::{GetModuleHandleA, GetProcAddress},
            Memory::{
                MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAllocEx,
                VirtualFreeEx, VirtualProtectEx,
            },
            SystemInformation::GetTickCount64,
            Threading::{
                OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
                PROCESS_VM_WRITE,
            },
        },
    };

    unsafe extern "system" {
        fn QueryPerformanceCounter(lpPerformanceCount: *mut i64) -> i32;
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct SpeedDataBlock {
        pub speed: f64,            // +0x00
        pub base_real_qpc: i64,    // +0x08
        pub base_virt_qpc: i64,    // +0x10
        pub base_real_tick: u64,   // +0x18
        pub base_virt_tick: u64,   // +0x20
        pub qpc_trampoline: u64,   // +0x28
        pub gtc64_trampoline: u64, // +0x30
        pub is_active: u32,        // +0x38
        pub _pad: u32,             // +0x3C
    }

    pub fn get_proc_address(module: &str, func: &str) -> Option<usize> {
        let module_cstr = std::ffi::CString::new(module).ok()?;
        let func_cstr = std::ffi::CString::new(func).ok()?;
        unsafe {
            let h = GetModuleHandleA(module_cstr.as_ptr() as *const u8);
            if h.is_null() {
                return None;
            }
            let p = GetProcAddress(h, func_cstr.as_ptr() as *const u8);
            p.map(|f| f as usize)
        }
    }

    pub fn determine_stolen_len(bytes: &[u8], ip: u64) -> usize {
        let decoder =
            iced_x86::Decoder::with_ip(64, bytes, ip, iced_x86::DecoderOptions::NONE);
        let mut total = 0;
        for instr in decoder {
            total += instr.len();
            if total >= 14 {
                return total;
            }
        }
        16
    }

    pub fn build_jump14(target_address: usize) -> [u8; 14] {
        let mut jmp = [0u8; 14];
        jmp[0..6].copy_from_slice(&[0xFF, 0x25, 0x00, 0x00, 0x00, 0x00]); // jmp qword ptr [rip+0]
        jmp[6..14].copy_from_slice(&(target_address as u64).to_le_bytes());
        jmp
    }

    pub fn build_trampoline(stolen_bytes: &[u8], resume_address: usize) -> Vec<u8> {
        let mut tramp = Vec::with_capacity(stolen_bytes.len() + 14);
        tramp.extend_from_slice(stolen_bytes);
        tramp.extend_from_slice(&build_jump14(resume_address));
        tramp
    }

    pub fn build_qpc_hook(trampoline_addr: usize, data_block_addr: usize) -> Vec<u8> {
        let mut code = Vec::with_capacity(96);
        // sub rsp, 0x38 (maintains 16-byte stack alignment across calls)
        code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x38]);
        // mov [rsp + 0x20], rcx (save lpPerformanceCount)
        code.extend_from_slice(&[0x48, 0x89, 0x4C, 0x24, 0x20]);
        // mov rax, trampoline_addr
        code.extend_from_slice(&[0x48, 0xB8]);
        code.extend_from_slice(&(trampoline_addr as u64).to_le_bytes());
        // call rax
        code.extend_from_slice(&[0xFF, 0xD0]);
        // test eax, eax
        code.extend_from_slice(&[0x85, 0xC0]);
        // jz done_qpc
        let jz1_idx = code.len();
        code.extend_from_slice(&[0x74, 0x00]);

        // mov rax, data_block_addr
        code.extend_from_slice(&[0x48, 0xB8]);
        code.extend_from_slice(&(data_block_addr as u64).to_le_bytes());
        // mov edx, [rax + 0x38] (is_active)
        code.extend_from_slice(&[0x8B, 0x50, 0x38]);
        // test edx, edx
        code.extend_from_slice(&[0x85, 0xD2]);
        // jz done_qpc
        let jz2_idx = code.len();
        code.extend_from_slice(&[0x74, 0x00]);

        // mov rcx, [rsp + 0x20] (lpPerformanceCount)
        code.extend_from_slice(&[0x48, 0x8B, 0x4C, 0x24, 0x20]);
        // mov rdx, [rcx] (real_qpc)
        code.extend_from_slice(&[0x48, 0x8B, 0x11]);
        // sub rdx, [rax + 0x08] (diff = real - base_real_qpc)
        code.extend_from_slice(&[0x48, 0x2B, 0x50, 0x08]);
        // cvtsi2sd xmm0, rdx
        code.extend_from_slice(&[0xF2, 0x48, 0x0F, 0x2A, 0xC2]);
        // mulsd xmm0, [rax + 0x00] (speed)
        code.extend_from_slice(&[0xF2, 0x0F, 0x59, 0x00]);
        // cvttsd2si rdx, xmm0
        code.extend_from_slice(&[0xF2, 0x48, 0x0F, 0x2C, 0xD0]);
        // add rdx, [rax + 0x10] (base_virt_qpc)
        code.extend_from_slice(&[0x48, 0x03, 0x50, 0x10]);
        // mov [rcx], rdx
        code.extend_from_slice(&[0x48, 0x89, 0x11]);
        // mov eax, 1 (TRUE)
        code.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]);

        // done_qpc:
        let done_offset = code.len();
        code[jz1_idx + 1] = (done_offset - (jz1_idx + 2)) as u8;
        code[jz2_idx + 1] = (done_offset - (jz2_idx + 2)) as u8;

        // add rsp, 0x38
        code.extend_from_slice(&[0x48, 0x83, 0xC4, 0x38]);
        // ret
        code.push(0xC3);

        code
    }

    pub fn build_gtc64_hook(trampoline_addr: usize, data_block_addr: usize) -> Vec<u8> {
        let mut code = Vec::with_capacity(64);
        // sub rsp, 0x28
        code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x28]);
        // mov rax, trampoline_addr
        code.extend_from_slice(&[0x48, 0xB8]);
        code.extend_from_slice(&(trampoline_addr as u64).to_le_bytes());
        // call rax
        code.extend_from_slice(&[0xFF, 0xD0]);

        // mov rcx, data_block_addr
        code.extend_from_slice(&[0x48, 0xB9]);
        code.extend_from_slice(&(data_block_addr as u64).to_le_bytes());
        // mov edx, [rcx + 0x38] (is_active)
        code.extend_from_slice(&[0x8B, 0x51, 0x38]);
        // test edx, edx
        code.extend_from_slice(&[0x85, 0xD2]);
        // jz done_gtc
        let jz_idx = code.len();
        code.extend_from_slice(&[0x74, 0x00]);

        // sub rax, [rcx + 0x18] (diff = real_tick - base_real_tick)
        code.extend_from_slice(&[0x48, 0x2B, 0x41, 0x18]);
        // cvtsi2sd xmm0, rax
        code.extend_from_slice(&[0xF2, 0x48, 0x0F, 0x2A, 0xC0]);
        // mulsd xmm0, [rcx + 0x00] (speed)
        code.extend_from_slice(&[0xF2, 0x0F, 0x59, 0x01]);
        // cvttsd2si rax, xmm0
        code.extend_from_slice(&[0xF2, 0x48, 0x0F, 0x2C, 0xC0]);
        // add rax, [rcx + 0x20] (base_virt_tick)
        code.extend_from_slice(&[0x48, 0x03, 0x41, 0x20]);

        // done_gtc:
        let done_offset = code.len();
        code[jz_idx + 1] = (done_offset - (jz_idx + 2)) as u8;

        // add rsp, 0x28
        code.extend_from_slice(&[0x48, 0x83, 0xC4, 0x28]);
        // ret
        code.push(0xC3);

        code
    }

    #[derive(Debug)]
    pub struct SpeedhackSession {
        pub pid: u32,
        pub process_handle: usize,
        pub remote_base: usize,
        pub data_block: Option<SpeedDataBlock>,
        pub qpc_hook: Option<(usize, Vec<u8>)>,   // (func_addr, original_bytes)
        pub gtc64_hook: Option<(usize, Vec<u8>)>, // (func_addr, original_bytes)
        pub speed: f64,
        pub is_enabled: bool,
    }

    impl Default for SpeedhackSession {
        fn default() -> Self {
            Self {
                pid: 0,
                process_handle: 0,
                remote_base: 0,
                data_block: None,
                qpc_hook: None,
                gtc64_hook: None,
                speed: 1.0,
                is_enabled: false,
            }
        }
    }

    impl SpeedhackSession {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn is_enabled(&self) -> bool {
            self.is_enabled
        }

        pub fn enable(&mut self, pid: u32, speed: f64) -> io::Result<()> {
            if self.is_enabled && self.pid == pid {
                return self.set_speed(speed);
            }
            if self.is_enabled {
                self.disable();
            }

            let handle = unsafe {
                OpenProcess(
                    PROCESS_VM_OPERATION
                        | PROCESS_VM_READ
                        | PROCESS_VM_WRITE
                        | PROCESS_QUERY_INFORMATION,
                    0,
                    pid,
                )
            };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }

            // Allocate 4096 bytes in target process
            let remote_mem = unsafe {
                VirtualAllocEx(
                    handle,
                    std::ptr::null(),
                    4096,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_EXECUTE_READWRITE,
                )
            };
            if remote_mem.is_null() {
                unsafe { CloseHandle(handle) };
                return Err(io::Error::last_os_error());
            }
            let remote_base = remote_mem as usize;

            // Layout offsets inside remote_base:
            // 0x00..0x3F : SpeedDataBlock
            // 0x80..0xDF : QPC Trampoline
            // 0xE0..0x13F: GTC64 Trampoline
            // 0x140..    : QPC Hook code
            // 0x200..    : GTC64 Hook code
            let data_block_addr = remote_base;
            let qpc_tramp_addr = remote_base + 0x80;
            let gtc64_tramp_addr = remote_base + 0xE0;
            let qpc_hook_addr = remote_base + 0x140;
            let gtc64_hook_addr = remote_base + 0x200;

            // Read current real time values
            let mut current_qpc = 0i64;
            unsafe { QueryPerformanceCounter(&mut current_qpc) };
            let current_tick = unsafe { GetTickCount64() };

            let data_block = SpeedDataBlock {
                speed,
                base_real_qpc: current_qpc,
                base_virt_qpc: current_qpc,
                base_real_tick: current_tick,
                base_virt_tick: current_tick,
                qpc_trampoline: qpc_tramp_addr as u64,
                gtc64_trampoline: gtc64_tramp_addr as u64,
                is_active: 1,
                _pad: 0,
            };

            // Locate QPC function address
            let qpc_addr = get_proc_address("KernelBase.dll", "QueryPerformanceCounter")
                .or_else(|| get_proc_address("kernel32.dll", "QueryPerformanceCounter"))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "QueryPerformanceCounter not found")
                })?;

            // Read original bytes at QPC
            let mut qpc_orig = [0u8; 32];
            let mut bytes_read = 0;
            unsafe {
                ReadProcessMemory(
                    handle,
                    qpc_addr as _,
                    qpc_orig.as_mut_ptr() as _,
                    qpc_orig.len(),
                    &mut bytes_read,
                );
            }
            let qpc_stolen_len = determine_stolen_len(&qpc_orig, qpc_addr as u64);
            let qpc_stolen_bytes = qpc_orig[..qpc_stolen_len].to_vec();

            // Build QPC trampoline and hook
            let qpc_tramp_code =
                build_trampoline(&qpc_stolen_bytes, qpc_addr + qpc_stolen_len);
            let qpc_hook_code = build_qpc_hook(qpc_tramp_addr, data_block_addr);

            // Locate GTC64 function address
            let gtc64_addr = get_proc_address("KernelBase.dll", "GetTickCount64")
                .or_else(|| get_proc_address("kernel32.dll", "GetTickCount64"));

            let mut gtc64_info = None;
            let mut gtc64_tramp_code = Vec::new();
            let mut gtc64_hook_code = Vec::new();
            let mut gtc64_stolen_len = 0;

            if let Some(addr) = gtc64_addr {
                let mut gtc64_orig = [0u8; 32];
                let mut read = 0;
                unsafe {
                    ReadProcessMemory(
                        handle,
                        addr as _,
                        gtc64_orig.as_mut_ptr() as _,
                        gtc64_orig.len(),
                        &mut read,
                    );
                }
                let stolen = determine_stolen_len(&gtc64_orig, addr as u64);
                let stolen_bytes = gtc64_orig[..stolen].to_vec();
                gtc64_stolen_len = stolen;
                gtc64_tramp_code = build_trampoline(&stolen_bytes, addr + stolen);
                gtc64_hook_code = build_gtc64_hook(gtc64_tramp_addr, data_block_addr);
                gtc64_info = Some((addr, stolen_bytes));
            }

            // Write all components into remote allocated memory
            unsafe {
                // 1. Write DataBlock
                let mut written = 0;
                WriteProcessMemory(
                    handle,
                    data_block_addr as _,
                    &data_block as *const _ as _,
                    std::mem::size_of::<SpeedDataBlock>(),
                    &mut written,
                );

                // 2. Write QPC Trampoline
                WriteProcessMemory(
                    handle,
                    qpc_tramp_addr as _,
                    qpc_tramp_code.as_ptr() as _,
                    qpc_tramp_code.len(),
                    &mut written,
                );

                // 3. Write QPC Hook
                WriteProcessMemory(
                    handle,
                    qpc_hook_addr as _,
                    qpc_hook_code.as_ptr() as _,
                    qpc_hook_code.len(),
                    &mut written,
                );

                // 4. Write GTC64 Trampoline & Hook if available
                if !gtc64_tramp_code.is_empty() {
                    WriteProcessMemory(
                        handle,
                        gtc64_tramp_addr as _,
                        gtc64_tramp_code.as_ptr() as _,
                        gtc64_tramp_code.len(),
                        &mut written,
                    );
                    WriteProcessMemory(
                        handle,
                        gtc64_hook_addr as _,
                        gtc64_hook_code.as_ptr() as _,
                        gtc64_hook_code.len(),
                        &mut written,
                    );
                }
            }

            // Patch QPC function prologue with 14-byte JMP
            let mut qpc_patch = vec![0x90u8; qpc_stolen_len]; // padded with NOPs
            qpc_patch[..14].copy_from_slice(&build_jump14(qpc_hook_addr));
            unsafe {
                let mut old_protect = 0;
                VirtualProtectEx(
                    handle,
                    qpc_addr as _,
                    qpc_stolen_len,
                    PAGE_EXECUTE_READWRITE,
                    &mut old_protect,
                );
                let mut written = 0;
                WriteProcessMemory(
                    handle,
                    qpc_addr as _,
                    qpc_patch.as_ptr() as _,
                    qpc_patch.len(),
                    &mut written,
                );
                VirtualProtectEx(
                    handle,
                    qpc_addr as _,
                    qpc_stolen_len,
                    old_protect,
                    &mut old_protect,
                );
            }

            // Patch GTC64 function prologue with 14-byte JMP if available
            if let Some((addr, _)) = gtc64_info.as_ref() {
                let mut gtc64_patch = vec![0x90u8; gtc64_stolen_len];
                gtc64_patch[..14].copy_from_slice(&build_jump14(gtc64_hook_addr));
                unsafe {
                    let mut old_protect = 0;
                    VirtualProtectEx(
                        handle,
                        *addr as _,
                        gtc64_stolen_len,
                        PAGE_EXECUTE_READWRITE,
                        &mut old_protect,
                    );
                    let mut written = 0;
                    WriteProcessMemory(
                        handle,
                        *addr as _,
                        gtc64_patch.as_ptr() as _,
                        gtc64_patch.len(),
                        &mut written,
                    );
                    VirtualProtectEx(
                        handle,
                        *addr as _,
                        gtc64_stolen_len,
                        old_protect,
                        &mut old_protect,
                    );
                }
            }

            self.pid = pid;
            self.process_handle = handle as usize;
            self.remote_base = remote_base;
            self.data_block = Some(data_block);
            self.qpc_hook = Some((qpc_addr, qpc_stolen_bytes));
            self.gtc64_hook = gtc64_info;
            self.speed = speed;
            self.is_enabled = true;

            Ok(())
        }

        pub fn set_speed(&mut self, target_speed: f64) -> io::Result<()> {
            if !self.is_enabled || self.remote_base == 0 || self.process_handle == 0 {
                self.speed = target_speed;
                return Ok(());
            }
            let handle = self.process_handle as HANDLE;

            // Read current data block from target
            let mut block = SpeedDataBlock {
                speed: 1.0,
                base_real_qpc: 0,
                base_virt_qpc: 0,
                base_real_tick: 0,
                base_virt_tick: 0,
                qpc_trampoline: 0,
                gtc64_trampoline: 0,
                is_active: 1,
                _pad: 0,
            };
            let mut read = 0;
            unsafe {
                ReadProcessMemory(
                    handle,
                    self.remote_base as _,
                    &mut block as *mut _ as _,
                    std::mem::size_of::<SpeedDataBlock>(),
                    &mut read,
                );
            }

            let mut current_qpc = 0i64;
            unsafe { QueryPerformanceCounter(&mut current_qpc) };
            let current_tick = unsafe { GetTickCount64() };

            // Advance virtual time smoothly:
            let qpc_diff = current_qpc - block.base_real_qpc;
            let new_virt_qpc = block.base_virt_qpc + ((qpc_diff as f64) * block.speed) as i64;

            let tick_diff = current_tick.saturating_sub(block.base_real_tick);
            let new_virt_tick =
                block.base_virt_tick + ((tick_diff as f64) * block.speed) as u64;

            block.speed = target_speed;
            block.base_real_qpc = current_qpc;
            block.base_virt_qpc = new_virt_qpc;
            block.base_real_tick = current_tick;
            block.base_virt_tick = new_virt_tick;

            unsafe {
                let mut written = 0;
                WriteProcessMemory(
                    handle,
                    self.remote_base as _,
                    &block as *const _ as _,
                    std::mem::size_of::<SpeedDataBlock>(),
                    &mut written,
                );
            }

            self.data_block = Some(block);
            self.speed = target_speed;
            Ok(())
        }

        pub fn disable(&mut self) {
            if !self.is_enabled || self.process_handle == 0 {
                self.is_enabled = false;
                return;
            }
            let handle = self.process_handle as HANDLE;

            // Mark inactive first
            if self.remote_base != 0 {
                let inactive = 0u32;
                let mut written = 0;
                unsafe {
                    WriteProcessMemory(
                        handle,
                        (self.remote_base + 0x38) as _,
                        &inactive as *const _ as _,
                        4,
                        &mut written,
                    );
                }
            }

            // Restore QPC original bytes
            if let Some((addr, orig)) = &self.qpc_hook {
                unsafe {
                    let mut old = 0;
                    VirtualProtectEx(
                        handle,
                        *addr as _,
                        orig.len(),
                        PAGE_EXECUTE_READWRITE,
                        &mut old,
                    );
                    let mut written = 0;
                    WriteProcessMemory(
                        handle,
                        *addr as _,
                        orig.as_ptr() as _,
                        orig.len(),
                        &mut written,
                    );
                    VirtualProtectEx(handle, *addr as _, orig.len(), old, &mut old);
                }
            }

            // Restore GTC64 original bytes
            if let Some((addr, orig)) = &self.gtc64_hook {
                unsafe {
                    let mut old = 0;
                    VirtualProtectEx(
                        handle,
                        *addr as _,
                        orig.len(),
                        PAGE_EXECUTE_READWRITE,
                        &mut old,
                    );
                    let mut written = 0;
                    WriteProcessMemory(
                        handle,
                        *addr as _,
                        orig.as_ptr() as _,
                        orig.len(),
                        &mut written,
                    );
                    VirtualProtectEx(handle, *addr as _, orig.len(), old, &mut old);
                }
            }

            // Free remote memory
            if self.remote_base != 0 {
                unsafe {
                    VirtualFreeEx(handle, self.remote_base as _, 0, MEM_RELEASE);
                }
            }

            unsafe {
                CloseHandle(handle);
            }

            self.process_handle = 0;
            self.remote_base = 0;
            self.qpc_hook = None;
            self.gtc64_hook = None;
            self.data_block = None;
            self.is_enabled = false;
        }
    }

    impl Drop for SpeedhackSession {
        fn drop(&mut self) {
            if self.is_enabled {
                self.disable();
            }
        }
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
#[derive(Default)]
pub struct SpeedhackSession;

#[cfg(not(windows))]
impl SpeedhackSession {
    pub fn new() -> Self {
        Self
    }
    pub fn is_enabled(&self) -> bool {
        false
    }
    pub fn enable(&mut self, _pid: u32, _speed: f64) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Windows only"))
    }
    pub fn set_speed(&mut self, _speed: f64) -> std::io::Result<()> {
        Ok(())
    }
    pub fn disable(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn test_verify_shellcode_encodings() {
        let dummy_data = 0x1234_5678_9ABC_DEF0usize;
        let dummy_tramp = 0x7FFE_3B10_BA90usize;

        let qpc_hook = build_qpc_hook(dummy_tramp, dummy_data);
        assert!(!qpc_hook.is_empty());
        let decoder =
            iced_x86::Decoder::with_ip(64, &qpc_hook, 0x1000, iced_x86::DecoderOptions::NONE);
        let mut count = 0;
        for instr in decoder {
            count += 1;
            assert!(!instr.is_invalid());
        }
        assert!(count >= 10, "QPC hook must contain valid instructions");

        let gtc64_hook = build_gtc64_hook(dummy_tramp, dummy_data);
        assert!(!gtc64_hook.is_empty());
        let decoder =
            iced_x86::Decoder::with_ip(64, &gtc64_hook, 0x2000, iced_x86::DecoderOptions::NONE);
        let mut count = 0;
        for instr in decoder {
            count += 1;
            assert!(!instr.is_invalid());
        }
        assert!(count >= 8, "GTC64 hook must contain valid instructions");
    }

    #[test]
    #[cfg(windows)]
    fn test_speedhack_session_enable_self_process() {
        let current_pid = std::process::id();
        let mut session = SpeedhackSession::new();
        let res = session.enable(current_pid, 2.0);
        assert!(res.is_ok(), "Enable on current process should succeed: {:?}", res);
        assert!(session.is_enabled);
        assert_eq!(session.speed, 2.0);

        // Test changing speed
        let set_res = session.set_speed(3.5);
        assert!(set_res.is_ok());
        assert_eq!(session.speed, 3.5);

        // Test disable restores cleanly
        session.disable();
        assert!(!session.is_enabled);
        assert_eq!(session.remote_base, 0);
    }
}
