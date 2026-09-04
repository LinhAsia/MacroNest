use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HMODULE, HWND, RECT},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        },
        System::Memory::{
            CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
            FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
        },
        System::Threading::{
            CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject,
        },
        UI::WindowsAndMessaging::GetWindowThreadProcessId,
    },
};

pub fn find_game_capture_binaries(paths: &crate::storage::AppPaths, is_32bit: bool) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let hook_name = if is_32bit { "graphics-hook32.dll" } else { "graphics-hook64.dll" };
    let inject_name = if is_32bit { "inject-helper32.exe" } else { "inject-helper64.exe" };
    let offsets_name = if is_32bit { "get-graphics-offsets32.exe" } else { "get-graphics-offsets64.exe" };

    // 1. Check local AppData bin/game_capture
    let local_hook = paths.game_capture_dir.join(hook_name);
    let local_inject = paths.game_capture_dir.join(inject_name);
    let local_offsets = paths.game_capture_dir.join(offsets_name);
    if local_hook.exists() && local_inject.exists() {
        return Some((local_hook, local_inject, local_offsets));
    }

    // 2. Check installed OBS Studio
    let obs_dir = PathBuf::from(r"D:\obs-studio\data\obs-plugins\win-capture");
    let obs_hook = obs_dir.join(hook_name);
    let obs_inject = obs_dir.join(inject_name);
    let obs_offsets = obs_dir.join(offsets_name);
    if obs_hook.exists() && obs_inject.exists() {
        return Some((obs_hook, obs_inject, obs_offsets));
    }

    // 3. Check Program Files OBS Studio
    let pf_dir = PathBuf::from(r"C:\Program Files\obs-studio\data\obs-plugins\win-capture");
    let pf_hook = pf_dir.join(hook_name);
    let pf_inject = pf_dir.join(inject_name);
    let pf_offsets = pf_dir.join(offsets_name);
    if pf_hook.exists() && pf_inject.exists() {
        return Some((pf_hook, pf_inject, pf_offsets));
    }

    None
}

pub fn is_game_capture_available(paths: &crate::storage::AppPaths) -> bool {
    find_game_capture_binaries(paths, false).is_some()
}

#[repr(C, packed(8))]
#[derive(Debug, Clone, Copy, Default)]
pub struct D3D8Offsets {
    pub present: u32,
}

#[repr(C, packed(8))]
#[derive(Debug, Clone, Copy, Default)]
pub struct D3D9Offsets {
    pub present: u32,
    pub present_ex: u32,
    pub present_swap: u32,
    pub d3d9_clsoff: u32,
    pub is_d3d9ex_clsoff: u32,
}

#[repr(C, packed(8))]
#[derive(Debug, Clone, Copy, Default)]
pub struct DXGIOffsets {
    pub present: u32,
    pub resize: u32,
    pub present1: u32,
}

#[repr(C, packed(8))]
#[derive(Debug, Clone, Copy, Default)]
pub struct DDRAWOffsets {
    pub surface_create: u32,
    pub surface_restore: u32,
    pub surface_release: u32,
    pub surface_unlock: u32,
    pub surface_blt: u32,
    pub surface_flip: u32,
    pub surface_set_palette: u32,
    pub palette_set_entries: u32,
}

#[repr(C, packed(8))]
#[derive(Debug, Clone, Copy, Default)]
pub struct DXGIOffsets2 {
    pub release: u32,
}

#[repr(C, packed(8))]
#[derive(Debug, Clone, Copy, Default)]
pub struct D3D12Offsets {
    pub execute_command_lists: u32,
}

#[repr(C, packed(8))]
#[derive(Debug, Clone, Copy, Default)]
pub struct GraphicsOffsets {
    pub d3d8: D3D8Offsets,
    pub d3d9: D3D9Offsets,
    pub dxgi: DXGIOffsets,
    pub ddraw: DDRAWOffsets,
    pub dxgi2: DXGIOffsets2,
    pub d3d12: D3D12Offsets,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureType {
    Memory = 0,
    Texture = 1,
}

#[repr(C, packed(8))]
pub struct HookInfo {
    pub hook_ver_major: u32,
    pub hook_ver_minor: u32,
    pub capture_type: CaptureType,
    pub window: u32,
    pub format: u32,
    pub cx: u32,
    pub cy: u32,
    pub unused_base_cx: u32,
    pub unused_base_cy: u32,
    pub pitch: u32,
    pub map_id: u32,
    pub map_size: u32,
    pub flip: bool,
    pub frame_interval: u64,
    pub unused_use_scale: bool,
    pub force_shmem: bool,
    pub capture_overlay: bool,
    pub allow_srgb_alias: bool,
    pub offsets: GraphicsOffsets,
    pub reserved: [u32; 126],
}

const _: () = assert!(std::mem::size_of::<HookInfo>() == 648);

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ShtexData {
    pub tex_handle: u32,
}

#[cfg(windows)]
pub struct GameCaptureSession {
    pub hwnd: HWND,
    pub pid: u32,
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    shared_texture: Option<ID3D11Texture2D>,
    staging_textures: Option<([ID3D11Texture2D; 3], u32, u32)>,
    write_idx: usize,
    copies_count: usize,
    hook_info_map: HANDLE,
    hook_info_ptr: *mut HookInfo,
    hook_init: HANDLE,
    hook_ready: HANDLE,
    hook_exit: HANDLE,
    hook_restart: HANDLE,
    hook_stop: HANDLE,
    tex_mutexes: [HANDLE; 2],
    keepalive: HANDLE,
    width: u32,
    height: u32,
    nvenc: Option<crate::nvenc::NvencHardwareEncoder>,
}

#[cfg(windows)]
unsafe impl Send for GameCaptureSession {}
#[cfg(windows)]
unsafe impl Sync for GameCaptureSession {}

#[cfg(windows)]
impl Drop for GameCaptureSession {
    fn drop(&mut self) {
        unsafe {
            if !self.hook_stop.0.is_null() {
                let _ = SetEvent(self.hook_stop);
            }
            if !self.hook_info_ptr.is_null() {
                let _ = UnmapViewOfFile(windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.hook_info_ptr as *mut _,
                });
            }
            let handles = [
                self.hook_info_map,
                self.hook_init,
                self.hook_ready,
                self.hook_exit,
                self.hook_restart,
                self.hook_stop,
                self.tex_mutexes[0],
                self.tex_mutexes[1],
                self.keepalive,
            ];
            for h in handles {
                if !h.0.is_null() {
                    let _ = CloseHandle(h);
                }
            }
        }
    }
}

static OFFSETS_64_CACHE: std::sync::OnceLock<GraphicsOffsets> = std::sync::OnceLock::new();
static OFFSETS_32_CACHE: std::sync::OnceLock<GraphicsOffsets> = std::sync::OnceLock::new();

pub fn get_graphics_offsets(offsets_exe: &Path, is_32bit: bool) -> GraphicsOffsets {
    let cache = if is_32bit {
        &OFFSETS_32_CACHE
    } else {
        &OFFSETS_64_CACHE
    };
    *cache.get_or_init(|| {
        let mut offsets = GraphicsOffsets::default();
        if offsets_exe.exists() {
            if let Ok(output) = Command::new(offsets_exe).creation_flags(0x0800_0000).output() {
                let str_out = String::from_utf8_lossy(&output.stdout);
                let mut section = "";
                for line in str_out.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with('[') && trimmed.ends_with(']') {
                        section = &trimmed[1..trimmed.len() - 1];
                        continue;
                    }
                    let parts: Vec<&str> = trimmed.split('=').collect();
                    if parts.len() == 2 {
                        let key = parts[0].trim();
                        let val = u32::from_str_radix(parts[1].trim().trim_start_matches("0x"), 16).unwrap_or(0);
                        match section {
                            "d3d8" => match key {
                                "present" => offsets.d3d8.present = val,
                                _ => {}
                            },
                            "d3d9" => match key {
                                "present" => offsets.d3d9.present = val,
                                "present_ex" => offsets.d3d9.present_ex = val,
                                "present_swap" => offsets.d3d9.present_swap = val,
                                "d3d9_clsoff" => offsets.d3d9.d3d9_clsoff = val,
                                "is_d3d9ex_clsoff" => offsets.d3d9.is_d3d9ex_clsoff = val,
                                _ => {}
                            },
                            "dxgi" => match key {
                                "present" => offsets.dxgi.present = val,
                                "present1" => offsets.dxgi.present1 = val,
                                "resize" => offsets.dxgi.resize = val,
                                "release" => offsets.dxgi2.release = val,
                                _ => {}
                            },
                            "d3d12" => match key {
                                "execute_command_lists" => offsets.d3d12.execute_command_lists = val,
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                }
            }
        }
        if offsets.dxgi.present == 0 {
            offsets.dxgi.present = 0x19960;
            offsets.dxgi.present1 = 0x19e00;
            offsets.dxgi.resize = 0x38530;
            offsets.dxgi2.release = 0x34460;
        }
        offsets
    })
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
unsafe fn open_shared_texture_on_best_device(
    tex_handle: u32,
) -> Result<(ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D)> {
    unsafe {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIAdapter};
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
        use windows::core::Interface;

        let handle = HANDLE(tex_handle as usize as *mut _);

        // 1. Enumerate adapters and prioritize discrete / NVIDIA GPU first
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            let mut i = 0;
            let mut adapters = Vec::new();
            while let Ok(adapter) = factory.EnumAdapters1(i) {
                i += 1;
                let is_dgpu = if let Ok(desc1) = adapter.GetDesc1() {
                    desc1.VendorId == 0x10DE || desc1.DedicatedVideoMemory > 1024 * 1024 * 1024
                } else {
                    false
                };
                adapters.push((adapter, is_dgpu));
            }
            adapters.sort_by_key(|(_, is_dgpu)| if *is_dgpu { 0 } else { 1 });

            for (adapter, _) in adapters {
                if let Ok(adapter0) = adapter.cast::<IDXGIAdapter>() {
                    let mut dev: Option<ID3D11Device> = None;
                    let mut ctx: Option<ID3D11DeviceContext> = None;
                    let hr = D3D11CreateDevice(
                        Some(&adapter0),
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut dev),
                        None,
                        Some(&mut ctx),
                    );
                    if hr.is_ok() {
                        if let (Some(d), Some(c)) = (dev, ctx) {
                            let mut shared_tex: Option<ID3D11Texture2D> = None;
                            if d.OpenSharedResource(handle, &mut shared_tex).is_ok() {
                                if let Some(tex) = shared_tex {
                                    return Ok((d, c, tex));
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Fallback to default hardware device
        let mut def_dev: Option<ID3D11Device> = None;
        let mut def_ctx: Option<ID3D11DeviceContext> = None;
        if D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut def_dev),
            None,
            Some(&mut def_ctx),
        ).is_ok() {
            if let (Some(d), Some(c)) = (def_dev, def_ctx) {
                let mut shared_tex: Option<ID3D11Texture2D> = None;
                if d.OpenSharedResource(handle, &mut shared_tex).is_ok() {
                    if let Some(tex) = shared_tex {
                        return Ok((d, c, tex));
                    }
                }
            }
        }

        bail!("Failed to open shared texture across GPU adapters (matching game GPU).");
    }
}

#[cfg(windows)]
impl GameCaptureSession {
    pub fn start(hwnd: HWND, paths: &crate::storage::AppPaths) -> Result<Self> {
        let mut pid: u32 = 0;
        let thread_id = unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid))
        };
        if pid == 0 {
            bail!("Could not determine target window process ID.");
        }
        if pid == std::process::id() {
            bail!("Cannot hook into MacroNest itself. Select the game window from the dropdown.");
        }

        let mut is_32bit = false;
        unsafe {
            use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, IsWow64Process};
            if let Ok(process_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut wow64 = windows::core::BOOL::from(false);
                if IsWow64Process(process_handle, &mut wow64).is_ok() {
                    is_32bit = wow64.as_bool();
                }
                let _ = CloseHandle(process_handle);
            }
        }

        let (hook_dll, inject_helper, offsets_exe) = find_game_capture_binaries(paths, is_32bit)
            .context("Game Capture binaries not found. Install Game Capture in Settings > Downloaded Tools.")?;

        let offsets = get_graphics_offsets(&offsets_exe, is_32bit);

        unsafe {
            use windows::Win32::Security::{
                InitializeSecurityDescriptor, SetSecurityDescriptorDacl, PSECURITY_DESCRIPTOR,
                SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
            };

            let mut sd = SECURITY_DESCRIPTOR::default();
            let _ = InitializeSecurityDescriptor(PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _), 1);
            let _ = SetSecurityDescriptorDacl(PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _), true, None, false);
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: &mut sd as *mut _ as *mut _,
                bInheritHandle: false.into(),
            };
            let sa_ptr = Some(&sa as *const _);

            let keepalive_name = to_wide(&format!("CaptureHook_KeepAlive{pid}"));
            let keepalive = CreateMutexW(sa_ptr, false, PCWSTR(keepalive_name.as_ptr()))?;

            let hook_init_name = to_wide(&format!("CaptureHook_Initialize{pid}"));
            let hook_init = CreateEventW(sa_ptr, false, false, PCWSTR(hook_init_name.as_ptr()))?;

            let hook_ready_name = to_wide(&format!("CaptureHook_HookReady{pid}"));
            let hook_ready = CreateEventW(sa_ptr, false, false, PCWSTR(hook_ready_name.as_ptr()))?;

            let hook_exit_name = to_wide(&format!("CaptureHook_Exit{pid}"));
            let hook_exit = CreateEventW(sa_ptr, false, false, PCWSTR(hook_exit_name.as_ptr()))?;

            let hook_restart_name = to_wide(&format!("CaptureHook_Restart{pid}"));
            let hook_restart = CreateEventW(sa_ptr, false, false, PCWSTR(hook_restart_name.as_ptr()))?;

            let hook_stop_name = to_wide(&format!("CaptureHook_Stop{pid}"));
            let hook_stop = CreateEventW(sa_ptr, false, false, PCWSTR(hook_stop_name.as_ptr()))?;

            let tex_m1_name = to_wide(&format!("CaptureHook_TextureMutex1{pid}"));
            let tex_m1 = CreateMutexW(sa_ptr, false, PCWSTR(tex_m1_name.as_ptr()))?;

            let tex_m2_name = to_wide(&format!("CaptureHook_TextureMutex2{pid}"));
            let tex_m2 = CreateMutexW(sa_ptr, false, PCWSTR(tex_m2_name.as_ptr()))?;

            let hook_info_name = to_wide(&format!("CaptureHook_HookInfo{pid}"));
            let hook_info_map = CreateFileMappingW(
                HANDLE(usize::MAX as *mut _),
                sa_ptr,
                PAGE_READWRITE,
                0,
                std::mem::size_of::<HookInfo>() as u32,
                PCWSTR(hook_info_name.as_ptr()),
            )?;

            let hook_info_view = MapViewOfFile(hook_info_map, FILE_MAP_ALL_ACCESS, 0, 0, std::mem::size_of::<HookInfo>());
            if hook_info_view.Value.is_null() {
                bail!("Failed to map hook info shared memory");
            }
            let hook_info_ptr = hook_info_view.Value as *mut HookInfo;

            std::ptr::write_bytes(hook_info_ptr, 0, 1);
            let hi = &mut *hook_info_ptr;
            hi.hook_ver_major = 1;
            hi.hook_ver_minor = 8;
            hi.capture_type = CaptureType::Texture;
            hi.window = hwnd.0 as usize as u32;
            hi.format = DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32;
            hi.offsets = offsets;
            hi.allow_srgb_alias = true;

            // Spawn inject-helper to inject graphics-hook into the game
            let inject_status = Command::new(&inject_helper)
                .creation_flags(0x0800_0000)
                .args([
                    hook_dll.to_str().unwrap_or_default(),
                    "0",
                    &pid.to_string(),
                ])
                .status();

            let mut inject_ok = match &inject_status {
                Ok(s) => s.success(),
                Err(_) => false,
            };

            if !inject_ok {
                // Try safe injection with thread_id (SetWindowsHookEx)
                if let Ok(safe_status) = Command::new(&inject_helper)
                    .creation_flags(0x0800_0000)
                    .args([
                        hook_dll.to_str().unwrap_or_default(),
                        "1",
                        &thread_id.to_string(),
                    ])
                    .status()
                {
                    inject_ok = safe_status.success();
                }
            }

            // Signal both hook_init (starts capture loop) and hook_restart (authorizes capture_should_init)
            let _ = SetEvent(hook_init);
            let _ = SetEvent(hook_restart);

            // Wait up to 10 seconds for the game hook to produce the first texture.
            // ponytail: Do NOT call SetEvent(hook_restart) in this wait loop. Spamming hook_restart
            // forces the game hook to repeatedly tear down and recreate DirectX capture resources on every frame,
            // causing severe lag in the game. Firing once above is sufficient.
            let start_wait = Instant::now();
            let mut ready = false;
            while start_wait.elapsed() < Duration::from_secs(10) {
                let wait_res = WaitForSingleObject(hook_ready, 50);
                if wait_res.0 == 0 {
                    ready = true;
                    break;
                }
                if !windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() {
                    bail!("Target game window closed.");
                }
            }
            if !ready {
                if let Ok(status) = &inject_status {
                    let code = status.code().unwrap_or(0);
                    if code == -3 || code == 253 || code == 0xFFFFFFFD_u32 as i32 {
                        bail!("Game process access denied. Please run MacroNest as Administrator to hook this game.");
                    }
                }
                bail!("Game hook did not signal ready within 10 seconds. Make sure the game is focused and actively rendering 3D graphics (DirectX 9/11/12 or OpenGL).");
            }

            // Read texture handle from CaptureHook_Texture shared memory
            let top_hwnd = windows::Win32::UI::WindowsAndMessaging::GetAncestor(hwnd, windows::Win32::UI::WindowsAndMessaging::GA_ROOT);
            let hi_map_id = (&*hook_info_ptr).map_id;
            let candidates = [
                format!("CaptureHook_Texture_{}_{}", top_hwnd.0 as usize, hi_map_id),
                format!("CaptureHook_Texture_{}_{}", hwnd.0 as usize, hi_map_id),
                format!("CaptureHook_Texture_{}_{}", top_hwnd.0 as usize, 1),
                format!("CaptureHook_Texture_{}_{}", hwnd.0 as usize, 1),
                format!("CaptureHook_Texture{pid}"),
            ];

            let mut shtex_opt: Option<ShtexData> = None;
            for candidate in &candidates {
                let name = to_wide(candidate);
                if let Ok(tex_map) = OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, PCWSTR(name.as_ptr())) {
                    let tex_view = MapViewOfFile(tex_map, FILE_MAP_ALL_ACCESS, 0, 0, std::mem::size_of::<ShtexData>());
                    if !tex_view.Value.is_null() {
                        let shtex = *(tex_view.Value as *const ShtexData);
                        let _ = UnmapViewOfFile(tex_view);
                        let _ = CloseHandle(tex_map);
                        if shtex.tex_handle != 0 {
                            shtex_opt = Some(shtex);
                            break;
                        }
                    } else {
                        let _ = CloseHandle(tex_map);
                    }
                }
            }

            let shtex = shtex_opt.context("Failed to open CaptureHook_Texture shared memory. Make sure the game is actively rendering.")?;

            // Open shared resource texture on best matching GPU device
            let (d3d_device, d3d_context, shared_texture) = open_shared_texture_on_best_device(shtex.tex_handle)?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            shared_texture.GetDesc(&mut desc);

            let mut nvenc = None;
            let dll_candidate = if paths.nvenc_dll.exists() {
                Some(paths.nvenc_dll.clone())
            } else if let Ok(exe_path) = std::env::current_exe() {
                let beside = exe_path.parent().unwrap_or(std::path::Path::new("")).join("nvenc_d3d11.dll");
                if beside.exists() {
                    Some(beside)
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(dll_path) = dll_candidate {
                let enc_w = desc.Width & !1;
                let enc_h = desc.Height & !1;
                match crate::nvenc::NvencHardwareEncoder::new(
                    &dll_path,
                    &d3d_device,
                    &shared_texture,
                    enc_w,
                    enc_h,
                    60,
                    15000,
                ) {
                    Ok(encoder) => {
                        let _ = std::fs::write(paths.root.join("game_capture.log"), "[MacroNest] NVENC hardware encoder active on NVIDIA GPU (100% VRAM zero-copy, 0% CPU)\n");
                        eprintln!("[MacroNest] NVENC hardware encoder active on GPU (100% VRAM zero-copy, 0% CPU)");
                        nvenc = Some(encoder);
                    }
                    Err(e) => {
                        let _ = std::fs::write(paths.root.join("game_capture.log"), format!("[MacroNest] NVENC init error: {e}\n"));
                        eprintln!("[MacroNest] NVENC init skipped: {e}. Using staging fallback.");
                    }
                }
            }

            Ok(Self {
                hwnd,
                pid,
                d3d_device,
                d3d_context,
                shared_texture: Some(shared_texture),
                staging_textures: None,
                write_idx: 0,
                copies_count: 0,
                hook_info_map,
                hook_info_ptr,
                hook_init,
                hook_ready,
                hook_exit,
                hook_restart,
                hook_stop,
                tex_mutexes: [tex_m1, tex_m2],
                keepalive,
                width: desc.Width,
                height: desc.Height,
                nvenc,
            })
        }
    }

    pub fn poll_into_buffer(&mut self, buffer: &mut Vec<u8>, expected_w: usize, expected_h: usize) -> Result<bool> {
        let Some(shared_tex) = &self.shared_texture else {
            return Ok(false);
        };

        unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            shared_tex.GetDesc(&mut desc);
            let width = desc.Width as usize;
            let height = desc.Height as usize;

            if expected_w > 0 && expected_h > 0 && (width != expected_w || height != expected_h) {
                return Ok(false);
            }

            if self.staging_textures.is_none() {
                let mut staging_desc = desc;
                staging_desc.Usage = D3D11_USAGE_STAGING;
                staging_desc.BindFlags = 0;
                staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                staging_desc.MiscFlags = 0;

                let mut s0 = None;
                let mut s1 = None;
                let mut s2 = None;
                self.d3d_device.CreateTexture2D(&staging_desc, None, Some(&mut s0))?;
                self.d3d_device.CreateTexture2D(&staging_desc, None, Some(&mut s1))?;
                self.d3d_device.CreateTexture2D(&staging_desc, None, Some(&mut s2))?;
                self.staging_textures = Some(([s0.unwrap(), s1.unwrap(), s2.unwrap()], desc.Width, desc.Height));
                self.write_idx = 0;
                self.copies_count = 0;
            }

            let (staging_textures, _, _) = self.staging_textures.as_ref().unwrap();
            let write_idx = self.write_idx;

            // Copy directly on GPU VRAM from shared game texture
            self.d3d_context.CopyResource(&staging_textures[write_idx], shared_tex);
            self.d3d_context.Flush();

            self.copies_count += 1;
            let read_idx = if self.copies_count >= 3 {
                (write_idx + 1) % 3
            } else {
                write_idx
            };

            let read_tex = &staging_textures[read_idx];
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.d3d_context.Map(read_tex, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

            let pitch = mapped.RowPitch as usize;
            let row_bytes = width * 4;
            let total_bytes = row_bytes * height;
            let src_ptr = mapped.pData as *const u8;

            buffer.resize(total_bytes, 0);
            let dst_ptr = buffer.as_mut_ptr();

            if pitch == row_bytes {
                std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, total_bytes);
            } else {
                for y in 0..height {
                    std::ptr::copy_nonoverlapping(src_ptr.add(y * pitch), dst_ptr.add(y * row_bytes), row_bytes);
                }
            }

            self.d3d_context.Unmap(read_tex, 0);
            self.write_idx = (write_idx + 1) % 3;

            Ok(true)
        }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn has_nvenc(&self) -> bool {
        self.nvenc.is_some()
    }

    pub fn poll_encoded_frame(&mut self, force_idr: bool) -> Result<Option<&'static [u8]>> {
        if let (Some(encoder), Some(shared_tex)) = (&self.nvenc, &self.shared_texture) {
            unsafe {
                let _ = WaitForSingleObject(self.hook_ready, 0);
                let packet = encoder.encode_frame(shared_tex, force_idr)?;
                Ok(Some(packet))
            }
        } else {
            bail!("NVENC hardware encoder not initialized");
        }
    }
}
