use std::{thread, time::Duration};

use anyhow::{Context, Result, bail};
use windows::Win32::{
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus},
    UI::WindowsAndMessaging::{
        BringWindowToTop, GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowRect,
        GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, SW_RESTORE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
        SetForegroundWindow, SetWindowPos, ShowWindow,
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
