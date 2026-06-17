use std::mem::size_of;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    LoadImageW, DestroyIcon, IMAGE_ICON, LR_LOADFROMFILE,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NOTIFYICONDATAW, NIM_ADD, NIM_MODIFY,
    NIF_MESSAGE, NIF_ICON, NIF_TIP,
};
use anyhow::Result;

use super::{
    HOOK_STATE, TRAY_UID, WMAPP_TRAYICON, runtime_icon_path,
};

pub(crate) fn blend_rgba_pixel(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    color: [u8; 4],
) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }

    let index = (y as usize * width + x as usize) * 4;
    let alpha = color[3] as f32 / 255.0;
    let inv = 1.0 - alpha;
    let dst = &mut pixels[index..index + 4];
    dst[0] = (dst[0] as f32 * inv + color[2] as f32 * alpha).round() as u8;
    dst[1] = (dst[1] as f32 * inv + color[1] as f32 * alpha).round() as u8;
    dst[2] = (dst[2] as f32 * inv + color[0] as f32 * alpha).round() as u8;
    dst[3] = dst[3].max(color[3]);
}

pub(crate) fn fill_rect_rgba(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    left: i32,
    top: i32,
    rect_width: i32,
    rect_height: i32,
    color: [u8; 4],
) {
    let right = left.saturating_add(rect_width).max(left + 1);
    let bottom = top.saturating_add(rect_height).max(top + 1);
    for y in top.max(0)..bottom {
        for x in left.max(0)..right {
            blend_rgba_pixel(pixels, width, height, x, y, color);
        }
    }
}

pub(crate) fn point_in_ellipse(
    x: i32,
    y: i32,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
) -> bool {
    let center_x = left as f32 + width as f32 * 0.5;
    let center_y = top as f32 + height as f32 * 0.5;
    let radius_x = (width as f32 * 0.5).max(1.0);
    let radius_y = (height as f32 * 0.5).max(1.0);
    let dx = (x as f32 + 0.5 - center_x) / radius_x;
    let dy = (y as f32 + 0.5 - center_y) / radius_y;
    dx * dx + dy * dy <= 1.0
}

pub(crate) fn fill_ellipse_rgba(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    left: i32,
    top: i32,
    ellipse_width: i32,
    ellipse_height: i32,
    color: [u8; 4],
) {
    let right = left.saturating_add(ellipse_width).max(left + 1);
    let bottom = top.saturating_add(ellipse_height).max(top + 1);
    for y in top.max(0)..bottom {
        for x in left.max(0)..right {
            if point_in_ellipse(x, y, left, top, ellipse_width, ellipse_height) {
                blend_rgba_pixel(pixels, width, height, x, y, color);
            }
        }
    }
}

pub(crate) fn draw_rect_outline_rgba(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    left: i32,
    top: i32,
    rect_width: i32,
    rect_height: i32,
    color: [u8; 4],
) {
    let right = left.saturating_add(rect_width).max(left + 1) - 1;
    let bottom = top.saturating_add(rect_height).max(top + 1) - 1;
    draw_line_rgba(pixels, width, height, left, top, right, top, color);
    draw_line_rgba(pixels, width, height, right, top, right, bottom, color);
    draw_line_rgba(pixels, width, height, right, bottom, left, bottom, color);
    draw_line_rgba(pixels, width, height, left, bottom, left, top, color);
}

pub(crate) fn draw_ellipse_outline_rgba(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    left: i32,
    top: i32,
    ellipse_width: i32,
    ellipse_height: i32,
    color: [u8; 4],
) {
    let steps = ((ellipse_width.max(ellipse_height) as f32) * std::f32::consts::TAU / 2.0)
        .round()
        .clamp(32.0, 360.0) as i32;
    let center_x = left as f32 + ellipse_width as f32 * 0.5;
    let center_y = top as f32 + ellipse_height as f32 * 0.5;
    let radius_x = ellipse_width as f32 * 0.5;
    let radius_y = ellipse_height as f32 * 0.5;
    let mut prev_x = center_x + radius_x;
    let mut prev_y = center_y;
    for step in 1..=steps {
        let angle = (step as f32 / steps as f32) * std::f32::consts::TAU;
        let next_x = center_x + radius_x * angle.cos();
        let next_y = center_y + radius_y * angle.sin();
        draw_line_rgba(
            pixels,
            width,
            height,
            prev_x.round() as i32,
            prev_y.round() as i32,
            next_x.round() as i32,
            next_y.round() as i32,
            color,
        );
        prev_x = next_x;
        prev_y = next_y;
    }
}

pub(crate) fn draw_square_brush_rgba(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    cx: i32,
    cy: i32,
    thickness: i32,
    color: [u8; 4],
) {
    let thickness = thickness.max(1);
    let left_extent = (thickness - 1) / 2;
    let right_extent = thickness / 2;
    for py in (cy - left_extent)..=(cy + right_extent) {
        for px in (cx - left_extent)..=(cx + right_extent) {
            blend_rgba_pixel(pixels, width, height, px, py, color);
        }
    }
}

pub(crate) fn draw_line_aa_impl(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: [u8; 4],
    thickness: i32,
) {
    let mut x = x0;
    let mut y = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        draw_square_brush_rgba(pixels, width, height, x, y, thickness, color);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = err * 2;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

pub(crate) fn draw_line_rgba(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: [u8; 4],
) {
    draw_line_aa_impl(pixels, width, height, x0, y0, x1, y1, color, 1);
}

pub(crate) fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = rgba.to_vec();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    bgra
}

pub(crate) unsafe fn add_tray_icon(hwnd: HWND) -> Result<()> {
    let mut data = notify_icon(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WMAPP_TRAYICON;
    let icon_path = runtime_icon_path(hwnd, HOOK_STATE.lock().macros_master_enabled)?;
    data.hIcon = windows::Win32::UI::WindowsAndMessaging::HICON(
        LoadImageW(
            None,
            PCWSTR(icon_path.as_ptr()),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE,
        )?
        .0,
    );
    let tip = "MacroNest".encode_utf16().collect::<Vec<_>>();
    for (index, value) in tip.into_iter().enumerate() {
        if index >= data.szTip.len().saturating_sub(1) {
            break;
        }

        data.szTip[index] = value;
    }

    let _ = Shell_NotifyIconW(NIM_ADD, &data);
    Ok(())
}

pub(crate) unsafe fn update_tray_icon(hwnd: HWND, enabled: bool) -> Result<()> {
    let mut data = notify_icon(hwnd);
    data.uFlags = NIF_ICON;
    let icon_path = runtime_icon_path(hwnd, enabled)?;
    data.hIcon = windows::Win32::UI::WindowsAndMessaging::HICON(
        LoadImageW(
            None,
            PCWSTR(icon_path.as_ptr()),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE,
        )?
        .0,
    );
    let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    if !data.hIcon.is_invalid() {
        let _ = DestroyIcon(data.hIcon);
    }

    Ok(())
}

pub(crate) fn notify_icon(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        ..Default::default()
    }
}

pub(crate) fn format_stopwatch_time(
    elapsed_ms: u64,
    show_minutes: bool,
    show_seconds: bool,
    show_ms: bool,
) -> String {
    let total_secs = elapsed_ms / 1000;
    let ms = elapsed_ms % 1000;
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    let mut parts = Vec::new();
    if show_minutes {
        parts.push(format!("{:02}", minutes));
    }

    if show_seconds {
        parts.push(format!("{:02}", seconds));
    }

    let mut time_str = parts.join(":");
    if show_ms {
        if time_str.is_empty() {
            time_str = format!("{:03}", ms);
        } else {
            time_str = format!("{}.{:03}", time_str, ms);
        }
    }

    if time_str.is_empty() {
        "00".to_string()
    } else {
        time_str
    }
}
