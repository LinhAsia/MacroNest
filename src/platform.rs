#[cfg(windows)]
mod windows_platform {
    use std::{env, path::Path, process::Command};

    use anyhow::{Result, bail};
    use eframe::Frame;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::{
        Win32::{
            Foundation::{CloseHandle, GetLastError, HANDLE, HWND},
            Graphics::Dwm::{
                DWMNCRP_ENABLED, DWMNCRP_USEWINDOWSTYLE, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
                DWMWA_NCRENDERING_POLICY, DWMWA_TRANSITIONS_FORCEDISABLED,
                DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
                DwmSetWindowAttribute,
            },
            System::Threading::{
                CreateMutexW, GetCurrentProcess, HIGH_PRIORITY_CLASS, SetPriorityClass,
            },
            System::{
                DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
                Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock},
            },
            UI::{
                Controls::MARGINS,
                Shell::{DROPFILES, IsUserAnAdmin, ShellExecuteW},
                WindowsAndMessaging::{
                    BringWindowToTop, FindWindowExW, FindWindowW, HWND_NOTOPMOST, HWND_TOPMOST,
                    IsWindowVisible, SW_HIDE, SW_RESTORE, SW_SHOWNA, SW_SHOWNORMAL, SWP_NOMOVE,
                    SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
                },
            },
        },
        core::{PCWSTR, w},
    };

    const MUTEX_NAME: &str = "Global\\CrosshairOverlaySingleInstance_v2";
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

    pub struct SingleInstanceGuard {
        handle: HANDLE,
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.handle);
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

        Ok(Some(SingleInstanceGuard { handle }))
    }

    pub fn set_high_priority() {
        unsafe {
            let _ = SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS);
        }
    }

    pub fn relaunch_as_admin_if_needed() -> Result<bool> {
        unsafe {
            if IsUserAnAdmin().as_bool() {
                return Ok(false);
            }
        }

        let exe = env::current_exe()?;
        let exe_wide = widestring(exe.as_os_str().to_string_lossy().as_ref());
        unsafe {
            let result = ShellExecuteW(
                Some(HWND(std::ptr::null_mut())),
                w!("runas"),
                PCWSTR(exe_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            if (result.0 as usize) <= 32 {
                bail!("Administrator elevation was cancelled or failed");
            }
        }
        Ok(true)
    }

    pub fn launch_process_as_admin(executable: &Path, arguments: Option<&str>) -> Result<()> {
        launch_process_as_admin_with_show(executable, arguments, SW_SHOWNORMAL)
    }

    pub fn launch_hidden_process_as_admin(
        executable: &Path,
        arguments: Option<&str>,
    ) -> Result<()> {
        launch_process_as_admin_with_show(executable, arguments, SW_HIDE)
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
            powershell.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let status = powershell
            .args(["-NoProfile", "-NonInteractive", "-Command", &command])
            .status()?;
        match status.code() {
            Some(code) => Ok(code as u32),
            None => bail!("Elevated process helper terminated without an exit code"),
        }
    }

    fn registry_key_exists(key_path: &str) -> bool {
        let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        let reg_exe = Path::new(&system_root)
            .join("System32")
            .join("reg.exe");
        Command::new(reg_exe)
            .args(["query", key_path])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn registry_value_contains_token(key_path: &str, value_name: &str, token: &str) -> bool {
        let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        let reg_exe = Path::new(&system_root)
            .join("System32")
            .join("reg.exe");
        let Ok(output) = Command::new(reg_exe)
            .args(["query", key_path, "/v", value_name])
            .output()
        else {
            return false;
        };
        if !output.status.success() {
            return false;
        }

        let token = token.to_ascii_lowercase();
        let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        let cleaned = stdout.replace("\\0", " ").replace('\0', " ");
        cleaned
            .split_whitespace()
            .any(|part| part == token)
    }

    pub fn is_interception_driver_installed() -> bool {
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

            let corner = DWMWCP_ROUND;
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
        if !path.exists() {
            bail!("Folder does not exist: {}", path.display());
        }

        let path_str = path.to_string_lossy().to_string();
        let path_wide = widestring(&path_str);

        unsafe {
            OpenClipboard(None)?;
            let _ = EmptyClipboard();

            // 1. Set text clipboard format (CF_UNICODETEXT = 13)
            let text_bytes = path_wide.len() * 2;
            if let Ok(h_text) = GlobalAlloc(GHND, text_bytes) {
                let p_text = GlobalLock(h_text);
                if !p_text.is_null() {
                    std::ptr::copy_nonoverlapping(
                        path_wide.as_ptr() as *const u8,
                        p_text as *mut u8,
                        text_bytes,
                    );
                    let _ = GlobalUnlock(h_text);
                    let _ = SetClipboardData(13, Some(HANDLE(h_text.0 as *mut _)));
                }
            }

            // 2. Set file drop clipboard format (CF_HDROP = 15)
            let mut file_list = path_wide.clone();
            file_list.push(0); // double-null terminator

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

    pub fn acquire_single_instance() -> Result<Option<SingleInstanceGuard>> {
        Ok(Some(SingleInstanceGuard))
    }

    pub fn set_high_priority() {}

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

    pub fn open_url_in_browser(_url: &str) -> Result<()> {
        Ok(())
    }

    pub fn copy_folder_to_clipboard(_path: &std::path::Path) -> Result<()> {
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
}

#[cfg(not(windows))]
pub use fallback::*;
