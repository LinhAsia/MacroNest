#![allow(unsafe_op_in_unsafe_fn)]

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM, HINSTANCE, COLORREF},
    Graphics::Gdi::{
        BeginPaint, EndPaint, PAINTSTRUCT, HDC, HGDIOBJ, CreateCompatibleDC, DeleteDC, SelectObject,
        StretchDIBits, DIB_RGB_COLORS, GetDC, ReleaseDC, CreateDIBSection, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DrawTextW, DT_CENTER, DT_SINGLELINE, DT_VCENTER, SetBkMode,
        SetTextColor, TRANSPARENT, CreateFontW, HFONT, FW_BOLD, DT_CALCRECT, DeleteObject, SRCCOPY,
        FONT_CHARSET, FONT_OUTPUT_PRECISION, FONT_CLIP_PRECISION, FONT_QUALITY,
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
                    InvalidateRect(hwnd, None, false);
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
                    state.current_point = Some((rx, ry));
                    InvalidateRect(hwnd, None, false);
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

unsafe fn draw_capture_to_dc(hdc: HDC, state: &CaptureState) -> anyhow::Result<()> {
    let w = state.width as usize;
    let h = state.height as usize;

    let mut pixmap = Pixmap::new(state.width as u32, state.height as u32)
        .ok_or_else(|| anyhow::anyhow!("Failed to create tiny-skia Pixmap"))?;

    // 1. Draw the screenshot onto the pixmap
    pixmap.data_mut().copy_from_slice(&state.capture_frame.rgba);

    // 2. Draw a dark overlay over the whole screen
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, 128); // 50% opacity
    let screen_rect = Rect::from_xywh(0.0, 0.0, state.width as f32, state.height as f32).unwrap();
    pixmap.fill_rect(screen_rect, &paint, tiny_skia::Transform::identity(), None);

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
        Some(pixmap.data().as_ptr() as *const std::ffi::c_void),
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
        NativeCaptureMode::PointClick { vietnamese } => {
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
    if let Some(curr) = state.current_point {
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

    // Restore font and delete
    let _ = SelectObject(hdc, old_font);
    let _ = DeleteObject(HGDIOBJ(font.0));

    Ok(())
}
