#[cfg(windows)]
mod windows_platform {
    use std::{env, path::Path};

    use anyhow::{Result, bail};
    use eframe::Frame;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::{
        Win32::{
            Foundation::{CloseHandle, GetLastError, HANDLE, HWND},
            Graphics::Dwm::{
                DWMNCRP_ENABLED, DWMNCRP_USEWINDOWSTYLE, DWMWA_BORDER_COLOR,
                DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
                DwmSetWindowAttribute, DwmExtendFrameIntoClientArea,
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
                Shell::{
                    DROPFILES, IsUserAnAdmin, ShellExecuteW,
                },
                WindowsAndMessaging::{
                    BringWindowToTop, FindWindowExW, FindWindowW, HWND_NOTOPMOST, HWND_TOPMOST,
                    IsWindowVisible, SW_HIDE, SW_RESTORE, SW_SHOWNA, SW_SHOWNORMAL, SWP_NOMOVE,
                    SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
                },
            },
        },
        core::{PCWSTR, w},
    };

    const MUTEX_NAME: &str = "Global\\CrosshairOverlaySingleInstance";
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
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr()))? };
        let already_exists =
            unsafe { GetLastError().0 } == windows::Win32::Foundation::ERROR_ALREADY_EXISTS.0;
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

    pub fn is_interception_driver_installed() -> bool {
        let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        let drivers_dir = Path::new(&system_root).join("System32").join("drivers");

        // Interception installs the keyboard/mouse driver pair in System32\drivers.
        // Some older packaging may expose a different driver name, so accept either shape.
        let legacy_driver = drivers_dir.join("interception.sys");
        let keyboard_driver = drivers_dir.join("keyboard.sys");
        let mouse_driver = drivers_dir.join("mouse.sys");

        legacy_driver.exists() || (keyboard_driver.exists() && mouse_driver.exists())
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

    pub fn hide_native_window(frame: &Frame) {
        let Ok(window_handle) = frame.window_handle() else {
            return;
        };
        let hwnd = match window_handle.as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
            _ => return,
        };

        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }

    pub fn show_native_window(frame: &Frame) {
        let Ok(window_handle) = frame.window_handle() else {
            return;
        };
        let hwnd = match window_handle.as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
            _ => return,
        };

        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
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

    pub fn hide_native_window(_frame: &Frame) {}
    pub fn show_native_window(_frame: &Frame) {}
    pub fn hide_taskbar() -> bool { false }
    pub fn show_taskbar() -> bool { false }
    pub fn is_taskbar_hidden() -> bool { false }
}

#[cfg(not(windows))]
pub use fallback::*;
