use eframe::egui;
use std::time::Instant;

use crate::{
    audiosense,
    model::{AppState, AudioSensePresetKind, ProfileRecord, TimerPreset},
    overlay::{OverlayCommand, UiCommand},
    window_list,
};

use super::{CrosshairApp, build_runtime_macro_groups, configure_theme};

impl CrosshairApp {
    pub(crate) fn sync_crosshair(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateProfiles(self.state.profiles.clone()));
    }

    pub(crate) fn run_all_startup_overlay_sync(&mut self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::Update(self.state.active_style.clone()));
        self.sync_profiles();
        self.sync_window_presets();
        self.sync_window_layouts();
        self.sync_mouse_sensitivity_presets();
        self.sync_mouse_sensitivity_settings();
        self.sync_mouse_driver_settings();
        self.sync_keyboard_arrow_mouse_settings();
        self.sync_macro_delay_settings();
        self.sync_macro_presets();
        self.sync_active_macro_folder_scope();
        self.sync_audio_settings();
        self.sync_groq_settings();
        self.sync_vision_presets();
        self.sync_ocr_presets();
        self.sync_vision_settings();
        self.sync_hud_presets();
        self.sync_timer_presets();
        self.sync_command_presets();
        self.sync_audio_sense_presets();
        self.sync_geometry_presets();
        self.sync_macro_master_enabled();
        self.sync_windows_key_locked();
        self.sync_native_focus_highlight_enabled();
        self.sync_focus_highlight_config();
        self.sync_protractor_state();
        self.sync_quick_key_display_config();
        self.sync_quick_screen_draw_config();
        self.sync_quick_key_sound_config();
        self.sync_vietnamese_input_enabled();
        self.sync_macro_master_hotkey();
        self.startup_overlay_sync_pending = false;
    }

    pub(crate) fn sync_macro_delay_settings(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateMacroDelays {
            mouse_click_delay_ms: self.state.macro_mouse_click_delay_ms,
            keyboard_key_press_delay_ms: self.state.macro_keyboard_key_press_delay_ms,
        });
    }

    pub(crate) fn sync_profiles(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateProfiles(self.state.profiles.clone()));
    }

    pub(crate) fn sync_crosshair_profile(&self, index: usize, profile: &ProfileRecord) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateCrosshairProfile {
                index,
                profile: profile.clone(),
            });
    }

    pub(crate) fn sync_macro_presets(&self) {
        let macro_groups = build_runtime_macro_groups(&self.state);
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateMacroPresets(macro_groups));
    }

    pub(crate) fn resolved_active_macro_folder_view(&self) -> Option<u32> {
        if !self.macro_folders_panel_open {
            return None;
        }
        self.active_macro_folder_view.filter(|folder_id| {
            self.state
                .macro_folders
                .iter()
                .any(|folder| folder.id == *folder_id)
        })
    }

    pub(crate) fn active_macro_folder_name(&self) -> Option<String> {
        self.resolved_active_macro_folder_view()
            .and_then(|folder_id| {
                self.state
                    .macro_folders
                    .iter()
                    .find(|folder| folder.id == folder_id)
                    .map(|folder| folder.name.clone())
            })
    }

    pub(crate) fn sync_active_macro_folder_scope(&self) {
        let active_folder_scope = self.resolved_active_macro_folder_view();
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetActiveMacroFolderScope(
                active_folder_scope,
            ));
    }

    pub(crate) fn sync_macro_master_enabled(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::SetMacrosMasterEnabled(
            self.state.macros_master_enabled,
        ));
    }

    pub(crate) fn sync_windows_key_locked(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::SetWindowsKeyLocked(
            self.state.windows_key_locked,
        ));
    }

    pub(crate) fn sync_native_focus_highlight_enabled(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetNativeFocusHighlightEnabled(
                self.state.native_focus_highlight_enabled,
            ));
    }

    pub(crate) fn sync_focus_highlight_config(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetFocusHighlightConfig {
                color: self.state.focus_highlight_color,
                decoration: self.state.focus_highlight_decoration,
            });
    }

    pub(crate) fn sync_protractor_state(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::SetProtractorEnabled(
            self.state.protractor_enabled,
        ));
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateProtractorConfig {
                scale: self.state.protractor_scale,
                needle1_angle: self.state.protractor_needle1_angle,
                needle2_angle: self.state.protractor_needle2_angle,
                center_x: self.state.protractor_center_x,
                center_y: self.state.protractor_center_y,
                thickness: self.state.protractor_thickness,
                calibrating: self.protractor_picking_active,
                ui_language: self.state.ui_language,
            });
    }

    pub(crate) fn sync_quick_key_display_config(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateQuickKeyDisplayConfig {
                enabled: self.state.quick_key_display_enabled,
                center_x: self.state.quick_key_display_x,
                center_y: self.state.quick_key_display_y,
                size: self.state.quick_key_display_size,
                mode: self.state.quick_key_display_mode,
                mascot_style: self.state.quick_key_display_mascot_style,
            });
    }

    pub(crate) fn sync_quick_screen_draw_config(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateScreenDrawConfig {
                enabled: self.state.quick_screen_draw_enabled,
                trigger: self.state.quick_screen_draw_hotkey.clone(),
                pass_trigger_through: self.state.quick_screen_draw_pass_trigger_through,
                color: self.state.quick_screen_draw_color,
                brush_size: self.state.quick_screen_draw_brush_size,
                smoothing: self.state.quick_screen_draw_smoothing,
                smoothing_amount: self.state.quick_screen_draw_smoothing_amount,
            });
    }

    pub(crate) fn sync_quick_key_sound_config(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateKeySoundConfig {
            enabled: self.state.quick_key_sound_enabled,
            style: self.state.quick_key_sound_style,
            volume: self.state.quick_key_sound_volume,
        });
    }

    pub(crate) fn sync_vietnamese_input_enabled(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetVietnameseInputEnabled(
                self.state.vietnamese_input_enabled,
            ));
    }

    pub(crate) fn sync_macro_master_hotkey(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateMacrosMasterHotkey(
                self.state.macros_master_hotkey.clone(),
            ));
    }

    pub(crate) fn sync_audio_settings(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateAudioSettings(
            self.state.audio_settings.clone(),
        ));
    }

    pub(crate) fn sync_groq_settings(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateGroqSettings(
            self.state.groq_settings.clone(),
        ));
    }

    pub(crate) fn apply_loaded_startup_state(
        &mut self,
        ctx: &egui::Context,
        state: AppState,
        startup_state_dirty: bool,
        startup_state_needs_cjk_fallback: bool,
    ) {
        self.state = state;
        self.startup_state_needs_cjk_fallback = startup_state_needs_cjk_fallback;
        self.save_name = self.state.selected_profile.clone().unwrap_or_default();
        self.last_active_panel = self.state.active_panel;
        self.panel_warmup_target = Some(self.state.active_panel);
        self.panel_warmup_frames_remaining = 1;
        self.warmed_panels.clear();
        {
            let mut vars = crate::overlay::RUNTIME_VARIABLES.lock();
            vars.clear();
            for (name, val) in &self.state.global_constants {
                vars.insert(name.clone(), *val as f64);
            }
        }
        let mut persist_pending = startup_state_dirty;
        if self.apply_startup_state_adjustments() {
            persist_pending = true;
        }
        self.startup_state_persist_pending |= persist_pending;
        self.startup_overlay_sync_pending = true;
        self.startup_cjk_font_check_pending = true;
        self.startup_shell_frames_remaining = self.startup_shell_frames_remaining.max(3);
        configure_theme(ctx, self.state.ui_theme);
        ctx.request_repaint();
    }

    pub(crate) fn sync_vision_settings(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateVisionSettings(
            self.state.vision_settings.clone(),
        ));
    }

    pub(crate) fn sync_timer_presets(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateTimerPresets(
            self.state.timer_presets.clone(),
        ));
    }

    pub(crate) fn sync_geometry_presets(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateGeometryPresets(
            self.state.geometry_presets.clone(),
        ));
    }

    pub(crate) fn migrate_legacy_audio_sense_state(&mut self) -> bool {
        let mut changed = false;

        for group in &mut self.state.macro_groups {
            for preset in &mut group.presets {
                for step in &mut preset.steps {
                    if step.audio_sense_spec.kind != AudioSensePresetKind::Pitch {
                        step.audio_sense_spec.kind = AudioSensePresetKind::Pitch;
                        changed = true;
                    }
                }
            }
        }

        if self.state.audio_sense_presets.is_empty() {
            if self.state.next_audio_sense_preset_id != 1 {
                self.state.next_audio_sense_preset_id = 1;
                changed = true;
            }
        } else {
            let next_id = self
                .state
                .audio_sense_presets
                .iter()
                .map(|preset| preset.id)
                .max()
                .unwrap_or(0)
                + 1;
            if self.state.next_audio_sense_preset_id != next_id {
                self.state.next_audio_sense_preset_id = next_id;
                changed = true;
            }
        }

        changed
    }

    pub(crate) fn apply_startup_state_adjustments(&mut self) -> bool {
        let mut changed = false;
        if self.ensure_master_presets_without_persist() {
            changed = true;
        }
        if self.state.groq_settings.details_open {
            self.state.groq_settings.details_open = false;
            changed = true;
        }
        for preset in &mut self.state.command_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }
        if self.migrate_legacy_audio_sense_state() {
            changed = true;
        }
        if self.state.vision_settings.use_arduino_mouse
            && self.state.vision_settings.use_interception
        {
            self.state.vision_settings.use_interception = false;
            changed = true;
        }
        if self.state.protractor_enabled {
            self.state.protractor_enabled = false;
            changed = true;
        }
        changed
    }

    pub(crate) fn sync_audio_sense_presets(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateAudioSensePresets(
                self.state.audio_sense_presets.clone(),
            ));
    }

    pub(crate) fn sync_timer_preview(&mut self, preset: Option<&TimerPreset>) {
        let next_id = preset.map(|preset| preset.id);
        if self.active_timer_preview_preset_id == next_id {
            if let Some(preset) = preset {
                let _ = self
                    .overlay_tx
                    .send(OverlayCommand::PreviewTimerPreset(Some(preset.clone())));
            }
            return;
        }
        self.active_timer_preview_preset_id = next_id;
        let _ = self
            .overlay_tx
            .send(OverlayCommand::PreviewTimerPreset(preset.cloned()));
    }

    pub(crate) fn clear_timer_preview(&mut self) {
        if self.active_timer_preview_preset_id.take().is_some() {
            let _ = self
                .overlay_tx
                .send(OverlayCommand::PreviewTimerPreset(None));
        }
    }

    pub(crate) fn disable_timer_preview_modes(&mut self) -> bool {
        let mut changed = false;
        for preset in &mut self.state.timer_presets {
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }
        if changed {
            self.clear_timer_preview();
        }
        changed
    }

    pub(crate) fn persist_blocking(&mut self) {
        self.invalidate_macro_variable_cache();
        if let Err(error) = self.paths.save_profiles(&self.state.profiles) {
            self.status = format!("Failed to save profiles: {error}");
            return;
        }
        if let Err(error) = self.paths.save_state(&self.state) {
            self.status = format!("Failed to save app state: {error}");
        }
    }

    pub(crate) fn persist(&mut self) {
        self.invalidate_macro_variable_cache();
        self.persist_dirty = true;
        self.persist_requested_at = Some(Instant::now());
    }

    pub(crate) fn persist_deferred(&mut self, _ctx: &egui::Context) {
        self.persist();
    }

    pub(crate) fn persist_timer_presets_deferred(&mut self, ctx: &egui::Context) {
        self.sync_timer_presets();
        self.persist_deferred(ctx);
    }

    pub(crate) fn invalidate_macro_variable_cache(&mut self) {
        self.macro_referenced_variables_cache = None;
    }

    pub(crate) fn schedule_open_windows_refresh(&mut self, status: Option<String>) {
        if self.open_windows_loading {
            return;
        }
        self.open_windows_loading = true;
        let ui_tx = self.ui_tx.clone();
        std::thread::spawn(move || {
            let windows = window_list::list_open_windows()
                .into_iter()
                .map(|item| item.selector)
                .collect();
            let _ = ui_tx.send(UiCommand::OpenWindowsLoaded { windows, status });
        });
    }

    pub(crate) fn ensure_open_windows_ready(&mut self, force: bool) {
        if self.open_windows_loading {
            return;
        }
        if !force
            && self.open_windows_loaded_once
            && self.last_window_refresh_at.elapsed() < super::OPEN_WINDOWS_REFRESH_INTERVAL
        {
            return;
        }
        self.schedule_open_windows_refresh(None);
    }

    pub(crate) fn sync_quick_action_window_selection(&mut self) {
        if self.open_windows.is_empty() {
            self.quick_action_window_selector.clear();
            return;
        }
        if self.quick_action_window_selector.is_empty()
            || !self
                .open_windows
                .iter()
                .any(|item| item == &self.quick_action_window_selector)
        {
            self.quick_action_window_selector = self.open_windows[0].clone();
        }
    }

    pub(crate) fn schedule_audio_sense_devices_refresh(&mut self) {
        if self.audio_sense_devices_loading {
            return;
        }
        self.audio_sense_devices_loading = true;
        let ui_tx = self.ui_tx.clone();
        std::thread::spawn(move || {
            let devices = audiosense::list_capture_devices().unwrap_or_default();
            let _ = ui_tx.send(UiCommand::AudioSenseDevicesLoaded { devices });
        });
    }

    pub(crate) fn ensure_audio_sense_devices_ready(&mut self, force: bool) {
        if self.audio_sense_devices_loading {
            return;
        }
        if !force
            && self.audio_sense_devices_loaded_once
            && self.last_audio_sense_devices_refresh_at.elapsed()
                < super::AUDIO_SENSE_DEVICES_REFRESH_INTERVAL
        {
            return;
        }
        self.schedule_audio_sense_devices_refresh();
    }
}
