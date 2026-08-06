#![windows_subsystem = "windows"]

mod ai;
mod app_icon;
mod audio;
mod audiosense;
mod frida_injector;
mod hotkey;
mod lang;
mod macro_code;
#[cfg(windows)]
mod memory_debugger;
mod model;
mod ocr;
mod overlay;
mod platform;
mod process_memory;
mod protractor;
mod render;
mod storage;
mod ui;
mod video_recorder;
mod window_list;

use anyhow::Result;
use crossbeam_channel::unbounded;
use std::sync::{Arc, Condvar, Mutex};

use crate::{
    model::{
        AppState, FocusHighlightDecoration, GeometryObject, GeometryPreset, GeometryShapeKind,
    },
    overlay::OverlayCommand,
    storage::AppPaths,
    ui::{CrosshairApp, PopupBlobApp, PopupBlobKind},
};

#[cfg(windows)]
unsafe extern "system" fn release_mouse_on_unhandled_exception(
    _: *const windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    // ponytail: this deliberately avoids application locks; an exception handler must be fail-open.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::ClipCursor(None);
    }
    windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_CONTINUE_SEARCH
}

#[cfg(windows)]
fn install_crash_input_release() {
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter(Some(
            release_mouse_on_unhandled_exception,
        ));
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::ClipCursor(None);
        }
        previous(info);
    }));
}

#[cfg(not(windows))]
compile_error!("This application currently supports Windows only.");

fn cleanup_post_update_artifacts() {
    if let Ok(current_exe) = std::env::current_exe() {
        let old_exe = current_exe.with_extension("exe.old");
        let _ = std::fs::remove_file(old_exe);
    }

    let temp_dir = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let should_remove = file_name == "macronest_update_error.txt"
                || (file_name.starts_with("macronest_update") && file_name.ends_with(".part"));
            if should_remove {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn load_startup_state(paths: &AppPaths) -> Result<(AppState, bool, bool)> {
    let startup_state_needs_cjk_fallback = std::fs::read_to_string(&paths.state_file)
        .map(|json| ui::text_has_cjk(&json))
        .unwrap_or(false);
    let (mut state, _) = paths.load_state()?;
    let mut state_changed = false;
    for preset in &mut state.vision_presets {
        if preset.is_pixel_counter && !preset.use_color_matching {
            preset.use_color_matching = true;
            state_changed = true;
        }
    }
    if normalize_geometry_presets(&mut state) {
        state_changed = true;
    }
    if normalize_focus_highlight_decoration(&mut state) {
        state_changed = true;
    }
    if normalize_legacy_active_window_targets(&mut state) {
        state_changed = true;
    }
    // The old UI could not express a path limit above 4096. Preserve the
    // user's intent to use that former maximum when upgrading the scanner.
    if state.memory_pointer_scan_result_limit == 4096 {
        state.memory_pointer_scan_result_limit =
            process_memory::PointerScanLimits::DEEP.result_limit;
        state_changed = true;
    }
    if state.reset_session_preset_visibility() {
        state_changed = true;
    }
    state.show_window = true;
    Ok((state, state_changed, startup_state_needs_cjk_fallback))
}

fn make_default_geometry_object(preset_id: u32) -> GeometryObject {
    GeometryObject::new(preset_id, GeometryShapeKind::Point)
}

fn normalize_geometry_presets(state: &mut AppState) -> bool {
    let mut changed = false;
    let original_presets = std::mem::take(&mut state.geometry_presets);
    let mut normalized_presets = Vec::with_capacity(original_presets.len());
    let mut next_preset_id = original_presets
        .iter()
        .map(|preset| preset.id)
        .max()
        .unwrap_or(0)
        .max(state.next_geometry_preset_id.saturating_sub(1))
        + 1;

    for preset in original_presets {
        let base_name = preset.name.clone();
        let mut objects: Vec<GeometryObject> = preset.objects;

        if objects.is_empty() {
            changed = true;
            normalized_presets.push(GeometryPreset {
                id: preset.id,
                name: base_name,
                enabled: preset.enabled,
                collapsed: preset.collapsed,
                objects: vec![make_default_geometry_object(preset.id)],
            });
            continue;
        }

        if objects.len() > 1 {
            changed = true;
        }

        for (index, mut object) in objects.drain(..).enumerate() {
            let preset_id = if index == 0 {
                preset.id
            } else {
                let id = next_preset_id;
                next_preset_id += 1;
                id
            };
            if object.id != preset_id {
                object.id = preset_id;
                changed = true;
            }
            let preset_name = if index == 0 {
                base_name.clone()
            } else {
                format!("{base_name} {}", index + 1)
            };
            normalized_presets.push(GeometryPreset {
                id: preset_id,
                name: preset_name,
                enabled: preset.enabled,
                collapsed: preset.collapsed,
                objects: vec![object],
            });
        }
    }

    let desired_next_id = normalized_presets
        .iter()
        .map(|preset| preset.id)
        .max()
        .unwrap_or(0)
        + 1;
    if state.next_geometry_preset_id != desired_next_id {
        state.next_geometry_preset_id = desired_next_id;
        changed = true;
    }
    state.geometry_presets = normalized_presets;
    changed
}

fn normalize_focus_highlight_decoration(state: &mut AppState) -> bool {
    let mut changed = false;
    if state.focus_highlight_rainbow_legacy
        && state.focus_highlight_decoration == FocusHighlightDecoration::Plain
    {
        state.focus_highlight_decoration = FocusHighlightDecoration::Rainbow;
        changed = true;
    }
    if state.focus_highlight_rainbow_legacy {
        state.focus_highlight_rainbow_legacy = false;
        changed = true;
    }
    changed
}

fn normalize_legacy_active_window_targets(state: &mut AppState) -> bool {
    fn normalize_target(target: &mut Option<String>, changed: &mut bool) {
        if target.as_deref() == Some("[Active Window]") {
            *target = None;
            *changed = true;
        }
    }

    fn normalize_extra_targets(extra_targets: &mut Vec<String>, changed: &mut bool) {
        let original_len = extra_targets.len();
        extra_targets.retain(|title| title != "[Active Window]");
        if extra_targets.len() != original_len {
            *changed = true;
        }
    }

    fn normalize_target_set(
        target: &mut Option<String>,
        extra_targets: &mut Vec<String>,
        match_duplicate_window_titles: &mut bool,
        changed: &mut bool,
    ) {
        let had_active_window = target.as_deref() == Some("[Active Window]");
        normalize_target(target, changed);
        normalize_extra_targets(extra_targets, changed);
        if had_active_window && *match_duplicate_window_titles {
            *match_duplicate_window_titles = false;
            *changed = true;
        }
    }

    let mut changed = false;

    for profile in &mut state.profiles {
        normalize_target(&mut profile.target_window_title, &mut changed);
        normalize_extra_targets(&mut profile.extra_target_window_titles, &mut changed);
    }

    for preset in &mut state.window_presets {
        normalize_target_set(
            &mut preset.target_window_title,
            &mut preset.extra_target_window_titles,
            &mut preset.match_duplicate_window_titles,
            &mut changed,
        );
    }

    for preset in &mut state.window_focus_presets {
        normalize_target_set(
            &mut preset.target_window_title,
            &mut preset.extra_target_window_titles,
            &mut preset.match_duplicate_window_titles,
            &mut changed,
        );
    }

    for layout in &mut state.window_layouts {
        for cell in &mut layout.cells {
            normalize_target_set(
                &mut cell.target_window_title,
                &mut cell.extra_target_window_titles,
                &mut cell.match_duplicate_window_titles,
                &mut changed,
            );
        }
    }

    for preset in &mut state.pin_presets {
        normalize_target_set(
            &mut preset.target_window_title,
            &mut preset.extra_target_window_titles,
            &mut preset.match_duplicate_window_titles,
            &mut changed,
        );
    }

    for preset in &mut state.zoom_presets {
        normalize_target(&mut preset.target_window_title, &mut changed);
        normalize_extra_targets(&mut preset.extra_target_window_titles, &mut changed);
    }

    for preset in &mut state.mouse_sensitivity_presets {
        normalize_target_set(
            &mut preset.target_window_title,
            &mut preset.extra_target_window_titles,
            &mut preset.match_duplicate_window_titles,
            &mut changed,
        );
    }

    for preset in &mut state.command_presets {
        normalize_target_set(
            &mut preset.target_window_title,
            &mut preset.extra_target_window_titles,
            &mut preset.match_duplicate_window_titles,
            &mut changed,
        );
    }

    for group in &mut state.macro_groups {
        normalize_target_set(
            &mut group.target_window_title,
            &mut group.extra_target_window_titles,
            &mut group.match_duplicate_window_titles,
            &mut changed,
        );
        for preset in &mut group.presets {
            normalize_target_set(
                &mut preset.event_target_window_title,
                &mut preset.event_extra_target_window_titles,
                &mut preset.event_match_duplicate_window_titles,
                &mut changed,
            );
        }
    }

    for preset in &mut state.vision_presets {
        normalize_target_set(
            &mut preset.target_window_title,
            &mut preset.extra_target_window_titles,
            &mut preset.match_duplicate_window_titles,
            &mut changed,
        );
    }

    changed
}

fn wait_for_startup_gate(startup_gate: &Arc<(Mutex<bool>, Condvar)>) {
    let (gate_lock, gate_ready) = &**startup_gate;
    let mut gate_open = gate_lock.lock().expect("startup gate poisoned");
    while !*gate_open {
        gate_open = gate_ready
            .wait(gate_open)
            .expect("startup gate wait poisoned");
    }
}

fn apply_process_startup_tuning(paths: &AppPaths) {
    platform::set_high_priority();

    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::LibraryLoader::SetDllDirectoryW;
        use windows::core::HSTRING;
        let _ = SetDllDirectoryW(&HSTRING::from(paths.bin_dir.as_os_str()));
    }
}

fn main() -> Result<()> {
    // Intel's legacy OpenCL ICD can access-violate inside OpenCV. MacroNest's image matching is
    // latency-safe on CPU and must not take the whole app (and an active ClipCursor) down with it.
    unsafe {
        std::env::set_var("OPENCV_OPENCL_RUNTIME", "disabled");
    }
    install_crash_input_release();
    let start_hidden_to_tray = std::env::args_os().any(|arg| arg == "--start-in-tray");
    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--already-running-popup") {
        return run_popup_blob(PopupBlobKind::AlreadyRunning);
    }

    platform::set_high_priority();

    let skip_admin = args.iter().any(|arg| arg == "--no-admin");
    if !skip_admin && platform::relaunch_as_admin_if_needed()? {
        return Ok(());
    }

    let _single_instance = match platform::acquire_single_instance()? {
        Some(guard) => guard,
        None => return Ok(()),
    };

    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }

    let paths = AppPaths::discover()?;
    cleanup_post_update_artifacts();
    apply_process_startup_tuning(&paths);

    {
        let background_paths = paths.clone();
        std::thread::spawn(move || {
            let _ = background_paths.ensure_dirs_and_assets();
            let _ = app_icon::ensure_ico_file(&background_paths.icon_file, 64);
            let _ = app_icon::ensure_disabled_ico_file(&background_paths.icon_file_disabled, 64);
        });
    }

    let state = AppState::default();

    let (ui_tx, ui_rx) = unbounded();
    {
        let startup_paths = paths.clone();
        let startup_ui_tx = ui_tx.clone();
        std::thread::spawn(move || match load_startup_state(&startup_paths) {
            Ok((state, startup_state_dirty, startup_state_needs_cjk_fallback)) => {
                let _ = startup_ui_tx.send(crate::overlay::UiCommand::StartupStateLoaded {
                    state,
                    startup_state_dirty,
                    startup_state_needs_cjk_fallback,
                });
            }
            Err(error) => {
                let _ = startup_ui_tx.send(crate::overlay::UiCommand::StartupStateLoadFailed(
                    error.to_string(),
                ));
            }
        });
    }
    let startup_gate: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
    // No separate icon background thread needed — the icon is either loaded fast from .ico
    // file below (for the viewport), or the background asset thread will generate it for next run.
    let overlay_handle_slot: Arc<Mutex<Option<overlay::OverlayHandle>>> =
        Arc::new(Mutex::new(None));
    let overlay_start_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    {
        let overlay_handle_slot = Arc::clone(&overlay_handle_slot);
        let overlay_start_error = Arc::clone(&overlay_start_error);
        let overlay_paths = paths.clone();
        let overlay_initial_style = state.active_style.clone();
        let overlay_ui_tx = ui_tx.clone();
        std::thread::spawn(move || {
            match overlay::start(overlay_paths, overlay_initial_style, overlay_ui_tx) {
                Ok(handle) => {
                    *overlay_handle_slot
                        .lock()
                        .expect("overlay handle slot poisoned") = Some(handle);
                }
                Err(error) => {
                    *overlay_start_error
                        .lock()
                        .expect("overlay start error slot poisoned") = Some(error.to_string());
                }
            }
        });
    }

    let (overlay_tx, overlay_rx) = unbounded::<OverlayCommand>();
    {
        let overlay_handle_slot = Arc::clone(&overlay_handle_slot);
        let overlay_start_error = Arc::clone(&overlay_start_error);
        std::thread::spawn(move || {
            let mut pending_commands: Vec<OverlayCommand> = Vec::new();
            loop {
                match overlay_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                    Ok(command) => pending_commands.push(command),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                }

                let handle_guard = overlay_handle_slot
                    .lock()
                    .expect("overlay handle slot poisoned");
                if let Some(handle) = handle_guard.as_ref() {
                    for command in pending_commands.drain(..) {
                        let should_exit = matches!(command, OverlayCommand::Exit);
                        handle.send(command);
                        if should_exit {
                            return;
                        }
                    }
                    continue;
                }

                drop(handle_guard);
                if overlay_start_error
                    .lock()
                    .expect("overlay start error slot poisoned")
                    .is_some()
                {
                    return;
                }
            }
        });
    }

    let app_title = format!(
        "MacroNest v{}",
        option_env!("MACRONEST_BUILD_TAG").unwrap_or(env!("CARGO_PKG_VERSION"))
    );
    // Try to load the viewport icon from the pre-generated .ico file (fast: just file I/O).
    // This ensures Windows shows the correct icon in the taskbar and Alt+Tab from the start.
    // If the .ico doesn't exist yet (first run), the background asset thread will create it
    // and it will be available on the next launch.
    let mut viewport_builder = eframe::egui::ViewportBuilder::default()
        .with_title(&app_title)
        .with_inner_size([1180.0, 780.0])
        .with_min_inner_size([1180.0, 780.0])
        .with_visible(false)
        .with_decorations(false)
        .with_transparent(true);
    if let Ok(icon) = app_icon::icon_data_from_ico_file(&paths.icon_file) {
        viewport_builder = viewport_builder.with_icon(std::sync::Arc::new(icon));
    }

    #[cfg(windows)]
    {
        unsafe {
            use windows::Win32::UI::HiDpi::GetDpiForSystem;
            use windows::Win32::UI::WindowsAndMessaging::{
                GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
            };
            let scr_w = GetSystemMetrics(SM_CXSCREEN) as f32;
            let scr_h = GetSystemMetrics(SM_CYSCREEN) as f32;
            let dpi = GetDpiForSystem() as f32;
            let scale = if dpi > 0.0 { dpi / 96.0 } else { 1.0 };
            let win_w = 1180.0;
            let win_h = 780.0;
            let x = ((scr_w / scale) - win_w) / 2.0;
            let y = (((scr_h / scale) - win_h) / 2.0).max(10.0);
            viewport_builder = viewport_builder.with_position([x.max(0.0), y]);
        }
    }

    let native_options = eframe::NativeOptions {
        viewport: viewport_builder,
        ..Default::default()
    };

    eframe::run_native(
        &app_title,
        native_options,
        Box::new(move |cc| {
            ui::configure_fonts(&cc.egui_ctx, false);
            ui::configure_theme(&cc.egui_ctx, state.ui_theme);
            Ok(Box::new(CrosshairApp::new(
                paths,
                state,
                overlay_tx,
                ui_tx,
                ui_rx,
                false,
                startup_gate,
                start_hidden_to_tray,
            )))
        }),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    Ok(())
}

fn run_popup_blob(kind: PopupBlobKind) -> Result<()> {
    let app_title = format!(
        "MacroNest v{}",
        option_env!("MACRONEST_BUILD_TAG").unwrap_or(env!("CARGO_PKG_VERSION"))
    );
    let app_icon = app_icon::icon_data(128).ok().map(Arc::new);
    let native_options = eframe::NativeOptions {
        viewport: {
            let mut viewport = eframe::egui::ViewportBuilder::default()
                .with_title(&app_title)
                .with_inner_size([560.0, 260.0])
                .with_min_inner_size([560.0, 260.0])
                .with_max_inner_size([560.0, 260.0])
                .with_resizable(false)
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_active(true);
            if let Some(icon) = app_icon {
                viewport = viewport.with_icon(icon);
            }
            viewport
        },
        ..Default::default()
    };

    eframe::run_native(
        &app_title,
        native_options,
        Box::new(move |cc| {
            ui::configure_fonts(&cc.egui_ctx, false);
            ui::configure_theme(&cc.egui_ctx, crate::model::UiThemeMode::Dark);
            Ok(Box::new(PopupBlobApp::new(
                kind,
                crate::model::UiThemeMode::Dark,
            )))
        }),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    Ok(())
}
