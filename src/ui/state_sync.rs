use eframe::egui;
use std::time::Instant;

use crate::{
    audiosense,
    model::{AppState, AudioSensePresetKind, ProfileRecord, TimerPreset},
    overlay::{MacroFolderScope, OverlayCommand, UiCommand},
    window_list,
};

use super::{CrosshairApp, build_runtime_macro_groups, configure_theme};

impl CrosshairApp {
    pub(crate) fn sync_crosshair(&mut self) {
        self.sync_profiles();
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

    pub(crate) fn sync_macro_delay_settings(&mut self) {
        let delays = (
            self.state.macro_mouse_click_delay_ms,
            self.state.macro_keyboard_key_press_delay_ms,
        );
        Self::sync_overlay_command_if_changed(
            &self.overlay_tx,
            delays,
            &mut self.last_synced_macro_delays,
            OverlayCommand::UpdateMacroDelays {
                mouse_click_delay_ms: delays.0,
                keyboard_key_press_delay_ms: delays.1,
            },
        );
    }

    pub(crate) fn sync_profiles(&mut self) {
        let profiles = self.state.profiles.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            profiles,
            &mut self.last_synced_profiles,
            OverlayCommand::UpdateProfiles,
        );
    }

    pub(crate) fn sync_crosshair_profile(&self, index: usize, profile: &ProfileRecord) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateCrosshairProfile {
                index,
                profile: profile.clone(),
            });
    }

    pub(crate) fn sync_overlay_state_if_changed<T>(
        overlay_tx: &crossbeam_channel::Sender<OverlayCommand>,
        state: T,
        last_synced: &mut Option<T>,
        command: impl FnOnce(T) -> OverlayCommand,
    ) where
        T: Clone + PartialEq,
    {
        if last_synced.as_ref() == Some(&state) {
            return;
        }
        *last_synced = Some(state.clone());
        let _ = overlay_tx.send(command(state));
    }

    pub(crate) fn update_synced_state<T>(state: T, last_synced: &mut Option<T>) -> bool
    where
        T: Clone + PartialEq,
    {
        if last_synced.as_ref() == Some(&state) {
            return false;
        }
        *last_synced = Some(state);
        true
    }

    pub(crate) fn sync_overlay_command_if_changed<T>(
        overlay_tx: &crossbeam_channel::Sender<OverlayCommand>,
        state: T,
        last_synced: &mut Option<T>,
        command: OverlayCommand,
    ) where
        T: Clone + PartialEq,
    {
        if !Self::update_synced_state(state, last_synced) {
            return;
        }
        let _ = overlay_tx.send(command);
    }

    pub(crate) fn sync_macro_presets(&mut self) {
        let macro_groups = build_runtime_macro_groups(&self.state);
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            macro_groups,
            &mut self.last_synced_macro_groups,
            OverlayCommand::UpdateMacroPresets,
        );
    }

    pub(crate) fn resolved_active_macro_folder_view(&self) -> Option<u32> {
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

    pub(crate) fn sync_active_macro_folder_scope(&mut self) {
        let active_folder_scope = self
            .resolved_active_macro_folder_view()
            .map(MacroFolderScope::Folder)
            .unwrap_or(MacroFolderScope::Root);
        if !Self::update_synced_state(
            active_folder_scope,
            &mut self.last_synced_active_macro_folder_scope,
        ) {
            return;
        }
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetActiveMacroFolderScope(active_folder_scope));
    }

    pub(crate) fn sync_macro_master_enabled(&mut self) {
        let enabled = self.state.macros_master_enabled;
        Self::sync_overlay_command_if_changed(
            &self.overlay_tx,
            enabled,
            &mut self.last_synced_macros_master_enabled,
            OverlayCommand::SetMacrosMasterEnabled(enabled),
        );
    }

    pub(crate) fn sync_windows_key_locked(&mut self) {
        let locked = self.state.windows_key_locked;
        Self::sync_overlay_command_if_changed(
            &self.overlay_tx,
            locked,
            &mut self.last_synced_windows_key_locked,
            OverlayCommand::SetWindowsKeyLocked(locked),
        );
    }

    pub(crate) fn sync_native_focus_highlight_enabled(&mut self) {
        let enabled = self.state.native_focus_highlight_enabled;
        Self::sync_overlay_command_if_changed(
            &self.overlay_tx,
            enabled,
            &mut self.last_synced_native_focus_highlight_enabled,
            OverlayCommand::SetNativeFocusHighlightEnabled(enabled),
        );
    }

    pub(crate) fn sync_focus_highlight_config(&mut self) {
        let config = (
            self.state.focus_highlight_color,
            self.state.focus_highlight_decoration,
        );
        if !Self::update_synced_state(config, &mut self.last_synced_focus_highlight_config) {
            return;
        }
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetFocusHighlightConfig {
                color: config.0,
                decoration: config.1,
            });
    }

    pub(crate) fn sync_protractor_state(&mut self) {
        let enabled = self.state.protractor_enabled;
        if self.last_synced_protractor_enabled != Some(enabled) {
            self.last_synced_protractor_enabled = Some(enabled);
            let _ = self
                .overlay_tx
                .send(OverlayCommand::SetProtractorEnabled(enabled));
        }
        let config = (
            self.state.protractor_scale,
            self.state.protractor_needle1_angle,
            self.state.protractor_needle2_angle,
            self.state.protractor_center_x,
            self.state.protractor_center_y,
            self.state.protractor_thickness,
            self.protractor_picking_active,
            self.state.ui_language,
        );
        if !Self::update_synced_state(config, &mut self.last_synced_protractor_config) {
            return;
        }
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateProtractorConfig {
                scale: config.0,
                needle1_angle: config.1,
                needle2_angle: config.2,
                center_x: config.3,
                center_y: config.4,
                thickness: config.5,
                calibrating: config.6,
                ui_language: config.7,
            });
    }

    pub(crate) fn sync_quick_key_display_config(&mut self) {
        let config = (
            self.state.quick_key_display_enabled,
            self.state.quick_key_display_x,
            self.state.quick_key_display_y,
            self.state.quick_key_display_size,
            self.state.quick_key_display_mode,
            self.state.quick_key_display_mascot_style,
        );
        if !Self::update_synced_state(config, &mut self.last_synced_quick_key_display_config) {
            return;
        }
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateQuickKeyDisplayConfig {
                enabled: config.0,
                center_x: config.1,
                center_y: config.2,
                size: config.3,
                mode: config.4,
                mascot_style: config.5,
            });
    }

    pub(crate) fn sync_quick_screen_draw_config(&mut self) {
        let config = (
            self.state.quick_screen_draw_enabled,
            self.state.quick_screen_draw_hotkey.clone(),
            self.state.quick_screen_draw_pass_trigger_through,
            self.state.quick_screen_draw_color,
            self.state.quick_screen_draw_brush_size,
            self.state.quick_screen_draw_smoothing,
            self.state.quick_screen_draw_smoothing_amount,
        );
        if !Self::update_synced_state(
            config.clone(),
            &mut self.last_synced_quick_screen_draw_config,
        ) {
            return;
        }
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateScreenDrawConfig {
                enabled: config.0,
                trigger: config.1,
                pass_trigger_through: config.2,
                color: config.3,
                brush_size: config.4,
                smoothing: config.5,
                smoothing_amount: config.6,
            });
    }

    pub(crate) fn sync_quick_key_sound_config(&mut self) {
        let config = (
            self.state.quick_key_sound_enabled,
            self.state.quick_key_sound_style,
            self.state.quick_key_sound_volume,
        );
        Self::sync_overlay_command_if_changed(
            &self.overlay_tx,
            config,
            &mut self.last_synced_quick_key_sound_config,
            OverlayCommand::UpdateKeySoundConfig {
                enabled: config.0,
                style: config.1,
                volume: config.2,
            },
        );
    }

    pub(crate) fn sync_vietnamese_input_enabled(&mut self) {
        let enabled = self.state.vietnamese_input_enabled;
        Self::sync_overlay_command_if_changed(
            &self.overlay_tx,
            enabled,
            &mut self.last_synced_vietnamese_input_enabled,
            OverlayCommand::SetVietnameseInputEnabled(enabled),
        );
    }

    pub(crate) fn sync_macro_master_hotkey(&mut self) {
        let binding = self.state.macros_master_hotkey.clone();
        if !Self::update_synced_state(binding.clone(), &mut self.last_synced_macro_master_hotkey) {
            return;
        }
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateMacrosMasterHotkey(binding));
    }

    pub(crate) fn sync_audio_settings(&mut self) {
        self.retain_referenced_audio_waveforms();
        let settings = self.state.audio_settings.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            settings,
            &mut self.last_synced_audio_settings,
            OverlayCommand::UpdateAudioSettings,
        );
    }

    pub(crate) fn sync_groq_settings(&mut self) {
        let settings = self.state.groq_settings.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            settings,
            &mut self.last_synced_groq_settings,
            OverlayCommand::UpdateGroqSettings,
        );
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

    pub(crate) fn sync_vision_settings(&mut self) {
        let settings = self.state.vision_settings.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            settings,
            &mut self.last_synced_vision_settings,
            OverlayCommand::UpdateVisionSettings,
        );
    }

    pub(crate) fn sync_timer_presets(&mut self) {
        let presets = self.state.timer_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            presets,
            &mut self.last_synced_timer_presets,
            OverlayCommand::UpdateTimerPresets,
        );
    }

    pub(crate) fn sync_geometry_presets(&mut self) {
        let presets = self.state.geometry_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            presets,
            &mut self.last_synced_geometry_presets,
            OverlayCommand::UpdateGeometryPresets,
        );
    }

    pub(crate) fn persist_geometry_presets(&mut self) {
        self.persist_after_sync(Self::sync_geometry_presets);
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

    pub(crate) fn sync_audio_sense_presets(&mut self) {
        let presets = self.state.audio_sense_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            presets,
            &mut self.last_synced_audio_sense_presets,
            OverlayCommand::UpdateAudioSensePresets,
        );
    }

    pub(crate) fn persist_audio_sense_presets(&mut self) {
        self.persist_after_sync(Self::sync_audio_sense_presets);
    }

    pub(crate) fn sync_timer_preview_preset(&mut self, preset: Option<TimerPreset>) {
        self.active_timer_preview_preset_id = preset.as_ref().map(|preset| preset.id);
        let _ = self
            .overlay_tx
            .send(OverlayCommand::PreviewTimerPreset(preset));
    }

    pub(crate) fn sync_timer_preview(&mut self, preset: Option<&TimerPreset>) {
        let next_id = preset.map(|preset| preset.id);
        if self.active_timer_preview_preset_id == next_id {
            if let Some(preset) = preset {
                self.sync_timer_preview_preset(Some(preset.clone()));
            }
            return;
        }
        self.sync_timer_preview_preset(preset.cloned());
    }

    pub(crate) fn clear_timer_preview(&mut self) {
        if self.active_timer_preview_preset_id.take().is_some() {
            self.sync_timer_preview_preset(None);
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

    pub(crate) fn persist_after_sync(&mut self, sync: impl FnOnce(&mut Self)) {
        sync(self);
        self.persist();
    }

    pub(crate) fn persist_after_syncs(
        &mut self,
        syncs: impl IntoIterator<Item = fn(&mut Self)>,
    ) {
        for sync in syncs {
            sync(self);
        }
        self.persist();
    }

    pub(crate) fn persist_deferred_after_sync(
        &mut self,
        ctx: &egui::Context,
        sync: impl FnOnce(&mut Self),
    ) {
        sync(self);
        self.persist_deferred(ctx);
    }

    pub(crate) fn persist_timer_presets_deferred(&mut self, ctx: &egui::Context) {
        self.persist_deferred_after_sync(ctx, Self::sync_timer_presets);
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
            let windows = window_list::list_open_windows();
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

    pub(crate) fn prime_open_windows_if_empty(&mut self) {
        if self.open_window_infos.is_empty() && !self.open_windows_loaded_once {
            self.ensure_open_windows_ready(false);
        }
    }

    pub(crate) fn sync_quick_action_window_selection(&mut self) {
        if self.open_window_infos.is_empty() {
            self.quick_action_window_selector.clear();
            return;
        }
        if self.quick_action_window_selector.is_empty()
            || !self
                .open_window_infos
                .iter()
                .any(|item| item.selector == self.quick_action_window_selector)
        {
            self.quick_action_window_selector = self.open_window_infos[0].selector.clone();
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

    pub(crate) fn prime_audio_sense_devices_if_empty(&mut self) {
        if self.audio_sense_devices.is_empty() && !self.audio_sense_devices_loaded_once {
            self.ensure_audio_sense_devices_ready(false);
        }
    }
}
