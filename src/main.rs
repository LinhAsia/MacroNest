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
use std::sync::{Arc, Mutex};

use crate::{
    model::AppState,
    overlay::OverlayCommand,
    storage::AppPaths,
    ui::{CrosshairApp, PopupBlobApp, PopupBlobKind},
};

#[cfg(not(windows))]
compile_error!("This application currently supports Windows only.");

fn load_startup_state(paths: &AppPaths) -> Result<(AppState, bool)> {
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
        preset
            .objects
            .retain(|obj| obj.name != "Point 1" && obj.name != "Point 2" && obj.name != "Point 3");
        if preset.objects.len() != old_len {
            state_changed = true;
        }
    }
    state.show_window = true;
    Ok((state, state_changed))
}

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
    let mut state = AppState::default();
    state.show_window = true;
    let (ui_tx, ui_rx) = unbounded();
    {
        let loader_paths = paths.clone();
        let loader_ui_tx = ui_tx.clone();
        std::thread::spawn(move || match load_startup_state(&loader_paths) {
            Ok((loaded_state, startup_state_dirty)) => {
                let _ = loader_ui_tx.send(crate::overlay::UiCommand::StartupStateLoaded {
                    state: loaded_state,
                    startup_state_dirty,
                });
            }
            Err(error) => {
                let _ = loader_ui_tx.send(crate::overlay::UiCommand::StartupStateLoadFailed(
                    error.to_string(),
                ));
            }
        });
    }
    let overlay_handle_slot: Arc<Mutex<Option<overlay::OverlayHandle>>> = Arc::new(Mutex::new(None));
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
                    *overlay_handle_slot.lock().expect("overlay handle slot poisoned") = Some(handle);
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

                let handle_guard = overlay_handle_slot.lock().expect("overlay handle slot poisoned");
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
            ui::configure_fonts(&cc.egui_ctx, false);
            ui::configure_theme(&cc.egui_ctx, state.ui_theme);
            Ok(Box::new(CrosshairApp::new(
                paths, state, overlay_tx, ui_tx, ui_rx, false,
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
