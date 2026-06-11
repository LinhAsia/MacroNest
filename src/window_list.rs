#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(windows)]
mod windows_impl {
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, RECT},
            Graphics::Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
                DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetWindowDC, HALFTONE, HGDIOBJ,
                ReleaseDC, SRCCOPY, SelectObject, SetStretchBltMode, StretchBlt,
            },
            Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
            UI::WindowsAndMessaging::{
                EnumWindows, GetForegroundWindow, GetSystemMetrics, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, IsWindowVisible, PW_RENDERFULLCONTENT,
                SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
            },
        },
        core::BOOL,
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

    pub fn capture_window_preview(
        title: Option<&str>,
        max_dimension: u32,
    ) -> Option<WindowPreviewFrame> {
        let hwnd = find_window_handle(title)?;
        unsafe { capture_window_preview_from_hwnd(hwnd, max_dimension.max(64)) }
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
        unsafe { capture_window_preview_from_hwnd(hwnd, max_dimension.max(64)) }
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

    unsafe extern "system" fn enum_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return true.into();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        if copied > 0 {
            let title = String::from_utf16_lossy(&buffer[..copied as usize])
                .trim()
                .to_owned();
            if !title.is_empty() {
                let windows = &mut *(lparam.0 as *mut Vec<WindowInfo>);
                windows.push(WindowInfo {
                    selector: window_selector(hwnd, &title),
                    title,
                });
            }
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
            && let Some(hwnd) =
                find_window_by_candidate(title_or_selector, match_duplicate_window_titles)
        {
            return Some(hwnd);
        }

        for title in extra_titles {
            if let Some(hwnd) = find_window_by_candidate(title, match_duplicate_window_titles) {
                return Some(hwnd);
            }
        }

        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() { None } else { Some(hwnd) }
    }

    fn find_window_by_candidate(
        title_or_selector: &str,
        match_duplicate_window_titles: bool,
    ) -> Option<HWND> {
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

    unsafe extern "system" fn find_window_by_candidate_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let (target_title, match_duplicate_window_titles, found) =
            &mut *(lparam.0 as *mut (&str, bool, &mut Option<HWND>));
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let Some(title) = window_title(hwnd) else {
            return true.into();
        };
        let mut matches = if *match_duplicate_window_titles {
            title == selector_base_title(target_title)
                || window_selector(hwnd, &title) == *target_title
        } else {
            title == *target_title
                || window_selector(hwnd, &title) == *target_title
                || (selector_base_title(target_title) != *target_title
                    && title == selector_base_title(target_title))
        };
        if !matches {
            matches = matches_browser_suffix(target_title, &title);
        }
        if matches {
            **found = Some(hwnd);
            return false.into();
        }
        true.into()
    }

    fn window_selector(hwnd: HWND, title: &str) -> String {
        format!("{title} (0x{:X})", hwnd.0 as usize)
    }

    fn selector_base_title(target: &str) -> &str {
        if let Some(prefix) = target.strip_suffix(')')
            && let Some((base, _)) = prefix.rsplit_once(" (0x")
        {
            return base;
        }
        target
    }

    fn clean_invisible_chars(s: &str) -> String {
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

    fn matches_browser_suffix(target: &str, candidate: &str) -> bool {
        let clean_target = clean_invisible_chars(target);
        let clean_candidate = clean_invisible_chars(candidate);
        let target_base = selector_base_title(&clean_target);
        let candidate_base = selector_base_title(&clean_candidate);
        
        let is_target_anti = target_base.contains(" - Antigravity IDE - ") || target_base.ends_with(" - Antigravity IDE");
        let is_cand_anti = candidate_base.contains(" - Antigravity IDE - ") || candidate_base.ends_with(" - Antigravity IDE");
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

    fn window_title(hwnd: HWND) -> Option<String> {
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

    unsafe fn capture_window_preview_from_hwnd(
        hwnd: HWND,
        max_dimension: u32,
    ) -> Option<WindowPreviewFrame> {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
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

    unsafe fn capture_window_region_from_hwnd(hwnd: HWND) -> Option<ScreenCaptureFrame> {
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
            let len = (width as usize) * (height as usize) * 4;
            let pixels = std::slice::from_raw_parts(bits as *const u8, len);
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

    pub fn capture_window_preview(
        _title: Option<&str>,
        _max_dimension: u32,
    ) -> Option<WindowPreviewFrame> {
        None
    }

    pub fn capture_window_preview_with_candidates(
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
}

#[cfg(not(windows))]
pub use fallback::*;
