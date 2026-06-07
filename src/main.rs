#![windows_subsystem = "windows"]

mod ai;
mod audiosense;
mod app_icon;
mod audio;
mod hotkey;
mod lang;
mod macro_code;
mod media;
mod model;
mod ocr;
mod overlay;
mod platform;
mod profile_code;
mod render;
mod storage;
mod ui;
mod window_list;

use anyhow::Result;
use crossbeam_channel::unbounded;

use crate::{
    overlay::OverlayCommand,
    storage::AppPaths,
    ui::{CrosshairApp, PopupBlobApp, PopupBlobKind},
};

#[cfg(not(windows))]
compile_error!("This application currently supports Windows only.");

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--already-running-popup") {
        return run_popup_blob(PopupBlobKind::AlreadyRunning);
    }

    let skip_admin = args.iter().any(|arg| arg == "--no-admin");
    if !skip_admin && platform::relaunch_as_admin_if_needed()? {
        return Ok(());
    }

    let _single_instance = match platform::acquire_single_instance()? {
        Some(guard) => guard,
        None => return Ok(()),
    };
    platform::set_high_priority();

    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }

    let paths = AppPaths::discover()?;

    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::LibraryLoader::SetDllDirectoryW;
        use windows::core::HSTRING;
        let _ = SetDllDirectoryW(&HSTRING::from(paths.bin_dir.as_os_str()));
    }

    app_icon::ensure_ico_file(&paths.icon_file, 64)?;
    app_icon::ensure_disabled_ico_file(&paths.icon_file_disabled, 64)?;
    let (mut state, _) = paths.load_state()?;
    let mut state_changed = false;
    for preset in &mut state.vision_presets {
        if preset.is_pixel_counter && !preset.use_color_matching {
            preset.use_color_matching = true;
            state_changed = true;
        }
    }
    for preset in &mut state.geometry_presets {
        let old_len = preset.objects.len();
        preset.objects.retain(|obj| obj.name != "Point 1" && obj.name != "Point 2" && obj.name != "Point 3");
        if preset.objects.len() != old_len {
            state_changed = true;
        }
    }
    if state_changed {
        let _ = paths.save_state(&state);
    }
    state.show_window = true;
    let (ui_tx, ui_rx) = unbounded();
    let overlay = overlay::start(paths.clone(), state.active_style.clone(), ui_tx.clone())?;
    overlay.send(OverlayCommand::Update(state.active_style.clone()));
    overlay.send(OverlayCommand::UpdateProfiles(state.profiles.clone()));
    overlay.send(OverlayCommand::UpdateWindowPresets(
        state.window_presets.clone(),
    ));
    overlay.send(OverlayCommand::UpdateWindowFocusPresets(
        state.window_focus_presets.clone(),
    ));
    overlay.send(OverlayCommand::UpdateMouseSensitivityPresets(
        state.mouse_sensitivity_presets.clone(),
    ));
    overlay.send(OverlayCommand::UpdateMouseSensitivitySettings {
        restore_on_exit: state.mouse_sensitivity_restore_on_exit,
        restore_speed: state.mouse_sensitivity_restore_speed,
    });
    overlay.send(OverlayCommand::UpdateKeyboardArrowMouseSettings {
        enabled: state.keyboard_arrow_mouse_enabled,
        step_px: state.keyboard_arrow_mouse_step_px,
    });
    overlay.send(OverlayCommand::UpdateVisionPresets(
        state.vision_presets.clone(),
    ));
    overlay.send(OverlayCommand::UpdateAudioSensePresets(
        state.audio_sense_presets.clone(),
    ));
    overlay.send(OverlayCommand::UpdateGeometryPresets(
        state.geometry_presets.clone(),
    ));
    let macro_groups = ui::build_runtime_macro_groups(&state);
    overlay.send(OverlayCommand::UpdateMacroPresets(macro_groups));
    overlay.send(OverlayCommand::UpdateAudioSettings(
        state.audio_settings.clone(),
    ));

    let (overlay_tx, overlay_rx) = unbounded::<OverlayCommand>();
    std::thread::spawn(move || {
        while let Ok(command) = overlay_rx.recv() {
            overlay.send(command.clone());
            if matches!(command, OverlayCommand::Exit) {
                break;
            }
        }
    });

    let app_title = "MacroNest v1.0";
    let mut viewport_builder = eframe::egui::ViewportBuilder::default()
        .with_title(app_title)
        .with_inner_size([1180.0, 780.0])
        .with_min_inner_size([1180.0, 780.0])
        .with_visible(false)
        .with_decorations(false)
        .with_transparent(true)
        .with_icon(std::sync::Arc::new(app_icon::icon_data(128)?));

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
        app_title,
        native_options,
        Box::new(move |cc| {
            let load_cjk_fallback = ui::app_state_needs_cjk_fallback(&state);
            ui::configure_fonts(&cc.egui_ctx, load_cjk_fallback);
            ui::configure_theme(&cc.egui_ctx, state.ui_theme);
            Ok(Box::new(CrosshairApp::new(
                paths, state, overlay_tx, ui_tx, ui_rx,
            )))
        }),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    Ok(())
}

fn run_popup_blob(kind: PopupBlobKind) -> Result<()> {
    let app_title = "MacroNest v1.0";
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(app_title)
            .with_inner_size([560.0, 260.0])
            .with_min_inner_size([560.0, 260.0])
            .with_max_inner_size([560.0, 260.0])
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        app_title,
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
