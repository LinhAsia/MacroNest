#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(windows)]
mod windows_impl {
    use windows::{
        Win32::{
            Foundation::{HMODULE, HWND, LPARAM, POINT, RECT},
            Graphics::Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, ClientToScreen, CreateCompatibleDC,
                CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetWindowDC,
                HALFTONE, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject, SetStretchBltMode, StretchBlt,
            },
            Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
            UI::WindowsAndMessaging::{
                BringWindowToTop, EnumWindows, GWL_EXSTYLE, GetClientRect, GetForegroundWindow,
                GetSystemMetrics, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
                GetWindowTextW, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsWindow, IsWindowVisible,
                PW_RENDERFULLCONTENT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
                SM_YVIRTUALSCREEN, SW_RESTORE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
                SetForegroundWindow, SetWindowPos, ShowWindow, WS_EX_TOPMOST,
            },
        },
        core::BOOL,
    };

    use anyhow::Context;
    use once_cell::sync::Lazy;
    use parking_lot::Mutex;
    use windows::{
        Graphics::{
            Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
            DirectX::DirectXPixelFormat,
        },
        Win32::{
            Graphics::{
                Direct3D::D3D_DRIVER_TYPE_HARDWARE,
                Direct3D11::{
                    D3D11_BIND_FLAG, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_RESOURCE_MISC_FLAG,
                    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
                    D3D11CreateDevice, ID3D11Device, ID3D11Texture2D,
                },
                Dxgi::IDXGIDevice,
            },
            System::WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
            },
        },
        core::Interface,
    };

    #[derive(Debug, Clone)]
    pub struct WindowInfo {
        pub title: String,
        pub selector: String,
    }

    #[derive(Debug, Clone)]
    pub struct WindowPreviewFrame {
        pub title: String,
        pub screen_x: i32,
        pub screen_y: i32,
        pub logical_width: i32,
        pub logical_height: i32,
        pub width: usize,
        pub height: usize,
        pub rgba: Vec<u8>,
    }

    #[derive(Debug, Clone)]
    pub struct ScreenCaptureFrame {
        pub screen_x: i32,
        pub screen_y: i32,
        pub width: usize,
        pub height: usize,
        pub rgba: Vec<u8>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum WindowMatchRule {
        Lowest,
        Highest,
        Leftmost,
        Rightmost,
    }

    pub fn list_open_windows() -> Vec<WindowInfo> {
        let mut windows: Vec<WindowInfo> = Vec::new();
        unsafe {
            let _ = EnumWindows(
                Some(enum_window_proc),
                LPARAM(&mut windows as *mut Vec<WindowInfo> as isize),
            );
        }
        windows.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        windows
    }

    pub fn capture_window_preview_with_candidates(
        primary_title: Option<&str>,
        extra_titles: &[String],
        match_duplicate_window_titles: bool,
        max_dimension: u32,
    ) -> Option<WindowPreviewFrame> {
        let hwnd = find_window_handle_with_candidates(
            primary_title,
            extra_titles,
            match_duplicate_window_titles,
        )?;
        unsafe { capture_window_preview_from_hwnd(hwnd, max_dimension.max(64), false) }
    }

    pub fn capture_window_client_preview_with_candidates(
        primary_title: Option<&str>,
        extra_titles: &[String],
        match_duplicate_window_titles: bool,
        max_dimension: u32,
    ) -> Option<WindowPreviewFrame> {
        let hwnd = find_window_handle_with_candidates(
            primary_title,
            extra_titles,
            match_duplicate_window_titles,
        )?;
        unsafe { capture_window_preview_from_hwnd(hwnd, max_dimension.max(64), true) }
    }

    pub fn capture_window_region_with_candidates(
        primary_title: Option<&str>,
        extra_titles: &[String],
        match_duplicate_window_titles: bool,
    ) -> Option<ScreenCaptureFrame> {
        let hwnd = find_window_handle_with_candidates(
            primary_title,
            extra_titles,
            match_duplicate_window_titles,
        )?;
        unsafe { capture_window_region_from_hwnd(hwnd) }
    }

    pub fn virtual_screen_bounds() -> (i32, i32, i32, i32) {
        unsafe {
            let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);
            (left, top, width, height)
        }
    }

    pub fn capture_virtual_screen_region(
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    ) -> Option<ScreenCaptureFrame> {
        unsafe { capture_screen_region_from_desktop(left, top, width.max(1), height.max(1)) }
    }

    pub fn is_window_topmost(selector: &str) -> bool {
        let Some(hwnd) = find_window_handle(Some(selector)) else {
            return false;
        };
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return false;
            }
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            (ex_style & WS_EX_TOPMOST.0) != 0
        }
    }

    pub fn set_window_topmost(selector: &str, topmost: bool) -> bool {
        let Some(hwnd) = find_window_handle(Some(selector)) else {
            return false;
        };
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return false;
            }
            if topmost && IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }

            let success = SetWindowPos(
                hwnd,
                Some(if topmost {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                }),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            )
            .is_ok();

            if success && topmost {
                let _ = BringWindowToTop(hwnd);
                let _ = SetForegroundWindow(hwnd);
            }

            success
        }
    }

    unsafe extern "system" fn enum_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        if let Some(title) = window_title(hwnd) {
            let windows = &mut *(lparam.0 as *mut Vec<WindowInfo>);
            windows.push(WindowInfo {
                selector: window_selector(hwnd, &title),
                title,
            });
        }
        true.into()
    }

    fn find_window_handle(title: Option<&str>) -> Option<HWND> {
        find_window_handle_with_candidates(title, &[], false)
    }

    fn find_window_handle_with_candidates(
        primary_title: Option<&str>,
        extra_titles: &[String],
        match_duplicate_window_titles: bool,
    ) -> Option<HWND> {
        if primary_title.is_none() && extra_titles.is_empty() {
            let hwnd = unsafe { GetForegroundWindow() };
            return if hwnd.0.is_null() { None } else { Some(hwnd) };
        }

        if let Some(title_or_selector) = primary_title
            && let Some(hwnd) = find_window_by_candidate_exact(title_or_selector).or_else(|| {
                find_window_by_candidate(title_or_selector, match_duplicate_window_titles)
            })
        {
            return Some(hwnd);
        }

        for title in extra_titles {
            if let Some(hwnd) = find_window_by_candidate_exact(title)
                .or_else(|| find_window_by_candidate(title, match_duplicate_window_titles))
            {
                return Some(hwnd);
            }
        }

        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() { None } else { Some(hwnd) }
    }

    pub fn parse_window_match_rule(target: &str) -> (&str, Option<WindowMatchRule>) {
        if let Some(s) = target.strip_suffix(" [Lowest]") {
            (s, Some(WindowMatchRule::Lowest))
        } else if let Some(s) = target.strip_suffix(" [Highest]") {
            (s, Some(WindowMatchRule::Highest))
        } else if let Some(s) = target.strip_suffix(" [Leftmost]") {
            (s, Some(WindowMatchRule::Leftmost))
        } else if let Some(s) = target.strip_suffix(" [Rightmost]") {
            (s, Some(WindowMatchRule::Rightmost))
        } else {
            (target, None)
        }
    }

    pub fn strip_rule_suffix(target: &str) -> &str {
        parse_window_match_rule(target).0
    }

    pub fn has_position_rule_suffix(target: &str) -> bool {
        parse_window_match_rule(target).1.is_some()
    }

    pub fn select_window_by_match_rule(
        candidates: &[HWND],
        rule: WindowMatchRule,
    ) -> Option<HWND> {
        let mut best_hwnd = None;
        let mut best_val = match rule {
            WindowMatchRule::Lowest | WindowMatchRule::Rightmost => i32::MIN,
            WindowMatchRule::Highest | WindowMatchRule::Leftmost => i32::MAX,
        };

        for hwnd in candidates {
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(*hwnd, &mut rect) }.is_ok() {
                let axis = match rule {
                    WindowMatchRule::Lowest | WindowMatchRule::Highest => rect.top,
                    WindowMatchRule::Leftmost | WindowMatchRule::Rightmost => rect.left,
                };
                let better = match rule {
                    WindowMatchRule::Lowest | WindowMatchRule::Rightmost => axis > best_val,
                    WindowMatchRule::Highest | WindowMatchRule::Leftmost => axis < best_val,
                };
                if better {
                    best_val = axis;
                    best_hwnd = Some(*hwnd);
                }
            }
        }

        best_hwnd.or_else(|| candidates.first().copied())
    }

    fn find_window_by_candidate_exact(title_or_selector: &str) -> Option<HWND> {
        if !looks_like_window_selector(title_or_selector) {
            return None;
        }

        let mut found = None;
        unsafe {
            let mut payload = (title_or_selector, &mut found);
            let _ = EnumWindows(
                Some(find_window_by_exact_selector_proc),
                LPARAM((&mut payload) as *mut _ as isize),
            );
        }
        found
    }

    pub fn window_matches_candidate_title(
        title: &str,
        selector: &str,
        clean_target: &str,
        match_duplicate_window_titles: bool,
    ) -> bool {
        let mut matches = if match_duplicate_window_titles {
            title == selector_base_title(clean_target) || selector == clean_target
        } else {
            title == clean_target
                || selector == clean_target
                || (selector_base_title(clean_target) != clean_target
                    && title == selector_base_title(clean_target))
        };
        if !matches {
            matches = matches_browser_suffix(clean_target, title);
        }
        matches
    }

    fn find_window_by_candidate(
        title_or_selector: &str,
        match_duplicate_window_titles: bool,
    ) -> Option<HWND> {
        let (base_title, rule) = parse_window_match_rule(title_or_selector);

        if let Some(rule) = rule {
            let mut candidates = Vec::new();
            unsafe {
                let mut payload = (base_title, match_duplicate_window_titles, &mut candidates);
                let _ = EnumWindows(
                    Some(find_all_windows_by_candidate_proc),
                    LPARAM((&mut payload) as *mut _ as isize),
                );
            }

            if candidates.is_empty() {
                return None;
            }

            return select_window_by_match_rule(&candidates, rule);
        }

        let mut found = None;
        unsafe {
            let mut payload = (title_or_selector, match_duplicate_window_titles, &mut found);
            let _ = EnumWindows(
                Some(find_window_by_candidate_proc),
                LPARAM((&mut payload) as *mut _ as isize),
            );
        }
        found
    }

    unsafe extern "system" fn find_window_by_exact_selector_proc(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> BOOL {
        let (target_selector, found) = &mut *(lparam.0 as *mut (&str, &mut Option<HWND>));
        let clean_selector = strip_rule_suffix(*target_selector);
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let Some(title) = window_title(hwnd) else {
            return true.into();
        };
        if window_selector(hwnd, &title) == clean_selector {
            **found = Some(hwnd);
            return false.into();
        }
        true.into()
    }

    unsafe extern "system" fn find_window_by_candidate_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let (target_title, match_duplicate_window_titles, found) =
            &mut *(lparam.0 as *mut (&str, bool, &mut Option<HWND>));
        let clean_title = strip_rule_suffix(*target_title);
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let Some(title) = window_title(hwnd) else {
            return true.into();
        };
        let selector = window_selector(hwnd, &title);
        if window_matches_candidate_title(
            &title,
            &selector,
            clean_title,
            *match_duplicate_window_titles,
        ) {
            **found = Some(hwnd);
            return false.into();
        }
        true.into()
    }

    unsafe extern "system" fn find_all_windows_by_candidate_proc(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> BOOL {
        let (target_title, match_duplicate_window_titles, candidates) =
            &mut *(lparam.0 as *mut (&str, bool, &mut Vec<HWND>));
        let clean_title = strip_rule_suffix(*target_title);
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let Some(title) = window_title(hwnd) else {
            return true.into();
        };
        let selector = window_selector(hwnd, &title);
        if window_matches_candidate_title(
            &title,
            &selector,
            clean_title,
            *match_duplicate_window_titles,
        ) {
            candidates.push(hwnd);
        }
        true.into()
    }

    fn window_selector(hwnd: HWND, title: &str) -> String {
        format!("{title} (0x{:X})", hwnd.0 as usize)
    }

    fn looks_like_window_selector(target: &str) -> bool {
        target.ends_with(')') && target.contains(" (0x")
    }

    pub fn selector_base_title(target: &str) -> &str {
        if let Some(prefix) = target.strip_suffix(')')
            && let Some((base, _)) = prefix.rsplit_once(" (0x")
        {
            return base;
        }
        target
    }

    pub fn clean_invisible_chars(s: &str) -> String {
        s.chars()
            .filter(|&c| c != '\u{200B}' && c != '\u{200C}' && c != '\u{200D}' && c != '\u{FEFF}')
            .collect()
    }

    const BROWSER_SUFFIXES: &[&str] = &[
        " - Microsoft Edge",
        " - Google Chrome",
        " - Brave",
        " - Firefox",
        " - Opera GX",
        " - Opera",
        " - Vivaldi",
        " - Chromium",
        " - Tor Browser",
        " - Arc",
        " - Visual Studio Code",
        " - VS Code",
        " - Discord",
        " - Slack",
        " - Spotify",
    ];

    pub fn matches_browser_suffix(target: &str, candidate: &str) -> bool {
        let clean_target = clean_invisible_chars(target);
        let clean_candidate = clean_invisible_chars(candidate);
        let target_base = selector_base_title(&clean_target);
        let candidate_base = selector_base_title(&clean_candidate);

        let is_target_anti = target_base.contains(" - Antigravity IDE - ")
            || target_base.ends_with(" - Antigravity IDE");
        let is_cand_anti = candidate_base.contains(" - Antigravity IDE - ")
            || candidate_base.ends_with(" - Antigravity IDE");
        if is_target_anti && is_cand_anti {
            return true;
        }

        for suffix in BROWSER_SUFFIXES {
            if target_base.ends_with(suffix) && candidate_base.ends_with(suffix) {
                return true;
            }
        }
        false
    }

    pub fn simplify_window_title(title: &str) -> String {
        let title = strip_rule_suffix(title);
        let clean = clean_invisible_chars(title);
        let base = selector_base_title(&clean);

        if base.contains(" - Antigravity IDE - ") || base.ends_with(" - Antigravity IDE") {
            return "Antigravity IDE".to_owned();
        }

        for suffix in BROWSER_SUFFIXES {
            if base.ends_with(suffix) {
                return suffix.trim_start_matches(" - ").to_owned();
            }
        }

        if let Some((_, last)) = base.rsplit_once(" - ") {
            let trimmed = last.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }

        base.to_owned()
    }

    pub fn window_title(hwnd: HWND) -> Option<String> {
        let length = unsafe { GetWindowTextLengthW(hwnd) };
        if length <= 0 {
            return None;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
        if copied <= 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&buffer[..copied as usize])
            .trim()
            .to_owned();
        if title.is_empty() { None } else { Some(title) }
    }

    unsafe fn client_rect_on_screen(hwnd: HWND) -> Option<RECT> {
        let mut client_rect = RECT::default();
        if GetClientRect(hwnd, &mut client_rect).is_err() {
            return None;
        }
        let mut top_left = POINT {
            x: client_rect.left,
            y: client_rect.top,
        };
        let mut bottom_right = POINT {
            x: client_rect.right,
            y: client_rect.bottom,
        };
        if !ClientToScreen(hwnd, &mut top_left).as_bool() {
            return None;
        }
        if !ClientToScreen(hwnd, &mut bottom_right).as_bool() {
            return None;
        }
        Some(RECT {
            left: top_left.x,
            top: top_left.y,
            right: bottom_right.x,
            bottom: bottom_right.y,
        })
    }

    unsafe fn capture_window_preview_from_hwnd(
        hwnd: HWND,
        max_dimension: u32,
        client_only: bool,
    ) -> Option<WindowPreviewFrame> {
        let rect = if client_only {
            client_rect_on_screen(hwnd)?
        } else {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return None;
            }
            rect
        };
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return None;
        }
        let screen_width = (rect.right - rect.left).max(1);
        let screen_height = (rect.bottom - rect.top).max(1);
        let scale = (max_dimension as f32 / screen_width as f32)
            .min(max_dimension as f32 / screen_height as f32)
            .min(1.0);
        let capture_width = ((screen_width as f32 * scale).round() as i32).max(1);
        let capture_height = ((screen_height as f32 * scale).round() as i32).max(1);

        let screen_dc = GetDC(None);
        let window_dc = GetWindowDC(Some(hwnd));
        if screen_dc.0.is_null() && window_dc.0.is_null() {
            return None;
        }
        let compat_dc = if !screen_dc.0.is_null() {
            screen_dc
        } else {
            window_dc
        };

        let full_dc = CreateCompatibleDC(Some(compat_dc));
        if full_dc.0.is_null() {
            if !screen_dc.0.is_null() {
                let _ = ReleaseDC(None, screen_dc);
            }
            if !window_dc.0.is_null() {
                let _ = ReleaseDC(Some(hwnd), window_dc);
            }
            return None;
        }
        let scaled_dc = CreateCompatibleDC(Some(compat_dc));
        if scaled_dc.0.is_null() {
            let _ = DeleteDC(full_dc);
            if !screen_dc.0.is_null() {
                let _ = ReleaseDC(None, screen_dc);
            }
            if !window_dc.0.is_null() {
                let _ = ReleaseDC(Some(hwnd), window_dc);
            }
            return None;
        }

        let mut full_info = BITMAPINFO::default();
        full_info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        full_info.bmiHeader.biWidth = screen_width;
        full_info.bmiHeader.biHeight = -screen_height;
        full_info.bmiHeader.biPlanes = 1;
        full_info.bmiHeader.biBitCount = 32;
        full_info.bmiHeader.biCompression = BI_RGB.0;

        let mut full_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let full_bitmap = CreateDIBSection(
            Some(compat_dc),
            &full_info,
            DIB_RGB_COLORS,
            &mut full_bits,
            None,
            0,
        )
        .ok()?;
        if full_bitmap.0.is_null() || full_bits.is_null() {
            let _ = DeleteDC(full_dc);
            let _ = DeleteDC(scaled_dc);
            if !screen_dc.0.is_null() {
                let _ = ReleaseDC(None, screen_dc);
            }
            if !window_dc.0.is_null() {
                let _ = ReleaseDC(Some(hwnd), window_dc);
            }
            return None;
        }

        let mut scaled_info = BITMAPINFO::default();
        scaled_info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        scaled_info.bmiHeader.biWidth = capture_width;
        scaled_info.bmiHeader.biHeight = -capture_height;
        scaled_info.bmiHeader.biPlanes = 1;
        scaled_info.bmiHeader.biBitCount = 32;
        scaled_info.bmiHeader.biCompression = BI_RGB.0;

        let mut scaled_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let scaled_bitmap = CreateDIBSection(
            Some(compat_dc),
            &scaled_info,
            DIB_RGB_COLORS,
            &mut scaled_bits,
            None,
            0,
        )
        .ok()?;
        if scaled_bitmap.0.is_null() || scaled_bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(full_bitmap.0));
            let _ = DeleteDC(full_dc);
            let _ = DeleteDC(scaled_dc);
            if !screen_dc.0.is_null() {
                let _ = ReleaseDC(None, screen_dc);
            }
            if !window_dc.0.is_null() {
                let _ = ReleaseDC(Some(hwnd), window_dc);
            }
            return None;
        }

        let full_old_obj = SelectObject(full_dc, HGDIOBJ(full_bitmap.0));
        let scaled_old_obj = SelectObject(scaled_dc, HGDIOBJ(scaled_bitmap.0));
        let _ = SetStretchBltMode(full_dc, HALFTONE);
        let _ = SetStretchBltMode(scaled_dc, HALFTONE);

        let copied_full =
            if PrintWindow(hwnd, full_dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool() {
                true
            } else if !window_dc.0.is_null() {
                StretchBlt(
                    full_dc,
                    0,
                    0,
                    screen_width,
                    screen_height,
                    Some(window_dc),
                    0,
                    0,
                    screen_width,
                    screen_height,
                    SRCCOPY,
                )
                .as_bool()
            } else if !screen_dc.0.is_null() {
                StretchBlt(
                    full_dc,
                    0,
                    0,
                    screen_width,
                    screen_height,
                    Some(screen_dc),
                    rect.left,
                    rect.top,
                    screen_width,
                    screen_height,
                    SRCCOPY,
                )
                .as_bool()
            } else {
                false
            };

        let copied = if copied_full {
            StretchBlt(
                scaled_dc,
                0,
                0,
                capture_width,
                capture_height,
                Some(full_dc),
                0,
                0,
                screen_width,
                screen_height,
                SRCCOPY,
            )
            .as_bool()
        } else {
            false
        };

        let rgba = if copied {
            let len = (capture_width as usize) * (capture_height as usize) * 4;
            let pixels = std::slice::from_raw_parts(scaled_bits as *const u8, len);
            let mut rgba = vec![0u8; len];
            for (dst, src) in rgba.chunks_exact_mut(4).zip(pixels.chunks_exact(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = 255;
            }
            rgba
        } else {
            Vec::new()
        };

        let title = window_title(hwnd).unwrap_or_else(|| "Focused window".to_owned());

        let _ = SelectObject(full_dc, full_old_obj);
        let _ = SelectObject(scaled_dc, scaled_old_obj);
        let _ = DeleteObject(HGDIOBJ(full_bitmap.0));
        let _ = DeleteObject(HGDIOBJ(scaled_bitmap.0));
        let _ = DeleteDC(full_dc);
        let _ = DeleteDC(scaled_dc);
        if !screen_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
        }
        if !window_dc.0.is_null() {
            let _ = ReleaseDC(Some(hwnd), window_dc);
        }

        if !copied || rgba.is_empty() {
            return None;
        }

        Some(WindowPreviewFrame {
            title,
            screen_x: rect.left,
            screen_y: rect.top,
            logical_width: screen_width,
            logical_height: screen_height,
            width: capture_width as usize,
            height: capture_height as usize,
            rgba,
        })
    }

    struct WgcSession {
        hwnd: HWND,
        dxgi_device: windows::Graphics::DirectX::Direct3D11::IDirect3DDevice,
        d3d_device: ID11Device,
        frame_pool: Direct3D11CaptureFramePool,
        session: GraphicsCaptureSession,
        staging_texture: Option<(ID3D11Texture2D, u32, u32)>,
    }

    unsafe impl Send for WgcSession {}
    unsafe impl Sync for WgcSession {}

    type ID11Device = ID3D11Device;

    impl Drop for WgcSession {
        fn drop(&mut self) {
            let _ = self.session.Close();
            let _ = self.frame_pool.Close();
        }
    }

    static WGC_MANAGER: Lazy<Mutex<Option<WgcSession>>> = Lazy::new(|| Mutex::new(None));

    fn init_wgc_session(hwnd: HWND) -> anyhow::Result<WgcSession> {
        let mut d3d_device: Option<ID3D11Device> = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                None,
            )?;
        }
        let d3d_device = d3d_device.context("Failed to create D3D11 Device")?;
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let dxgi_device_winrt = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
        let dxgi_device_winrt: windows::Graphics::DirectX::Direct3D11::IDirect3DDevice =
            dxgi_device_winrt.cast()?;

        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
        let size = item.Size()?;

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &dxgi_device_winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )?;

        let session = frame_pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;

        Ok(WgcSession {
            hwnd,
            dxgi_device: dxgi_device_winrt,
            d3d_device,
            frame_pool,
            session,
            staging_texture: None,
        })
    }

    impl WgcSession {
        fn get_next_frame(&mut self) -> anyhow::Result<ScreenCaptureFrame> {
            let mut frame_opt = None;
            for _ in 0..15 {
                if let Ok(frame) = self.frame_pool.TryGetNextFrame() {
                    frame_opt = Some(frame);
                    while let Ok(next) = self.frame_pool.TryGetNextFrame() {
                        frame_opt = Some(next);
                    }
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }

            let frame = frame_opt.context("No frame available from WGC pool")?;
            let surface = frame.Surface()?;
            let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
            let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };

            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe {
                texture.GetDesc(&mut desc);
            }
            let width = desc.Width;
            let height = desc.Height;

            let mut recreate_staging = true;
            if let Some((_, st_w, st_h)) = self.staging_texture {
                if st_w == width && st_h == height {
                    recreate_staging = false;
                }
            }

            if recreate_staging {
                let mut staging_desc = desc;
                staging_desc.Usage = D3D11_USAGE_STAGING;
                staging_desc.BindFlags = 0;
                staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                staging_desc.MiscFlags = 0;

                let mut staging = None;
                unsafe {
                    self.d3d_device
                        .CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
                }
                self.staging_texture = Some((staging.unwrap(), width, height));
            }

            let (staging_tex, _, _) = self.staging_texture.as_ref().unwrap();

            let d3d_context = unsafe { self.d3d_device.GetImmediateContext()? };

            unsafe {
                d3d_context.CopyResource(staging_tex, &texture);
            }

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe {
                d3d_context.Map(staging_tex, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            }

            let pitch = mapped.RowPitch as usize;
            let src_slice = unsafe {
                std::slice::from_raw_parts(mapped.pData as *const u8, pitch * height as usize)
            };
            let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
            for y in 0..height as usize {
                let src_offset = y * pitch;
                let dst_offset = y * (width as usize) * 4;
                let src_row = &src_slice[src_offset..(src_offset + (width as usize) * 4)];
                let dst_row = &mut rgba[dst_offset..(dst_offset + (width as usize) * 4)];
                for (dst, src) in dst_row.chunks_exact_mut(4).zip(src_row.chunks_exact(4)) {
                    dst[0] = src[2];
                    dst[1] = src[1];
                    dst[2] = src[0];
                    dst[3] = src[3];
                }
            }

            unsafe {
                d3d_context.Unmap(staging_tex, 0);
            }

            let mut rect = RECT::default();
            let _ = unsafe { GetWindowRect(self.hwnd, &mut rect) };

            Ok(ScreenCaptureFrame {
                screen_x: rect.left,
                screen_y: rect.top,
                width: width as usize,
                height: height as usize,
                rgba,
            })
        }
    }

    fn capture_wgc_frame(hwnd: HWND) -> Option<ScreenCaptureFrame> {
        let mut manager = WGC_MANAGER.lock();
        let mut reinit = true;
        if let Some(ref session) = *manager {
            if session.hwnd == hwnd {
                reinit = false;
            }
        }

        if reinit {
            *manager = None;
            match init_wgc_session(hwnd) {
                Ok(session) => {
                    *manager = Some(session);
                }
                Err(_) => {
                    return None;
                }
            }
        }

        let session = manager.as_mut().unwrap();
        match session.get_next_frame() {
            Ok(frame) => Some(frame),
            Err(_) => {
                *manager = None;
                None
            }
        }
    }

    pub(crate) fn close_window_capture_session() {
        let mut manager = WGC_MANAGER.lock();
        *manager = None;
    }

    pub(crate) unsafe fn capture_window_region_from_hwnd(hwnd: HWND) -> Option<ScreenCaptureFrame> {
        if let Some(frame) = capture_wgc_frame(hwnd) {
            return Some(frame);
        }

        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return None;
        }
        let left = rect.left;
        let top = rect.top;
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        capture_screen_region_from_desktop(left, top, width, height)
    }

    unsafe fn capture_screen_region_from_desktop(
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    ) -> Option<ScreenCaptureFrame> {
        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            return None;
        }

        let compat_dc = CreateCompatibleDC(Some(screen_dc));
        if compat_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
            return None;
        }

        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap =
            CreateDIBSection(Some(screen_dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bitmap.0.is_null() || bits.is_null() {
            let _ = DeleteDC(compat_dc);
            let _ = ReleaseDC(None, screen_dc);
            return None;
        }

        let old_obj = SelectObject(compat_dc, HGDIOBJ(bitmap.0));
        let copied = BitBlt(
            compat_dc,
            0,
            0,
            width,
            height,
            Some(screen_dc),
            left,
            top,
            SRCCOPY,
        )
        .is_ok();

        let rgba = if copied {
            let pixel_count = (width as usize) * (height as usize);
            let len = pixel_count * 4;
            let mut rgba = vec![0u8; len];
            unsafe {
                let src_ptr = bits as *const u32;
                let dst_ptr = rgba.as_mut_ptr() as *mut u32;
                for i in 0..pixel_count {
                    let pixel = *src_ptr.add(i);
                    let b = pixel & 0xFF;
                    let g = (pixel >> 8) & 0xFF;
                    let r = (pixel >> 16) & 0xFF;
                    *dst_ptr.add(i) = r | (g << 8) | (b << 16) | (255 << 24);
                }
            }
            rgba
        } else {
            Vec::new()
        };

        let _ = SelectObject(compat_dc, old_obj);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(compat_dc);
        let _ = ReleaseDC(None, screen_dc);

        if !copied || rgba.is_empty() {
            return None;
        }

        Some(ScreenCaptureFrame {
            screen_x: left,
            screen_y: top,
            width: width as usize,
            height: height as usize,
            rgba,
        })
    }
}

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(not(windows))]
mod fallback {
    #[derive(Debug, Clone)]
    pub struct WindowInfo {
        pub title: String,
        pub selector: String,
    }

    #[derive(Debug, Clone)]
    pub struct WindowPreviewFrame {
        pub title: String,
        pub screen_x: i32,
        pub screen_y: i32,
        pub logical_width: i32,
        pub logical_height: i32,
        pub width: usize,
        pub height: usize,
        pub rgba: Vec<u8>,
    }

    pub fn list_open_windows() -> Vec<WindowInfo> {
        Vec::new()
    }

    pub fn capture_window_preview_with_candidates(
        _primary_title: Option<&str>,
        _extra_titles: &[String],
        _match_duplicate_window_titles: bool,
        _max_dimension: u32,
    ) -> Option<WindowPreviewFrame> {
        None
    }

    pub fn capture_window_client_preview_with_candidates(
        _primary_title: Option<&str>,
        _extra_titles: &[String],
        _match_duplicate_window_titles: bool,
        _max_dimension: u32,
    ) -> Option<WindowPreviewFrame> {
        None
    }

    #[derive(Debug, Clone)]
    pub struct ScreenCaptureFrame {
        pub screen_x: i32,
        pub screen_y: i32,
        pub width: usize,
        pub height: usize,
        pub rgba: Vec<u8>,
    }

    pub fn capture_window_region_with_candidates(
        _primary_title: Option<&str>,
        _extra_titles: &[String],
        _match_duplicate_window_titles: bool,
    ) -> Option<ScreenCaptureFrame> {
        None
    }

    pub fn is_window_topmost(_selector: &str) -> bool {
        false
    }

    pub fn set_window_topmost(_selector: &str, _topmost: bool) -> bool {
        false
    }

    pub(crate) fn close_window_capture_session() {}
}

#[cfg(not(windows))]
pub use fallback::*;
