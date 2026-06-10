use std::{
    collections::HashSet,
    mem::size_of,
    os::raw::c_void,
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use windows::Win32::{
    Foundation::RECT,
    Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
    Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST, MonitorFromPoint},
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus},
    UI::WindowsAndMessaging::{
        BringWindowToTop, GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowRect,
        GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic,
        SW_RESTORE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
    },
};

use super::{
    HOOK_STATE, WindowFocusPreset, WindowPreset, calculate_window_bounds, ensure_window_restored,
    find_target_window_hwnd, is_internal_app_window, remove_window_title_bar,
    replay_held_inputs_after_focus, resolve_window_target, restore_window_title_bar,
    window_belongs_to_current_process,
};

pub(super) fn apply_window_preset_by_id(spec: &str) -> Result<()> {
    let preset = {
        let hook_state = HOOK_STATE.lock();
        let spec = spec.trim();
        hook_state
            .window_presets
            .iter()
            .find(|preset| preset.id.to_string() == spec)
            .cloned()
            .or_else(|| {
                hook_state
                    .window_presets
                    .iter()
                    .find(|preset| preset.name.trim().eq_ignore_ascii_case(spec))
                    .cloned()
            })
    }
    .context("Window preset was not found")?;
    apply_window_preset_for_macro(&preset)
}

pub(super) fn focus_window_by_preset_id(spec: &str) -> Result<()> {
    let spec = spec.trim();
    let preset = {
        let hook_state = HOOK_STATE.lock();
        hook_state
            .window_focus_presets
            .iter()
            .find(|preset| preset.id.to_string() == spec)
            .cloned()
            .or_else(|| {
                hook_state
                    .window_focus_presets
                    .iter()
                    .find(|preset| preset.name.trim().eq_ignore_ascii_case(spec))
                    .cloned()
            })
    };
    if let Some(preset) = preset {
        focus_window_for_preset(&preset)
    } else {
        // Fallback: if spec matches no preset, treat it as a direct window title/selector
        let clean_title = if let Some(prefix) = spec.strip_suffix(')')
            && let Some((base, _)) = prefix.rsplit_once(" (0x")
        {
            base
        } else {
            spec
        };
        focus_window_for_title(Some(clean_title), &[], false, true)
    }
}

pub(super) fn focus_window_for_preset(preset: &WindowFocusPreset) -> Result<()> {
    focus_window_for_title(
        preset.target_window_title.as_deref(),
        &preset.extra_target_window_titles,
        preset.match_duplicate_window_titles,
        true,
    )
}

pub(super) fn apply_window_preset(preset: &WindowPreset) -> Result<()> {
    apply_window_preset_impl(preset, true)
}

pub(super) fn apply_window_preset_animated(preset: &WindowPreset) -> Result<()> {
    if !preset.animate_enabled {
        return Ok(());
    }
    unsafe {
        let target = resolve_window_target(
            preset.target_window_title.as_deref(),
            &preset.extra_target_window_titles,
            preset.match_duplicate_window_titles,
            false,
        );
        if target.0.is_null() {
            bail!("No foreground window is available");
        }
        let target_root = GetAncestor(target, GA_ROOT);
        if !target_root.0.is_null()
            && window_belongs_to_current_process(target_root)
            && !is_internal_app_window(target_root)
        {
            return Ok(());
        }

        ensure_window_restored(target);
        if preset.remove_title_bar {
            let _ = remove_window_title_bar(target);
        } else {
            let _ = restore_window_title_bar(target);
        }
        super::wait_for_window_frame_to_settle(target);

        let mut start = windows::Win32::Foundation::RECT::default();
        GetWindowRect(target, &mut start)?;
        let end = calculate_window_bounds(target, preset)?;
        super::animate_window_rect(target, start, end, preset.animate_duration_ms.max(60))?;
    }
    Ok(())
}

pub(super) fn restore_window_title_bar_for_preset(preset: &WindowPreset) -> Result<()> {
    if !preset.restore_titlebar_enabled {
        return Ok(());
    }
    unsafe {
        let target = resolve_window_target(
            preset.target_window_title.as_deref(),
            &preset.extra_target_window_titles,
            preset.match_duplicate_window_titles,
            false,
        );
        if target.0.is_null() {
            bail!("No foreground window is available");
        }
        let target_root = GetAncestor(target, GA_ROOT);
        if !target_root.0.is_null()
            && window_belongs_to_current_process(target_root)
            && !is_internal_app_window(target_root)
        {
            return Ok(());
        }

        let _ = ShowWindow(target, SW_RESTORE);
        restore_window_title_bar(target)?;
    }
    Ok(())
}

fn focus_window_for_title(
    target_title: Option<&str>,
    extra_target_titles: &[String],
    match_duplicate_window_titles: bool,
    prefer_other_if_foreground_matches: bool,
) -> Result<()> {
    let hwnd = find_target_window_hwnd(
        target_title,
        extra_target_titles,
        match_duplicate_window_titles,
        prefer_other_if_foreground_matches,
    )
    .context("Target window was not found")?;
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground == hwnd && !IsIconic(hwnd).as_bool() {
            return Ok(());
        }
        let current_thread = GetCurrentThreadId();
        let target_thread = GetWindowThreadProcessId(hwnd, None);
        let foreground_thread = if foreground.0.is_null() {
            0
        } else {
            GetWindowThreadProcessId(foreground, None)
        };

        let attach_foreground = foreground_thread != 0 && foreground_thread != current_thread;
        let attach_target = target_thread != 0 && target_thread != current_thread;

        if attach_foreground {
            let _ = AttachThreadInput(foreground_thread, current_thread, true);
        }
        if attach_target {
            let _ = AttachThreadInput(target_thread, current_thread, true);
        }

        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = BringWindowToTop(hwnd);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
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
        let _ = SetForegroundWindow(hwnd);
        let _ = SetActiveWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        thread::sleep(Duration::from_millis(18));
        replay_held_inputs_after_focus();

        if attach_target {
            let _ = AttachThreadInput(target_thread, current_thread, false);
        }
        if attach_foreground {
            let _ = AttachThreadInput(foreground_thread, current_thread, false);
        }
    }
    Ok(())
}

pub(super) fn apply_window_preset_for_macro(preset: &WindowPreset) -> Result<()> {
    apply_window_preset_impl(preset, false)
}

fn apply_window_preset_impl(preset: &WindowPreset, require_enabled: bool) -> Result<()> {
    if require_enabled && !preset.enabled {
        return Ok(());
    }
    if preset.animate_enabled {
        return apply_window_preset_animated(preset);
    }
    unsafe {
        let target = resolve_window_target(
            preset.target_window_title.as_deref(),
            &preset.extra_target_window_titles,
            preset.match_duplicate_window_titles,
            false,
        );
        if target.0.is_null() {
            bail!("No foreground window is available");
        }
        let target_root = GetAncestor(target, GA_ROOT);
        if !target_root.0.is_null()
            && window_belongs_to_current_process(target_root)
            && !is_internal_app_window(target_root)
        {
            return Ok(());
        }

        let _ = ShowWindow(target, SW_RESTORE);
        if preset.remove_title_bar {
            let _ = remove_window_title_bar(target);
        } else {
            let _ = restore_window_title_bar(target);
        }
        let bounds = calculate_window_bounds(target, preset)?;
        let _ = SetWindowPos(
            target,
            None,
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
    Ok(())
}

pub(super) fn apply_window_layout(layout: &crate::model::WindowLayout) -> Result<()> {
    use windows::Win32::Foundation::{HWND, LPARAM, POINT};
    use windows::Win32::Graphics::Gdi::MonitorFromWindow;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, IsWindowVisible};

    if !layout.enabled {
        return Ok(());
    }
    if layout.rows == 0 || layout.cols == 0 {
        bail!("Layout has no rows/cols");
    }

    let rows = layout.rows.max(1);
    let cols = layout.cols.max(1);

    let monitor = unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTONEAREST)
        } else {
            MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)
        }
    };

    let work_rect = unsafe {
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut mi).as_bool() {
            mi.rcWork
        } else {
            RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
        }
    };

    let total_w = (work_rect.right - work_rect.left) as f32;
    let total_h = (work_rect.bottom - work_rect.top) as f32;

    let row_ratios: Vec<f32> = {
        let prov: Vec<f32> = layout.row_ratios.iter().take(rows).copied().map(|v| v.max(0.01)).collect();
        if prov.len() == rows {
            let sum: f32 = prov.iter().sum();
            prov.iter().map(|v| v / sum).collect()
        } else {
            vec![1.0 / rows as f32; rows]
        }
    };
    let col_ratios: Vec<f32> = {
        let prov: Vec<f32> = layout.col_ratios.iter().take(cols).copied().map(|v| v.max(0.01)).collect();
        if prov.len() == cols {
            let sum: f32 = prov.iter().sum();
            prov.iter().map(|v| v / sum).collect()
        } else {
            vec![1.0 / cols as f32; cols]
        }
    };

    let mut row_starts: Vec<i32> = vec![0];
    {
        let mut acc = 0.0f32;
        for r in &row_ratios {
            acc += r * total_h;
            row_starts.push(acc.round() as i32);
        }
    }
    let mut col_starts: Vec<i32> = vec![0];
    {
        let mut acc = 0.0f32;
        for c in &col_ratios {
            acc += c * total_w;
            col_starts.push(acc.round() as i32);
        }
    }

    let mut used_hwnds: HashSet<isize> = HashSet::new();
    let mut focus_targets: Vec<HWND> = Vec::new();

    for cell in &layout.cells {
        if cell.row >= rows || cell.col >= cols {
            continue;
        }

        let titles: Vec<&str> = std::iter::once(cell.target_window_title.as_deref())
            .chain(cell.extra_target_window_titles.iter().map(|s| Some(s.as_str())))
            .flatten()
            .collect();

        struct FindPayload<'a> {
            title: &'a str,
            match_dup: bool,
            used: &'a HashSet<isize>,
            result: Option<HWND>,
        }
        unsafe extern "system" fn find_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
            let p = &mut *(lparam.0 as *mut FindPayload<'_>);
            if !IsWindowVisible(hwnd).as_bool() {
                return true.into();
            }
            if p.used.contains(&(hwnd.0 as isize)) {
                return true.into();
            }
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut buf);
            if len == 0 {
                return true.into();
            }
            let win_title = String::from_utf16_lossy(&buf[..len as usize]);
            let matches = if p.match_dup {
                win_title.to_lowercase().contains(&p.title.to_lowercase())
            } else {
                win_title.trim().eq_ignore_ascii_case(p.title.trim())
            };
            if matches {
                p.result = Some(hwnd);
                return false.into();
            }
            true.into()
        }

        let hwnd: Option<HWND> = unsafe {
            let mut found = None;
            for title in &titles {
                let mut payload = FindPayload {
                    title,
                    match_dup: cell.match_duplicate_window_titles,
                    used: &used_hwnds,
                    result: None,
                };
                let _ = EnumWindows(Some(find_proc), LPARAM((&mut payload) as *mut _ as isize));
                if payload.result.is_some() {
                    found = payload.result;
                    break;
                }
            }
            if found.is_none() && titles.is_empty() {
                let fg = GetForegroundWindow();
                if !fg.0.is_null() && !used_hwnds.contains(&(fg.0 as isize)) {
                    found = Some(fg);
                }
            }
            found
        };

        let hwnd = match hwnd {
            Some(h) => h,
            None => continue,
        };

        used_hwnds.insert(hwnd.0 as isize);

        let row_span = cell.row_span.max(1);
        let col_span = cell.col_span.max(1);
        let end_row = (cell.row + row_span).min(rows);
        let end_col = (cell.col + col_span).min(cols);

        let cell_x = work_rect.left + col_starts[cell.col];
        let cell_y = work_rect.top + row_starts[cell.row];
        let cell_w = col_starts[end_col] - col_starts[cell.col];
        let cell_h = row_starts[end_row] - row_starts[cell.row];

        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let mut wr = RECT::default();
            let _ = GetWindowRect(hwnd, &mut wr);
            let mut fr = RECT::default();
            let frame_ok = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut fr as *mut _ as *mut c_void,
                size_of::<RECT>() as u32,
            ).is_ok();
            let (li, ti, ri, bi) = if frame_ok {
                (fr.left - wr.left, fr.top - wr.top, wr.right - fr.right, wr.bottom - fr.bottom)
            } else {
                (0, 0, 0, 0)
            };
            let _ = SetWindowPos(
                hwnd, None,
                cell_x - li, cell_y - ti,
                cell_w + li + ri, cell_h + ti + bi,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }

        if layout.focus_on_apply {
            focus_targets.push(hwnd);
        }
    }

    if layout.focus_on_apply {
        for hwnd in focus_targets {
            unsafe {
                let fg = GetForegroundWindow();
                let cur_tid = GetCurrentThreadId();
                let tgt_tid = GetWindowThreadProcessId(hwnd, None);
                let fg_tid = if fg.0.is_null() { 0 } else { GetWindowThreadProcessId(fg, None) };
                if fg_tid != 0 && fg_tid != cur_tid {
                    let _ = AttachThreadInput(fg_tid, cur_tid, true);
                }
                if tgt_tid != 0 && tgt_tid != cur_tid {
                    let _ = AttachThreadInput(tgt_tid, cur_tid, true);
                }
                let _ = BringWindowToTop(hwnd);
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
                let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
                let _ = SetForegroundWindow(hwnd);
                let _ = SetActiveWindow(hwnd);
                let _ = SetFocus(Some(hwnd));
                thread::sleep(Duration::from_millis(15));
                if fg_tid != 0 && fg_tid != cur_tid {
                    let _ = AttachThreadInput(fg_tid, cur_tid, false);
                }
                if tgt_tid != 0 && tgt_tid != cur_tid {
                    let _ = AttachThreadInput(tgt_tid, cur_tid, false);
                }
            }
        }
    }

    Ok(())
}

