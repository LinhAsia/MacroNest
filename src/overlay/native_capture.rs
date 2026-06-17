#![allow(unsafe_op_in_unsafe_fn)]

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM, HINSTANCE, COLORREF},
    Graphics::Gdi::{
        BeginPaint, EndPaint, PAINTSTRUCT, HDC, HGDIOBJ, CreateCompatibleDC, DeleteDC, SelectObject,
        StretchDIBits, DIB_RGB_COLORS, GetDC, ReleaseDC, CreateDIBSection, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DrawTextW, DT_CENTER, DT_SINGLELINE, DT_VCENTER, SetBkMode,
        SetTextColor, TRANSPARENT, CreateFontW, HFONT, FW_BOLD, DT_CALCRECT, DeleteObject, SRCCOPY,
        FONT_CHARSET, FONT_OUTPUT_PRECISION, FONT_CLIP_PRECISION, FONT_QUALITY,
        CreatePen, MoveToEx, LineTo, PS_SOLID,
    },
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
        PostQuitMessage, RegisterClassW, ShowWindow, TranslateMessage,
        CS_HREDRAW, CS_VREDRAW, MSG, WNDCLASSW, WS_EX_TOPMOST, WS_EX_TOOLWINDOW,
        WS_POPUP, SW_SHOW, SWP_NOACTIVATE, SetWindowPos, LoadCursorW, IDC_ARROW,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_KEYDOWN, WM_DESTROY,
        SW_HIDE, SW_SHOWNORMAL,
        WINDOW_EX_STYLE, WINDOW_STYLE, SWP_SHOWWINDOW, WM_CREATE,
        CREATESTRUCTW, WM_NCCREATE, GWLP_USERDATA, WINDOW_LONG_PTR_INDEX,
        PostMessageW, DestroyWindow, HWND_TOPMOST, SetWindowLongPtrW, GetWindowLongPtrW, GetCursorPos,
    },
    UI::Input::KeyboardAndMouse::{
        SetCapture, ReleaseCapture, VK_ESCAPE,
    },
};
use windows::core::w;

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

use tiny_skia::{Pixmap, Paint, Rect, Color, Stroke, PathBuilder, PixmapPaint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCaptureMode {
    ProtractorCalibration {
        ui_language: crate::model::UiLanguage,
    },
    RegionSelect {
        is_template: bool,
        vietnamese: bool,
    },
    PointClick {
        vietnamese: bool,
        dim_background: bool,
    },
}

#[derive(Debug, Clone)]
pub enum NativeCaptureResult {
    Cancelled,
    ProtractorPoints(Vec<(i32, i32)>),
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
}

struct CaptureState {
    capture_frame: crate::window_list::ScreenCaptureFrame,
    dimmed_rgba: Vec<u8>,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    mode: NativeCaptureMode,

    // Interaction state
    start_point: Option<(i32, i32)>,
    current_point: Option<(i32, i32)>,
    protractor_points: Vec<(i32, i32)>,

    // Result
    result: NativeCaptureResult,
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
        Self {
            dimmed_rgba: dim_capture_frame(&capture_frame),
            capture_frame,
            left,
            top,
            width,
            height,
            mode,
            start_point: None,
            current_point: None,
            protractor_points: Vec::new(),
            result: NativeCaptureResult::Cancelled,
        }
    }
}

fn dim_capture_frame(capture_frame: &crate::window_list::ScreenCaptureFrame) -> Vec<u8> {
    let mut dimmed = capture_frame.rgba.clone();
    for pixel in dimmed.chunks_exact_mut(4) {
        pixel[0] = ((pixel[0] as u16 * 127) / 255) as u8;
        pixel[1] = ((pixel[1] as u16 * 127) / 255) as u8;
        pixel[2] = ((pixel[2] as u16 * 127) / 255) as u8;
    }
    dimmed
}

fn region_select_rect(state: &CaptureState) -> Option<RECT> {
    let (start, curr) = (state.start_point?, state.current_point?);
    let left = start.0.min(curr.0);
    let top = start.1.min(curr.1);
    let right = start.0.max(curr.0);
    let bottom = start.1.max(curr.1);
    if right - left < 2 || bottom - top < 2 {
        return None;
    }
    Some(RECT {
        left,
        top,
        right,
        bottom,
    })
}

fn union_selection_dirty_rect(previous: Option<RECT>, next: Option<RECT>) -> Option<RECT> {
    let padding = 6;
    let expand = |rect: RECT| RECT {
        left: rect.left - padding,
        top: rect.top - padding,
        right: rect.right + padding,
        bottom: rect.bottom + padding,
    };
    match (previous.map(expand), next.map(expand)) {
        (Some(a), Some(b)) => Some(RECT {
            left: a.left.min(b.left),
            top: a.top.min(b.top),
            right: a.right.max(b.right),
            bottom: a.bottom.max(b.bottom),
        }),
        (Some(rect), None) | (None, Some(rect)) => Some(rect),
        (None, None) => None,
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
        let instance = HINSTANCE(windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap().0);
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
        ).unwrap();

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), left, top, width, height, SWP_SHOWWINDOW);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        state.result
    }
}

unsafe extern "system" fn capture_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = lparam.0 as *const CREATESTRUCTW;
            let state = (*cs).lpCreateParams as *mut CaptureState;
            SetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0), state as isize);
            LRESULT(1)
        }
        WM_CREATE => {
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let state = get_state(hwnd);
            if let Some(state) = state {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let rx = pt.x - state.left;
                    let ry = pt.y - state.top;
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
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let state = get_state(hwnd);
            if let Some(state) = state {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let previous_rect = if matches!(state.mode, NativeCaptureMode::RegionSelect { .. }) {
                        region_select_rect(state)
                    } else {
                        None
                    };
                    let rx = pt.x - state.left;
                    let ry = pt.y - state.top;
                    state.current_point = Some((rx, ry));
                    unsafe {
                        if matches!(state.mode, NativeCaptureMode::RegionSelect { .. }) {
                            let next_rect = region_select_rect(state);
                            if let Some(dirty) = union_selection_dirty_rect(previous_rect, next_rect) {
                                InvalidateRect(hwnd, Some(&dirty), false);
                            } else {
                                InvalidateRect(hwnd, None, false);
                            }
                        } else {
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
                ReleaseCapture();
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let rx = pt.x - state.left;
                    let ry = pt.y - state.top;

                    match state.mode {
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
                            DestroyWindow(hwnd);
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
                            DestroyWindow(hwnd);
                        }
                        NativeCaptureMode::ProtractorCalibration { .. } => {
                            state.protractor_points.push((rx + state.left, ry + state.top));
                            state.start_point = None;
                            if state.protractor_points.len() == 3 {
                                state.result = NativeCaptureResult::ProtractorPoints(state.protractor_points.clone());
                                DestroyWindow(hwnd);
                            } else {
                                InvalidateRect(hwnd, None, false);
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.0.is_null() {
                let state = get_state(hwnd);
                if let Some(state) = state {
                    let _ = draw_capture_to_dc(hdc, state);
                }
            }
            EndPaint(hwnd, &ps);
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
    windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), Some(lp_rect), erase);
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

fn circle_from_3_points(
    p1: (i32, i32),
    p2: (i32, i32),
    p3: (i32, i32),
) -> Option<((i32, i32), f32)> {
    let (x1, y1) = (p1.0 as f64, p1.1 as f64);
    let (x2, y2) = (p2.0 as f64, p2.1 as f64);
    let (x3, y3) = (p3.0 as f64, p3.1 as f64);

    let d = 2.0 * (x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2));
    if d.abs() < 1e-6 {
        return None;
    }

    let ux = ((x1 * x1 + y1 * y1) * (y2 - y3) + (x2 * x2 + y2 * y2) * (y3 - y1) + (x3 * x3 + y3 * y3) * (y1 - y2)) / d;
    let uy = ((x1 * x1 + y1 * y1) * (x3 - x2) + (x2 * x2 + y2 * y2) * (x1 - x3) + (x3 * x3 + y3 * y3) * (x2 - x1)) / d;

    let r = ((x1 - ux).powi(2) + (y1 - uy).powi(2)).sqrt();
    Some(((ux.round() as i32, uy.round() as i32), r as f32))
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

unsafe fn draw_capture_to_dc(hdc: HDC, state: &CaptureState) -> anyhow::Result<()> {
    if matches!(state.mode, NativeCaptureMode::RegionSelect { .. }) {
        draw_region_select_capture_to_dc(hdc, state)?;
        return Ok(());
    }

    let w = state.width as usize;
    let h = state.height as usize;

    let mut pixmap = Pixmap::new(state.width as u32, state.height as u32)
        .ok_or_else(|| anyhow::anyhow!("Failed to create tiny-skia Pixmap"))?;

    // 1. Draw the screenshot onto the pixmap
    pixmap.data_mut().copy_from_slice(&state.capture_frame.rgba);

    // 2. Draw a dark overlay over the whole screen when the capture flow asks for it.
    let should_dim_background = !matches!(
        state.mode,
        NativeCaptureMode::PointClick {
            dim_background: false,
            ..
        }
    );
    if should_dim_background {
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 128); // 50% opacity
        let screen_rect =
            Rect::from_xywh(0.0, 0.0, state.width as f32, state.height as f32).unwrap();
        pixmap.fill_rect(screen_rect, &paint, tiny_skia::Transform::identity(), None);
    }

    // 3. Render specific overlay elements based on capture mode
    match state.mode {
        NativeCaptureMode::RegionSelect { .. } => {
            if let (Some(start), Some(curr)) = (state.start_point, state.current_point) {
                let x = start.0.min(curr.0);
                let y = start.1.min(curr.1);
                let rw = (start.0 - curr.0).abs() as usize;
                let rh = (start.1 - curr.1).abs() as usize;

                if rw >= 2 && rh >= 2 {
                    // Blit back original screenshot region (so it is bright)
                    blit_rect(&state.capture_frame.rgba, w, pixmap.data_mut(), w, x as usize, y as usize, rw, rh);

                    // Draw selection border
                    let mut border_paint = Paint::default();
                    border_paint.set_color_rgba8(0, 160, 255, 255); // Blue border
                    let mut stroke = Stroke::default();
                    stroke.width = 1.5;

                    let mut pb = PathBuilder::new();
                    pb.move_to(x as f32, y as f32);
                    pb.line_to((x as f32 + rw as f32), y as f32);
                    pb.line_to((x as f32 + rw as f32), (y as f32 + rh as f32));
                    pb.line_to(x as f32, (y as f32 + rh as f32));
                    pb.close();
                    if let Some(path) = pb.finish() {
                        pixmap.stroke_path(&path, &border_paint, &stroke, tiny_skia::Transform::identity(), None);
                    }
                }
            }
        }
        NativeCaptureMode::ProtractorCalibration { .. } => {
            // Draw already-clicked points
            let mut pt_paint = Paint::default();
            pt_paint.set_color_rgba8(255, 50, 50, 255);
            let mut stroke = Stroke::default();
            stroke.width = 2.0;

            let mut white_paint = Paint::default();
            white_paint.set_color_rgba8(255, 255, 255, 255);

            for (idx, pt) in state.protractor_points.iter().enumerate() {
                let rx = pt.0 - state.left;
                let ry = pt.1 - state.top;

                // Draw filled point (radius 6)
                let mut pb = PathBuilder::new();
                pb.push_circle(rx as f32, ry as f32, 6.0);
                let path = pb.finish().unwrap();
                pixmap.fill_path(&path, &pt_paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);

                // Draw outline (radius 10)
                let mut pb = PathBuilder::new();
                pb.push_circle(rx as f32, ry as f32, 10.0);
                let path = pb.finish().unwrap();
                pixmap.stroke_path(&path, &white_paint, &stroke, tiny_skia::Transform::identity(), None);
            }

            // Draw line/circle preview based on current mouse coordinate
            if let Some(curr) = state.current_point {
                let count = state.protractor_points.len();
                if count == 1 {
                    // Draw dashed line from Point 1 to cursor
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
                    pixmap.stroke_path(&path, &line_paint, &dashed_stroke, tiny_skia::Transform::identity(), None);
                } else if count == 2 {
                    // Draw circle passing through Point 1, Point 2 and cursor
                    let pt1 = state.protractor_points[0];
                    let pt2 = state.protractor_points[1];
                    let curr_abs = (curr.0 + state.left, curr.1 + state.top);

                    if let Some((center, radius)) = circle_from_3_points(pt1, pt2, curr_abs) {
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
                        pixmap.stroke_path(&path, &circle_paint, &dashed_stroke, tiny_skia::Transform::identity(), None);
                    }
                }
            }
        }
        NativeCaptureMode::PointClick { .. } => {
            // Draw crosshair at current point
            if let Some(curr) = state.current_point {
                let mut ch_paint = Paint::default();
                ch_paint.set_color_rgba8(0, 160, 255, 200);
                let mut stroke = Stroke::default();
                stroke.width = 1.0;

                let cx = curr.0 as f32;
                let cy = curr.1 as f32;

                let mut pb = PathBuilder::new();
                pb.move_to(cx - 10.0, cy);
                pb.line_to(cx + 10.0, cy);
                pb.move_to(cx, cy - 10.0);
                pb.line_to(cx, cy + 10.0);
                let path = pb.finish().unwrap();
                pixmap.stroke_path(&path, &ch_paint, &stroke, tiny_skia::Transform::identity(), None);
            }
        }
    }

    let show_preview_panel = matches!(state.mode, NativeCaptureMode::PointClick { .. });
    let show_cursor_tooltip = !matches!(state.mode, NativeCaptureMode::RegionSelect { .. });

    // Draw coordinate & color magnifier preview panel
    let mut center_color = (0u8, 0u8, 0u8, 255u8);
    let mut panel_x = 0.0f32;
    let mut panel_y = 0.0f32;
    let mut preview_panel_visible = false;

    if show_preview_panel && let Some(curr) = state.current_point {
        preview_panel_visible = true;

        let panel_w = 200.0f32;
        let panel_h = 246.0f32;
        let margin = 18.0f32;

        let pointer_x = curr.0 as f32;
        let pointer_y = curr.1 as f32;
        let safe_r = 40.0f32;
        let safe_left = pointer_x - safe_r;
        let safe_right = pointer_x + safe_r;
        let safe_top = pointer_y - safe_r;
        let safe_bottom = pointer_y + safe_r;

        panel_x = state.width as f32 - panel_w - margin;
        panel_y = margin;

        let candidates = [
            (state.width as f32 - panel_w - margin, margin),
            (margin, margin),
            (state.width as f32 - panel_w - margin, state.height as f32 - panel_h - margin),
            (margin, state.height as f32 - panel_h - margin),
        ];

        for &(cx, cy) in &candidates {
            let intersects = !(cx + panel_w < safe_left || cx > safe_right || cy + panel_h < safe_top || cy > safe_bottom);
            if !intersects {
                panel_x = cx;
                panel_y = cy;
                break;
            }
        }

        // Draw background
        let mut bg_paint = Paint::default();
        bg_paint.set_color_rgba8(12, 18, 28, 255);
        let mut border_paint = Paint::default();
        border_paint.set_color_rgba8(110, 156, 210, 255);
        let border_stroke = Stroke {
            width: 1.0,
            ..Default::default()
        };
        draw_rounded_rect(&mut pixmap, panel_x, panel_y, panel_w, panel_h, 10.0, &bg_paint, Some((&border_paint, &border_stroke)));

        // Draw magnified preview
        let content_left = panel_x + 28.0;
        let preview_y = panel_y + 12.0;
        let preview_w = 144.0f32;
        let preview_h = 144.0f32;
        let sample_size = 17;
        let cell_size = preview_w / sample_size as f32;

        for dy in 0..sample_size {
            let sy = curr.1 - 8 + dy as i32;
            for dx in 0..sample_size {
                let sx = curr.0 - 8 + dx as i32;

                let mut r = 0u8;
                let mut g = 0u8;
                let mut b = 0u8;
                let mut a = 255u8;

                if sx >= 0 && sx < state.width && sy >= 0 && sy < state.height {
                    let idx = (sy as usize * state.width as usize + sx as usize) * 4;
                    if idx + 3 < state.capture_frame.rgba.len() {
                        r = state.capture_frame.rgba[idx];
                        g = state.capture_frame.rgba[idx + 1];
                        b = state.capture_frame.rgba[idx + 2];
                        a = state.capture_frame.rgba[idx + 3];
                    }
                }

                if dx == 8 && dy == 8 {
                    center_color = (r, g, b, a);
                }

                let cx = content_left + dx as f32 * cell_size;
                let cy = preview_y + dy as f32 * cell_size;
                let cell_rect = Rect::from_xywh(cx, cy, cell_size, cell_size).unwrap();
                let mut cell_paint = Paint::default();
                cell_paint.set_color_rgba8(r, g, b, a);
                pixmap.fill_rect(cell_rect, &cell_paint, tiny_skia::Transform::identity(), None);
            }
        }

        // Draw center pixel border highlight
        let center_cx = content_left + 8.0 * cell_size;
        let center_cy = preview_y + 8.0 * cell_size;
        let mut center_pb = PathBuilder::new();
        center_pb.move_to(center_cx, center_cy);
        center_pb.line_to(center_cx + cell_size, center_cy);
        center_pb.line_to(center_cx + cell_size, center_cy + cell_size);
        center_pb.line_to(center_cx, center_cy + cell_size);
        center_pb.close();
        if let Some(center_path) = center_pb.finish() {
            let mut center_border_paint = Paint::default();
            center_border_paint.set_color_rgba8(120, 220, 255, 255);
            let center_border_stroke = Stroke {
                width: 2.0,
                ..Default::default()
            };
            pixmap.stroke_path(
                &center_path,
                &center_border_paint,
                &center_border_stroke,
                tiny_skia::Transform::identity(),
                None,
            );
        }

        // Draw preview outline border
        let mut preview_pb = PathBuilder::new();
        let radius = 6.0f32;
        preview_pb.move_to(content_left + radius, preview_y);
        preview_pb.line_to(content_left + preview_w - radius, preview_y);
        preview_pb.quad_to(content_left + preview_w, preview_y, content_left + preview_w, preview_y + radius);
        preview_pb.line_to(content_left + preview_w, preview_y + preview_h - radius);
        preview_pb.quad_to(content_left + preview_w, preview_y + preview_h, content_left + preview_w - radius, preview_y + preview_h);
        preview_pb.line_to(content_left + radius, preview_y + preview_h);
        preview_pb.quad_to(content_left, preview_y + preview_h, content_left, preview_y + preview_h - radius);
        preview_pb.line_to(content_left, preview_y + radius);
        preview_pb.quad_to(content_left, preview_y, content_left + radius, preview_y);
        preview_pb.close();
        if let Some(preview_path) = preview_pb.finish() {
            let mut preview_border_paint = Paint::default();
            preview_border_paint.set_color_rgba8(146, 192, 248, 255);
            let preview_border_stroke = Stroke {
                width: 1.0,
                ..Default::default()
            };
            pixmap.stroke_path(
                &preview_path,
                &preview_border_paint,
                &preview_border_stroke,
                tiny_skia::Transform::identity(),
                None,
            );
        }

        // Draw swatch
        let swatch_x = panel_x + 12.0;
        let swatch_y = panel_y + 168.0;
        let mut swatch_fill_paint = Paint::default();
        swatch_fill_paint.set_color_rgba8(center_color.0, center_color.1, center_color.2, 255);
        let mut swatch_border_paint = Paint::default();
        swatch_border_paint.set_color_rgba8(255, 255, 255, 255);
        let swatch_border_stroke = Stroke {
            width: 1.0,
            ..Default::default()
        };
        draw_rounded_rect(
            &mut pixmap,
            swatch_x,
            swatch_y,
            26.0,
            26.0,
            6.0,
            &swatch_fill_paint,
            Some((&swatch_border_paint, &swatch_border_stroke)),
        );
    }

    // 4. Copy Pixmap to GDI window HDC
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
        hdc,
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

    // 5. Draw status bar & instructions using GDI DrawTextW
    let status_text = match state.mode {
        NativeCaptureMode::ProtractorCalibration { ui_language } => {
            let count = state.protractor_points.len();
            match ui_language {
                crate::model::UiLanguage::Vietnamese => {
                    match count {
                        0 => "Cân chỉnh: Click điểm 1/3 trên màn hình. Nhấn Esc để hủy.",
                        1 => "Cân chỉnh: Click điểm 2/3 trên màn hình. Nhấn Esc để hủy.",
                        _ => "Cân chỉnh: Click điểm 3/3 trên màn hình. Nhấn Esc để hủy.",
                    }
                }
                crate::model::UiLanguage::English | crate::model::UiLanguage::Icon => {
                    match count {
                        0 => "Calibration: Click point 1/3 on screen. Press Esc to cancel.",
                        1 => "Calibration: Click point 2/3 on screen. Press Esc to cancel.",
                        _ => "Calibration: Click point 3/3 on screen. Press Esc to cancel.",
                    }
                }
            }
        }
        NativeCaptureMode::RegionSelect { is_template, vietnamese } => {
            if vietnamese {
                if is_template {
                    "Kéo chuột trên màn hình để chọn mẫu ảnh (template). Nhấn Esc để hủy."
                } else {
                    "Kéo chuột trên màn hình để chọn vùng tìm kiếm ảnh. Nhấn Esc để hủy."
                }
            } else {
                if is_template {
                    "Drag on screen to pick an image template. Press Esc to cancel."
                } else {
                    "Drag on screen to pick the image search area. Press Esc to cancel."
                }
            }
        }
        NativeCaptureMode::PointClick { vietnamese, .. } => {
            if vietnamese {
                "Nhấp chuột vào một điểm trên màn hình để lấy tọa độ/màu sắc. Nhấn Esc để hủy."
            } else {
                "Click a point on screen to capture. Press Esc to cancel."
            }
        }
    };

    let font = CreateFontW(
        22, 0, 0, 0,
        FW_BOLD.0 as i32,
        0, 0, 0,
        FONT_CHARSET(0),
        FONT_OUTPUT_PRECISION(0),
        FONT_CLIP_PRECISION(0),
        FONT_QUALITY(0),
        0,
        w!("Segoe UI"),
    );

    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, rgb(255, 255, 255));

    let mut text_u16: Vec<u16> = status_text.encode_utf16().collect();
    let mut calc_rect = RECT::default();
    let _ = DrawTextW(hdc, &mut text_u16, &mut calc_rect, DT_CALCRECT);
    let text_w = calc_rect.right - calc_rect.left;
    let text_h = calc_rect.bottom - calc_rect.top;

    let pill_w = text_w + 48;
    let pill_h = text_h + 16;
    let pill_x = (state.width - pill_w) / 2;
    let pill_y = 40;

    // Draw pill background (using GDI round rect)
    let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(rgb(12, 18, 28));
    let pen = windows::Win32::Graphics::Gdi::CreatePen(
        windows::Win32::Graphics::Gdi::PS_SOLID,
        1,
        rgb(110, 156, 210),
    );
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));

    let _ = windows::Win32::Graphics::Gdi::RoundRect(
        hdc,
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
    let _ = DrawTextW(hdc, &mut text_u16, &mut text_rect, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

    // Clean up pill graphics objects
    let _ = SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    let _ = DeleteObject(HGDIOBJ(pen.0));

    // Render coordinates tooltip next to mouse cursor
    if show_cursor_tooltip && let Some(curr) = state.current_point {
        let abs_x = curr.0 + state.left;
        let abs_y = curr.1 + state.top;
        let coords_str = format!("X: {}, Y: {}", abs_x, abs_y);
        let mut coords_u16: Vec<u16> = coords_str.encode_utf16().collect();

        let mut c_calc = RECT::default();
        let _ = DrawTextW(hdc, &mut coords_u16, &mut c_calc, DT_CALCRECT);
        let cw = c_calc.right - c_calc.left;
        let ch = c_calc.bottom - c_calc.top;

        let tooltip_x = curr.0 + 15;
        let tooltip_y = curr.1 + 15;

        // Draw small dark tooltip background
        let t_brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(rgb(15, 23, 42));
        let t_pen = windows::Win32::Graphics::Gdi::CreatePen(
            windows::Win32::Graphics::Gdi::PS_SOLID,
            1,
            rgb(0, 160, 255),
        );
        let old_tb = SelectObject(hdc, HGDIOBJ(t_brush.0));
        let old_tp = SelectObject(hdc, HGDIOBJ(t_pen.0));

        let _ = windows::Win32::Graphics::Gdi::RoundRect(
            hdc,
            tooltip_x,
            tooltip_y,
            tooltip_x + cw + 16,
            tooltip_y + ch + 10,
            6,
            6,
        );

        let mut t_rect = RECT {
            left: tooltip_x + 8,
            top: tooltip_y + 5,
            right: tooltip_x + cw + 8,
            bottom: tooltip_y + ch + 5,
        };
        let _ = DrawTextW(hdc, &mut coords_u16, &mut t_rect, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

        let _ = SelectObject(hdc, old_tb);
        let _ = SelectObject(hdc, old_tp);
        let _ = DeleteObject(HGDIOBJ(t_brush.0));
        let _ = DeleteObject(HGDIOBJ(t_pen.0));
    }

    // Draw preview panel text if panel is visible
    if preview_panel_visible {
        let vietnamese = match state.mode {
            NativeCaptureMode::ProtractorCalibration { ui_language } => ui_language == crate::model::UiLanguage::Vietnamese,
            NativeCaptureMode::RegionSelect { vietnamese, .. } => vietnamese,
            NativeCaptureMode::PointClick { vietnamese, .. } => vietnamese,
        };

        // Create Hex Code Font (18px bold)
        let hex_font = CreateFontW(
            18, 0, 0, 0,
            700, // FW_BOLD
            0, 0, 0,
            FONT_CHARSET(0),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(0),
            0,
            w!("Segoe UI"),
        );

        // Create Label Font (13px normal)
        let label_font = CreateFontW(
            13, 0, 0, 0,
            400, // FW_NORMAL
            0, 0, 0,
            FONT_CHARSET(0),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(0),
            0,
            w!("Segoe UI"),
        );

        let _ = SetBkMode(hdc, TRANSPARENT);

        // 1. Draw Hex code
        let hex_str = format!("#{:02X}{:02X}{:02X}", center_color.0, center_color.1, center_color.2);
        let mut hex_u16: Vec<u16> = hex_str.encode_utf16().collect();
        let mut hex_rect = RECT {
            left: (panel_x + 48.0) as i32,
            top: (panel_y + 171.0) as i32,
            right: (panel_x + 192.0) as i32,
            bottom: (panel_y + 194.0) as i32,
        };
        let old_f = SelectObject(hdc, HGDIOBJ(hex_font.0));
        let _ = SetTextColor(hdc, rgb(255, 255, 255));
        let _ = DrawTextW(hdc, &mut hex_u16, &mut hex_rect, DT_SINGLELINE | DT_VCENTER);

        // 2. Draw Coordinates
        if let Some(curr) = state.current_point {
            let abs_x = curr.0 + state.left;
            let abs_y = curr.1 + state.top;
            let coords_str = format!("X: {abs_x}  Y: {abs_y}");
            let mut coords_u16: Vec<u16> = coords_str.encode_utf16().collect();
            let mut coords_rect = RECT {
                left: (panel_x + 12.0) as i32,
                top: (panel_y + 202.0) as i32,
                right: (panel_x + 192.0) as i32,
                bottom: (panel_y + 220.0) as i32,
            };
            let _ = SelectObject(hdc, HGDIOBJ(label_font.0));
            let _ = SetTextColor(hdc, rgb(188, 206, 230));
            let _ = DrawTextW(hdc, &mut coords_u16, &mut coords_rect, DT_SINGLELINE | DT_VCENTER);
        }

        // 3. Draw Center Pixel label
        let center_pixel_text = if vietnamese { "Pixel trung tam" } else { "Center pixel" };
        let mut center_u16: Vec<u16> = center_pixel_text.encode_utf16().collect();
        let mut center_rect = RECT {
            left: (panel_x + 12.0) as i32,
            top: (panel_y + 222.0) as i32,
            right: (panel_x + 192.0) as i32,
            bottom: (panel_y + 240.0) as i32,
        };
        let _ = SelectObject(hdc, HGDIOBJ(label_font.0));
        let _ = SetTextColor(hdc, rgb(188, 206, 230));
        let _ = DrawTextW(hdc, &mut center_u16, &mut center_rect, DT_SINGLELINE | DT_VCENTER);

        // Cleanup
        let _ = SelectObject(hdc, old_f);
        let _ = DeleteObject(HGDIOBJ(hex_font.0));
        let _ = DeleteObject(HGDIOBJ(label_font.0));
    }

    // Restore font and delete
    let _ = SelectObject(hdc, old_font);
    let _ = DeleteObject(HGDIOBJ(font.0));

    Ok(())
}

unsafe fn draw_region_select_capture_to_dc(
    hdc: HDC,
    state: &CaptureState,
) -> anyhow::Result<()> {
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

    let mut dimmed_bgra = state.dimmed_rgba.clone();
    for pixel in dimmed_bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let _ = StretchDIBits(
        hdc,
        0,
        0,
        state.width,
        state.height,
        0,
        0,
        state.width,
        state.height,
        Some(dimmed_bgra.as_ptr() as *const std::ffi::c_void),
        &bmi,
        DIB_RGB_COLORS,
        SRCCOPY,
    );

    if let Some(rect) = region_select_rect(state) {
        let select_w = rect.right - rect.left;
        let select_h = rect.bottom - rect.top;
        if select_w >= 2 && select_h >= 2 {
            let mut select_bgra = state.capture_frame.rgba.clone();
            for pixel in select_bgra.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }

            let _ = StretchDIBits(
                hdc,
                rect.left,
                rect.top,
                select_w,
                select_h,
                rect.left,
                rect.top,
                select_w,
                select_h,
                Some(select_bgra.as_ptr() as *const std::ffi::c_void),
                &bmi,
                DIB_RGB_COLORS,
                SRCCOPY,
            );

            let pen = CreatePen(PS_SOLID, 2, rgb(0, 160, 255));
            let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
            let _ = MoveToEx(hdc, rect.left, rect.top, None);
            let _ = LineTo(hdc, rect.right, rect.top);
            let _ = LineTo(hdc, rect.right, rect.bottom);
            let _ = LineTo(hdc, rect.left, rect.bottom);
            let _ = LineTo(hdc, rect.left, rect.top);
            let _ = SelectObject(hdc, old_pen);
            let _ = DeleteObject(HGDIOBJ(pen.0));
        }
    }

    let status_text =
        "Drag on screen to capture a region with your drawing. Press Esc to cancel.";
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
    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, rgb(255, 255, 255));

    let mut text_u16: Vec<u16> = status_text.encode_utf16().collect();
    let mut calc_rect = RECT::default();
    let _ = DrawTextW(hdc, &mut text_u16, &mut calc_rect, DT_CALCRECT);
    let text_w = calc_rect.right - calc_rect.left;
    let text_h = calc_rect.bottom - calc_rect.top;
    let pill_w = text_w + 48;
    let pill_h = text_h + 16;
    let pill_x = (state.width - pill_w) / 2;
    let pill_y = 40;

    let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(rgb(12, 18, 28));
    let pen = CreatePen(PS_SOLID, 1, rgb(110, 156, 210));
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    let _ = windows::Win32::Graphics::Gdi::RoundRect(
        hdc,
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
        hdc,
        &mut text_u16,
        &mut text_rect,
        DT_CENTER | DT_SINGLELINE | DT_VCENTER,
    );

    let _ = SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, old_pen);
    let _ = SelectObject(hdc, old_font);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    let _ = DeleteObject(HGDIOBJ(pen.0));
    let _ = DeleteObject(HGDIOBJ(font.0));

    Ok(())
}
