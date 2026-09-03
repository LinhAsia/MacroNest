#![allow(unsafe_op_in_unsafe_fn)]

use windows::Win32::{
    Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleBitmap,
        CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush,
        DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject,
        DrawTextW, EndPaint, FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION,
        FONT_QUALITY, FW_BOLD, FW_NORMAL, FW_SEMIBOLD, FillRect, GetDC, HDC, HFONT, HGDIOBJ, LineTo, MoveToEx, PAINTSTRUCT,
        PS_SOLID, Rectangle, ReleaseDC, SRCCOPY, SelectObject, SetBkMode, SetTextColor,
        SetViewportOrgEx, StretchDIBits, TRANSPARENT, UpdateWindow,
    },
    UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN, VK_SHIFT},
    UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
        DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW, HCURSOR,
        HWND_TOPMOST, IDC_ARROW, IDC_CROSS, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE,
        IDC_SIZEWE, IMAGE_CURSOR, KillTimer, LR_SHARED, LoadCursorW, LoadImageW, MSG, PostMessageW,
        PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOW, SW_SHOWNORMAL, SWP_NOACTIVATE,
        SWP_SHOWWINDOW, SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
        WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CREATE, WM_DESTROY, WM_ERASEBKGND,
        WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_PAINT,
        WM_RBUTTONUP, WM_SETCURSOR, WM_SYSKEYUP, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP,
    },
};
use windows::core::w;

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionSelectKind {
    ImageTemplate,
    ImageSearchArea,
    Screenshot,
    Ocr,
    VideoRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeCaptureMode {
    ProtractorCalibration {
        ui_language: crate::model::UiLanguage,
    },
    DistanceMeasure {
        ui_language: crate::model::UiLanguage,
    },
    RegionSelect {
        kind: RegionSelectKind,
        ui_language: crate::model::UiLanguage,
        hold_hotkey: Option<crate::model::HotkeyBinding>,
    },
    PointClick {
        ui_language: crate::model::UiLanguage,
        dim_background: bool,
    },
    RegionAdjust {
        // Initial region in screen coordinates
        initial_x: i32,
        initial_y: i32,
        initial_w: i32,
        initial_h: i32,
        ui_language: crate::model::UiLanguage,
    },
}

#[derive(Debug, Clone)]
pub enum NativeCaptureResult {
    Cancelled,
    ProtractorPoints(Vec<(i32, i32)>),
    DistancePoints(Vec<(i32, i32)>),
    SelectedRegion {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    ClickedPoint {
        x: i32,
        y: i32,
        color: Option<crate::model::RgbaColor>,
    },
    AdjustedRegion {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdjustDragKind {
    Move,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

struct CaptureState {
    capture_frame: crate::window_list::ScreenCaptureFrame,
    original_bgra: Vec<u8>,
    dimmed_bgra: Vec<u8>,
    render_bgra: Vec<u8>,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    mode: NativeCaptureMode,

    // Interaction state
    start_point: Option<(i32, i32)>,
    current_point: Option<(i32, i32)>,
    protractor_points: Vec<(i32, i32)>,

    // RegionAdjust state
    adjust_rect: RECT, // in window-local coords
    adjust_drag: Option<AdjustDragKind>,
    adjust_drag_origin: (i32, i32), // cursor pos when drag started
    adjust_rect_origin: RECT,       // rect state when drag started

    // Result
    result: NativeCaptureResult,
    created_at: std::time::Instant,

    // GDI caching to avoid per-frame allocations & full screen blits
    bg_dc: Option<HDC>,
    bg_bmp: Option<windows::Win32::Graphics::Gdi::HBITMAP>,
    paint_dc: Option<HDC>,
    paint_bmp: Option<windows::Win32::Graphics::Gdi::HBITMAP>,
    last_panel_rect: Option<RECT>,
    last_cursor_near_pill: bool,
    last_pill_rect: Option<RECT>,
}

impl Drop for CaptureState {
    fn drop(&mut self) {
        unsafe {
            use windows::Win32::Graphics::Gdi::{DeleteDC, DeleteObject, HGDIOBJ};
            if let Some(dc) = self.bg_dc.take() {
                let _ = DeleteDC(dc);
            }
            if let Some(bmp) = self.bg_bmp.take() {
                let _ = DeleteObject(HGDIOBJ(bmp.0));
            }
            if let Some(dc) = self.paint_dc.take() {
                let _ = DeleteDC(dc);
            }
            if let Some(bmp) = self.paint_bmp.take() {
                let _ = DeleteObject(HGDIOBJ(bmp.0));
            }
        }
    }
}

fn point_click_panel_rect(width: i32, height: i32, curr: (i32, i32)) -> RECT {
    let panel_w = 184i32;
    let panel_h = 246i32;
    let margin = 24i32;

    let pointer_x = curr.0;
    let pointer_y = curr.1;
    let safe_r = 70i32;
    let safe_left = pointer_x - safe_r;
    let safe_right = pointer_x + safe_r;
    let safe_top = pointer_y - safe_r;
    let safe_bottom = pointer_y + safe_r;

    let candidates = [
        (width - panel_w - margin, margin),
        (margin, margin),
        (width - panel_w - margin, height - panel_h - margin),
        (margin, height - panel_h - margin),
    ];

    let mut panel_x = candidates[0].0;
    let mut panel_y = candidates[0].1;

    for &(cx, cy) in &candidates {
        let intersects = !(cx + panel_w < safe_left
            || cx > safe_right
            || cy + panel_h < safe_top
            || cy > safe_bottom);
        if !intersects {
            panel_x = cx;
            panel_y = cy;
            break;
        }
    }

    RECT {
        left: panel_x - 2,
        top: panel_y - 2,
        right: panel_x + panel_w + 4,
        bottom: panel_y + panel_h + 4,
    }
}

impl CaptureState {
    fn new(
        capture_frame: crate::window_list::ScreenCaptureFrame,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        mode: NativeCaptureMode,
    ) -> Self {
        let adjust_rect = if let NativeCaptureMode::RegionAdjust {
            initial_x,
            initial_y,
            initial_w,
            initial_h,
            ..
        } = mode
        {
            RECT {
                left: initial_x - left,
                top: initial_y - top,
                right: initial_x - left + initial_w,
                bottom: initial_y - top + initial_h,
            }
        } else {
            RECT::default()
        };

        let mut original_bgra = capture_frame.rgba.clone();
        for pixel in original_bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }

        let mut dimmed_bgra = original_bgra.clone();
        for pixel in dimmed_bgra.chunks_exact_mut(4) {
            pixel[0] = ((pixel[0] as u16 * 127) / 255) as u8;
            pixel[1] = ((pixel[1] as u16 * 127) / 255) as u8;
            pixel[2] = ((pixel[2] as u16 * 127) / 255) as u8;
        }

        let render_bgra = dimmed_bgra.clone();

        let (start_point, current_point) = if let NativeCaptureMode::RegionSelect {
            hold_hotkey: Some(_),
            ..
        } = &mode
        {
            let mut pt = POINT::default();
            if unsafe { GetCursorPos(&mut pt).is_ok() } {
                let p = (pt.x - left, pt.y - top);
                (Some(p), Some(p))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Self {
            capture_frame,
            original_bgra,
            dimmed_bgra,
            render_bgra,
            left,
            top,
            width,
            height,
            mode,
            start_point,
            current_point,
            protractor_points: Vec::new(),
            adjust_rect,
            adjust_drag: None,
            adjust_drag_origin: (0, 0),
            adjust_rect_origin: RECT::default(),
            result: NativeCaptureResult::Cancelled,
            created_at: std::time::Instant::now(),
            bg_dc: None,
            bg_bmp: None,
            paint_dc: None,
            paint_bmp: None,
            last_panel_rect: None,
            last_cursor_near_pill: false,
            last_pill_rect: None,
        }
    }
}





pub fn run_capture_overlay(
    capture_frame: crate::window_list::ScreenCaptureFrame,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    mode: NativeCaptureMode,
) -> NativeCaptureResult {
    unsafe {
        let instance = HINSTANCE(
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .unwrap()
                .0,
        );
        let class_name = w!("MacroNestCaptureWindow");

        let mut class = WNDCLASSW::default();
        class.lpfnWndProc = Some(capture_wnd_proc);
        class.hInstance = instance;
        class.lpszClassName = class_name;
        class.style = CS_HREDRAW | CS_VREDRAW;
        class.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();

        // Register class (ignore error if already registered)
        let _ = RegisterClassW(&class);

        let mut state = CaptureState::new(capture_frame, left, top, width, height, mode);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Capture Overlay"),
            WS_POPUP,
            left,
            top,
            width,
            height,
            None,
            None,
            Some(instance),
            Some(&mut state as *mut CaptureState as *const std::ffi::c_void),
        )
        .unwrap();

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            left,
            top,
            width,
            height,
            SWP_SHOWWINDOW,
        );
        let _ = SetForegroundWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        state.result.clone()
    }
}

unsafe extern "system" fn capture_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_NCCREATE => {
            let cs = lparam.0 as *const CREATESTRUCTW;
            let state = (*cs).lpCreateParams as *mut CaptureState;
            SetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0), state as isize);
            LRESULT(1)
        }
        WM_CREATE => {
            let state = get_state(hwnd);
            if let Some(state) = state
                && matches!(state.mode, NativeCaptureMode::RegionSelect { hold_hotkey: Some(_), .. })
            {
                let _ = SetTimer(Some(hwnd), 1, 10, None);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let state = get_state(hwnd);
            if let Some(state) = state {
                if let NativeCaptureMode::RegionSelect { hold_hotkey: Some(ref trigger), .. } = state.mode {
                    let mut pt = POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let rx = pt.x - state.left;
                        let ry = pt.y - state.top;
                        if state.current_point != Some((rx, ry)) {
                            state.current_point = Some((rx, ry));
                            unsafe {
                                InvalidateRect(hwnd, None, false);
                            }
                        }
                    }

                    if state.created_at.elapsed() >= std::time::Duration::from_millis(50)
                        && !crate::overlay::screen_draw_trigger_binding_is_down(trigger)
                    {
                        let _ = KillTimer(Some(hwnd), 1);
                        if let Some(start) = state.start_point {
                            let mut pt = POINT::default();
                            let (rx, ry) = if GetCursorPos(&mut pt).is_ok() {
                                (pt.x - state.left, pt.y - state.top)
                            } else if let Some(cur) = state.current_point {
                                cur
                            } else {
                                start
                            };
                            let x1 = start.0;
                            let y1 = start.1;
                            let x2 = rx;
                            let y2 = ry;
                            let fx = x1.min(x2) + state.left;
                            let fy = y1.min(y2) + state.top;
                            let fw = (x1 - x2).abs();
                            let fh = (y1 - y2).abs();
                            if fw >= 2 && fh >= 2 {
                                state.result = NativeCaptureResult::SelectedRegion {
                                    x: fx,
                                    y: fy,
                                    width: fw,
                                    height: fh,
                                };
                            }
                        }
                        let _ = DestroyWindow(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_KEYUP | WM_SYSKEYUP => {
            let state = get_state(hwnd);
            if let Some(state) = state {
                if let NativeCaptureMode::RegionSelect { hold_hotkey: Some(ref trigger), .. } = state.mode {
                    if state.created_at.elapsed() >= std::time::Duration::from_millis(50)
                        && !crate::overlay::screen_draw_trigger_binding_is_down(trigger)
                    {
                        let _ = KillTimer(Some(hwnd), 1);
                        if let Some(start) = state.start_point {
                            let mut pt = POINT::default();
                            let (rx, ry) = if GetCursorPos(&mut pt).is_ok() {
                                (pt.x - state.left, pt.y - state.top)
                            } else if let Some(cur) = state.current_point {
                                cur
                            } else {
                                start
                            };
                            let x1 = start.0;
                            let y1 = start.1;
                            let x2 = rx;
                            let y2 = ry;
                            let fx = x1.min(x2) + state.left;
                            let fy = y1.min(y2) + state.top;
                            let fw = (x1 - x2).abs();
                            let fh = (y1 - y2).abs();
                            if fw >= 2 && fh >= 2 {
                                state.result = NativeCaptureResult::SelectedRegion {
                                    x: fx,
                                    y: fy,
                                    width: fw,
                                    height: fh,
                                };
                            }
                        }
                        let _ = DestroyWindow(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if let Some(state) = get_state(hwnd)
                && matches!(state.mode, NativeCaptureMode::PointClick { .. })
            {
                unsafe {
                    SetCursor(None);
                    return LRESULT(1);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let state = get_state(hwnd);
            if let Some(state) = state {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let rx = pt.x - state.left;
                    let ry = pt.y - state.top;
                    if matches!(state.mode, NativeCaptureMode::RegionAdjust { .. }) {
                        // Determine drag kind from hit test
                        let kind = adjust_hit_test(&state.adjust_rect, rx, ry);
                        state.adjust_drag = Some(kind);
                        state.adjust_drag_origin = (rx, ry);
                        state.adjust_rect_origin = state.adjust_rect;
                        SetCapture(hwnd);
                        InvalidateRect(hwnd, None, false);
                    } else {
                        state.start_point = Some((rx, ry));
                        state.current_point = Some((rx, ry));
                        SetCapture(hwnd);
                        unsafe {
                            let dirty = RECT {
                                left: (rx - 8).max(0),
                                top: (ry - 8).max(0),
                                right: (rx + 8).min(state.width),
                                bottom: (ry + 8).min(state.height),
                            };
                            InvalidateRect(hwnd, Some(&dirty), false);
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let state = get_state(hwnd);
            if let Some(state) = state {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let rx = pt.x - state.left;
                    let ry = pt.y - state.top;
                    if matches!(state.mode, NativeCaptureMode::RegionAdjust { .. }) {
                        if let Some(drag) = state.adjust_drag {
                            // Apply drag delta to rect
                            let dx = rx - state.adjust_drag_origin.0;
                            let dy = ry - state.adjust_drag_origin.1;
                            let ro = state.adjust_rect_origin;
                            let min_size = 4i32;
                            state.adjust_rect = apply_adjust_drag(
                                ro,
                                drag,
                                dx,
                                dy,
                                min_size,
                                state.width,
                                state.height,
                            );
                        }
                        unsafe {
                            InvalidateRect(hwnd, None, false);
                        }
                    } else if matches!(state.mode, NativeCaptureMode::PointClick { .. }) {
                        let old_pt = state.current_point;
                        state.current_point = Some((rx, ry));
                        let new_panel_rect = point_click_panel_rect(state.width, state.height, (rx, ry));

                        let pill_rect = state.last_pill_rect.unwrap_or(RECT {
                            left: (state.width - 750) / 2,
                            top: 36,
                            right: (state.width + 750) / 2,
                            bottom: 84,
                        });
                        let margin_x = 40;
                        let margin_y = 30;
                        let is_near_pill = rx >= pill_rect.left - margin_x
                            && rx <= pill_rect.right + margin_x
                            && ry >= pill_rect.top - margin_y
                            && ry <= pill_rect.bottom + margin_y;

                        let pill_transition = is_near_pill != state.last_cursor_near_pill;
                        state.last_cursor_near_pill = is_near_pill;

                        unsafe {
                            if pill_transition {
                                InvalidateRect(hwnd, Some(&pill_rect), false);
                            }

                            if let Some((ox, oy)) = old_pt {
                                let old_cross_rect = RECT {
                                    left: (ox - 18).max(0),
                                    top: (oy - 18).max(0),
                                    right: (ox + 19).min(state.width),
                                    bottom: (oy + 19).min(state.height),
                                };
                                InvalidateRect(hwnd, Some(&old_cross_rect), false);
                            }
                            let new_cross_rect = RECT {
                                left: (rx - 18).max(0),
                                top: (ry - 18).max(0),
                                right: (rx + 19).min(state.width),
                                bottom: (ry + 19).min(state.height),
                            };
                            InvalidateRect(hwnd, Some(&new_cross_rect), false);

                            if let Some(old_rect) = state.last_panel_rect {
                                if old_rect != new_panel_rect {
                                    InvalidateRect(hwnd, Some(&old_rect), false);
                                    InvalidateRect(hwnd, Some(&new_panel_rect), false);
                                } else {
                                    InvalidateRect(hwnd, Some(&new_panel_rect), false);
                                }
                            } else {
                                InvalidateRect(hwnd, Some(&new_panel_rect), false);
                            }
                            state.last_panel_rect = Some(new_panel_rect);
                        }
                    } else {
                        state.current_point =
                            Some(distance_measure_constrained_local_point(state, (rx, ry)));
                        unsafe {
                            InvalidateRect(hwnd, None, false);
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let state = get_state(hwnd);
            if let Some(state) = state {
                let _ = ReleaseCapture();
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let rx = pt.x - state.left;
                    let ry = pt.y - state.top;
                    let constrained_point =
                        distance_measure_constrained_local_point(state, (rx, ry));

                    match state.mode {
                        NativeCaptureMode::RegionAdjust { .. } => {
                            // End drag, keep rect as-is (confirm via Enter or right-click)
                            state.adjust_drag = None;
                            unsafe {
                                InvalidateRect(hwnd, None, false);
                            }
                        }
                        NativeCaptureMode::RegionSelect { .. } => {
                            if let Some(start) = state.start_point {
                                let x1 = start.0;
                                let y1 = start.1;
                                let x2 = rx;
                                let y2 = ry;

                                let fx = x1.min(x2) + state.left;
                                let fy = y1.min(y2) + state.top;
                                let fw = (x1 - x2).abs();
                                let fh = (y1 - y2).abs();

                                if fw >= 2 && fh >= 2 {
                                    state.result = NativeCaptureResult::SelectedRegion {
                                        x: fx,
                                        y: fy,
                                        width: fw,
                                        height: fh,
                                    };
                                }
                            }
                            let _ = DestroyWindow(hwnd);
                        }
                        NativeCaptureMode::PointClick { .. } => {
                            let w = state.width;
                            let rx_u = rx.clamp(0, state.width - 1) as usize;
                            let ry_u = ry.clamp(0, state.height - 1) as usize;
                            let idx = (ry_u * w as usize + rx_u) * 4;
                            let color = if idx + 3 < state.capture_frame.rgba.len() {
                                let r = state.capture_frame.rgba[idx];
                                let g = state.capture_frame.rgba[idx + 1];
                                let b = state.capture_frame.rgba[idx + 2];
                                let a = state.capture_frame.rgba[idx + 3];
                                Some(crate::model::RgbaColor { r, g, b, a })
                            } else {
                                None
                            };

                            state.result = NativeCaptureResult::ClickedPoint {
                                x: rx + state.left,
                                y: ry + state.top,
                                color,
                            };
                            let _ = DestroyWindow(hwnd);
                        }
                        NativeCaptureMode::ProtractorCalibration { .. } => {
                            state
                                .protractor_points
                                .push((rx + state.left, ry + state.top));
                            state.start_point = None;
                            if state.protractor_points.len() == 3 {
                                state.result = NativeCaptureResult::ProtractorPoints(
                                    state.protractor_points.clone(),
                                );
                                let _ = DestroyWindow(hwnd);
                            } else {
                                unsafe {
                                    InvalidateRect(hwnd, None, false);
                                }
                            }
                        }
                        NativeCaptureMode::DistanceMeasure { .. } => {
                            state.protractor_points.push((
                                constrained_point.0 + state.left,
                                constrained_point.1 + state.top,
                            ));
                            state.start_point = None;
                            if state.protractor_points.len() == 2 {
                                state.result = NativeCaptureResult::DistancePoints(
                                    state.protractor_points.clone(),
                                );
                                let _ = DestroyWindow(hwnd);
                            } else {
                                unsafe {
                                    InvalidateRect(hwnd, None, false);
                                }
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let state = get_state(hwnd);
            if let Some(state) = state {
                if matches!(state.mode, NativeCaptureMode::RegionAdjust { .. }) {
                    // Right-click = confirm current rect
                    let ar = state.adjust_rect;
                    let rw = (ar.right - ar.left).abs();
                    let rh = (ar.bottom - ar.top).abs();
                    if rw >= 2 && rh >= 2 {
                        state.result = NativeCaptureResult::AdjustedRegion {
                            x: ar.left.min(ar.right) + state.left,
                            y: ar.top.min(ar.bottom) + state.top,
                            width: rw,
                            height: rh,
                        };
                    }
                    let _ = DestroyWindow(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let state = get_state(hwnd);
            if wparam.0 == VK_ESCAPE.0 as usize {
                let _ = DestroyWindow(hwnd);
            } else if wparam.0 == VK_RETURN.0 as usize {
                if let Some(state) = state {
                    if matches!(state.mode, NativeCaptureMode::RegionAdjust { .. }) {
                        let ar = state.adjust_rect;
                        let rw = (ar.right - ar.left).abs();
                        let rh = (ar.bottom - ar.top).abs();
                        if rw >= 2 && rh >= 2 {
                            state.result = NativeCaptureResult::AdjustedRegion {
                                x: ar.left.min(ar.right) + state.left,
                                y: ar.top.min(ar.bottom) + state.top,
                                width: rw,
                                height: rh,
                            };
                        }
                        let _ = DestroyWindow(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.0.is_null() {
                let state = get_state(hwnd);
                if let Some(state) = state {
                    if matches!(state.mode, NativeCaptureMode::PointClick { .. }) {
                        let mut pt = POINT::default();
                        if GetCursorPos(&mut pt).is_ok() {
                            state.current_point = Some((pt.x - state.left, pt.y - state.top));
                        }
                    }
                    let _ = draw_capture_to_dc(hdc, state, Some(ps.rcPaint));
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn get_state<'a>(hwnd: HWND) -> Option<&'a mut CaptureState> {
    let ptr = GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0));
    if ptr == 0 {
        None
    } else {
        Some(&mut *(ptr as *mut CaptureState))
    }
}

unsafe fn InvalidateRect(hwnd: HWND, rect: Option<&RECT>, erase: bool) {
    let lp_rect = match rect {
        Some(r) => r as *const RECT,
        None => std::ptr::null(),
    };
    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), Some(lp_rect), erase);
}

const ADJUST_HANDLE_RADIUS: i32 = 10;

fn adjust_hit_test(r: &RECT, rx: i32, ry: i32) -> AdjustDragKind {
    let cx = (r.left + r.right) / 2;
    let cy = (r.top + r.bottom) / 2;
    let hr = ADJUST_HANDLE_RADIUS;
    // Check 8 handles in priority order: corners first, then edges
    let handles = [
        (r.left, r.top, AdjustDragKind::NW),
        (r.right, r.top, AdjustDragKind::NE),
        (r.right, r.bottom, AdjustDragKind::SE),
        (r.left, r.bottom, AdjustDragKind::SW),
        (cx, r.top, AdjustDragKind::N),
        (r.right, cy, AdjustDragKind::E),
        (cx, r.bottom, AdjustDragKind::S),
        (r.left, cy, AdjustDragKind::W),
    ];
    for (hx, hy, kind) in handles {
        if (rx - hx).abs() <= hr && (ry - hy).abs() <= hr {
            return kind;
        }
    }
    AdjustDragKind::Move
}

fn apply_adjust_drag(
    origin: RECT,
    drag: AdjustDragKind,
    dx: i32,
    dy: i32,
    min_size: i32,
    max_w: i32,
    max_h: i32,
) -> RECT {
    let mut r = origin;
    match drag {
        AdjustDragKind::Move => {
            let nw = (r.right - r.left).max(min_size);
            let nh = (r.bottom - r.top).max(min_size);
            r.left = (r.left + dx).clamp(0, max_w - nw);
            r.top = (r.top + dy).clamp(0, max_h - nh);
            r.right = r.left + nw;
            r.bottom = r.top + nh;
        }
        AdjustDragKind::N => {
            r.top = (r.top + dy).clamp(0, r.bottom - min_size);
        }
        AdjustDragKind::S => {
            r.bottom = (r.bottom + dy).clamp(r.top + min_size, max_h);
        }
        AdjustDragKind::W => {
            r.left = (r.left + dx).clamp(0, r.right - min_size);
        }
        AdjustDragKind::E => {
            r.right = (r.right + dx).clamp(r.left + min_size, max_w);
        }
        AdjustDragKind::NW => {
            r.top = (r.top + dy).clamp(0, r.bottom - min_size);
            r.left = (r.left + dx).clamp(0, r.right - min_size);
        }
        AdjustDragKind::NE => {
            r.top = (r.top + dy).clamp(0, r.bottom - min_size);
            r.right = (r.right + dx).clamp(r.left + min_size, max_w);
        }
        AdjustDragKind::SE => {
            r.bottom = (r.bottom + dy).clamp(r.top + min_size, max_h);
            r.right = (r.right + dx).clamp(r.left + min_size, max_w);
        }
        AdjustDragKind::SW => {
            r.bottom = (r.bottom + dy).clamp(r.top + min_size, max_h);
            r.left = (r.left + dx).clamp(0, r.right - min_size);
        }
    }
    r
}

unsafe fn draw_region_adjust_to_dc(hdc: HDC, state: &mut CaptureState) -> anyhow::Result<()> {
    let mem_dc = CreateCompatibleDC(Some(hdc));
    let mem_bmp = CreateCompatibleBitmap(hdc, state.width, state.height);
    let old_bmp = SelectObject(mem_dc, HGDIOBJ(mem_bmp.0));

    let ar = state.adjust_rect;
    let sw = state.width as usize;

    state.render_bgra.copy_from_slice(&state.dimmed_bgra);

    let sel_l = ar.left.clamp(0, state.width) as usize;
    let sel_t = ar.top.clamp(0, state.height) as usize;
    let sel_r = ar.right.clamp(0, state.width) as usize;
    let sel_b = ar.bottom.clamp(0, state.height) as usize;
    let sel_w = sel_r.saturating_sub(sel_l);
    let sel_h = sel_b.saturating_sub(sel_t);
    if sel_w > 0 && sel_h > 0 {
        blit_rect(
            &state.original_bgra,
            sw,
            &mut state.render_bgra,
            sw,
            sel_l,
            sel_t,
            sel_w,
            sel_h,
        );
    }

    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: state.width,
        biHeight: -state.height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    let _ = StretchDIBits(
        mem_dc,
        0,
        0,
        state.width,
        state.height,
        0,
        0,
        state.width,
        state.height,
        Some(state.render_bgra.as_ptr() as *const std::ffi::c_void),
        &bmi,
        DIB_RGB_COLORS,
        SRCCOPY,
    );

    // Draw selection border
    let pen = CreatePen(PS_SOLID, 2, rgb(0, 160, 255));
    let old_pen = SelectObject(mem_dc, HGDIOBJ(pen.0));
    let null_brush =
        windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::NULL_BRUSH);
    let old_brush = SelectObject(mem_dc, null_brush);
    let _ = windows::Win32::Graphics::Gdi::Rectangle(mem_dc, ar.left, ar.top, ar.right, ar.bottom);
    let _ = SelectObject(mem_dc, old_pen);
    let _ = SelectObject(mem_dc, old_brush);
    let _ = DeleteObject(HGDIOBJ(pen.0));

    // Draw 8 resize handles (filled squares with blue border)
    let hr = ADJUST_HANDLE_RADIUS as i32;
    let cx = (ar.left + ar.right) / 2;
    let cy = (ar.top + ar.bottom) / 2;
    let handle_centers = [
        (ar.left, ar.top),
        (cx, ar.top),
        (ar.right, ar.top),
        (ar.right, cy),
        (ar.right, ar.bottom),
        (cx, ar.bottom),
        (ar.left, ar.bottom),
        (ar.left, cy),
    ];
    let h_fill = CreateSolidBrush(rgb(220, 238, 255));
    let h_pen = CreatePen(PS_SOLID, 2, rgb(0, 130, 220));
    let old_pen = SelectObject(mem_dc, HGDIOBJ(h_pen.0));
    let old_brush = SelectObject(mem_dc, HGDIOBJ(h_fill.0));
    for (hx, hy) in handle_centers {
        let _ = windows::Win32::Graphics::Gdi::Rectangle(
            mem_dc,
            hx - hr / 2,
            hy - hr / 2,
            hx + hr / 2,
            hy + hr / 2,
        );
    }
    let _ = SelectObject(mem_dc, old_pen);
    let _ = SelectObject(mem_dc, old_brush);
    let _ = DeleteObject(HGDIOBJ(h_pen.0));
    let _ = DeleteObject(HGDIOBJ(h_fill.0));

    // Size label badge
    let rw = (ar.right - ar.left).abs();
    let rh = (ar.bottom - ar.top).abs();
    let size_text = format!("{rw} × {rh}");
    let font = CreateFontW(
        15,
        0,
        0,
        0,
        FW_BOLD.0 as i32,
        0,
        0,
        0,
        FONT_CHARSET(0),
        FONT_OUTPUT_PRECISION(0),
        FONT_CLIP_PRECISION(0),
        FONT_QUALITY(0),
        0,
        w!("Segoe UI"),
    );
    let old_font = SelectObject(mem_dc, HGDIOBJ(font.0));
    let mut sz_u16: Vec<u16> = size_text.encode_utf16().collect();
    let mut sz_calc = RECT::default();
    let _ = DrawTextW(mem_dc, &mut sz_u16, &mut sz_calc, DT_CALCRECT);
    let text_w = sz_calc.right - sz_calc.left;
    let text_h = sz_calc.bottom - sz_calc.top;

    let badge_w = text_w + 16;
    let badge_h = text_h + 8;
    let badge_x = ar.left.clamp(4, (state.width - badge_w - 4).max(4));
    let badge_y = if ar.top >= badge_h + 6 {
        ar.top - badge_h - 4
    } else {
        (ar.top + 6).min((state.height - badge_h - 4).max(0))
    };

    let b_brush = CreateSolidBrush(rgb(15, 23, 42));
    let b_pen = CreatePen(PS_SOLID, 1, rgb(0, 160, 255));
    let old_b = SelectObject(mem_dc, HGDIOBJ(b_brush.0));
    let old_p = SelectObject(mem_dc, HGDIOBJ(b_pen.0));

    let _ = windows::Win32::Graphics::Gdi::RoundRect(
        mem_dc,
        badge_x,
        badge_y,
        badge_x + badge_w,
        badge_y + badge_h,
        6,
        6,
    );

    let mut lbl_rect = RECT {
        left: badge_x,
        top: badge_y,
        right: badge_x + badge_w,
        bottom: badge_y + badge_h,
    };
    let _ = SetBkMode(mem_dc, TRANSPARENT);
    let _ = SetTextColor(mem_dc, rgb(255, 255, 255));
    let _ = DrawTextW(
        mem_dc,
        &mut sz_u16,
        &mut lbl_rect,
        DT_CENTER | DT_SINGLELINE | DT_VCENTER,
    );

    let _ = SelectObject(mem_dc, old_b);
    let _ = SelectObject(mem_dc, old_p);
    let _ = SelectObject(mem_dc, old_font);
    let _ = DeleteObject(HGDIOBJ(b_brush.0));
    let _ = DeleteObject(HGDIOBJ(b_pen.0));
    let _ = DeleteObject(HGDIOBJ(font.0));

    // Status bar pill
    let status_text = "Drag to move/resize. Right-click or Enter to confirm. Esc to cancel.";
    let font2 = CreateFontW(
        22,
        0,
        0,
        0,
        FW_BOLD.0 as i32,
        0,
        0,
        0,
        FONT_CHARSET(0),
        FONT_OUTPUT_PRECISION(0),
        FONT_CLIP_PRECISION(0),
        FONT_QUALITY(0),
        0,
        w!("Segoe UI"),
    );
    let old_font2 = SelectObject(mem_dc, HGDIOBJ(font2.0));
    let _ = SetTextColor(mem_dc, rgb(255, 255, 255));
    let mut txt_u16: Vec<u16> = status_text.encode_utf16().collect();
    let mut calc_rect = RECT::default();
    let _ = DrawTextW(mem_dc, &mut txt_u16, &mut calc_rect, DT_CALCRECT);
    let text_w = calc_rect.right - calc_rect.left;
    let text_h = calc_rect.bottom - calc_rect.top;
    let pill_w = text_w + 48;
    let pill_h = text_h + 16;
    let pill_x = (state.width - pill_w) / 2;
    let pill_y = 40;

    let brush2 = CreateSolidBrush(rgb(12, 18, 28));
    let pen2 = CreatePen(PS_SOLID, 1, rgb(110, 156, 210));
    let old_brush2 = SelectObject(mem_dc, HGDIOBJ(brush2.0));
    let old_pen2 = SelectObject(mem_dc, HGDIOBJ(pen2.0));
    let _ = windows::Win32::Graphics::Gdi::RoundRect(
        mem_dc,
        pill_x,
        pill_y,
        pill_x + pill_w,
        pill_y + pill_h,
        18,
        18,
    );
    let mut text_rect = RECT {
        left: pill_x,
        top: pill_y,
        right: pill_x + pill_w,
        bottom: pill_y + pill_h,
    };
    let _ = DrawTextW(
        mem_dc,
        &mut txt_u16,
        &mut text_rect,
        DT_CENTER | DT_SINGLELINE | DT_VCENTER,
    );
    let _ = SelectObject(mem_dc, old_brush2);
    let _ = SelectObject(mem_dc, old_pen2);
    let _ = SelectObject(mem_dc, old_font2);
    let _ = DeleteObject(HGDIOBJ(brush2.0));
    let _ = DeleteObject(HGDIOBJ(pen2.0));
    let _ = DeleteObject(HGDIOBJ(font2.0));

    let _ = BitBlt(
        hdc,
        0,
        0,
        state.width,
        state.height,
        Some(mem_dc),
        0,
        0,
        SRCCOPY,
    );

    let _ = SelectObject(mem_dc, old_bmp);
    let _ = DeleteObject(HGDIOBJ(mem_bmp.0));
    let _ = DeleteDC(mem_dc);

    Ok(())
}

fn blit_rect(
    src: &[u8],
    src_w: usize,
    dst: &mut [u8],
    dst_w: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) {
    for row in 0..h {
        let src_y = y + row;
        let dst_y = y + row;
        let src_idx = (src_y * src_w + x) * 4;
        let dst_idx = (dst_y * dst_w + x) * 4;
        let len = w * 4;
        if src_idx + len <= src.len() && dst_idx + len <= dst.len() {
            dst[dst_idx..dst_idx + len].copy_from_slice(&src[src_idx..src_idx + len]);
        }
    }
}

fn protractor_circle_too_small(state: &CaptureState) -> bool {
    if state.protractor_points.len() != 2 {
        return false;
    }

    let Some(curr) = state.current_point else {
        return false;
    };

    let pt1 = state.protractor_points[0];
    let pt2 = state.protractor_points[1];
    let curr_abs = (curr.0 + state.left, curr.1 + state.top);

    let Some((_, radius)) = crate::protractor::circle_from_3_points(pt1, pt2, curr_abs) else {
        return false;
    };

    radius < crate::protractor::PROTRACTOR_MIN_CALIBRATION_RADIUS
}

fn protractor_calibration_status_text(
    state: &CaptureState,
    ui_language: crate::model::UiLanguage,
) -> &'static str {
    let count = state.protractor_points.len();
    let too_small = count >= 2 && protractor_circle_too_small(state);

    match ui_language {
        crate::model::UiLanguage::Vietnamese => match count {
            0 => "Căn chỉnh: Click điểm 1/3 trên màn hình. Nhấn Esc để hủy.",
            1 => "Căn chỉnh: Click điểm 2/3 trên màn hình. Nhấn Esc để hủy.",
            _ if too_small => {
                "Căn chỉnh: vòng tròn quá nhỏ. Hãy chọn điểm 3 xa hơn hoặc nhấn Esc để hủy."
            }
            _ => "Căn chỉnh: Click điểm 3/3 trên màn hình. Nhấn Esc để hủy.",
        },
        crate::model::UiLanguage::English | crate::model::UiLanguage::Icon => match count {
            0 => "Calibration: Click point 1/3 on screen. Press Esc to cancel.",
            1 => "Calibration: Click point 2/3 on screen. Press Esc to cancel.",
            _ if too_small => {
                "Calibration: circle too small. Pick point 3 farther away or press Esc to cancel."
            }
            _ => "Calibration: Click point 3/3 on screen. Press Esc to cancel.",
        },
    }
}

fn protractor_cursor_warning_text(
    state: &CaptureState,
    ui_language: crate::model::UiLanguage,
) -> Option<&'static str> {
    if state.protractor_points.len() < 2 || !protractor_circle_too_small(state) {
        return None;
    }

    Some(match ui_language {
        crate::model::UiLanguage::Vietnamese => "Vòng tròn quá nhỏ",
        crate::model::UiLanguage::English | crate::model::UiLanguage::Icon => "Circle too small",
    })
}

fn distance_measure_preview(state: &CaptureState) -> Option<(f64, (i32, i32), (i32, i32))> {
    let pt1 = *state.protractor_points.first()?;
    let curr = state.current_point?;
    let pt2 = (curr.0 + state.left, curr.1 + state.top);
    let dx = (pt2.0 - pt1.0) as f64;
    let dy = (pt2.1 - pt1.1) as f64;
    Some((dx.hypot(dy), pt1, pt2))
}

fn native_capture_shift_held() -> bool {
    unsafe {
        (windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(VK_SHIFT.0 as i32) as u16
            & 0x8000)
            != 0
    }
}

fn snap_point_to_45_degrees(anchor: (i32, i32), point: (i32, i32)) -> (i32, i32) {
    let dx = (point.0 - anchor.0) as f32;
    let dy = (point.1 - anchor.1) as f32;
    let radius = dx.hypot(dy);
    if radius <= f32::EPSILON {
        return point;
    }

    let snapped_angle =
        (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
    (
        anchor.0 + (radius * snapped_angle.cos()).round() as i32,
        anchor.1 + (radius * snapped_angle.sin()).round() as i32,
    )
}

fn distance_measure_constrained_local_point(state: &CaptureState, point: (i32, i32)) -> (i32, i32) {
    if !matches!(state.mode, NativeCaptureMode::DistanceMeasure { .. })
        || !native_capture_shift_held()
    {
        return point;
    }

    let Some(&(anchor_x, anchor_y)) = state.protractor_points.first() else {
        return point;
    };
    snap_point_to_45_degrees((anchor_x - state.left, anchor_y - state.top), point)
}

fn distance_measure_status_text(
    _state: &CaptureState,
    ui_language: crate::model::UiLanguage,
) -> &'static str {
    match ui_language {
        crate::model::UiLanguage::Vietnamese => {
            "Thước đo: Click điểm A, rê chuột để đo, click điểm B để chốt. Nhấn Esc để hủy."
        }
        crate::model::UiLanguage::English | crate::model::UiLanguage::Icon => {
            "Ruler: Click point A, move the mouse to measure, click point B to confirm. Press Esc to cancel."
        }
    }
}

fn draw_rounded_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    fill_paint: &Paint,
    stroke: Option<(&Paint, &Stroke)>,
) {
    let mut pb = PathBuilder::new();
    pb.move_to(x + radius, y);
    pb.line_to(x + w - radius, y);
    pb.quad_to(x + w, y, x + w, y + radius);
    pb.line_to(x + w, y + h - radius);
    pb.quad_to(x + w, y + h, x + w - radius, y + h);
    pb.line_to(x + radius, y + h);
    pb.quad_to(x, y + h, x, y + h - radius);
    pb.line_to(x, y + radius);
    pb.quad_to(x, y, x + radius, y);
    pb.close();

    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            fill_paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
        if let Some((stroke_paint, stroke_val)) = stroke {
            pixmap.stroke_path(
                &path,
                stroke_paint,
                stroke_val,
                tiny_skia::Transform::identity(),
                None,
            );
        }
    }
}

unsafe fn draw_capture_to_dc(
    hdc: HDC,
    state: &mut CaptureState,
    _dirty: Option<RECT>,
) -> anyhow::Result<()> {
    if matches!(state.mode, NativeCaptureMode::RegionAdjust { .. }) {
        draw_region_adjust_to_dc(hdc, state)?;
        return Ok(());
    }

    // Use cached memory DC double-buffering to eliminate allocations & lag during painting
    if state.paint_dc.is_none() {
        let mem_dc = CreateCompatibleDC(Some(hdc));
        let mem_bmp = CreateCompatibleBitmap(hdc, state.width, state.height);
        SelectObject(mem_dc, HGDIOBJ(mem_bmp.0));
        state.paint_dc = Some(mem_dc);
        state.paint_bmp = Some(mem_bmp);
    }
    let mem_dc = state.paint_dc.unwrap();

    let w = state.width as usize;
    let h = state.height as usize;
    let is_point_click = matches!(state.mode, NativeCaptureMode::PointClick { .. });

    if is_point_click {
        // Cache background DC with pre-rendered dimmed image in VRAM
        if state.bg_dc.is_none() {
            let bg_dc = CreateCompatibleDC(Some(hdc));
            let bg_bmp = CreateCompatibleBitmap(hdc, state.width, state.height);
            SelectObject(bg_dc, HGDIOBJ(bg_bmp.0));

            let is_dimmed = match state.mode {
                NativeCaptureMode::PointClick { dim_background, .. } => dim_background,
                _ => true,
            };
            let src = if is_dimmed {
                &state.dimmed_bgra
            } else {
                &state.original_bgra
            };

            let mut bmi = BITMAPINFO::default();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: state.width,
                biHeight: -state.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };

            let _ = StretchDIBits(
                bg_dc,
                0,
                0,
                state.width,
                state.height,
                0,
                0,
                state.width,
                state.height,
                Some(src.as_ptr() as *const std::ffi::c_void),
                &bmi,
                DIB_RGB_COLORS,
                SRCCOPY,
            );

            state.bg_dc = Some(bg_dc);
            state.bg_bmp = Some(bg_bmp);
        }

        let bg_dc = state.bg_dc.unwrap();
        let (dirty_x, dirty_y, dirty_w, dirty_h) = if let Some(r) = _dirty {
            let l = r.left.clamp(0, state.width);
            let t = r.top.clamp(0, state.height);
            let rw = (r.right - l).clamp(0, state.width - l);
            let rh = (r.bottom - t).clamp(0, state.height - t);
            (l, t, rw, rh)
        } else {
            (0, 0, state.width, state.height)
        };

        if dirty_w > 0 && dirty_h > 0 {
            let _ = BitBlt(
                mem_dc,
                dirty_x,
                dirty_y,
                dirty_w,
                dirty_h,
                Some(bg_dc),
                dirty_x,
                dirty_y,
                SRCCOPY,
            );
        }

        if let Some(curr) = state.current_point {
            let cx = curr.0;
            let cy = curr.1;

            // 1. Outer black border (thickness 4) - ensures 100% visibility on white and light backgrounds
            let border_pen = CreatePen(PS_SOLID, 4, rgb(0, 0, 0));
            let old_pen = SelectObject(mem_dc, HGDIOBJ(border_pen.0));
            let _ = MoveToEx(mem_dc, cx - 15, cy, None);
            let _ = LineTo(mem_dc, cx + 16, cy);
            let _ = MoveToEx(mem_dc, cx, cy - 15, None);
            let _ = LineTo(mem_dc, cx, cy + 16);
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(HGDIOBJ(border_pen.0));

            // 2. Inner bright cyan core (thickness 2) - bright and distinct on dark backgrounds
            let inner_pen = CreatePen(PS_SOLID, 2, rgb(0, 220, 255));
            let old_pen = SelectObject(mem_dc, HGDIOBJ(inner_pen.0));
            let _ = MoveToEx(mem_dc, cx - 14, cy, None);
            let _ = LineTo(mem_dc, cx + 15, cy);
            let _ = MoveToEx(mem_dc, cx, cy - 14, None);
            let _ = LineTo(mem_dc, cx, cy + 15);
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(HGDIOBJ(inner_pen.0));

            // 3. Crisp white center dot
            let _ = windows::Win32::Graphics::Gdi::SetPixel(mem_dc, cx, cy, rgb(255, 255, 255));
        }
    } else if matches!(
        state.mode,
        NativeCaptureMode::RegionSelect { .. }
    ) {
        state.render_bgra.copy_from_slice(&state.dimmed_bgra);

        if let (Some(start), Some(curr)) = (state.start_point, state.current_point) {
            let x = start.0.min(curr.0).clamp(0, state.width) as usize;
            let y = start.1.min(curr.1).clamp(0, state.height) as usize;
            let rw = (start.0 - curr.0).abs() as usize;
            let rh = (start.1 - curr.1).abs() as usize;
            let rw = rw.min(w.saturating_sub(x));
            let rh = rh.min(h.saturating_sub(y));

            if rw >= 2 && rh >= 2 {
                blit_rect(
                    &state.original_bgra,
                    w,
                    &mut state.render_bgra,
                    w,
                    x,
                    y,
                    rw,
                    rh,
                );
            }
        }

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: state.width,
            biHeight: -state.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let _ = StretchDIBits(
            mem_dc,
            0,
            0,
            state.width,
            state.height,
            0,
            0,
            state.width,
            state.height,
            Some(state.render_bgra.as_ptr() as *const std::ffi::c_void),
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        );

        if let (Some(start), Some(curr)) = (state.start_point, state.current_point) {
            let x = start.0.min(curr.0);
            let y = start.1.min(curr.1);
            let rw = (start.0 - curr.0).abs();
            let rh = (start.1 - curr.1).abs();

            if rw >= 2 && rh >= 2 {
                let border_pen = CreatePen(PS_SOLID, 2, rgb(0, 160, 255));
                let old_pen = SelectObject(mem_dc, HGDIOBJ(border_pen.0));
                let old_brush = SelectObject(
                    mem_dc,
                    windows::Win32::Graphics::Gdi::GetStockObject(
                        windows::Win32::Graphics::Gdi::NULL_BRUSH,
                    ),
                );
                let _ = windows::Win32::Graphics::Gdi::Rectangle(mem_dc, x, y, x + rw, y + rh);
                SelectObject(mem_dc, old_pen);
                SelectObject(mem_dc, old_brush);
                let _ = DeleteObject(HGDIOBJ(border_pen.0));

                // Draw live region dimension label badge
                let size_text = format!("{rw} × {rh}");
                let font_size = CreateFontW(
                    15,
                    0,
                    0,
                    0,
                    FW_BOLD.0 as i32,
                    0,
                    0,
                    0,
                    FONT_CHARSET(0),
                    FONT_OUTPUT_PRECISION(0),
                    FONT_CLIP_PRECISION(0),
                    FONT_QUALITY(0),
                    0,
                    w!("Segoe UI"),
                );
                let old_font = SelectObject(mem_dc, HGDIOBJ(font_size.0));
                let mut sz_u16: Vec<u16> = size_text.encode_utf16().collect();
                let mut sz_calc = RECT::default();
                let _ = DrawTextW(mem_dc, &mut sz_u16, &mut sz_calc, DT_CALCRECT);
                let text_w = sz_calc.right - sz_calc.left;
                let text_h = sz_calc.bottom - sz_calc.top;

                let badge_w = text_w + 16;
                let badge_h = text_h + 8;
                let badge_x = x.clamp(4, (state.width - badge_w - 4).max(4));
                let badge_y = if y >= badge_h + 6 {
                    y - badge_h - 4
                } else {
                    (y + 6).min((state.height - badge_h - 4).max(0))
                };

                let b_brush = CreateSolidBrush(rgb(15, 23, 42));
                let b_pen = CreatePen(PS_SOLID, 1, rgb(0, 160, 255));
                let old_b = SelectObject(mem_dc, HGDIOBJ(b_brush.0));
                let old_p = SelectObject(mem_dc, HGDIOBJ(b_pen.0));

                let _ = windows::Win32::Graphics::Gdi::RoundRect(
                    mem_dc,
                    badge_x,
                    badge_y,
                    badge_x + badge_w,
                    badge_y + badge_h,
                    6,
                    6,
                );

                let mut lbl_rect = RECT {
                    left: badge_x,
                    top: badge_y,
                    right: badge_x + badge_w,
                    bottom: badge_y + badge_h,
                };
                let _ = SetBkMode(mem_dc, TRANSPARENT);
                let _ = SetTextColor(mem_dc, rgb(255, 255, 255));
                let _ = DrawTextW(
                    mem_dc,
                    &mut sz_u16,
                    &mut lbl_rect,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                );

                let _ = SelectObject(mem_dc, old_b);
                let _ = SelectObject(mem_dc, old_p);
                let _ = SelectObject(mem_dc, old_font);
                let _ = DeleteObject(HGDIOBJ(b_brush.0));
                let _ = DeleteObject(HGDIOBJ(b_pen.0));
                let _ = DeleteObject(HGDIOBJ(font_size.0));
            }
        }
    } else {
        let mut pixmap = Pixmap::new(state.width as u32, state.height as u32)
            .ok_or_else(|| anyhow::anyhow!("Failed to create tiny-skia Pixmap"))?;

        // 1. Draw the screenshot onto the pixmap
        pixmap.data_mut().copy_from_slice(&state.capture_frame.rgba);

        // 2. Draw a dark overlay over the whole screen
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 128); // 50% opacity
        let screen_rect =
            Rect::from_xywh(0.0, 0.0, state.width as f32, state.height as f32).unwrap();
        pixmap.fill_rect(screen_rect, &paint, tiny_skia::Transform::identity(), None);

        // 3. Render specific overlay elements based on capture mode
        match state.mode {
            NativeCaptureMode::ProtractorCalibration { .. } => {
                let mut pt_paint = Paint::default();
                pt_paint.set_color_rgba8(255, 50, 50, 255);
                let mut stroke = Stroke::default();
                stroke.width = 2.0;

                let mut white_paint = Paint::default();
                white_paint.set_color_rgba8(255, 255, 255, 255);

                for pt in &state.protractor_points {
                    let rx = pt.0 - state.left;
                    let ry = pt.1 - state.top;

                    let mut pb = PathBuilder::new();
                    pb.push_circle(rx as f32, ry as f32, 6.0);
                    let path = pb.finish().unwrap();
                    pixmap.fill_path(
                        &path,
                        &pt_paint,
                        tiny_skia::FillRule::Winding,
                        tiny_skia::Transform::identity(),
                        None,
                    );

                    let mut pb = PathBuilder::new();
                    pb.push_circle(rx as f32, ry as f32, 10.0);
                    let path = pb.finish().unwrap();
                    pixmap.stroke_path(
                        &path,
                        &white_paint,
                        &stroke,
                        tiny_skia::Transform::identity(),
                        None,
                    );
                }

                if let Some(curr) = state.current_point {
                    let count = state.protractor_points.len();
                    if count == 1 {
                        let pt1 = state.protractor_points[0];
                        let r1x = pt1.0 - state.left;
                        let r1y = pt1.1 - state.top;

                        let mut line_paint = Paint::default();
                        line_paint.set_color_rgba8(255, 50, 50, 180);
                        let mut dashed_stroke = Stroke::default();
                        dashed_stroke.width = 1.5;
                        dashed_stroke.dash = tiny_skia::StrokeDash::new(vec![4.0, 4.0], 0.0);

                        let mut pb = PathBuilder::new();
                        pb.move_to(r1x as f32, r1y as f32);
                        pb.line_to(curr.0 as f32, curr.1 as f32);
                        let path = pb.finish().unwrap();
                        pixmap.stroke_path(
                            &path,
                            &line_paint,
                            &dashed_stroke,
                            tiny_skia::Transform::identity(),
                            None,
                        );
                    } else if count == 2 {
                        let pt1 = state.protractor_points[0];
                        let pt2 = state.protractor_points[1];
                        let curr_abs = (curr.0 + state.left, curr.1 + state.top);

                        if let Some((center, radius)) =
                            crate::protractor::circle_from_3_points(pt1, pt2, curr_abs)
                        {
                            let rcx = center.0 - state.left;
                            let rcy = center.1 - state.top;

                            let mut circle_paint = Paint::default();
                            circle_paint.set_color_rgba8(255, 50, 50, 180);
                            let mut dashed_stroke = Stroke::default();
                            dashed_stroke.width = 1.5;
                            dashed_stroke.dash = tiny_skia::StrokeDash::new(vec![4.0, 4.0], 0.0);

                            let mut pb = PathBuilder::new();
                            pb.push_circle(rcx as f32, rcy as f32, radius);
                            let path = pb.finish().unwrap();
                            pixmap.stroke_path(
                                &path,
                                &circle_paint,
                                &dashed_stroke,
                                tiny_skia::Transform::identity(),
                                None,
                            );
                        }
                    }
                }
            }
            NativeCaptureMode::DistanceMeasure { .. } => {
                let mut pt_paint = Paint::default();
                pt_paint.set_color_rgba8(255, 196, 0, 255);
                let mut stroke = Stroke::default();
                stroke.width = 2.0;

                let mut white_paint = Paint::default();
                white_paint.set_color_rgba8(255, 255, 255, 255);

                for pt in &state.protractor_points {
                    let rx = pt.0 - state.left;
                    let ry = pt.1 - state.top;

                    let mut pb = PathBuilder::new();
                    pb.push_circle(rx as f32, ry as f32, 6.0);
                    let path = pb.finish().unwrap();
                    pixmap.fill_path(
                        &path,
                        &pt_paint,
                        tiny_skia::FillRule::Winding,
                        tiny_skia::Transform::identity(),
                        None,
                    );

                    let mut pb = PathBuilder::new();
                    pb.push_circle(rx as f32, ry as f32, 10.0);
                    let path = pb.finish().unwrap();
                    pixmap.stroke_path(
                        &path,
                        &white_paint,
                        &stroke,
                        tiny_skia::Transform::identity(),
                        None,
                    );
                }

                if let Some(curr) = state.current_point
                    && let Some(pt1) = state.protractor_points.first()
                {
                    let r1x = pt1.0 - state.left;
                    let r1y = pt1.1 - state.top;

                    let mut line_paint = Paint::default();
                    line_paint.set_color_rgba8(255, 196, 0, 220);
                    let mut dashed_stroke = Stroke::default();
                    dashed_stroke.width = 1.8;
                    dashed_stroke.dash = tiny_skia::StrokeDash::new(vec![6.0, 4.0], 0.0);

                    let mut pb = PathBuilder::new();
                    pb.move_to(r1x as f32, r1y as f32);
                    pb.line_to(curr.0 as f32, curr.1 as f32);
                    let path = pb.finish().unwrap();
                    pixmap.stroke_path(
                        &path,
                        &line_paint,
                        &dashed_stroke,
                        tiny_skia::Transform::identity(),
                        None,
                    );
                }
            }
            _ => {}
        }

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: state.width,
            biHeight: -state.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let mut bgra = pixmap.data().to_vec();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }

        let _ = StretchDIBits(
            mem_dc,
            0,
            0,
            state.width,
            state.height,
            0,
            0,
            state.width,
            state.height,
            Some(bgra.as_ptr() as *const std::ffi::c_void),
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }

    // Draw status bar & instructions pill using GDI DrawTextW on mem_dc
    let status_text = match state.mode {
        NativeCaptureMode::ProtractorCalibration { ui_language } => {
            protractor_calibration_status_text(state, ui_language)
        }
        NativeCaptureMode::DistanceMeasure { ui_language } => {
            distance_measure_status_text(state, ui_language)
        }
        NativeCaptureMode::RegionSelect {
            kind,
            ui_language,
            ..
        } => {
            let is_vn = ui_language == crate::model::UiLanguage::Vietnamese;
            match kind {
                RegionSelectKind::Screenshot => {
                    if is_vn {
                        "Kéo chuột trên màn hình để chọn vùng chụp ảnh. Nhấn Esc để hủy."
                    } else {
                        "Drag on screen to select screenshot region. Press Esc to cancel."
                    }
                }
                RegionSelectKind::Ocr => {
                    if is_vn {
                        "Kéo chuột trên màn hình để chọn vùng nhận diện chữ. Nhấn Esc để hủy."
                    } else {
                        "Drag on screen to select OCR text region. Press Esc to cancel."
                    }
                }
                RegionSelectKind::VideoRecord => {
                    if is_vn {
                        "Kéo chuột trên màn hình để chọn vùng quay video. Nhấn Esc để hủy."
                    } else {
                        "Drag on screen to select video recording region. Press Esc to cancel."
                    }
                }
                RegionSelectKind::ImageTemplate => {
                    if is_vn {
                        "Kéo chuột trên màn hình để chọn mẫu ảnh. Nhấn Esc để hủy."
                    } else {
                        "Drag on screen to pick an image template. Press Esc to cancel."
                    }
                }
                RegionSelectKind::ImageSearchArea => {
                    if is_vn {
                        "Kéo chuột trên màn hình để chọn vùng tìm kiếm hình ảnh. Nhấn Esc để hủy."
                    } else {
                        "Drag on screen to pick the image search area. Press Esc to cancel."
                    }
                }
            }
        }
        NativeCaptureMode::PointClick { ui_language, .. } => {
            if ui_language == crate::model::UiLanguage::Vietnamese {
                "Nhấp vào một điểm trên màn hình để chọn. Nhấn Esc để hủy."
            } else {
                "Click a point on screen to capture. Press Esc to cancel."
            }
        }
        NativeCaptureMode::RegionAdjust { ui_language, .. } => {
            if ui_language == crate::model::UiLanguage::Vietnamese {
                "Kéo các viền hộp để thay đổi kích thước, kéo giữa để di chuyển. Nhấn Enter để xác nhận, Esc để hủy."
            } else {
                "Drag borders to resize, center to move. Press Enter to confirm, Esc to cancel."
            }
        }
    };

    let font = CreateFontW(
        22,
        0,
        0,
        0,
        FW_BOLD.0 as i32,
        0,
        0,
        0,
        FONT_CHARSET(0),
        FONT_OUTPUT_PRECISION(0),
        FONT_CLIP_PRECISION(0),
        FONT_QUALITY(0),
        0,
        w!("Segoe UI"),
    );

    let old_font = SelectObject(mem_dc, HGDIOBJ(font.0));
    let _ = SetBkMode(mem_dc, TRANSPARENT);
    let _ = SetTextColor(mem_dc, rgb(255, 255, 255));

    let mut text_u16: Vec<u16> = status_text.encode_utf16().collect();
    let mut calc_rect = RECT::default();
    let _ = DrawTextW(mem_dc, &mut text_u16, &mut calc_rect, DT_CALCRECT);
    let text_w = calc_rect.right - calc_rect.left;
    let text_h = calc_rect.bottom - calc_rect.top;

    let pill_w = text_w + 48;
    let pill_h = text_h + 16;
    let pill_x = (state.width - pill_w) / 2;
    let pill_y = 40;

    let pill_rect = RECT {
        left: (pill_x - 4).max(0),
        top: (pill_y - 4).max(0),
        right: (pill_x + pill_w + 4).min(state.width),
        bottom: (pill_y + pill_h + 4).min(state.height),
    };
    state.last_pill_rect = Some(pill_rect);

    let cursor_near_pill = state.current_point.is_some_and(|(cx, cy)| {
        let margin_x = 40;
        let margin_y = 30;
        cx >= pill_x - margin_x
            && cx <= pill_x + pill_w + margin_x
            && cy >= pill_y - margin_y
            && cy <= pill_y + pill_h + margin_y
    });

    if !cursor_near_pill {
        let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(rgb(12, 18, 28));
        let pen = windows::Win32::Graphics::Gdi::CreatePen(
            windows::Win32::Graphics::Gdi::PS_SOLID,
            1,
            rgb(110, 156, 210),
        );
        let old_brush = SelectObject(mem_dc, HGDIOBJ(brush.0));
        let old_pen = SelectObject(mem_dc, HGDIOBJ(pen.0));

        let _ = windows::Win32::Graphics::Gdi::RoundRect(
            mem_dc,
            pill_x,
            pill_y,
            pill_x + pill_w,
            pill_y + pill_h,
            18,
            18,
        );

        let mut text_rect = RECT {
            left: pill_x,
            top: pill_y,
            right: pill_x + pill_w,
            bottom: pill_y + pill_h,
        };
        let _ = DrawTextW(
            mem_dc,
            &mut text_u16,
            &mut text_rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );

        let _ = SelectObject(mem_dc, old_brush);
        let _ = SelectObject(mem_dc, old_pen);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
    let _ = SelectObject(mem_dc, old_font);
    let _ = DeleteObject(HGDIOBJ(font.0));

    // Render coordinate & color magnifier preview panel for PointClick
    if matches!(state.mode, NativeCaptureMode::PointClick { .. })
        && let Some(curr) = state.current_point
    {
        let panel_w = 184i32;
        let panel_h = 246i32;
        let margin = 24i32;

        let pointer_x = curr.0;
        let pointer_y = curr.1;
        let safe_r = 70i32;
        let safe_left = pointer_x - safe_r;
        let safe_right = pointer_x + safe_r;
        let safe_top = pointer_y - safe_r;
        let safe_bottom = pointer_y + safe_r;

        let candidates = [
            (state.width - panel_w - margin, margin),
            (margin, margin),
            (state.width - panel_w - margin, state.height - panel_h - margin),
            (margin, state.height - panel_h - margin),
        ];

        let mut panel_x = candidates[0].0;
        let mut panel_y = candidates[0].1;

        for &(cx, cy) in &candidates {
            let intersects = !(cx + panel_w < safe_left
                || cx > safe_right
                || cy + panel_h < safe_top
                || cy > safe_bottom);
            if !intersects {
                panel_x = cx;
                panel_y = cy;
                break;
            }
        }

        // Panel background and border
        let p_brush = CreateSolidBrush(rgb(12, 18, 28));
        let p_pen = CreatePen(PS_SOLID, 1, rgb(110, 156, 210));
        let old_pb = SelectObject(mem_dc, HGDIOBJ(p_brush.0));
        let old_pp = SelectObject(mem_dc, HGDIOBJ(p_pen.0));
        let _ = windows::Win32::Graphics::Gdi::RoundRect(
            mem_dc,
            panel_x,
            panel_y,
            panel_x + panel_w,
            panel_y + panel_h,
            12,
            12,
        );
        let _ = SelectObject(mem_dc, old_pb);
        let _ = SelectObject(mem_dc, old_pp);
        let _ = DeleteObject(HGDIOBJ(p_brush.0));
        let _ = DeleteObject(HGDIOBJ(p_pen.0));

        // Magnifier 17x17 grid
        let sample_size = 17usize;
        let cell_px = 8i32;
        let grid_w = (sample_size as i32) * cell_px; // 136
        let grid_h = (sample_size as i32) * cell_px; // 136
        let grid_x = panel_x + (panel_w - grid_w) / 2;
        let grid_y = panel_y + 14;

        let mut center_color = (0u8, 0u8, 0u8);
        let mut grid_bgra = [0u8; 17 * 17 * 4];

        for dy in 0..sample_size {
            let sy = curr.1 - 8 + dy as i32;
            for dx in 0..sample_size {
                let sx = curr.0 - 8 + dx as i32;
                let mut r = 0u8;
                let mut g = 0u8;
                let mut b = 0u8;
                if sx >= 0 && sx < state.width && sy >= 0 && sy < state.height {
                    let idx = (sy as usize * state.width as usize + sx as usize) * 4;
                    if idx + 3 < state.capture_frame.rgba.len() {
                        r = state.capture_frame.rgba[idx];
                        g = state.capture_frame.rgba[idx + 1];
                        b = state.capture_frame.rgba[idx + 2];
                    }
                }
                if dx == 8 && dy == 8 {
                    center_color = (r, g, b);
                }
                let out_idx = (dy * sample_size + dx) * 4;
                grid_bgra[out_idx] = b;
                grid_bgra[out_idx + 1] = g;
                grid_bgra[out_idx + 2] = r;
                grid_bgra[out_idx + 3] = 255;
            }
        }

        let mut grid_bmi = BITMAPINFO::default();
        grid_bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: sample_size as i32,
            biHeight: -(sample_size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let _ = StretchDIBits(
            mem_dc,
            grid_x,
            grid_y,
            grid_w,
            grid_h,
            0,
            0,
            sample_size as i32,
            sample_size as i32,
            Some(grid_bgra.as_ptr() as *const std::ffi::c_void),
            &grid_bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        );

        // Grid border
        let g_pen = CreatePen(PS_SOLID, 1, rgb(65, 80, 105));
        let null_brush = windows::Win32::Graphics::Gdi::GetStockObject(
            windows::Win32::Graphics::Gdi::NULL_BRUSH,
        );
        let old_gp = SelectObject(mem_dc, HGDIOBJ(g_pen.0));
        let old_gb = SelectObject(mem_dc, null_brush);
        let _ = windows::Win32::Graphics::Gdi::Rectangle(
            mem_dc,
            grid_x - 1,
            grid_y - 1,
            grid_x + grid_w + 1,
            grid_y + grid_h + 1,
        );

        // Center pixel highlight (box around 8th cell)
        let center_box_x = grid_x + 8 * cell_px;
        let center_box_y = grid_y + 8 * cell_px;
        let c_pen = CreatePen(PS_SOLID, 1, rgb(255, 255, 255));
        let _ = SelectObject(mem_dc, HGDIOBJ(c_pen.0));
        let _ = windows::Win32::Graphics::Gdi::Rectangle(
            mem_dc,
            center_box_x,
            center_box_y,
            center_box_x + cell_px + 1,
            center_box_y + cell_px + 1,
        );

        let _ = SelectObject(mem_dc, old_gp);
        let _ = SelectObject(mem_dc, old_gb);
        let _ = DeleteObject(HGDIOBJ(g_pen.0));
        let _ = DeleteObject(HGDIOBJ(c_pen.0));

        // Color swatch box
        let swatch_x = grid_x;
        let swatch_y = grid_y + grid_h + 10;
        let swatch_size = 24i32;
        let swatch_brush = CreateSolidBrush(rgb(center_color.0, center_color.1, center_color.2));
        let swatch_pen = CreatePen(PS_SOLID, 1, rgb(255, 255, 255));
        let old_sb = SelectObject(mem_dc, HGDIOBJ(swatch_brush.0));
        let old_sp = SelectObject(mem_dc, HGDIOBJ(swatch_pen.0));
        let _ = windows::Win32::Graphics::Gdi::RoundRect(
            mem_dc,
            swatch_x,
            swatch_y,
            swatch_x + swatch_size,
            swatch_y + swatch_size,
            4,
            4,
        );
        let _ = SelectObject(mem_dc, old_sb);
        let _ = SelectObject(mem_dc, old_sp);
        let _ = DeleteObject(HGDIOBJ(swatch_brush.0));
        let _ = DeleteObject(HGDIOBJ(swatch_pen.0));

        // Hex Code text
        let hex_font = CreateFontW(
            17,
            0,
            0,
            0,
            FW_BOLD.0 as i32,
            0,
            0,
            0,
            FONT_CHARSET(0),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(0),
            0,
            w!("Segoe UI"),
        );
        let old_hf = SelectObject(mem_dc, HGDIOBJ(hex_font.0));
        let hex_str = format!(
            "#{:02X}{:02X}{:02X}",
            center_color.0, center_color.1, center_color.2
        );
        let mut hex_u16: Vec<u16> = hex_str.encode_utf16().collect();
        let mut hex_rect = RECT {
            left: swatch_x + swatch_size + 8,
            top: swatch_y,
            right: panel_x + panel_w - 10,
            bottom: swatch_y + swatch_size,
        };
        let _ = SetTextColor(mem_dc, rgb(255, 255, 255));
        let _ = DrawTextW(
            mem_dc,
            &mut hex_u16,
            &mut hex_rect,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        let _ = SelectObject(mem_dc, old_hf);
        let _ = DeleteObject(HGDIOBJ(hex_font.0));

        // Coordinates & RGB text
        let label_font = CreateFontW(
            13,
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            FONT_CHARSET(0),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(0),
            0,
            w!("Segoe UI"),
        );
        let old_lf = SelectObject(mem_dc, HGDIOBJ(label_font.0));

        let abs_x = curr.0 + state.left;
        let abs_y = curr.1 + state.top;
        let coords_str = format!("X: {abs_x}   Y: {abs_y}");
        let mut coords_u16: Vec<u16> = coords_str.encode_utf16().collect();
        let mut coords_rect = RECT {
            left: grid_x,
            top: swatch_y + swatch_size + 6,
            right: panel_x + panel_w - 10,
            bottom: swatch_y + swatch_size + 22,
        };
        let _ = SetTextColor(mem_dc, rgb(188, 206, 230));
        let _ = DrawTextW(
            mem_dc,
            &mut coords_u16,
            &mut coords_rect,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );

        let rgb_str = format!(
            "RGB: ({}, {}, {})",
            center_color.0, center_color.1, center_color.2
        );
        let mut rgb_u16: Vec<u16> = rgb_str.encode_utf16().collect();
        let mut rgb_rect = RECT {
            left: grid_x,
            top: coords_rect.bottom + 2,
            right: panel_x + panel_w - 10,
            bottom: coords_rect.bottom + 18,
        };
        let _ = SetTextColor(mem_dc, rgb(140, 165, 195));
        let _ = DrawTextW(
            mem_dc,
            &mut rgb_u16,
            &mut rgb_rect,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );

        let _ = SelectObject(mem_dc, old_lf);
        let _ = DeleteObject(HGDIOBJ(label_font.0));
    }

    // Render coordinates tooltip next to mouse cursor
    let show_cursor_tooltip = !matches!(
        state.mode,
        NativeCaptureMode::RegionSelect { .. } | NativeCaptureMode::PointClick { .. }
    );
    if show_cursor_tooltip && let Some(curr) = state.current_point {
        let abs_x = curr.0 + state.left;
        let abs_y = curr.1 + state.top;
        let coords_str = match state.mode {
            NativeCaptureMode::DistanceMeasure { .. } => {
                if let Some((distance, point_a, _)) = distance_measure_preview(state) {
                    format!(
                        "A({}, {}) -> B({}, {}) = {:.2}px",
                        point_a.0, point_a.1, abs_x, abs_y, distance
                    )
                } else {
                    format!("X: {}, Y: {}", abs_x, abs_y)
                }
            }
            NativeCaptureMode::PointClick { .. } => {
                let rx_u = curr.0.clamp(0, state.width - 1) as usize;
                let ry_u = curr.1.clamp(0, state.height - 1) as usize;
                let idx = (ry_u * state.width as usize + rx_u) * 4;
                if idx + 2 < state.capture_frame.rgba.len() {
                    let r = state.capture_frame.rgba[idx];
                    let g = state.capture_frame.rgba[idx + 1];
                    let b = state.capture_frame.rgba[idx + 2];
                    format!("X: {abs_x}, Y: {abs_y}  |  #{r:02X}{g:02X}{b:02X}")
                } else {
                    format!("X: {abs_x}, Y: {abs_y}")
                }
            }
            _ => format!("X: {}, Y: {}", abs_x, abs_y),
        };
        let tooltip_warning = match state.mode {
            NativeCaptureMode::ProtractorCalibration { ui_language } => {
                protractor_cursor_warning_text(state, ui_language)
            }
            _ => None,
        };
        let mut coords_u16: Vec<u16> = coords_str.encode_utf16().collect();

        let mut c_calc = RECT::default();
        let _ = DrawTextW(mem_dc, &mut coords_u16, &mut c_calc, DT_CALCRECT);
        let cw = c_calc.right - c_calc.left;
        let ch = c_calc.bottom - c_calc.top;
        let (warning_u16, warning_w, warning_h) = if let Some(text) = tooltip_warning {
            let mut warning_u16: Vec<u16> = text.encode_utf16().collect();
            let mut warning_calc = RECT::default();
            let _ = DrawTextW(mem_dc, &mut warning_u16, &mut warning_calc, DT_CALCRECT);
            (
                Some(warning_u16),
                warning_calc.right - warning_calc.left,
                warning_calc.bottom - warning_calc.top,
            )
        } else {
            (None, 0, 0)
        };
        let content_w = cw.max(warning_w);
        let content_h = if warning_u16.is_some() {
            ch + 4 + warning_h
        } else {
            ch
        };

        let tooltip_w = content_w + 16;
        let tooltip_h = content_h + 10;
        let max_tooltip_x = (state.width - tooltip_w - 8).max(8);
        let max_tooltip_y = (state.height - tooltip_h - 8).max(8);
        let tooltip_x = (curr.0 + 15).clamp(8, max_tooltip_x);
        let tooltip_y = (curr.1 + 15).clamp(8, max_tooltip_y);

        let t_brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(rgb(15, 23, 42));
        let t_pen = windows::Win32::Graphics::Gdi::CreatePen(
            windows::Win32::Graphics::Gdi::PS_SOLID,
            1,
            if warning_u16.is_some() {
                rgb(255, 140, 72)
            } else {
                rgb(0, 160, 255)
            },
        );
        let old_tb = SelectObject(mem_dc, HGDIOBJ(t_brush.0));
        let old_tp = SelectObject(mem_dc, HGDIOBJ(t_pen.0));

        let _ = windows::Win32::Graphics::Gdi::RoundRect(
            mem_dc,
            tooltip_x,
            tooltip_y,
            tooltip_x + tooltip_w,
            tooltip_y + tooltip_h,
            6,
            6,
        );

        let mut coords_rect = RECT {
            left: tooltip_x + 8,
            top: tooltip_y + 5,
            right: tooltip_x + content_w + 8,
            bottom: tooltip_y + ch + 5,
        };
        let _ = SetTextColor(mem_dc, rgb(255, 255, 255));
        let _ = DrawTextW(
            mem_dc,
            &mut coords_u16,
            &mut coords_rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );

        if let Some(mut warning_u16) = warning_u16 {
            let _ = SetTextColor(mem_dc, rgb(255, 196, 148));
            let mut warning_rect = RECT {
                left: tooltip_x + 8,
                top: coords_rect.bottom + 4,
                right: tooltip_x + content_w + 8,
                bottom: coords_rect.bottom + 4 + warning_h,
            };
            let _ = DrawTextW(
                mem_dc,
                &mut warning_u16,
                &mut warning_rect,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            );
        }

        let _ = SelectObject(mem_dc, old_tb);
        let _ = SelectObject(mem_dc, old_tp);
        let _ = DeleteObject(HGDIOBJ(t_brush.0));
        let _ = DeleteObject(HGDIOBJ(t_pen.0));
    }

    // Finally: atomic BitBlt of completed frame from mem_dc to window hdc
    if is_point_click {
        let (dirty_x, dirty_y, dirty_w, dirty_h) = if let Some(r) = _dirty {
            let l = r.left.clamp(0, state.width);
            let t = r.top.clamp(0, state.height);
            let rw = (r.right - l).clamp(0, state.width - l);
            let rh = (r.bottom - t).clamp(0, state.height - t);
            (l, t, rw, rh)
        } else {
            (0, 0, state.width, state.height)
        };
        if dirty_w > 0 && dirty_h > 0 {
            let _ = BitBlt(
                hdc,
                dirty_x,
                dirty_y,
                dirty_w,
                dirty_h,
                Some(mem_dc),
                dirty_x,
                dirty_y,
                SRCCOPY,
            );
        }
    } else {
        let _ = BitBlt(
            hdc,
            0,
            0,
            state.width,
            state.height,
            Some(mem_dc),
            0,
            0,
            SRCCOPY,
        );
    }

    Ok(())
}

pub fn run_native_ocr_capture_overlay(
    trigger: Option<crate::model::HotkeyBinding>,
    ocr_lang: String,
    ui_language: crate::model::UiLanguage,
) {
    let (left, top, width, height) = crate::window_list::virtual_screen_bounds();
    let result = if let Some(capture) =
        crate::window_list::capture_virtual_screen_region(left, top, width, height)
    {
        let mode = NativeCaptureMode::RegionSelect {
            kind: RegionSelectKind::Ocr,
            ui_language,
            hold_hotkey: trigger,
        };
        run_capture_overlay(capture, left, top, width, height, mode)
    } else {
        NativeCaptureResult::Cancelled
    };

    if let NativeCaptureResult::SelectedRegion {
        x,
        y,
        width: w,
        height: h,
    } = result
    {
        if w >= 4 && h >= 4 {
            if let Some(region_frame) =
                crate::window_list::capture_virtual_screen_region(x, y, w, h)
            {
                let rect = Some(windows::Win32::Foundation::RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                });
                let ocr_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::ocr::perform_ocr(
                        &region_frame.rgba,
                        region_frame.width as u32,
                        region_frame.height as u32,
                        &ocr_lang,
                    )
                }));

                match ocr_res {
                    Ok(Ok(ocr_result)) => {
                        let trimmed = ocr_result.text.trim();
                        if !trimmed.is_empty() {
                            let text_to_copy = trimmed.to_owned();
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    let _ = clipboard.set_text(text_to_copy.clone());
                                }
                            }));
                            let toast_msg = if ui_language == crate::model::UiLanguage::Vietnamese {
                                format!(
                                    "✓ Đã sao chép: \"{}\"",
                                    text_to_copy.chars().take(40).collect::<String>()
                                )
                            } else {
                                format!(
                                    "✓ Copied: \"{}\"",
                                    text_to_copy.chars().take(40).collect::<String>()
                                )
                            };
                            crate::overlay::show_ocr_copy_toast_async(rect, toast_msg, false);
                        } else {
                            let toast_msg = if ui_language == crate::model::UiLanguage::Vietnamese {
                                "⚠ Không nhận diện được văn bản".to_owned()
                            } else {
                                "⚠ No text recognized".to_owned()
                            };
                            crate::overlay::show_ocr_copy_toast_async(rect, toast_msg, true);
                        }
                    }
                    _ => {
                        let toast_msg = if ui_language == crate::model::UiLanguage::Vietnamese {
                            "⚠ Lỗi nhận diện chữ".to_owned()
                        } else {
                            "⚠ OCR recognition failed".to_owned()
                        };
                        crate::overlay::show_ocr_copy_toast_async(rect, toast_msg, true);
                    }
                }
            }
        }
    }
}

pub fn run_native_video_record_region_overlay(
    trigger: Option<crate::model::HotkeyBinding>,
    ui_language: crate::model::UiLanguage,
) {
    let (left, top, width, height) = crate::window_list::virtual_screen_bounds();
    let result = if let Some(capture) =
        crate::window_list::capture_virtual_screen_region(left, top, width, height)
    {
        let mode = NativeCaptureMode::RegionSelect {
            kind: RegionSelectKind::VideoRecord,
            ui_language,
            hold_hotkey: trigger,
        };
        run_capture_overlay(capture, left, top, width, height, mode)
    } else {
        NativeCaptureResult::Cancelled
    };

    if let NativeCaptureResult::SelectedRegion {
        x,
        y,
        width: w,
        height: h,
    } = result
    {
        if w >= 4 && h >= 4 {
            if !crate::video_recorder::is_recording() && !crate::video_recorder::is_busy() {
                crate::video_recorder::start_region_async((x, y, w, h));
            }
            crate::overlay::request_ui_repaint();
        }
    }
}

pub fn run_native_screenshot_capture_overlay(
    trigger: Option<crate::model::HotkeyBinding>,
    ui_language: crate::model::UiLanguage,
) {
    let (left, top, width, height) = crate::window_list::virtual_screen_bounds();
    let result = if let Some(capture) =
        crate::window_list::capture_virtual_screen_region(left, top, width, height)
    {
        let mode = NativeCaptureMode::RegionSelect {
            kind: RegionSelectKind::Screenshot,
            ui_language,
            hold_hotkey: trigger,
        };
        run_capture_overlay(capture, left, top, width, height, mode)
    } else {
        NativeCaptureResult::Cancelled
    };

    if let NativeCaptureResult::SelectedRegion {
        x,
        y,
        width: w,
        height: h,
    } = result
    {
        if w >= 4 && h >= 4 {
            if let Some(region_frame) =
                crate::window_list::capture_virtual_screen_region(x, y, w, h)
            {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_image(arboard::ImageData {
                            width: region_frame.width,
                            height: region_frame.height,
                            bytes: std::borrow::Cow::Owned(region_frame.rgba),
                        });
                    }
                }));
                let rect = Some(windows::Win32::Foundation::RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                });
                let toast_msg = if ui_language == crate::model::UiLanguage::Vietnamese {
                    "✓ Đã chụp ảnh màn hình và sao chép vào bộ nhớ tạm".to_owned()
                } else {
                    "✓ Screenshot copied to clipboard".to_owned()
                };
                crate::overlay::show_ocr_copy_toast_async(rect, toast_msg, false);
            }
        }
    }
}


