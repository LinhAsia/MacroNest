#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(windows)]
mod windows_impl {
    use windows::{
        Win32::{
            Foundation::{HMODULE, HWND, LPARAM, POINT, RECT, WPARAM},
            Graphics::Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, ClientToScreen, CreateCompatibleDC,
                CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetWindowDC,
                HALFTONE, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject, SetStretchBltMode, StretchBlt,
            },
            Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
            Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
            System::Com::{
                CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            },
            UI::Shell::{ExtractIconExW, SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGetFileInfoW},
            UI::WindowsAndMessaging::{
                BringWindowToTop, DI_NORMAL, DestroyIcon, DrawIconEx, EnumWindows,
                GA_ROOT, GA_ROOTOWNER, GetAncestor, GetClassLongPtrW, GET_CLASS_LONG_INDEX, GetClientRect, GetForegroundWindow,
                GetSystemMetrics, GetWindowRect, GetWindowTextLengthW,
                GetWindowTextW, GetWindowThreadProcessId, HICON,
                IsIconic, IsWindow, IsWindowVisible, PW_RENDERFULLCONTENT, SM_CXVIRTUALSCREEN,
                SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SMTO_ABORTIFHUNG,
                SW_RESTORE, SendMessageTimeoutW,
                SetForegroundWindow, ShowWindow, WindowFromPoint, WM_GETICON,
            },
        },
        core::{BOOL, PCWSTR},
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
                    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
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
        pub process_id: u32,
        pub process_path: String,
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
        Unfocused,
        Focused,
    }

    pub fn list_open_windows() -> Vec<WindowInfo> {
        let mut windows: Vec<WindowInfo> = Vec::new();
        unsafe {
            let _ = EnumWindows(
                Some(enum_window_proc),
                LPARAM(&mut windows as *mut Vec<WindowInfo> as isize),
            );
        }
        windows.sort_by(|a, b| {
            a.title
                .chars()
                .map(|c| c.to_ascii_lowercase())
                .cmp(b.title.chars().map(|c| c.to_ascii_lowercase()))
        });
        windows
    }

    pub fn process_id_for_window(selector: Option<&str>) -> Option<u32> {
        let hwnd = find_window_handle(selector)?;
        let mut process_id = 0;
        unsafe {
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        }
        (process_id != 0).then_some(process_id)
    }

    pub fn window_client_bounds(selector: Option<&str>) -> Option<(i32, i32, i32, i32)> {
        let hwnd = find_window_handle(selector)?;
        let rect = unsafe { client_rect_on_screen(hwnd)? };
        Some((
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        ))
    }

    pub fn window_at_point(screen_x: i32, screen_y: i32) -> Option<(String, (i32, i32, i32, i32))> {
        let pt = POINT { x: screen_x, y: screen_y };
        let hwnd = unsafe { WindowFromPoint(pt) };
        if hwnd.0.is_null() {
            return None;
        }
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        let target_hwnd = if !root.0.is_null() { root } else { hwnd };
        let title = window_title(target_hwnd)?;
        let rect = unsafe { client_rect_on_screen(target_hwnd)? };
        Some((
            title,
            (
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ),
        ))
    }

    pub fn process_icon_rgba(path: &str) -> Option<Vec<u8>> {
        if path.is_empty() {
            return None;
        }
        let com_inited = unsafe {
            CoInitializeEx(
                None,
                COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
            )
            .is_ok()
        };

        let wide = path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut info = SHFILEINFOW::default();
        let found = unsafe {
            SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_SMALLICON,
            )
        };

        let mut hicon = if found != 0 && !info.hIcon.0.is_null() {
            Some(info.hIcon)
        } else {
            None
        };

        if hicon.is_none() {
            let mut small_icon = HICON(std::ptr::null_mut());
            let extracted = unsafe {
                ExtractIconExW(
                    PCWSTR(wide.as_ptr()),
                    0,
                    None,
                    Some(&mut small_icon),
                    1,
                )
            };
            if extracted > 0 && !small_icon.0.is_null() {
                hicon = Some(small_icon);
            }
        }

        let result = hicon.and_then(|icon| unsafe {
            let rgba = hicon_rgba(icon);
            let _ = DestroyIcon(icon);
            rgba
        });

        if com_inited {
            unsafe { CoUninitialize() };
        }

        result
    }

    pub fn hwnd_from_selector(selector: &str) -> Option<HWND> {
        if let Some(prefix) = selector.strip_suffix(')') {
            if let Some((_, hex)) = prefix.rsplit_once(" (0x") {
                if let Ok(val) = usize::from_str_radix(hex, 16) {
                    let hwnd = HWND(val as *mut _);
                    if unsafe { IsWindow(Some(hwnd)).as_bool() } {
                        return Some(hwnd);
                    }
                }
            }
        }
        find_window_handle(Some(selector))
    }

    pub fn window_icon_rgba(selector: &str) -> Option<Vec<u8>> {
        let hwnd = hwnd_from_selector(selector)?;
        unsafe {
            let mut hicon = None;
            for icon_type in [2usize, 0, 1] {
                let mut res: usize = 0;
                if SendMessageTimeoutW(
                    hwnd,
                    WM_GETICON,
                    WPARAM(icon_type),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    50,
                    Some(&mut res),
                )
                .0 != 0
                    && res != 0
                {
                    hicon = Some(HICON(res as *mut _));
                    break;
                }
            }
            if hicon.is_none() {
                for index in [-34i32, -14] {
                    let res = GetClassLongPtrW(hwnd, GET_CLASS_LONG_INDEX(index));
                    if res != 0 {
                        hicon = Some(HICON(res as *mut _));
                        break;
                    }
                }
            }
            if let Some(icon) = hicon {
                return hicon_rgba(icon);
            }
        }
        None
    }

    pub fn window_or_process_icon_rgba(path: &str, selector: Option<&str>) -> Option<Vec<u8>> {
        if let Some(sel) = selector {
            if let Some(rgba) = window_icon_rgba(sel) {
                return Some(rgba);
            }
        }
        if !path.is_empty() {
            if let Some(rgba) = process_icon_rgba(path) {
                return Some(rgba);
            }
        }
        None
    }

    unsafe fn hicon_rgba(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<Vec<u8>> {
        let screen = GetDC(None);
        if screen.0.is_null() {
            return None;
        }
        let dc = CreateCompatibleDC(Some(screen));
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 16,
                biHeight: -16,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..BITMAPINFOHEADER::default()
            },
            ..BITMAPINFO::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap =
            CreateDIBSection(Some(dc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let old = SelectObject(dc, HGDIOBJ(bitmap.0));
        std::ptr::write_bytes(bits, 0, 16 * 16 * 4);
        let drawn = DrawIconEx(dc, 0, 0, icon, 16, 16, 0, None, DI_NORMAL).is_ok();
        let mut rgba = vec![0; 16 * 16 * 4];
        if drawn {
            let source = std::slice::from_raw_parts(bits.cast::<u8>(), rgba.len());
            for (src, dst) in source.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
                dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
            }
        }
        let _ = SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
        let _ = ReleaseDC(None, screen);
        drawn.then_some(rgba)
    }

    pub fn capture_window_preview_with_candidates(
        primary_title: Option<&str>,
        extra_titles: &[String],
        match_duplicate_window_titles: bool,
        max_dimension: u32,
    ) -> Option<WindowPreviewFrame> {
        capture_window_preview_with_candidates_impl(
            primary_title,
            extra_titles,
            match_duplicate_window_titles,
            max_dimension,
            false,
        )
    }

    pub fn capture_window_client_preview_with_candidates(
        primary_title: Option<&str>,
        extra_titles: &[String],
        match_duplicate_window_titles: bool,
        max_dimension: u32,
    ) -> Option<WindowPreviewFrame> {
        capture_window_preview_with_candidates_impl(
            primary_title,
            extra_titles,
            match_duplicate_window_titles,
            max_dimension,
            true,
        )
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

    pub fn focus_window(selector: &str) -> bool {
        let Some(hwnd) = find_window_handle(Some(selector)) else {
            return false;
        };
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return false;
            }
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            true
        }
    }

    unsafe extern "system" fn enum_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        if let Some(title) = window_title(hwnd) {
            let mut process_id = 0;
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut process_id));
            let windows = &mut *(lparam.0 as *mut Vec<WindowInfo>);
            windows.push(WindowInfo {
                selector: window_selector(hwnd, &title),
                title,
                process_id,
                process_path: String::new(),
            });
        }
        true.into()
    }

    pub(crate) fn find_window_handle(title: Option<&str>) -> Option<HWND> {
        match title {
            Some(selector) => find_window_by_candidate_chain(selector, false),
            None => current_foreground_window(),
        }
    }

    fn current_foreground_window() -> Option<HWND> {
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.0.is_null()).then_some(hwnd)
    }

    fn find_window_by_candidate_chain(
        title_or_selector: &str,
        match_duplicate_window_titles: bool,
    ) -> Option<HWND> {
        find_window_by_candidate_exact(title_or_selector)
            .or_else(|| find_window_by_candidate(title_or_selector, match_duplicate_window_titles))
    }

    fn capture_window_preview_with_candidates_impl(
        primary_title: Option<&str>,
        extra_titles: &[String],
        match_duplicate_window_titles: bool,
        max_dimension: u32,
        client_only: bool,
    ) -> Option<WindowPreviewFrame> {
        let hwnd = find_window_handle_with_candidates(
            primary_title,
            extra_titles,
            match_duplicate_window_titles,
        )?;
        unsafe { capture_window_preview_from_hwnd(hwnd, max_dimension.max(64), client_only) }
    }

    fn find_window_handle_with_candidates(
        primary_title: Option<&str>,
        extra_titles: &[String],
        match_duplicate_window_titles: bool,
    ) -> Option<HWND> {
        if primary_title.is_none() && extra_titles.is_empty() {
            return current_foreground_window();
        }

        if let Some(title_or_selector) = primary_title
            && let Some(hwnd) =
                find_window_by_candidate_chain(title_or_selector, match_duplicate_window_titles)
        {
            return Some(hwnd);
        }

        for title in extra_titles {
            if let Some(hwnd) = find_window_by_candidate_chain(title, match_duplicate_window_titles)
            {
                return Some(hwnd);
            }
        }

        current_foreground_window()
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
        } else if let Some(s) = target.strip_suffix(" [The Unfocused One]") {
            (s, Some(WindowMatchRule::Unfocused))
        } else if let Some(s) = target.strip_suffix(" [Unfocused]") {
            (s, Some(WindowMatchRule::Unfocused))
        } else if let Some(s) = target.strip_suffix(" [The Focused One]") {
            (s, Some(WindowMatchRule::Focused))
        } else if let Some(s) = target.strip_suffix(" [Focused]") {
            (s, Some(WindowMatchRule::Focused))
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

    pub fn find_focused_candidate(candidates: &[HWND]) -> Option<HWND> {
        if candidates.is_empty() {
            return None;
        }

        let pid_of = |h: HWND| -> u32 {
            if h.0.is_null() {
                0
            } else {
                let mut pid = 0;
                unsafe {
                    let _ = GetWindowThreadProcessId(h, Some(&mut pid));
                }
                pid
            }
        };

        let root_of = |h: HWND| -> (HWND, HWND) {
            if h.0.is_null() {
                (h, h)
            } else {
                unsafe {
                    let root = GetAncestor(h, GA_ROOT);
                    let root_owner = GetAncestor(h, GA_ROOTOWNER);
                    (
                        if root.0.is_null() { h } else { root },
                        if root_owner.0.is_null() { h } else { root_owner },
                    )
                }
            }
        };

        let cand_info: Vec<(HWND, u32, (HWND, HWND))> = candidates
            .iter()
            .copied()
            .map(|h| (h, pid_of(h), root_of(h)))
            .collect();

        let live = unsafe { GetForegroundWindow() };
        let cached = HWND(crate::overlay::FOREGROUND_WINDOW_HWND.load(std::sync::atomic::Ordering::Relaxed) as *mut _);
        let current_pid = std::process::id();

        for candidate_fg in [live, cached] {
            if candidate_fg.0.is_null() {
                continue;
            }
            let fg_pid = pid_of(candidate_fg);
            if fg_pid == current_pid {
                continue;
            }

            let (fg_root, fg_root_owner) = root_of(candidate_fg);

            // 1. Exact HWND match
            if let Some(&(h, _, _)) = cand_info.iter().find(|(h, _, _)| *h == candidate_fg) {
                return Some(h);
            }

            // 2. Root or RootOwner match (covers child controls and owned popup windows)
            if let Some(&(h, _, _)) = cand_info.iter().find(|(h, _, (c_root, c_root_owner))| {
                *h == fg_root
                    || *h == fg_root_owner
                    || *c_root == fg_root
                    || *c_root_owner == fg_root_owner
                    || *c_root == fg_root_owner
                    || *c_root_owner == fg_root
            }) {
                return Some(h);
            }

            // 3. Process ID match (vital for multiple game instances running side by side)
            if fg_pid != 0 {
                if let Some(&(h, _, _)) = cand_info.iter().find(|(_, p, _)| *p == fg_pid) {
                    return Some(h);
                }
            }
        }

        None
    }

    pub fn select_window_by_match_rule(candidates: &[HWND], rule: WindowMatchRule) -> Option<HWND> {
        if rule == WindowMatchRule::Unfocused {
            if let Some(focused) = find_focused_candidate(candidates) {
                if let Some(unfocused) = candidates.iter().copied().find(|&hwnd| hwnd != focused) {
                    return Some(unfocused);
                }
                return Some(focused);
            }
            if candidates.len() > 1 {
                // Foreground is outside candidates (e.g. MacroNest). Return candidate 1 so
                // Focused (candidate 0) and Unfocused (candidate 1) never resolve to the same window.
                return Some(candidates[1]);
            }
            return candidates.first().copied();
        }

        if rule == WindowMatchRule::Focused {
            if let Some(focused) = find_focused_candidate(candidates) {
                return Some(focused);
            }
            return candidates.first().copied();
        }

        let mut best_hwnd = None;
        let mut best_val = match rule {
            WindowMatchRule::Lowest | WindowMatchRule::Rightmost => i32::MIN,
            WindowMatchRule::Highest | WindowMatchRule::Leftmost => i32::MAX,
            WindowMatchRule::Unfocused | WindowMatchRule::Focused => unreachable!(),
        };

        for hwnd in candidates {
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(*hwnd, &mut rect) }.is_ok() {
                let axis = match rule {
                    WindowMatchRule::Lowest | WindowMatchRule::Highest => rect.top,
                    WindowMatchRule::Leftmost | WindowMatchRule::Rightmost => rect.left,
                    WindowMatchRule::Unfocused | WindowMatchRule::Focused => unreachable!(),
                };
                let better = match rule {
                    WindowMatchRule::Lowest | WindowMatchRule::Rightmost => axis > best_val,
                    WindowMatchRule::Highest | WindowMatchRule::Leftmost => axis < best_val,
                    WindowMatchRule::Unfocused | WindowMatchRule::Focused => unreachable!(),
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

        find_first_window_by_exact_selector(title_or_selector)
    }

    pub fn window_matches_candidate_title(
        title: &str,
        selector: &str,
        clean_target: &str,
        match_duplicate_window_titles: bool,
    ) -> bool {
        let trimmed_title = title.trim();
        let trimmed_target = clean_target.trim();
        if trimmed_title.is_empty() || trimmed_target.is_empty() {
            return false;
        }

        let is_specific_selector = looks_like_window_selector(clean_target);
        let base = selector_base_title(clean_target);

        let mut matches = if match_duplicate_window_titles {
            title == base || (!selector.is_empty() && selector == clean_target)
        } else if is_specific_selector {
            !selector.is_empty() && selector == clean_target
        } else {
            title == clean_target
                || title.eq_ignore_ascii_case(clean_target)
                || (!selector.is_empty() && selector == clean_target)
        };

        if !matches && !is_specific_selector {
            matches = matches_browser_suffix(clean_target, title);
        }

        if !matches && !is_specific_selector {
            if let Some((prefix, rest)) = title.split_at_checked(clean_target.len()) {
                if prefix.eq_ignore_ascii_case(clean_target)
                    && (rest.starts_with(" - ")
                        || rest.starts_with(" — ")
                        || rest.starts_with(" (")
                        || rest.starts_with(" : "))
                {
                    matches = true;
                }
            }
        }

        if !matches && !is_specific_selector {
            let simplified_cand = simplify_window_title(title);
            let simplified_target = simplify_window_title(clean_target);
            if !simplified_cand.is_empty()
                && is_known_app_simplified_title(&simplified_cand)
                && simplified_cand.eq_ignore_ascii_case(&simplified_target)
            {
                matches = true;
            }
        }

        matches
    }

    fn is_known_app_simplified_title(simplified: &str) -> bool {
        simplified.eq_ignore_ascii_case("Antigravity IDE")
            || BROWSER_SUFFIXES.iter().any(|s| {
                s.trim_start_matches(" - ").eq_ignore_ascii_case(simplified)
            })
    }

    fn find_window_by_candidate(
        title_or_selector: &str,
        match_duplicate_window_titles: bool,
    ) -> Option<HWND> {
        let (base_title, rule) = parse_window_match_rule(title_or_selector);

        if let Some(rule) = rule {
            let candidates =
                find_all_windows_by_candidate(base_title, match_duplicate_window_titles);
            if candidates.is_empty() {
                return None;
            }

            return select_window_by_match_rule(&candidates, rule);
        }

        find_first_window_by_candidate(title_or_selector, match_duplicate_window_titles)
    }

    pub(crate) fn find_all_windows_by_candidate(
        title_or_selector: &str,
        match_duplicate_window_titles: bool,
    ) -> Vec<HWND> {
        let clean_target = strip_rule_suffix(title_or_selector);
        let mut candidates = Vec::new();
        unsafe {
            let mut payload = (
                clean_target,
                match_duplicate_window_titles,
                &mut candidates,
            );
            let _ = EnumWindows(
                Some(find_all_windows_by_candidate_proc),
                LPARAM((&mut payload) as *mut _ as isize),
            );
        }
        candidates
    }

    fn find_first_window_by_exact_selector(title_or_selector: &str) -> Option<HWND> {
        let clean = strip_rule_suffix(title_or_selector);
        if let Some(prefix) = clean.strip_suffix(')')
            && let Some((_, hex)) = prefix.rsplit_once(" (0x")
            && let Ok(val) = usize::from_str_radix(hex, 16)
        {
            let hwnd = HWND(val as *mut _);
            if unsafe { IsWindow(Some(hwnd)).as_bool() } {
                return Some(hwnd);
            }
        }
        let mut found = None;
        unsafe {
            let mut payload = (clean, &mut found);
            let _ = EnumWindows(
                Some(find_window_by_exact_selector_proc),
                LPARAM((&mut payload) as *mut _ as isize),
            );
        }
        found
    }

    fn find_first_window_by_candidate(
        title_or_selector: &str,
        match_duplicate_window_titles: bool,
    ) -> Option<HWND> {
        let clean_target = strip_rule_suffix(title_or_selector);
        let mut found = None;
        unsafe {
            let mut payload = (clean_target, match_duplicate_window_titles, &mut found);
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
        if exact_selector_window_matches(hwnd, target_selector) {
            **found = Some(hwnd);
            return false.into();
        }
        true.into()
    }

    unsafe extern "system" fn find_window_by_candidate_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let (target_title, match_duplicate_window_titles, found) =
            &mut *(lparam.0 as *mut (&str, bool, &mut Option<HWND>));
        if candidate_window_matches(hwnd, target_title, *match_duplicate_window_titles) {
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
        if candidate_window_matches(hwnd, target_title, *match_duplicate_window_titles)
            && !candidates.contains(&hwnd)
        {
            candidates.push(hwnd);
        }
        true.into()
    }

    pub fn window_selector(hwnd: HWND, title: &str) -> String {
        format!("{title} (0x{:X})", hwnd.0 as usize)
    }

    fn exact_selector_window_matches(hwnd: HWND, target_selector: &str) -> bool {
        let clean_selector = strip_rule_suffix(target_selector);
        if !unsafe { IsWindowVisible(hwnd).as_bool() } {
            return false;
        }
        let Some(title) = window_title(hwnd) else {
            return false;
        };
        window_selector(hwnd, &title) == clean_selector
    }

    fn candidate_window_matches(
        hwnd: HWND,
        target_title: &str,
        match_duplicate_window_titles: bool,
    ) -> bool {
        let clean_title = strip_rule_suffix(target_title);
        let base_title = selector_base_title(clean_title);
        if !unsafe { IsWindow(Some(hwnd)).as_bool() } {
            return false;
        }
        if !unsafe { IsWindowVisible(hwnd).as_bool() } {
            return false;
        }
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        if !root.0.is_null() && root != hwnd {
            return false;
        }
        let Some(title) = window_title(hwnd) else {
            return false;
        };
        let selector = window_selector(hwnd, &title);
        window_matches_candidate_title(
            &title,
            &selector,
            base_title,
            match_duplicate_window_titles,
        )
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

    pub fn clean_invisible_chars(s: &str) -> std::borrow::Cow<'_, str> {
        if !s.chars().any(|c| matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')) {
            std::borrow::Cow::Borrowed(s)
        } else {
            std::borrow::Cow::Owned(
                s.chars()
                    .filter(|&c| !matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}'))
                    .collect(),
            )
        }
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

    fn extract_simplified_title<'a>(base: &'a str) -> Option<&'a str> {
        if base.contains(" - Antigravity IDE - ") || base.ends_with(" - Antigravity IDE") {
            return Some("Antigravity IDE");
        }
        for suffix in BROWSER_SUFFIXES {
            if base.ends_with(suffix) {
                return Some(suffix.trim_start_matches(" - "));
            }
        }
        if let Some((_, last)) = base.rsplit_once(" - ") {
            let trimmed = last.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        None
    }

    pub fn simplify_window_title(title: &str) -> std::borrow::Cow<'_, str> {
        let stripped = strip_rule_suffix(title);
        if !stripped.chars().any(|c| matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')) {
            let base = selector_base_title(stripped);
            std::borrow::Cow::Borrowed(extract_simplified_title(base).unwrap_or(base))
        } else {
            let cleaned = clean_invisible_chars(stripped);
            let base = selector_base_title(&cleaned);
            match extract_simplified_title(base) {
                Some(s) => std::borrow::Cow::Owned(s.to_owned()),
                None => std::borrow::Cow::Owned(base.to_owned()),
            }
        }
    }

    pub fn window_title(hwnd: HWND) -> Option<String> {
        let length = unsafe { GetWindowTextLengthW(hwnd) };
        if length <= 0 {
            return None;
        }
        let needed = length as usize + 1;
        let mut stack_buf = [0u16; 256];
        let mut heap_buf;
        let slice: &[u16] = if needed <= stack_buf.len() {
            let copied = unsafe { GetWindowTextW(hwnd, &mut stack_buf) };
            if copied <= 0 {
                return None;
            }
            &stack_buf[..copied as usize]
        } else {
            heap_buf = vec![0u16; needed];
            let copied = unsafe { GetWindowTextW(hwnd, &mut heap_buf) };
            if copied <= 0 {
                return None;
            }
            &heap_buf[..copied as usize]
        };
        let s = String::from_utf16_lossy(slice);
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else if trimmed.len() == s.len() {
            Some(s)
        } else {
            Some(trimmed.to_owned())
        }
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

    fn downscale_rgba(
        src_rgba: &[u8],
        src_w: usize,
        src_h: usize,
        dst_w: usize,
        dst_h: usize,
    ) -> Vec<u8> {
        if src_w == dst_w && src_h == dst_h {
            return src_rgba.to_vec();
        }
        let mut dst = vec![0u8; dst_w * dst_h * 4];
        for y in 0..dst_h {
            let src_y = (y * src_h) / dst_h;
            for x in 0..dst_w {
                let src_x = (x * src_w) / dst_w;
                let src_idx = (src_y * src_w + src_x) * 4;
                let dst_idx = (y * dst_w + x) * 4;
                if src_idx + 3 < src_rgba.len() && dst_idx + 3 < dst.len() {
                    dst[dst_idx..dst_idx + 4].copy_from_slice(&src_rgba[src_idx..src_idx + 4]);
                }
            }
        }
        dst
    }

    fn is_rgba_frame_blank(rgba: &[u8]) -> bool {
        if rgba.is_empty() {
            return true;
        }
        let mut non_zero_count = 0usize;
        let sample_step = (rgba.len() / 400).max(4) & !3;
        let mut i = 0;
        while i + 3 < rgba.len() {
            if rgba[i] > 10 || rgba[i + 1] > 10 || rgba[i + 2] > 10 {
                non_zero_count += 1;
                if non_zero_count > 5 {
                    return false;
                }
            }
            i += sample_step;
        }
        true
    }

    unsafe fn capture_window_preview_from_hwnd(
        hwnd: HWND,
        max_dimension: u32,
        client_only: bool,
    ) -> Option<WindowPreviewFrame> {
        let title = window_title(hwnd).unwrap_or_else(|| "Focused window".to_owned());

        if let Some(wgc_frame) = capture_wgc_frame(hwnd) {
            let scale = (max_dimension as f32 / wgc_frame.width as f32)
                .min(max_dimension as f32 / wgc_frame.height as f32)
                .min(1.0);
            let capture_width = ((wgc_frame.width as f32 * scale).round() as usize).max(1);
            let capture_height = ((wgc_frame.height as f32 * scale).round() as usize).max(1);
            let scaled_rgba = downscale_rgba(
                &wgc_frame.rgba,
                wgc_frame.width,
                wgc_frame.height,
                capture_width,
                capture_height,
            );
            if !is_rgba_frame_blank(&scaled_rgba) {
                return Some(WindowPreviewFrame {
                    title,
                    screen_x: wgc_frame.screen_x,
                    screen_y: wgc_frame.screen_y,
                    logical_width: wgc_frame.width as i32,
                    logical_height: wgc_frame.height as i32,
                    width: capture_width,
                    height: capture_height,
                    rgba: scaled_rgba,
                });
            }
        }

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

        let mut rgba = if copied {
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

        if is_rgba_frame_blank(&rgba) {
            if let Some(desktop_frame) =
                capture_screen_region_from_desktop(rect.left, rect.top, screen_width, screen_height)
            {
                rgba = downscale_rgba(
                    &desktop_frame.rgba,
                    desktop_frame.width,
                    desktop_frame.height,
                    capture_width as usize,
                    capture_height as usize,
                );
            }
        }

        if rgba.is_empty() || is_rgba_frame_blank(&rgba) {
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

    pub(crate) struct WgcSession {
        pub(crate) hwnd: HWND,
        _dxgi_device: windows::Graphics::DirectX::Direct3D11::IDirect3DDevice,
        d3d_device: ID3D11Device,
        frame_pool: Direct3D11CaptureFramePool,
        session: GraphicsCaptureSession,
        staging_textures: Option<([ID3D11Texture2D; 2], u32, u32)>,
        write_idx: usize,
        copies_count: usize,
    }

    unsafe impl Send for WgcSession {}
    unsafe impl Sync for WgcSession {}

    impl Drop for WgcSession {
        fn drop(&mut self) {
            let _ = self.session.Close();
            let _ = self.frame_pool.Close();
        }
    }

    static WGC_MANAGER: Lazy<Mutex<Option<WgcSession>>> = Lazy::new(|| Mutex::new(None));

    pub(crate) fn init_wgc_session(hwnd: HWND) -> anyhow::Result<WgcSession> {
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
            2,
            size,
        )?;

        let session = frame_pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;

        Ok(WgcSession {
            hwnd,
            _dxgi_device: dxgi_device_winrt,
            d3d_device,
            frame_pool,
            session,
            staging_textures: None,
            write_idx: 0,
            copies_count: 0,
        })
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[target_feature(enable = "sse4.1")]
    unsafe fn fast_streaming_copy(src: *const u8, dst: *mut u8, len: usize) {
        use std::arch::x86_64::*;
        let chunks = len / 64;
        let mut s = src as *const __m128i;
        let mut d = dst as *mut __m128i;

        for _ in 0..chunks {
            let l0 = _mm_stream_load_si128(s);
            let l1 = _mm_stream_load_si128(s.add(1));
            let l2 = _mm_stream_load_si128(s.add(2));
            let l3 = _mm_stream_load_si128(s.add(3));

            _mm_storeu_si128(d, l0);
            _mm_storeu_si128(d.add(1), l1);
            _mm_storeu_si128(d.add(2), l2);
            _mm_storeu_si128(d.add(3), l3);

            s = s.add(4);
            d = d.add(4);
        }

        let remainder = len % 64;
        if remainder > 0 {
            std::ptr::copy_nonoverlapping(s as *const u8, d as *mut u8, remainder);
        }
    }

    impl WgcSession {
        pub(crate) fn poll_into_buffer(
            &mut self,
            buffer: &mut Vec<u8>,
            expected_w: usize,
            expected_h: usize,
        ) -> anyhow::Result<bool> {
            let mut frame_opt = None;
            while let Ok(frame) = self.frame_pool.TryGetNextFrame() {
                frame_opt = Some(frame);
            }

            let Some(frame) = frame_opt else {
                return Ok(false);
            };

            let surface = frame.Surface()?;
            let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
            let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };

            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe {
                texture.GetDesc(&mut desc);
            }
            let width = desc.Width as usize;
            let height = desc.Height as usize;

            if expected_w > 0 && expected_h > 0 && (width != expected_w || height != expected_h) {
                return Ok(false);
            }

            let mut recreate_staging = true;
            if let Some((_, st_w, st_h)) = self.staging_textures {
                if st_w == desc.Width && st_h == desc.Height {
                    recreate_staging = false;
                }
            }

            if recreate_staging {
                let mut staging_desc = desc;
                staging_desc.Usage = D3D11_USAGE_STAGING;
                staging_desc.BindFlags = 0;
                staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                staging_desc.MiscFlags = 0;

                let mut s0 = None;
                let mut s1 = None;
                unsafe {
                    self.d3d_device
                        .CreateTexture2D(&staging_desc, None, Some(&mut s0))?;
                    self.d3d_device
                        .CreateTexture2D(&staging_desc, None, Some(&mut s1))?;
                }
                self.staging_textures = Some((
                    [s0.unwrap(), s1.unwrap()],
                    desc.Width,
                    desc.Height,
                ));
                self.write_idx = 0;
                self.copies_count = 0;
            }

            let (staging_textures, _, _) = self.staging_textures.as_ref().unwrap();
            let d3d_context = unsafe { self.d3d_device.GetImmediateContext()? };

            let write_idx = self.write_idx;
            unsafe {
                d3d_context.CopyResource(&staging_textures[write_idx], &texture);
            }
            drop(texture);
            drop(access);
            drop(surface);
            drop(frame);

            self.copies_count += 1;
            let read_idx = if self.copies_count > 1 {
                1 - write_idx
            } else {
                write_idx
            };

            let read_tex = &staging_textures[read_idx];
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe {
                d3d_context.Map(
                    read_tex,
                    0,
                    D3D11_MAP_READ,
                    0,
                    Some(&mut mapped),
                )?;
            }

            let pitch = mapped.RowPitch as usize;
            let row_bytes = width * 4;
            let total_bytes = row_bytes * height;
            let src_ptr = mapped.pData as *const u8;

            buffer.resize(total_bytes, 0);
            let dst_ptr = buffer.as_mut_ptr();

            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                if is_x86_feature_detected!("sse4.1") {
                    if pitch == row_bytes {
                        unsafe {
                            fast_streaming_copy(src_ptr, dst_ptr, total_bytes);
                        }
                    } else {
                        for y in 0..height {
                            let src_offset = y * pitch;
                            let dst_offset = y * row_bytes;
                            unsafe {
                                fast_streaming_copy(
                                    src_ptr.add(src_offset),
                                    dst_ptr.add(dst_offset),
                                    row_bytes,
                                );
                            }
                        }
                    }
                } else if pitch == row_bytes {
                    unsafe {
                        std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, total_bytes);
                    }
                } else {
                    for y in 0..height {
                        let src_offset = y * pitch;
                        let dst_offset = y * row_bytes;
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                src_ptr.add(src_offset),
                                dst_ptr.add(dst_offset),
                                row_bytes,
                            );
                        }
                    }
                }
            }

            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            {
                if pitch == row_bytes {
                    unsafe {
                        std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, total_bytes);
                    }
                } else {
                    for y in 0..height {
                        let src_offset = y * pitch;
                        let dst_offset = y * row_bytes;
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                src_ptr.add(src_offset),
                                dst_ptr.add(dst_offset),
                                row_bytes,
                            );
                        }
                    }
                }
            }

            unsafe {
                d3d_context.Unmap(read_tex, 0);
            }

            self.write_idx = 1 - write_idx;
            Ok(true)
        }

        pub(crate) fn get_next_frame(&mut self) -> anyhow::Result<ScreenCaptureFrame> {
            let mut rect = RECT::default();
            let _ = unsafe { GetWindowRect(self.hwnd, &mut rect) };
            let mut buf = Vec::new();
            for _ in 0..100 {
                if self.poll_into_buffer(&mut buf, 0, 0)? {
                    let (_, w, h) = self.staging_textures.as_ref().unwrap();
                    return Ok(ScreenCaptureFrame {
                        screen_x: rect.left,
                        screen_y: rect.top,
                        width: *w as usize,
                        height: *h as usize,
                        rgba: buf,
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            anyhow::bail!("No frame available from WGC pool")
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
        pub process_id: u32,
        pub process_path: String,
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

    pub fn process_id_for_window(_selector: Option<&str>) -> Option<u32> {
        None
    }

    pub fn window_client_bounds(_selector: Option<&str>) -> Option<(i32, i32, i32, i32)> {
        None
    }

    pub fn window_at_point(_screen_x: i32, _screen_y: i32) -> Option<(String, (i32, i32, i32, i32))> {
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

    pub(crate) fn close_window_capture_session() {}
}

#[cfg(not(windows))]
pub use fallback::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use windows::Win32::Foundation::HWND;

    #[test]
    fn clean_invisible_chars_borrows_clean_string() {
        let clean = "Normal Window Title";
        assert!(matches!(clean_invisible_chars(clean), Cow::Borrowed(_)));
        assert_eq!(clean_invisible_chars(clean), "Normal Window Title");
    }

    #[test]
    fn clean_invisible_chars_cleans_zero_width_and_bom() {
        let dirty = "A\u{200B}B\u{200C}C\u{200D}D\u{FEFF}E";
        assert!(matches!(clean_invisible_chars(dirty), Cow::Owned(_)));
        assert_eq!(clean_invisible_chars(dirty), "ABCDE");
    }

    #[test]
    fn selector_base_title_extracts_base() {
        assert_eq!(selector_base_title("Calculator (0x1234)"), "Calculator");
        assert_eq!(selector_base_title("Plain Title"), "Plain Title");
    }

    #[test]
    fn simplify_window_title_borrows_typical_strings() {
        let chrome = "GitHub - Google Chrome";
        let simplified = simplify_window_title(chrome);
        assert!(matches!(simplified, Cow::Borrowed(_)));
        assert_eq!(simplified, "Google Chrome");

        let ide = "main.rs - Antigravity IDE";
        assert_eq!(simplify_window_title(ide), "Antigravity IDE");

        let dash = "Project Name - Subtitle";
        assert_eq!(simplify_window_title(dash), "Subtitle");
    }

    #[test]
    fn window_match_rules_and_browser_matching() {
        assert_eq!(strip_rule_suffix("Window [Lowest]"), "Window");
        assert_eq!(parse_window_match_rule("Window [Lowest]").1, Some(WindowMatchRule::Lowest));
        assert_eq!(strip_rule_suffix("Window [Highest]"), "Window");
        assert_eq!(parse_window_match_rule("Window [Highest]").1, Some(WindowMatchRule::Highest));
        assert_eq!(strip_rule_suffix("Window [Leftmost]"), "Window");
        assert_eq!(parse_window_match_rule("Window [Leftmost]").1, Some(WindowMatchRule::Leftmost));
        assert_eq!(strip_rule_suffix("Window [Rightmost]"), "Window");
        assert_eq!(parse_window_match_rule("Window [Rightmost]").1, Some(WindowMatchRule::Rightmost));
        assert_eq!(strip_rule_suffix("Window [The Unfocused One]"), "Window");
        assert_eq!(parse_window_match_rule("Window [The Unfocused One]").1, Some(WindowMatchRule::Unfocused));
        assert_eq!(strip_rule_suffix("Window [Unfocused]"), "Window");
        assert_eq!(parse_window_match_rule("Window [Unfocused]").1, Some(WindowMatchRule::Unfocused));
        assert_eq!(strip_rule_suffix("Window [The Focused One]"), "Window");
        assert_eq!(parse_window_match_rule("Window [The Focused One]").1, Some(WindowMatchRule::Focused));
        assert_eq!(strip_rule_suffix("Window [Focused]"), "Window");
        assert_eq!(parse_window_match_rule("Window [Focused]").1, Some(WindowMatchRule::Focused));
        assert!(has_position_rule_suffix("Microsoft Edge [Leftmost]"));
        assert!(has_position_rule_suffix("Microsoft Edge [The Unfocused One]"));
        assert!(has_position_rule_suffix("Microsoft Edge [Unfocused]"));
        assert!(has_position_rule_suffix("Microsoft Edge [The Focused One]"));
        assert!(has_position_rule_suffix("Microsoft Edge [Focused]"));
        assert!(!has_position_rule_suffix("Microsoft Edge"));

        assert!(window_matches_candidate_title(
            "Doc - Google Chrome",
            "Doc - Google Chrome (0x100)",
            "Other - Google Chrome",
            false
        ));

        // Ensure IdentityV does not match Edge or empty titles
        assert!(!window_matches_candidate_title(
            "New Tab - Microsoft Edge",
            "New Tab - Microsoft Edge (0x200)",
            "IdentityV",
            false
        ));
        assert!(!window_matches_candidate_title(
            "",
            "",
            "IdentityV",
            false
        ));
        assert!(!window_matches_candidate_title(
            "v",
            "v (0x300)",
            "IdentityV",
            false
        ));
        assert!(window_matches_candidate_title(
            "IdentityV",
            "IdentityV (0x400)",
            "IdentityV",
            false
        ));
        assert!(window_matches_candidate_title(
            "IdentityV - Patch 1.0",
            "IdentityV - Patch 1.0 (0x400)",
            "IdentityV",
            false
        ));

        // When match_duplicate_window_titles is false and target is a specific selector:
        // ONLY the window with the exact selector must match.
        assert!(!window_matches_candidate_title(
            "Roblox",
            "Roblox (0x200)",
            "Roblox (0x100)",
            false
        ));
        assert!(window_matches_candidate_title(
            "Roblox",
            "Roblox (0x100)",
            "Roblox (0x100)",
            false
        ));

        // When match_duplicate_window_titles is true:
        // Any duplicate window sharing the base title matches.
        assert!(window_matches_candidate_title(
            "Roblox",
            "Roblox (0x200)",
            "Roblox (0x100)",
            true
        ));

        // Substring and unrelated subtitle false positives must be rejected
        assert!(!window_matches_candidate_title(
            "Epic Games Launcher",
            "Epic Games Launcher (0x500)",
            "Game",
            false
        ));
        assert!(!window_matches_candidate_title(
            "Game - v1.0",
            "Game - v1.0 (0x600)",
            "Settings - v1.0",
            false
        ));
    }

    #[test]
    fn select_window_by_match_rule_focused_unfocused() {
        let fake_hwnd_1 = HWND(0x1000 as *mut _);
        let fake_hwnd_2 = HWND(0x2000 as *mut _);
        let candidates = vec![fake_hwnd_1, fake_hwnd_2];

        // Ensure clear cache initially
        crate::overlay::FOREGROUND_WINDOW_HWND.store(0, std::sync::atomic::Ordering::Relaxed);

        // When foreground is outside candidates
        let focused = select_window_by_match_rule(&candidates, WindowMatchRule::Focused);
        let unfocused = select_window_by_match_rule(&candidates, WindowMatchRule::Unfocused);

        assert_eq!(focused, Some(fake_hwnd_1));
        assert_eq!(unfocused, Some(fake_hwnd_2));
        assert_ne!(focused, unfocused);

        // When fake_hwnd_2 becomes foreground (e.g. user Alt-Tabs to window 2)
        crate::overlay::FOREGROUND_WINDOW_HWND.store(0x2000, std::sync::atomic::Ordering::Relaxed);
        let focused_2 = select_window_by_match_rule(&candidates, WindowMatchRule::Focused);
        let unfocused_2 = select_window_by_match_rule(&candidates, WindowMatchRule::Unfocused);
        assert_eq!(focused_2, Some(fake_hwnd_2));
        assert_eq!(unfocused_2, Some(fake_hwnd_1));
        assert_ne!(focused_2, unfocused_2);

        // When user Alt-Tabs back to window 1
        crate::overlay::FOREGROUND_WINDOW_HWND.store(0x1000, std::sync::atomic::Ordering::Relaxed);
        let focused_1 = select_window_by_match_rule(&candidates, WindowMatchRule::Focused);
        let unfocused_1 = select_window_by_match_rule(&candidates, WindowMatchRule::Unfocused);
        assert_eq!(focused_1, Some(fake_hwnd_1));
        assert_eq!(unfocused_1, Some(fake_hwnd_2));
        assert_ne!(focused_1, unfocused_1);

        // Clean up
        crate::overlay::FOREGROUND_WINDOW_HWND.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}
