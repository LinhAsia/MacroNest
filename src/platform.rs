#[cfg(windows)]
mod windows_platform {
    use std::{
        env,
        path::{Path, PathBuf},
        process::{Command, Output},
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };

    use anyhow::{Context, Result, bail};
    use eframe::Frame;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::{
        Win32::{
            Foundation::{CloseHandle, GetLastError, HANDLE, HWND},
            Graphics::Dwm::{
                DWMNCRP_ENABLED, DWMNCRP_USEWINDOWSTYLE, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
                DWMWA_NCRENDERING_POLICY, DWMWA_TRANSITIONS_FORCEDISABLED,
                DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND,
                DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
            },
            System::Threading::{
                CreateMutexW, GetCurrentProcess, GetCurrentThreadId, HIGH_PRIORITY_CLASS,
                SetPriorityClass,
            },
            System::{
                DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
                Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock},
            },
            UI::{
                Controls::MARGINS,
                Shell::{DROPFILES, IsUserAnAdmin, ShellExecuteW},
                WindowsAndMessaging::{
                    BringWindowToTop, EnumThreadWindows, FindWindowExW, FindWindowW, GWL_EXSTYLE,
                    GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, HWND_NOTOPMOST,
                    HWND_TOPMOST, IsWindowVisible, SW_HIDE, SW_RESTORE, SW_SHOWNA, SW_SHOWNORMAL,
                    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
                    SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow,
                    WS_EX_NOACTIVATE,
                },
            },
        },
        core::{PCWSTR, w},
    };

    const MUTEX_NAME: &str = "Local\\MacroNestSingleInstance_v3";
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    static INTERCEPTION_DRIVER_INSTALLED_CACHE: OnceLock<Mutex<Option<(Instant, bool)>>> =
        OnceLock::new();

    fn spawn_popup_arg(arg: &str) {
        if let Ok(exe) = env::current_exe() {
            let exe_wide = widestring(exe.as_os_str().to_string_lossy().as_ref());
            let arg_wide = widestring(arg);
            unsafe {
                let _ = ShellExecuteW(
                    Some(HWND(std::ptr::null_mut())),
                    w!("open"),
                    PCWSTR(exe_wide.as_ptr()),
                    PCWSTR(arg_wide.as_ptr()),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                );
            }
        }
    }

    struct SendHandle(HANDLE);
    unsafe impl Send for SendHandle {}
    unsafe impl Sync for SendHandle {}

    static SINGLE_INSTANCE_HANDLE: OnceLock<std::sync::Mutex<Option<SendHandle>>> = OnceLock::new();

    pub struct SingleInstanceGuard {
        handle: HANDLE,
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            release_single_instance();
        }
    }

    pub fn release_single_instance() {
        if let Some(cell) = SINGLE_INSTANCE_HANDLE.get() {
            if let Ok(mut lock) = cell.lock() {
                if let Some(send_handle) = lock.take() {
                    unsafe {
                        let _ = CloseHandle(send_handle.0);
                    }
                }
            }
        }
    }

    pub fn acquire_single_instance() -> Result<Option<SingleInstanceGuard>> {
        let name = widestring(MUTEX_NAME);
        let err_before = unsafe { GetLastError().0 };
        unsafe {
            windows::Win32::Foundation::SetLastError(windows::Win32::Foundation::WIN32_ERROR(0));
        }
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr()))? };
        let err_after = unsafe { GetLastError().0 };

        let already_exists = err_after == windows::Win32::Foundation::ERROR_ALREADY_EXISTS.0;
        if already_exists {
            spawn_popup_arg("--already-running-popup");
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Ok(None);
        }

        let cell = SINGLE_INSTANCE_HANDLE.get_or_init(|| std::sync::Mutex::new(None));
        if let Ok(mut lock) = cell.lock() {
            *lock = Some(SendHandle(handle));
        }

        Ok(Some(SingleInstanceGuard { handle }))
    }

    pub fn disable_power_throttling() {
        unsafe {
            #[repr(C)]
            struct ProcessPowerThrottlingState {
                version: u32,
                control_mask: u32,
                state_mask: u32,
            }
            const PROCESS_POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
            const PROCESS_POWER_THROTTLING_EXECUTION_SPEED: u32 = 0x1;
            const PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION: u32 = 0x4;
            const PROCESS_POWER_THROTTLING: i32 = 4;

            let state = ProcessPowerThrottlingState {
                version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                    | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
                state_mask: 0,
            };

            if let Ok(kernel32) =
                windows::Win32::System::LibraryLoader::GetModuleHandleW(w!("kernel32.dll"))
            {
                type SetProcessInformationFn = unsafe extern "system" fn(
                    windows::Win32::Foundation::HANDLE,
                    i32,
                    *const std::ffi::c_void,
                    u32,
                ) -> i32;

                if let Some(func) = windows::Win32::System::LibraryLoader::GetProcAddress(
                    kernel32,
                    windows::core::s!("SetProcessInformation"),
                ) {
                    let set_proc_info: SetProcessInformationFn = std::mem::transmute(func);
                    let _ = set_proc_info(
                        GetCurrentProcess(),
                        PROCESS_POWER_THROTTLING,
                        &state as *const _ as *const std::ffi::c_void,
                        std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
                    );
                }
            }
        }
    }

    pub fn set_current_thread_high_priority() {
        // Set worker threads to below normal priority so heavy scanning never starves game or UI threads.
        unsafe {
            use windows::Win32::System::Threading::{
                GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
            };
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
    }

    pub fn set_high_priority() {
        unsafe {
            let _ = SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS);
        }
        disable_power_throttling();
    }

    pub fn relaunch_as_admin_if_needed() -> Result<bool> {
        unsafe {
            if IsUserAnAdmin().as_bool() {
                return Ok(false);
            }
        }

        let exe = env::current_exe()?;
        let exe_wide = widestring(exe.as_os_str().to_string_lossy().as_ref());
        let startup_arg = env::args_os()
            .any(|arg| arg == "--start-in-tray")
            .then(|| widestring("--start-in-tray"));
        unsafe {
            let result = ShellExecuteW(
                Some(HWND(std::ptr::null_mut())),
                w!("runas"),
                PCWSTR(exe_wide.as_ptr()),
                startup_arg
                    .as_ref()
                    .map(|arg| PCWSTR(arg.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            if (result.0 as usize) <= 32 {
                bail!("Administrator elevation was cancelled or failed");
            }
        }
        Ok(true)
    }

    pub fn is_running_as_admin() -> bool {
        unsafe { IsUserAnAdmin().as_bool() }
    }

    pub fn launch_process_as_admin(executable: &Path, arguments: Option<&str>) -> Result<()> {
        launch_process_as_admin_with_show(executable, arguments, SW_SHOWNORMAL)
    }

    pub fn run_hidden_process_as_admin_and_wait(
        executable: &Path,
        arguments: Option<&str>,
        timeout_ms: u32,
    ) -> Result<u32> {
        run_process_as_admin_and_wait_with_show(executable, arguments, SW_HIDE, timeout_ms)
    }

    fn launch_process_as_admin_with_show(
        executable: &Path,
        arguments: Option<&str>,
        show_command: windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD,
    ) -> Result<()> {
        let exe_wide = widestring(executable.as_os_str().to_string_lossy().as_ref());
        let args_wide = arguments.map(widestring);
        let dir_wide = executable
            .parent()
            .map(|dir| widestring(dir.as_os_str().to_string_lossy().as_ref()));
        unsafe {
            let result = ShellExecuteW(
                Some(HWND(std::ptr::null_mut())),
                w!("runas"),
                PCWSTR(exe_wide.as_ptr()),
                args_wide
                    .as_ref()
                    .map(|s| PCWSTR(s.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                dir_wide
                    .as_ref()
                    .map(|s| PCWSTR(s.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                show_command,
            );
            if (result.0 as usize) <= 32 {
                bail!("Administrator elevation was cancelled or failed");
            }
        }
        Ok(())
    }

    fn run_process_as_admin_and_wait_with_show(
        executable: &Path,
        arguments: Option<&str>,
        show_command: windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD,
        timeout_ms: u32,
    ) -> Result<u32> {
        let exe = powershell_single_quote(executable.as_os_str().to_string_lossy().as_ref());
        let args = powershell_single_quote(arguments.unwrap_or_default());
        let window_style = if show_command == SW_HIDE {
            "Hidden"
        } else {
            "Normal"
        };
        let command = format!(
            "$p = Start-Process -FilePath {exe} -ArgumentList {args} -Verb RunAs -WindowStyle {window_style} -PassThru; if (-not $p) {{ exit 1 }}; if (-not $p.WaitForExit({timeout_ms})) {{ try {{ $p.Kill() }} catch {{}}; exit 124 }}; exit $p.ExitCode"
        );
        let mut powershell = Command::new("powershell");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            powershell.creation_flags(CREATE_NO_WINDOW);
        }
        let status = powershell
            .args(["-NoProfile", "-NonInteractive", "-Command", &command])
            .status()?;
        match status.code() {
            Some(code) => Ok(code as u32),
            None => bail!("Elevated process helper terminated without an exit code"),
        }
    }

    fn hidden_command_output(command: &mut Command) -> std::io::Result<Output> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        command.output()
    }

    fn registry_key_exists(key_path: &str) -> bool {
        let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        let reg_exe = Path::new(&system_root).join("System32").join("reg.exe");
        let mut command = Command::new(reg_exe);
        command.args(["query", key_path]);
        hidden_command_output(&mut command)
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn registry_value_contains_token(key_path: &str, value_name: &str, token: &str) -> bool {
        let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        let reg_exe = Path::new(&system_root).join("System32").join("reg.exe");
        let mut command = Command::new(reg_exe);
        command.args(["query", key_path, "/v", value_name]);
        let Ok(output) = hidden_command_output(&mut command) else {
            return false;
        };
        if !output.status.success() {
            return false;
        }

        let token = token.to_ascii_lowercase();
        let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        let cleaned = stdout.replace("\\0", " ").replace('\0', " ");
        cleaned.split_whitespace().any(|part| part == token)
    }

    fn detect_interception_driver_installed() -> bool {
        let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        let drivers_dir = Path::new(&system_root).join("System32").join("drivers");

        let legacy_driver = drivers_dir.join("interception.sys");
        if legacy_driver.exists() {
            return true;
        }

        // Interception registers keyboard/mouse upper-filter services and rewrites
        // the class UpperFilters entries during install. These markers disappear
        // after uninstall even before a reboot fully unloads the drivers.
        registry_key_exists("HKLM\\SYSTEM\\CurrentControlSet\\Services\\keyboard")
            || registry_key_exists("HKLM\\SYSTEM\\CurrentControlSet\\Services\\mouse")
            || registry_value_contains_token(
                "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4D36E96B-E325-11CE-BFC1-08002BE10318}",
                "UpperFilters",
                "keyboard",
            )
            || registry_value_contains_token(
                "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4D36E96F-E325-11CE-BFC1-08002BE10318}",
                "UpperFilters",
                "mouse",
            )
    }

    pub fn is_interception_driver_installed() -> bool {
        let cache = INTERCEPTION_DRIVER_INSTALLED_CACHE.get_or_init(|| Mutex::new(None));
        {
            let guard = cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some((checked_at, installed)) = *guard
                && checked_at.elapsed() < Duration::from_secs(5)
            {
                return installed;
            }
        }

        let installed = detect_interception_driver_installed();
        let mut guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some((Instant::now(), installed));
        installed
    }

    pub fn get_system_uptime() -> std::time::Duration {
        // Approximate: use wmic to get LastBootUpTime and compare to now.
        // If we can't determine it, return Duration::ZERO (safe fallback — won't clear the marker).
        let Ok(output) = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "(Get-Date) - (Get-CimInstance Win32_OperatingSystem).LastBootUpTime | Select-Object -ExpandProperty TotalMilliseconds"])
            .output()
        else {
            return std::time::Duration::ZERO;
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let ms: f64 = stdout.trim().parse().unwrap_or(0.0);
        std::time::Duration::from_millis(ms as u64)
    }

    pub fn restart_windows() -> Result<()> {
        let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        let shutdown = Path::new(&system_root)
            .join("System32")
            .join("shutdown.exe");
        launch_process_as_admin(&shutdown, Some("/r /t 0"))
    }

    pub fn set_native_window_shadow(frame: &Frame, enabled: bool) -> bool {
        let Ok(window_handle) = frame.window_handle() else {
            return false;
        };
        let hwnd = match window_handle.as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
            _ => return false,
        };

        unsafe {
            let policy = if enabled {
                DWMNCRP_ENABLED
            } else {
                DWMNCRP_USEWINDOWSTYLE
            };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_NCRENDERING_POLICY,
                &policy as *const _ as *const _,
                std::mem::size_of_val(&policy) as u32,
            );

            let corner = if enabled {
                DWMWCP_ROUND
            } else {
                DWMWCP_DONOTROUND
            };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const _,
                std::mem::size_of_val(&corner) as u32,
            );

            let border_color = DWMWA_COLOR_NONE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &border_color as *const _ as *const _,
                std::mem::size_of_val(&border_color) as u32,
            );

            let margins = if enabled {
                MARGINS {
                    cxLeftWidth: -1,
                    cxRightWidth: -1,
                    cyTopHeight: -1,
                    cyBottomHeight: -1,
                }
            } else {
                MARGINS {
                    cxLeftWidth: 0,
                    cxRightWidth: 0,
                    cyTopHeight: 0,
                    cyBottomHeight: 0,
                }
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        }
        true
    }

    pub fn set_native_window_transitions_disabled(frame: &Frame, disabled: bool) -> bool {
        let Ok(window_handle) = frame.window_handle() else {
            return false;
        };
        let hwnd = match window_handle.as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
            _ => return false,
        };

        unsafe {
            let disabled = i32::from(disabled);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                &disabled as *const _ as *const _,
                std::mem::size_of_val(&disabled) as u32,
            );
        }
        true
    }

    pub fn bring_native_window_to_front(frame: &Frame) {
        let Ok(window_handle) = frame.window_handle() else {
            return;
        };
        let hwnd = match window_handle.as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
            _ => return,
        };

        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_NOTOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    pub fn make_frame_no_activate(frame: &Frame) -> bool {
        let Ok(window_handle) = frame.window_handle() else {
            return false;
        };
        let hwnd = match window_handle.as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
            _ => return false,
        };
        make_hwnd_no_activate(hwnd)
    }

    static MAIN_HWND: parking_lot::Mutex<Option<isize>> = parking_lot::Mutex::new(None);
    static RECORDING_HICON: parking_lot::Mutex<Option<isize>> = parking_lot::Mutex::new(None);

    pub fn get_main_hwnd() -> Option<HWND> {
        let raw = (*MAIN_HWND.lock())?;
        if raw != 0 {
            let hwnd = HWND(raw as *mut std::ffi::c_void);
            if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() } {
                return Some(hwnd);
            }
        }
        None
    }

    pub fn cache_main_hwnd(frame: &Frame) {
        if let Ok(window_handle) = frame.window_handle() {
            if let RawWindowHandle::Win32(handle) = window_handle.as_raw() {
                let hwnd_val = handle.hwnd.get() as isize;
                *MAIN_HWND.lock() = Some(hwnd_val);
                crate::overlay::set_cached_app_ui_hwnd(hwnd_val);
            }
        }
    }

    pub fn create_hicon_from_rgba(
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::UI::WindowsAndMessaging::*;

        unsafe {
            let mut bgra = Vec::with_capacity(rgba.len());
            for chunk in rgba.chunks_exact(4) {
                bgra.push(chunk[2]);
                bgra.push(chunk[1]);
                bgra.push(chunk[0]);
                bgra.push(chunk[3]);
            }

            let hdc = GetDC(None);
            if hdc.0.is_null() {
                return None;
            }

            let bi = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };

            let mut ppv_bits = std::ptr::null_mut();
            let hbm_color = CreateDIBSection(
                Some(hdc),
                &bi as *const _ as *const _,
                DIB_RGB_COLORS,
                &mut ppv_bits,
                None,
                0,
            )
            .ok()?;

            if !ppv_bits.is_null() {
                std::ptr::copy_nonoverlapping(bgra.as_ptr(), ppv_bits as *mut u8, bgra.len());
            }

            let hbm_mask = CreateBitmap(width as i32, height as i32, 1, 1, None);

            let icon_info = ICONINFO {
                fIcon: true.into(),
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: hbm_mask,
                hbmColor: hbm_color,
            };

            let hicon = CreateIconIndirect(&icon_info).ok();

            let _ = DeleteObject(hbm_color.into());
            let _ = DeleteObject(hbm_mask.into());
            let _ = ReleaseDC(None, hdc);

            hicon
        }
    }

    pub fn update_native_taskbar_recording_state(is_recording: bool) {
        use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
        use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
        use windows::Win32::UI::WindowsAndMessaging::HICON;
        use windows::core::w;

        let Some(raw_hwnd) = *MAIN_HWND.lock() else {
            return;
        };
        let hwnd = HWND(raw_hwnd as *mut _);

        unsafe {
            if let Ok(taskbar) =
                CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
            {
                let _ = taskbar.HrInit();
                if is_recording {
                    if RECORDING_HICON.lock().is_none() {
                        if let Ok(icon_data) = crate::app_icon::recording_overlay_badge_icon_data(24) {
                            if let Some(hicon) = create_hicon_from_rgba(
                                icon_data.width,
                                icon_data.height,
                                &icon_data.rgba,
                            ) {
                                *RECORDING_HICON.lock() = Some(hicon.0 as isize);
                            }
                        }
                    }
                    if let Some(raw_hicon) = *RECORDING_HICON.lock() {
                        let hicon = HICON(raw_hicon as *mut _);
                        let _ = taskbar.SetOverlayIcon(hwnd, hicon, w!("Recording"));
                    }
                } else {
                    let _ = taskbar.SetOverlayIcon(
                        hwnd,
                        HICON(std::ptr::null_mut()),
                        windows::core::PCWSTR::null(),
                    );
                }
            }
        }
    }

    pub fn make_hwnd_no_activate(hwnd: HWND) -> bool {
        unsafe {
            let mut style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if style & WS_EX_NOACTIVATE.0 == 0 {
                style |= WS_EX_NOACTIVATE.0;
                let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, style as i32);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
                );
            }
        }
        true
    }

    struct EnumThreadWindowsCtx {
        target_title: String,
        found_hwnd: Option<HWND>,
    }

    unsafe extern "system" fn enum_thread_windows_proc(
        hwnd: HWND,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::core::BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut EnumThreadWindowsCtx) };
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            let actual_len = unsafe { GetWindowTextW(hwnd, &mut buf) };
            if actual_len > 0 {
                let title = String::from_utf16_lossy(&buf[..actual_len as usize]);
                if title == ctx.target_title {
                    ctx.found_hwnd = Some(hwnd);
                    return windows::core::BOOL::from(false); // Stop enumeration
                }
            }
        }
        windows::core::BOOL::from(true) // Continue enumeration
    }

    /// Apply WS_EX_NOACTIVATE to a window found by its title.
    /// Returns true if the window was found and the style applied.
    pub fn make_window_title_no_activate(title: &str) -> bool {
        let mut ctx = EnumThreadWindowsCtx {
            target_title: title.to_string(),
            found_hwnd: None,
        };
        unsafe {
            let thread_id = GetCurrentThreadId();
            let _ = EnumThreadWindows(
                thread_id,
                Some(enum_thread_windows_proc),
                windows::Win32::Foundation::LPARAM(&mut ctx as *mut _ as isize),
            );
        }
        if let Some(hwnd) = ctx.found_hwnd {
            make_hwnd_no_activate(hwnd)
        } else {
            // Fallback to FindWindowW if thread enumeration fails or doesn't find it yet
            let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
            let hwnd = unsafe {
                FindWindowW(PCWSTR::null(), PCWSTR(title_wide.as_ptr()))
                    .unwrap_or(HWND(std::ptr::null_mut()))
            };
            if hwnd.0.is_null() {
                return false;
            }
            make_hwnd_no_activate(hwnd)
        }
    }

    fn taskbar_windows() -> Vec<HWND> {
        let mut windows = Vec::new();
        unsafe {
            let primary = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null())
                .unwrap_or(HWND(std::ptr::null_mut()));
            if !primary.0.is_null() {
                windows.push(primary);
            }

            let mut previous = HWND(std::ptr::null_mut());
            loop {
                let next = FindWindowExW(
                    None,
                    Some(previous),
                    w!("Shell_SecondaryTrayWnd"),
                    PCWSTR::null(),
                )
                .unwrap_or(HWND(std::ptr::null_mut()));
                if next.0.is_null() {
                    break;
                }
                windows.push(next);
                previous = next;
            }
        }
        windows
    }

    pub fn hide_taskbar() -> bool {
        let windows = taskbar_windows();
        if windows.is_empty() {
            return false;
        }
        for hwnd in windows {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
        true
    }

    pub fn show_taskbar() -> bool {
        let windows = taskbar_windows();
        if windows.is_empty() {
            return false;
        }
        for hwnd in windows {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNA);
            }
        }
        true
    }

    pub fn is_taskbar_hidden() -> bool {
        let windows = taskbar_windows();
        !windows.is_empty()
            && windows
                .iter()
                .all(|hwnd| unsafe { !IsWindowVisible(*hwnd).as_bool() })
    }

    pub fn open_folder_in_explorer(path: &Path) -> Result<()> {
        if !path.exists() {
            bail!("Folder does not exist: {}", path.display());
        }

        let path_wide = widestring(path.as_os_str().to_string_lossy().as_ref());
        unsafe {
            let result = ShellExecuteW(
                Some(HWND(std::ptr::null_mut())),
                w!("open"),
                PCWSTR(path_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            if (result.0 as usize) <= 32 {
                bail!("Failed to open folder: {}", path.display());
            }
        }
        Ok(())
    }

    pub fn reveal_file_in_explorer(path: &Path) -> Result<()> {
        if !path.is_file() {
            bail!("File does not exist: {}", path.display());
        }
        Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .with_context(|| format!("Failed to reveal {}", path.display()))?;
        Ok(())
    }

    pub fn open_file(path: &Path) -> Result<()> {
        if !path.is_file() {
            bail!("File does not exist: {}", path.display());
        }
        let path_wide = widestring(path.as_os_str().to_string_lossy().as_ref());
        unsafe {
            let result = ShellExecuteW(
                Some(HWND(std::ptr::null_mut())),
                w!("open"),
                PCWSTR(path_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            if (result.0 as usize) <= 32 {
                bail!("Failed to open file: {}", path.display());
            }
        }
        Ok(())
    }

    pub fn open_url_in_browser(url: &str) -> Result<()> {
        let url_wide = widestring(url);
        unsafe {
            let result = ShellExecuteW(
                Some(HWND(std::ptr::null_mut())),
                w!("open"),
                PCWSTR(url_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            if (result.0 as usize) <= 32 {
                bail!("Failed to open URL: {url}");
            }
        }
        Ok(())
    }

    pub fn copy_folder_to_clipboard(path: &Path) -> Result<()> {
        copy_paths_to_clipboard(&[path.to_path_buf()])
    }

    pub fn copy_paths_to_clipboard(paths: &[PathBuf]) -> Result<()> {
        if paths.is_empty() {
            bail!("No files or folders to copy");
        }
        for path in paths {
            if !path.exists() {
                bail!("Path does not exist: {}", path.display());
            }
        }

        let text = paths
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\r\n");
        let text_wide = widestring(&text);
        let mut file_list = Vec::new();
        for path in paths {
            file_list.extend(widestring(path.to_string_lossy().as_ref()));
        }
        file_list.push(0);

        unsafe {
            OpenClipboard(None)?;
            let _ = EmptyClipboard();

            // 1. Set text clipboard format (CF_UNICODETEXT = 13)
            let text_bytes = text_wide.len() * 2;
            if let Ok(h_text) = GlobalAlloc(GHND, text_bytes) {
                let p_text = GlobalLock(h_text);
                if !p_text.is_null() {
                    std::ptr::copy_nonoverlapping(
                        text_wide.as_ptr() as *const u8,
                        p_text as *mut u8,
                        text_bytes,
                    );
                    let _ = GlobalUnlock(h_text);
                    let _ = SetClipboardData(13, Some(HANDLE(h_text.0 as *mut _)));
                }
            }

            // 2. Set file drop clipboard format (CF_HDROP = 15)
            let dropfiles_size = std::mem::size_of::<DROPFILES>();
            let total_size = dropfiles_size + file_list.len() * 2;

            if let Ok(h_drop) = GlobalAlloc(GHND, total_size) {
                let p_drop = GlobalLock(h_drop);
                if !p_drop.is_null() {
                    let dropfiles = DROPFILES {
                        pFiles: dropfiles_size as u32,
                        pt: windows::Win32::Foundation::POINT { x: 0, y: 0 },
                        fNC: windows::core::BOOL::from(false),
                        fWide: windows::core::BOOL::from(true),
                    };

                    std::ptr::copy_nonoverlapping(
                        &dropfiles as *const DROPFILES as *const u8,
                        p_drop as *mut u8,
                        dropfiles_size,
                    );

                    std::ptr::copy_nonoverlapping(
                        file_list.as_ptr() as *const u8,
                        (p_drop as usize + dropfiles_size) as *mut u8,
                        file_list.len() * 2,
                    );

                    let _ = GlobalUnlock(h_drop);
                    let _ = SetClipboardData(15, Some(HANDLE(h_drop.0 as *mut _)));
                }
            }

            let _ = CloseClipboard();
        }

        Ok(())
    }

    fn widestring(value: &str) -> Vec<u16> {
        let mut wide: Vec<u16> = value.encode_utf16().collect();
        wide.push(0);
        wide
    }

    fn powershell_single_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    pub fn trim_working_set() {
        unsafe {
            use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
            let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
        }
    }
}

#[cfg(windows)]
pub use windows_platform::*;

#[cfg(not(windows))]
mod fallback {
    use anyhow::Result;
    use eframe::Frame;

    pub struct SingleInstanceGuard;

    pub fn relaunch_as_admin_if_needed() -> Result<bool> {
        Ok(false)
    }

    pub fn is_running_as_admin() -> bool {
        false
    }

    pub fn acquire_single_instance() -> Result<Option<SingleInstanceGuard>> {
        Ok(Some(SingleInstanceGuard))
    }

    pub fn release_single_instance() {}

    pub fn set_high_priority() {}

    pub fn set_current_thread_high_priority() {}

    pub fn disable_power_throttling() {}

    pub fn set_native_window_shadow(_frame: &Frame, _enabled: bool) -> bool {
        true
    }

    pub fn run_hidden_process_as_admin_and_wait(
        _executable: &std::path::Path,
        _arguments: Option<&str>,
        _timeout_ms: u32,
    ) -> Result<u32> {
        Ok(0)
    }

    pub fn set_native_window_transitions_disabled(_frame: &Frame, _disabled: bool) -> bool {
        true
    }

    pub fn open_folder_in_explorer(_path: &std::path::Path) -> Result<()> {
        Ok(())
    }

    pub fn reveal_file_in_explorer(_path: &std::path::Path) -> Result<()> {
        Ok(())
    }

    pub fn open_file(_path: &std::path::Path) -> Result<()> {
        Ok(())
    }

    pub fn open_url_in_browser(_url: &str) -> Result<()> {
        Ok(())
    }

    pub fn copy_folder_to_clipboard(_path: &std::path::Path) -> Result<()> {
        Ok(())
    }

    pub fn copy_paths_to_clipboard(_paths: &[std::path::PathBuf]) -> Result<()> {
        Ok(())
    }

    pub fn hide_taskbar() -> bool {
        false
    }
    pub fn show_taskbar() -> bool {
        false
    }
    pub fn is_taskbar_hidden() -> bool {
        false
    }

    pub fn get_system_uptime() -> std::time::Duration {
        std::time::Duration::ZERO
    }

    pub fn trim_working_set() {}
}

#[cfg(not(windows))]
pub use fallback::*;
