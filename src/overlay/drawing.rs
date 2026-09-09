use anyhow::Result;
use std::mem::size_of;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, IMAGE_ICON, LR_LOADFROMFILE, LoadImageW,
};
use windows::core::PCWSTR;

use super::{HOOK_STATE, TRAY_UID, WMAPP_TRAYICON, runtime_icon_path};

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
    match (show_minutes, show_seconds, show_ms) {
        (false, false, false) => "00".to_string(),
        (false, false, true) => elapsed_ms.to_string(),
        (false, true, false) => total_secs.to_string(),
        (false, true, true) => format!("{total_secs}.{ms:03}"),
        (true, false, false) => minutes.to_string(),
        (true, false, true) => format!("{minutes}.{ms:03}"),
        (true, true, false) => format!("{minutes:02}:{seconds:02}"),
        (true, true, true) => format!("{minutes:02}:{seconds:02}.{ms:03}"),
    }
}

#[cfg(test)]
mod tests {
    use super::format_stopwatch_time;

    #[test]
    fn stopwatch_ms_only_keeps_total_value() {
        assert_eq!(format_stopwatch_time(678_888, false, false, true), "678888");
    }

    #[test]
    fn stopwatch_sec_only_keeps_total_value() {
        assert_eq!(format_stopwatch_time(125_432, false, true, false), "125");
    }

    #[test]
    fn stopwatch_min_sec_ms_keeps_classic_format() {
        assert_eq!(
            format_stopwatch_time(125_432, true, true, true),
            "02:05.432"
        );
    }
}
