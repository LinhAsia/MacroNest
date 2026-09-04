use eframe::egui::{self, Button, Color32, ComboBox, DragValue, Frame, Grid, RichText, TextEdit};

use crate::model::{
    EspAngleUnit, EspMarkerKind, EspMarkerSource, EspOrientationSource,
    EspPitchInput, EspPreset, MemoryValueType, RgbaColor,
};

use super::CrosshairApp;

#[cfg(windows)]
use crate::memory_debugger::debugger::{
    AccessWatch, WatchEvent, disassemble_from, is_instruction_compatible,
    resolve_module_offset,
};

#[cfg(windows)]
pub(super) struct EspEntityRootCapture {
    pub(super) preset_id: u32,
    pid: u32,
    required: usize,
    scan_mode: crate::model::EspAutoScanMode,
    hit_step: usize,
    multi_strides: Vec<usize>,
    merge_pairs: bool,
    drop_nearest: bool,
    addresses: Vec<usize>,
    rx: std::sync::mpsc::Receiver<WatchEvent>,
    active: Option<AccessWatch>,
    hud_preset_id: Option<u32>,
    started_at: std::time::Instant,
    last_hit_at: std::time::Instant,
    timeout_at: Option<std::time::Instant>,
}

#[cfg(windows)]
const ESP_ENTITY_ROOT_CAPTURE_LIMIT: usize = 4096;

impl CrosshairApp {
    #[cfg(windows)]
    fn show_esp_entity_capture_hud(&self, hud_preset_id: Option<u32>, text: String) {
        let Some(hud_preset_id) = hud_preset_id else {
            return;
        };
        let Some(mut hud) = self
            .state
            .hud_presets
            .iter()
            .find(|hud| hud.id == hud_preset_id)
            .cloned()
        else {
            return;
        };
        hud.text = text;
        let _ = self
            .overlay_tx
            .send(crate::overlay::OverlayCommand::PreviewHudPreset(vec![hud]));
    }

    #[cfg(windows)]
    fn finalize_esp_entity_capture(
        &mut self,
        mut capture: EspEntityRootCapture,
        status_override: Option<&str>,
    ) {
        if let Some(mut active) = capture.active.take() {
            std::thread::spawn(move || {
                active.stop();
            });
        }
        let (candidate, matched, resolved_addresses) = match capture.scan_mode {
            crate::model::EspAutoScanMode::AllHits => {
                let addresses = capture.addresses.clone();
                let count = addresses.len();
                let candidate_root = addresses.first().copied();
                (candidate_root, count, Some(addresses))
            }
            crate::model::EspAutoScanMode::HitOrder => {
                let (filtered, count) = crate::model::entity_hits_in_capture_order_progress(
                    &capture.addresses,
                    capture.required as u32,
                    capture.hit_step as u32,
                );
                let candidate_root = filtered.first().copied();
                (candidate_root, count, Some(filtered))
            }
            crate::model::EspAutoScanMode::MultiStride => {
                let (chain, count) = crate::model::entity_multi_stride_hit_progress(
                    &capture.addresses,
                    capture.required as u32,
                    &capture.multi_strides,
                );
                let candidate_root = chain.as_ref().and_then(|c| c.first().copied());
                (candidate_root, count, chain)
            }
            crate::model::EspAutoScanMode::Stride => {
                let (cand, count) = self
                    .state
                    .esp_presets
                    .iter()
                    .find(|preset| preset.id == capture.preset_id)
                    .map(|preset| {
                        crate::model::entity_instruction_hit_progress(
                            &capture.addresses,
                            capture.required as u32,
                            preset.entity_stride,
                        )
                    })
                    .unwrap_or((None, 0));
                (cand, count, None)
            }
        };

        if capture.scan_mode == crate::model::EspAutoScanMode::HitOrder
            || capture.scan_mode == crate::model::EspAutoScanMode::MultiStride
            || capture.scan_mode == crate::model::EspAutoScanMode::AllHits
        {
            let mut final_addresses = resolved_addresses.unwrap_or_default();
            if capture.merge_pairs {
                if let Some(preset) = self
                    .state
                    .esp_presets
                    .iter()
                    .find(|p| p.id == capture.preset_id)
                {
                    final_addresses = merge_entity_addresses_by_3d_proximity(
                        capture.pid,
                        preset,
                        &final_addresses,
                    );
                }
            }
            let mut dropped_self = false;
            if capture.drop_nearest && final_addresses.len() > 1 {
                if let Some(preset) = self
                    .state
                    .esp_presets
                    .iter()
                    .find(|p| p.id == capture.preset_id)
                {
                    if let Some(dropped_idx) = find_nearest_entity_index(
                        capture.pid,
                        preset,
                        &final_addresses,
                    ) {
                        final_addresses.remove(dropped_idx);
                        dropped_self = true;
                    }
                }
            }
            if let Some(preset) = self
                .state
                .esp_presets
                .iter_mut()
                .find(|preset| preset.id == capture.preset_id)
            {
                if let Some(first_addr) = final_addresses.first() {
                    preset.entity_root = format!("0x{first_addr:X}");
                }
                preset.entity_count = final_addresses.len() as u32;
                preset.entity_hit_order_addresses = final_addresses.clone();
                preset.entity_list_enabled = true;
            }
            let feedback_msg = if let Some(status) = status_override {
                status.to_owned()
            } else if dropped_self {
                format!(
                    "Captured {} addresses (dropped self -> {} active)",
                    matched,
                    final_addresses.len()
                )
            } else if capture.scan_mode == crate::model::EspAutoScanMode::AllHits {
                format!("Captured all {} entity addresses", final_addresses.len())
            } else {
                format!(
                    "Captured {} entities ({} active)",
                    matched,
                    final_addresses.len()
                )
            };
            self.esp_entity_capture_feedback
                .insert(capture.preset_id, feedback_msg);
            if capture.hud_preset_id.is_some() {
                let _ = self
                    .overlay_tx
                    .send(crate::overlay::OverlayCommand::PreviewHudPreset(Vec::new()));
            }
            self.esp_entity_capture_hud_hide_at = None;
        } else if let Some(mut root) = candidate {
            let mut final_count = matched.max(1) as u32;
            let mut dropped_self = false;
            if capture.drop_nearest && final_count > 1 {
                if let Some(preset) = self
                    .state
                    .esp_presets
                    .iter()
                    .find(|p| p.id == capture.preset_id)
                {
                    let stride = preset.entity_stride.max(1) as usize;
                    let addresses: Vec<usize> = (0..final_count as usize)
                        .map(|i| root.saturating_add(i * stride))
                        .collect();
                    if let Some(dropped_idx) = find_nearest_entity_index(
                        capture.pid,
                        preset,
                        &addresses,
                    ) {
                        if dropped_idx == 0 {
                            root = root.saturating_add(stride);
                        }
                        final_count = final_count.saturating_sub(1).max(1);
                        dropped_self = true;
                    }
                }
            }
            if let Some(preset) = self
                .state
                .esp_presets
                .iter_mut()
                .find(|preset| preset.id == capture.preset_id)
            {
                preset.entity_root = format!("0x{root:X}");
                preset.entity_count = final_count;
                preset.entity_list_enabled = true;
            }
            let msg = if let Some(status) = status_override {
                status.to_owned()
            } else if dropped_self {
                format!("Root updated: 0x{root:X} (dropped self -> {final_count} active)")
            } else {
                format!("Root updated: 0x{root:X} ({matched}/{})", capture.required)
            };
            self.esp_entity_capture_feedback
                .insert(capture.preset_id, msg);
            if capture.hud_preset_id.is_some() {
                let _ = self
                    .overlay_tx
                    .send(crate::overlay::OverlayCommand::PreviewHudPreset(Vec::new()));
            }
            self.esp_entity_capture_hud_hide_at = None;
        } else if let Some(status) = status_override {
            self.esp_entity_capture_feedback
                .insert(capture.preset_id, status.to_owned());
            if capture.hud_preset_id.is_some() {
                let _ = self
                    .overlay_tx
                    .send(crate::overlay::OverlayCommand::PreviewHudPreset(Vec::new()));
            }
            self.esp_entity_capture_hud_hide_at = None;
        }

        self.persist_esp_presets();
        crate::platform::trim_working_set();
    }

    #[cfg(windows)]
    pub(crate) fn stop_esp_entity_root_capture(&mut self, status: Option<&str>) {
        let Some(capture) = self.esp_entity_root_capture.take() else {
            return;
        };
        self.finalize_esp_entity_capture(capture, status);
    }

    #[cfg(windows)]
    pub(crate) fn start_esp_entity_root_capture(&mut self, preset_id: u32, timeout_ms: Option<u64>) {
        self.stop_esp_entity_root_capture(Some("Stopped"));
        let Some(preset) = self
            .state
            .esp_presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .cloned()
        else {
            return;
        };
        let Some(code) = self
            .state
            .memory_code_list
            .iter()
            .find(|code| {
                code.module
                    .eq_ignore_ascii_case(&preset.entity_auto_code_module)
                    && code.offset == preset.entity_auto_code_offset
            })
            .cloned()
        else {
            self.esp_entity_capture_feedback
                .insert(preset_id, "Select an instruction first".to_owned());
            return;
        };
        let Some(pid) = crate::window_list::process_id_for_window(Some(&preset.target_window)) else {
            self.esp_entity_capture_feedback
                .insert(preset_id, "Target window is not running".to_owned());
            return;
        };
        let instruction_address = match resolve_module_offset(pid, &code.module, code.offset) {
            Ok(address) => address,
            Err(error) => {
                self.esp_entity_capture_feedback
                    .insert(preset_id, format!("Instruction unavailable: {error}"));
                return;
            }
        };
        let current_instruction = disassemble_from(
            pid,
            instruction_address,
            self.state.memory_debugger_architecture,
            1,
        )
        .ok()
        .and_then(|mut lines| lines.pop())
        .map(|(_, _, instruction)| instruction);
        if current_instruction
            .as_ref()
            .is_none_or(|current| !is_instruction_compatible(&code.instruction, current))
        {
            self.esp_entity_capture_feedback.insert(
                preset_id,
                "Saved instruction no longer matches this game build".to_owned(),
            );
            return;
        }
        let hud_preset_id = if preset.entity_auto_hud_enabled {
            let Some(id) = preset.entity_auto_hud_preset_id else {
                self.esp_entity_capture_feedback
                    .insert(preset_id, "Select a HUD preset or disable HUD".to_owned());
                return;
            };
            Some(id)
        } else {
            None
        };
        self.close_memory_debuggers();
        let required = preset.entity_auto_capture_count.clamp(1, 512) as usize;
        let scan_mode = preset.scan_mode();
        let hit_step = preset.entity_auto_hit_step.clamp(1, 32) as usize;
        let multi_strides = crate::model::parse_multi_strides(&preset.entity_multi_strides);
        let merge_pairs = preset.entity_hit_order_merge_pairs;
        let drop_nearest = preset.entity_hit_order_drop_nearest;
        let effective_timeout_ms = if scan_mode == crate::model::EspAutoScanMode::AllHits {
            timeout_ms.or_else(|| {
                let secs = preset.entity_auto_scan_duration_secs;
                if secs > 0.0 {
                    Some((secs * 1000.0).max(100.0) as u64)
                } else {
                    Some(1000)
                }
            })
        } else {
            timeout_ms
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let started = AccessWatch::start_unique(
            pid,
            instruction_address,
            self.state.memory_debugger_architecture,
            ESP_ENTITY_ROOT_CAPTURE_LIMIT,
            move |event| {
                let _ = tx.send(event);
            },
        );
        match started {
            Ok(active) => {
                let now = std::time::Instant::now();
                let timeout_at = effective_timeout_ms.and_then(|ms| {
                    if ms > 0 {
                        Some(now + std::time::Duration::from_millis(ms))
                    } else {
                        None
                    }
                });
                self.esp_entity_root_capture = Some(EspEntityRootCapture {
                    preset_id,
                    pid,
                    required,
                    scan_mode,
                    hit_step,
                    multi_strides,
                    merge_pairs,
                    drop_nearest,
                    addresses: Vec::with_capacity(128),
                    rx,
                    active: Some(active),
                    hud_preset_id,
                    started_at: now,
                    last_hit_at: now,
                    timeout_at,
                });
                let initial_feedback = match scan_mode {
                    crate::model::EspAutoScanMode::AllHits => "Scanning... 0 addresses".to_owned(),
                    crate::model::EspAutoScanMode::HitOrder => format!("Captured 0/{required}"),
                    _ => format!("Matched 0/{required}"),
                };
                self.esp_entity_capture_feedback.insert(preset_id, initial_feedback);
                self.esp_entity_capture_hud_hide_at = None;
                self.show_esp_entity_capture_hud(
                    hud_preset_id,
                    if scan_mode == crate::model::EspAutoScanMode::AllHits {
                        "Entity scan: 0 addresses".to_owned()
                    } else {
                        format!("Entity scan: 0/{required}")
                    },
                );
            }
            Err(error) => {
                self.esp_entity_capture_feedback
                    .insert(preset_id, format!("Unable to start debugger: {error}"));
            }
        }
    }

    #[cfg(windows)]
    pub(crate) fn poll_esp_entity_root_capture(&mut self, ctx: &egui::Context) {
        if self
            .esp_entity_capture_hud_hide_at
            .is_some_and(|hide_at| std::time::Instant::now() >= hide_at)
        {
            self.esp_entity_capture_hud_hide_at = None;
            let _ = self
                .overlay_tx
                .send(crate::overlay::OverlayCommand::PreviewHudPreset(Vec::new()));
        }
        let Some(mut capture) = self.esp_entity_root_capture.take() else {
            return;
        };
        let mut changed = false;
        let mut stopped = None;
        while let Ok(event) = capture.rx.try_recv() {
            match event {
                WatchEvent::Started { .. } => {}
                WatchEvent::AccessHit { data_address, .. } => {
                    if !capture.addresses.contains(&data_address) {
                        capture.addresses.push(data_address);
                    }
                    capture.last_hit_at = std::time::Instant::now();
                    changed = true;
                }
                WatchEvent::CaptureLimitReached(limit) => {
                    if capture.scan_mode == crate::model::EspAutoScanMode::Stride {
                        stopped = Some(format!(
                            "Stopped after {limit} unique addresses without a matching Stride group"
                        ));
                    }
                }
                WatchEvent::Error(error) => stopped = Some(format!("Debugger stopped: {error}")),
                WatchEvent::Stopped if stopped.is_none() => {
                    stopped = Some(match capture.scan_mode {
                        crate::model::EspAutoScanMode::AllHits => "Scan finished".to_owned(),
                        crate::model::EspAutoScanMode::HitOrder => {
                            "Debugger stopped before enough unique addresses were captured".to_owned()
                        }
                        crate::model::EspAutoScanMode::MultiStride => {
                            "Debugger stopped before a complete Multi-Stride group was found".to_owned()
                        }
                        crate::model::EspAutoScanMode::Stride => {
                            "Debugger stopped before a complete Stride group was found".to_owned()
                        }
                    })
                }
                WatchEvent::Stopped
                | WatchEvent::AddressHit { .. }
                | WatchEvent::BatchProgress { .. } => {}
            }
        }
        let (candidate, matched, resolved_addresses) = match capture.scan_mode {
            crate::model::EspAutoScanMode::AllHits => {
                let addresses = capture.addresses.clone();
                let count = addresses.len();
                let candidate_root = addresses.first().copied();
                (candidate_root, count, Some(addresses))
            }
            crate::model::EspAutoScanMode::HitOrder => {
                let (filtered, count) = crate::model::entity_hits_in_capture_order_progress(
                    &capture.addresses,
                    capture.required as u32,
                    capture.hit_step as u32,
                );
                let candidate_root = filtered.first().copied();
                (candidate_root, count, Some(filtered))
            }
            crate::model::EspAutoScanMode::MultiStride => {
                let (chain, count) = crate::model::entity_multi_stride_hit_progress(
                    &capture.addresses,
                    capture.required as u32,
                    &capture.multi_strides,
                );
                let candidate_root = chain.as_ref().and_then(|c| c.first().copied());
                (candidate_root, count, chain)
            }
            crate::model::EspAutoScanMode::Stride => {
                let (cand, count) = self
                    .state
                    .esp_presets
                    .iter()
                    .find(|preset| preset.id == capture.preset_id)
                    .map(|preset| {
                        crate::model::entity_instruction_hit_progress(
                            &capture.addresses,
                            capture.required as u32,
                            preset.entity_stride,
                        )
                    })
                    .unwrap_or((None, 0));
                (cand, count, None)
            }
        };

        if let Some(candidate_root) = candidate {
            if let Some(preset) = self
                .state
                .esp_presets
                .iter_mut()
                .find(|preset| preset.id == capture.preset_id)
            {
                let formatted_root = format!("0x{candidate_root:X}");
                let mut needs_sync = false;
                if preset.entity_root != formatted_root {
                    preset.entity_root = formatted_root;
                    needs_sync = true;
                }
                if !preset.entity_list_enabled {
                    preset.entity_list_enabled = true;
                    needs_sync = true;
                }
                if capture.scan_mode == crate::model::EspAutoScanMode::HitOrder
                    || capture.scan_mode == crate::model::EspAutoScanMode::MultiStride
                    || capture.scan_mode == crate::model::EspAutoScanMode::AllHits
                {
                    if let Some(resolved) = &resolved_addresses {
                        if &preset.entity_hit_order_addresses != resolved {
                            preset.entity_hit_order_addresses = resolved.clone();
                            preset.entity_count = resolved.len() as u32;
                            needs_sync = true;
                        }
                    }
                }
                if needs_sync {
                    self.persist_esp_presets();
                }
            }
        }

        let timed_out = capture
            .timeout_at
            .is_some_and(|deadline| std::time::Instant::now() >= deadline);
        if timed_out && stopped.is_none() {
            if capture.scan_mode != crate::model::EspAutoScanMode::AllHits {
                stopped = Some("Scan timed out".to_owned());
            }
        }

        let is_complete = if capture.scan_mode == crate::model::EspAutoScanMode::AllHits {
            timed_out
        } else {
            matched >= capture.required || timed_out
        };
        if is_complete || stopped.is_some() {
            self.finalize_esp_entity_capture(capture, stopped.as_deref());
            ctx.request_repaint();
            return;
        }

        if changed {
            let feedback = match capture.scan_mode {
                crate::model::EspAutoScanMode::AllHits => {
                    format!("Scanned {} addresses", capture.addresses.len())
                }
                crate::model::EspAutoScanMode::HitOrder => {
                    format!("Captured {matched}/{}", capture.required)
                }
                _ => {
                    format!("Matched {matched}/{}", capture.required)
                }
            };
            self.esp_entity_capture_feedback.insert(capture.preset_id, feedback.clone());
            self.show_esp_entity_capture_hud(capture.hud_preset_id, feedback);
            ctx.request_repaint();
        }

        self.esp_entity_root_capture = Some(capture);
        ctx.request_repaint_after(std::time::Duration::from_millis(35));
    }

    pub(crate) fn render_esp_panel(&mut self, ui: &mut egui::Ui) {
        let mut dirty = false;
        if ui.button("+ Add ESP preset").clicked() {
            let id = Self::allocate_next_id(
                &self.state.esp_presets,
                &mut self.state.next_esp_preset_id,
                |preset| preset.id,
            );
            self.state.esp_presets.push(EspPreset::new(id));
            dirty = true;
        }
        ui.add_space(6.0);

        let windows = crate::window_list::list_open_windows();
        let mut remove = None;
        let mut copy_preset_index = None;
        let mut paste_after = None;
        let mut auto_capture_start = None;
        let mut auto_capture_stop = None;
        let can_paste = matches!(
            self.preset_clipboard,
            Some(crate::ui::PresetClipboard::Esp(_))
        );
        for index in 0..self.state.esp_presets.len() {
            let mut preset = self.state.esp_presets[index].clone();
            let migrated_marker_source = preset.migrate_marker_sources();
            let before = preset.clone();
            let calibration_feedback = self.esp_calibration_feedback.get(&preset.id).cloned();
            let entity_capture_feedback = self
                .esp_entity_capture_feedback
                .get(&preset.id)
                .cloned();
            #[cfg(windows)]
            let entity_capture_active = self
                .esp_entity_root_capture
                .as_ref()
                .is_some_and(|capture| capture.preset_id == preset.id);
            #[cfg(not(windows))]
            let entity_capture_active = false;
            Self::show_preset_card(ui, false, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [Self::preset_header_name_width(ui), 21.0],
                        TextEdit::singleline(&mut preset.name),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                can_paste,
                                Button::new("Paste").min_size(egui::vec2(84.0, 24.0)),
                            )
                            .clicked()
                        {
                            paste_after = Some(index);
                        }
                        if Self::sound_style_toggle_button(ui, "Copy").clicked() {
                            copy_preset_index = Some(index);
                        }
                        if Self::sound_style_remove_button(ui).clicked() {
                            remove = Some(preset.id);
                        }
                        if Self::sound_style_toggle_button(
                            ui,
                            if preset.collapsed { "Show" } else { "Hide" },
                        )
                        .clicked()
                        {
                            preset.collapsed = !preset.collapsed;
                        }
                        if ui
                            .add(
                                Button::new(if preset.enabled { "ESP On" } else { "ESP Off" })
                                    .selected(preset.enabled)
                                    .min_size(egui::vec2(84.0, 24.0)),
                            )
                            .clicked()
                        {
                            preset.enabled = !preset.enabled;
                        }
                    });
                });
                if preset.collapsed {
                    return;
                }

                ui.separator();
                Grid::new(("esp_addresses", preset.id))
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Target window");
                        let matched_window = windows
                            .iter()
                            .find(|window| window.selector == preset.target_window)
                            .or_else(|| {
                                let title =
                                    crate::window_list::selector_base_title(&preset.target_window);
                                (!title.is_empty())
                                    .then(|| windows.iter().find(|window| window.title == title))
                                    .flatten()
                            })
                            .cloned();
                        if let Some(window) = &matched_window
                            && preset.target_window != window.selector
                        {
                            preset.target_window = window.selector.clone();
                        }
                        if !windows.is_empty() && matched_window.is_none() && !preset.target_window.is_empty() {
                            preset.target_window.clear();
                        }
                        let target_label = matched_window
                            .as_ref()
                            .map(|window| format!("{} [PID {}]", window.title, window.process_id))
                            .unwrap_or_else(|| "Select window".to_string());
                        ComboBox::from_id_salt(("esp_window", preset.id))
                            .selected_text(target_label)
                            .width(320.0)
                            .show_ui(ui, |ui| {
                                for window in &windows {
                                    ui.selectable_value(
                                        &mut preset.target_window,
                                        window.selector.clone(),
                                        format!("{} [PID {}]", window.title, window.process_id),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label("Target source");
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut preset.entity_list_enabled,
                                false,
                                "Single target",
                            );
                            ui.selectable_value(
                                &mut preset.entity_list_enabled,
                                true,
                                "Entity list",
                            );
                        });
                        ui.end_row();
                        if preset.entity_list_enabled {
                            let root_step = *preset
                                .entity_root_step
                                .get_or_insert(preset.entity_stride.max(1));
                            let root_step_multiplier = preset.entity_root_step_multiplier.max(1);
                            let navigation_step = root_step.saturating_mul(root_step_multiplier);
                            ui.label("Entity root");
                            ui.horizontal(|ui| {
                                ui.add(
                                    TextEdit::singleline(&mut preset.entity_root)
                                        .desired_width(280.0)
                                        .hint_text(
                                            RichText::new(
                                                "stable pointer to first entity X / @alias",
                                            )
                                            .color(ui.visuals().weak_text_color()),
                                        ),
                                );
                                let raw_root = crate::model::shift_raw_entity_root(
                                    &preset.entity_root,
                                    root_step,
                                    0,
                                )
                                .is_some();
                                if ui
                                    .add_enabled(raw_root, egui::Button::new("▲"))
                                    .on_hover_text(
                                        "Replace the raw root address with root - (Step x multiplier)",
                                    )
                                    .clicked()
                                {
                                    if let Some(root) = crate::model::shift_raw_entity_root(
                                        &preset.entity_root,
                                        navigation_step,
                                        -1,
                                    ) {
                                        preset.entity_root = root;
                                    }
                                }
                                if ui
                                    .add_enabled(raw_root, egui::Button::new("▼"))
                                    .on_hover_text(
                                        "Replace the raw root address with root + (Step x multiplier)",
                                    )
                                    .clicked()
                                {
                                    if let Some(root) = crate::model::shift_raw_entity_root(
                                        &preset.entity_root,
                                        navigation_step,
                                        1,
                                    ) {
                                        preset.entity_root = root;
                                    }
                                }
                                ui.label("Step");
                                if let Some(step) = preset.entity_root_step.as_mut() {
                                    ui.add(
                                        DragValue::new(step)
                                            .range(1..=0x10000)
                                            .hexadecimal(1, false, false),
                                    )
                                        .on_hover_text(
                                            "Raw-address navigation step in hexadecimal bytes; defaults to Stride.",
                                        );
                                }
                                ui.label("x");
                                ui.add(
                                    DragValue::new(&mut preset.entity_root_step_multiplier)
                                        .range(1..=1_000_000),
                                )
                                .on_hover_text(
                                    "Decimal multiplier. Each arrow moves Entity root by Step x this value.",
                                );
                            });
                            ui.end_row();
                            ui.label("Entity layout");
                            ui.horizontal(|ui| {
                                ui.label("X");
                                ui.add(DragValue::new(&mut preset.entity_x_offset));
                                ui.label("Y");
                                ui.add(DragValue::new(&mut preset.entity_y_offset));
                                ui.label("Z");
                                ui.add(DragValue::new(&mut preset.entity_z_offset));
                                ui.label("Stride");
                                ui.add(
                                    DragValue::new(&mut preset.entity_stride)
                                        .range(1..=0x10000)
                                        .hexadecimal(1, false, false),
                                );
                                ui.label("Count");
                                ui.add(
                                    DragValue::new(&mut preset.entity_count).range(1..=512),
                                );
                            });
                            ui.end_row();
                            ui.label("Auto root");
                            ui.vertical(|ui| {
                                let selected_code = self
                                    .state
                                    .memory_code_list
                                    .iter()
                                    .find(|code| {
                                        code.module.eq_ignore_ascii_case(
                                            &preset.entity_auto_code_module,
                                        ) && code.offset == preset.entity_auto_code_offset
                                    });
                                let format_code_label = |code: &crate::model::MemoryCodeEntry| {
                                    if !code.name.trim().is_empty() {
                                        format!(
                                            "{} — {}+{:X}  {}",
                                            code.name, code.module, code.offset, code.instruction
                                        )
                                    } else {
                                        format!(
                                            "{}+{:X}  {}",
                                            code.module, code.offset, code.instruction
                                        )
                                    }
                                };
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    ComboBox::from_id_salt(("esp_auto_root_code", preset.id))
                                        .selected_text(selected_code.map_or(
                                            "Select instruction".to_owned(),
                                            format_code_label,
                                        ))
                                            .width(260.0)
                                            .show_ui(ui, |ui| {
                                                for code in &self.state.memory_code_list {
                                                    let selected = code.module.eq_ignore_ascii_case(
                                                        &preset.entity_auto_code_module,
                                                    ) && code.offset == preset.entity_auto_code_offset;
                                                    if ui
                                                        .selectable_label(
                                                            selected,
                                                            format_code_label(code),
                                                        )
                                                        .clicked()
                                                    {
                                                        preset.entity_auto_code_module =
                                                            code.module.clone();
                                                        preset.entity_auto_code_offset = code.offset;
                                                    }
                                                }
                                            });

                                        let mut scan_mode = preset.scan_mode();
                                        let scan_mode_text = match scan_mode {
                                            crate::model::EspAutoScanMode::Stride => "Single Stride",
                                            crate::model::EspAutoScanMode::MultiStride => "Multi Stride",
                                            crate::model::EspAutoScanMode::HitOrder => "Hit Order",
                                            crate::model::EspAutoScanMode::AllHits => "All Hits",
                                        };
                                        ComboBox::from_id_salt(("esp_auto_scan_mode", preset.id))
                                            .selected_text(scan_mode_text)
                                            .width(100.0)
                                            .show_ui(ui, |ui| {
                                                if ui
                                                    .selectable_value(
                                                        &mut scan_mode,
                                                        crate::model::EspAutoScanMode::Stride,
                                                        "Single Stride",
                                                    )
                                                    .clicked()
                                                {
                                                    preset.entity_auto_scan_mode =
                                                        crate::model::EspAutoScanMode::Stride;
                                                    preset.entity_auto_hit_order = false;
                                                }
                                                if ui
                                                    .selectable_value(
                                                        &mut scan_mode,
                                                        crate::model::EspAutoScanMode::MultiStride,
                                                        "Multi Stride",
                                                    )
                                                    .clicked()
                                                {
                                                    preset.entity_auto_scan_mode =
                                                        crate::model::EspAutoScanMode::MultiStride;
                                                    preset.entity_auto_hit_order = false;
                                                }
                                                if ui
                                                    .selectable_value(
                                                        &mut scan_mode,
                                                        crate::model::EspAutoScanMode::HitOrder,
                                                        "Hit Order",
                                                    )
                                                    .clicked()
                                                {
                                                    preset.entity_auto_scan_mode =
                                                        crate::model::EspAutoScanMode::HitOrder;
                                                    preset.entity_auto_hit_order = true;
                                                }
                                                if ui
                                                    .selectable_value(
                                                        &mut scan_mode,
                                                        crate::model::EspAutoScanMode::AllHits,
                                                        "All Hits",
                                                    )
                                                    .clicked()
                                                {
                                                    preset.entity_auto_scan_mode =
                                                        crate::model::EspAutoScanMode::AllHits;
                                                    preset.entity_auto_hit_order = false;
                                                }
                                            });

                                        if scan_mode == crate::model::EspAutoScanMode::AllHits {
                                            ui.label("Duration");
                                            ui.add(
                                                DragValue::new(&mut preset.entity_auto_scan_duration_secs)
                                                    .range(0.1..=30.0)
                                                    .speed(0.1)
                                                    .suffix("s"),
                                            )
                                            .on_hover_text("Scan time duration in seconds (default: 1.0s).");
                                        }

                                        if scan_mode == crate::model::EspAutoScanMode::MultiStride {
                                            ui.label("Strides");
                                            ui.add(
                                                egui::TextEdit::singleline(&mut preset.entity_multi_strides)
                                                    .desired_width(120.0)
                                                    .hint_text("e.g. 2260, 25D0"),
                                            )
                                            .on_hover_text(
                                                "Comma-separated hex/dec strides for variable entity struct sizes (e.g. 2260, 25D0 or 0x2260, 0x25D0).",
                                            );
                                        }

                                        if scan_mode != crate::model::EspAutoScanMode::AllHits {
                                            ui.label("Need");
                                            ui.add(
                                                DragValue::new(&mut preset.entity_auto_capture_count)
                                                    .range(1..=512),
                                            )
                                            .on_hover_text(match scan_mode {
                                                crate::model::EspAutoScanMode::HitOrder => {
                                                    "Stop after this many filtered entities are captured."
                                                }
                                                crate::model::EspAutoScanMode::MultiStride => {
                                                    "Stop after finding a chain of this many entities connected by allowed strides."
                                                }
                                                crate::model::EspAutoScanMode::Stride => {
                                                    "Stop only after this many addresses form one group at the configured Stride."
                                                }
                                                crate::model::EspAutoScanMode::AllHits => "",
                                            });
                                        }

                                        if scan_mode == crate::model::EspAutoScanMode::HitOrder {
                                            ui.label("Step");
                                            ui.add(
                                                DragValue::new(&mut preset.entity_auto_hit_step)
                                                    .range(1..=32),
                                            )
                                            .on_hover_text(
                                                "Number of address hits per entity (e.g. 2 for AABB min/max pair). Automatically selects the smaller base address in each pair (even or odd index).",
                                            );
                                        }

                                        if scan_mode == crate::model::EspAutoScanMode::HitOrder
                                            || scan_mode == crate::model::EspAutoScanMode::MultiStride
                                            || scan_mode == crate::model::EspAutoScanMode::AllHits
                                        {
                                            ui.checkbox(
                                                &mut preset.entity_hit_order_merge_pairs,
                                                "Merge pairs",
                                            )
                                            .on_hover_text(
                                                "Merge captured addresses that share the same 3D world position into a single entity.",
                                            );
                                        }

                                        ui.checkbox(
                                            &mut preset.entity_hit_order_drop_nearest,
                                            "Drop self",
                                        )
                                        .on_hover_text(
                                            "After capturing all entities, remove the entity with the smallest distance to camera (local player).",
                                        );

                                        ui.checkbox(&mut preset.entity_auto_hud_enabled, "HUD");
                                        if preset.entity_auto_hud_enabled {
                                            let hud_name = preset
                                                .entity_auto_hud_preset_id
                                                .and_then(|id| {
                                                    self.state
                                                        .hud_presets
                                                        .iter()
                                                        .find(|hud| hud.id == id)
                                                })
                                                .map_or("Select HUD", |hud| hud.name.as_str());
                                            ComboBox::from_id_salt(("esp_auto_root_hud", preset.id))
                                                .selected_text(hud_name)
                                                .width(110.0)
                                                .show_ui(ui, |ui| {
                                                    for hud in &self.state.hud_presets {
                                                        ui.selectable_value(
                                                            &mut preset.entity_auto_hud_preset_id,
                                                            Some(hud.id),
                                                            &hud.name,
                                                        );
                                                    }
                                                });
                                        }
                                    });

                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        if entity_capture_active {
                                            if ui.button("Stop").clicked() {
                                                auto_capture_stop = Some(preset.id);
                                            }
                                        } else if ui.button("Scan").clicked() {
                                            auto_capture_start = Some(preset.id);
                                        }
                                        if let Some(status) = &entity_capture_feedback {
                                            ui.label(
                                                RichText::new(status)
                                                    .color(ui.visuals().weak_text_color()),
                                            );
                                        }
                                    });
                                });
                            ui.end_row();
                            ui.label("");
                            ui.label(
                                RichText::new(
                                    "Offsets are bytes. Stride and Step are hexadecimal bytes. Runtime reads are capped at 512 slots.",
                                )
                                .weak(),
                            );
                            ui.end_row();
                            ui.label("");
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut preset.entity_aabb_center, "AABB center");
                                if preset.entity_aabb_center {
                                    ui.label("Pair delta");
                                    ui.add(
                                        DragValue::new(&mut preset.entity_aabb_pair_offset)
                                            .range(-0x10000..=0x10000),
                                    );
                                    ui.label("bytes");
                                }
                            });
                            ui.end_row();
                            ui.label("Entity colors");
                            ui.horizontal_wrapped(|ui| {
                                ui.label("Entity #");
                                let mut target_idx: u32 = ui.data_mut(|d| {
                                    *d.get_temp_mut_or(
                                        egui::Id::new(("entity_color_idx", preset.id)),
                                        1u32,
                                    )
                                });
                                if ui
                                    .add(
                                        DragValue::new(&mut target_idx)
                                            .range(1..=preset.entity_count.max(1) as u32),
                                    )
                                    .changed()
                                {
                                    ui.data_mut(|d| {
                                        d.insert_temp(
                                            egui::Id::new(("entity_color_idx", preset.id)),
                                            target_idx,
                                        )
                                    });
                                }

                                let mut pick_color = ui.data_mut(|d| {
                                    *d.get_temp_mut_or(
                                        egui::Id::new(("entity_color_picker", preset.id)),
                                        Color32::from_rgb(255, 0, 0),
                                    )
                                });
                                if ui.color_edit_button_srgba(&mut pick_color).changed() {
                                    ui.data_mut(|d| {
                                        d.insert_temp(
                                            egui::Id::new(("entity_color_picker", preset.id)),
                                            pick_color,
                                        )
                                    });
                                }

                                if ui
                                    .button("Set color")
                                    .on_hover_text("Assign custom color to selected Entity #")
                                    .clicked()
                                {
                                    preset.custom_entity_colors.insert(
                                        target_idx,
                                        crate::model::RgbaColor {
                                            r: pick_color.r(),
                                            g: pick_color.g(),
                                            b: pick_color.b(),
                                            a: pick_color.a(),
                                        },
                                    );
                                }

                                if !preset.custom_entity_colors.is_empty() {
                                    ui.label("| Active:");
                                    let mut to_remove = None;
                                    let mut sorted_keys: Vec<_> =
                                        preset.custom_entity_colors.keys().copied().collect();
                                    sorted_keys.sort_unstable();
                                    for key in sorted_keys {
                                        if let Some(c) = preset.custom_entity_colors.get(&key) {
                                            let color_32 = Color32::from_rgba_unmultiplied(
                                                c.r, c.g, c.b, c.a,
                                            );
                                            ui.horizontal(|ui| {
                                                let (rect, _) = ui.allocate_exact_size(
                                                    egui::vec2(12.0, 12.0),
                                                    egui::Sense::hover(),
                                                );
                                                ui.painter().rect_filled(rect, 2.0, color_32);
                                                ui.label(format!("#{}", key));
                                                if ui.small_button("❌").clicked() {
                                                    to_remove = Some(key);
                                                }
                                            });
                                        }
                                    }
                                    if let Some(key) = to_remove {
                                        preset.custom_entity_colors.remove(&key);
                                    }
                                }
                            });
                            ui.end_row();
                        } else {
                            for (label, value) in [
                                ("Target X", &mut preset.target_x),
                                ("Target Y", &mut preset.target_y),
                                ("Target Z", &mut preset.target_z),
                            ] {
                                memory_expression_row(ui, label, value);
                            }
                        }
                    });

                ui.add_space(2.0);

                ui.columns(2, |columns| {
                    // --- Left Column: Camera Position (X, Y, Z) ---
                    columns[0].vertical(|ui| {
                        Grid::new(("esp_camera_pos_grid", preset.id))
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                for (label, value) in [
                                    ("Camera X", &mut preset.camera_x),
                                    ("Camera Y", &mut preset.camera_y),
                                    ("Camera Z", &mut preset.camera_z),
                                ] {
                                    ui.label(label);
                                    ui.add(
                                        TextEdit::singleline(value)
                                            .desired_width(ui.available_width() - 8.0)
                                            .hint_text(
                                                RichText::new(
                                                    "address / module+offset [offsets] / @alias",
                                                )
                                                .color(ui.visuals().weak_text_color()),
                                            ),
                                    );
                                    ui.end_row();
                                }
                            });
                    });

                    // --- Right Column: Orientation Source & Direction/Pitch ---
                    columns[1].vertical(|ui| {
                        Grid::new(("esp_camera_orient_grid", preset.id))
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Orientation source");
                                ComboBox::from_id_salt(("esp_orientation_source", preset.id))
                                    .selected_text(match preset.orientation_source {
                                        EspOrientationSource::Angles => "Yaw + pitch angles",
                                        EspOrientationSource::DirectionPairPitch => {
                                            "Horizontal direction pair + pitch"
                                        }
                                    })
                                    .width(ui.available_width() - 8.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut preset.orientation_source,
                                            EspOrientationSource::Angles,
                                            "Yaw + pitch angles",
                                        );
                                        ui.selectable_value(
                                            &mut preset.orientation_source,
                                            EspOrientationSource::DirectionPairPitch,
                                            "Horizontal direction pair + pitch",
                                        );
                                    });
                                ui.end_row();

                                match preset.orientation_source {
                                    EspOrientationSource::Angles => {
                                        for (label, value) in [
                                            ("Camera yaw", &mut preset.camera_yaw),
                                            ("Camera pitch", &mut preset.camera_pitch),
                                        ] {
                                            ui.label(label);
                                            ui.add(
                                                TextEdit::singleline(value)
                                                    .desired_width(ui.available_width() - 8.0)
                                                    .hint_text(
                                                        RichText::new(
                                                            "address / module+offset [offsets] / @alias",
                                                        )
                                                        .color(ui.visuals().weak_text_color()),
                                                    ),
                                            );
                                            ui.end_row();
                                        }
                                    }
                                    EspOrientationSource::DirectionPairPitch => {
                                        for (label, value) in [
                                            ("Camera direction A", &mut preset.camera_direction_a),
                                            ("Camera pitch", &mut preset.camera_pitch),
                                            ("Camera direction B", &mut preset.camera_direction_b),
                                        ] {
                                            ui.label(label);
                                            ui.add(
                                                TextEdit::singleline(value)
                                                    .desired_width(ui.available_width() - 8.0)
                                                    .hint_text(
                                                        RichText::new(
                                                            "address / module+offset [offsets] / @alias",
                                                        )
                                                        .color(ui.visuals().weak_text_color()),
                                                    ),
                                            );
                                            ui.end_row();
                                        }
                                    }
                                }
                            });
                    });
                });

                ui.add_space(4.0);

                ui.columns(2, |columns| {
                    // --- Left Column: Box 1 (Camera & 3D Projection) + Box 2 (Calibration & Helpers) ---
                    columns[0].vertical(|ui| {
                        // --- BOX 1: Camera & 3D Projection ---
                        Frame::group(ui.style())
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .rounding(4.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Camera & 3D Projection")
                                        .strong()
                                        .color(Color32::from_rgb(100, 200, 255)),
                                );
                                ui.add_space(2.0);

                                // Row 1: Coordinate system & FOV
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Value type:");
                                    ComboBox::from_id_salt(("esp_type", preset.id))
                                        .selected_text(memory_type_name(preset.value_type))
                                        .show_ui(ui, |ui| {
                                            for value_type in [
                                                MemoryValueType::I8,
                                                MemoryValueType::I16,
                                                MemoryValueType::I32,
                                                MemoryValueType::F32,
                                                MemoryValueType::I64,
                                                MemoryValueType::F64,
                                            ] {
                                                ui.selectable_value(
                                                    &mut preset.value_type,
                                                    value_type,
                                                    memory_type_name(value_type),
                                                );
                                            }
                                        });



                                    ui.add_space(4.0);
                                    ui.label("FOV:");
                                    ui.add(
                                        DragValue::new(&mut preset.horizontal_fov)
                                            .speed(1.0)
                                            .range(1.0..=179.0),
                                    );
                                });

                                ui.add_space(2.0);

                                // Row 2: Angle units / Direction toggles
                                ui.horizontal_wrapped(|ui| {
                                    if preset.orientation_source == EspOrientationSource::Angles {
                                        angle_unit(ui, "Yaw", preset.id, &mut preset.yaw_unit);
                                        angle_unit(ui, "Pitch", preset.id, &mut preset.pitch_unit);
                                    } else {
                                        ui.label(RichText::new("Yaw = atan2(Dir B, Dir A)").weak());
                                        angle_unit(ui, "Pitch", preset.id, &mut preset.pitch_unit);
                                        ui.checkbox(&mut preset.swap_direction_pair, "Swap A/B");
                                        ui.checkbox(&mut preset.invert_direction_a, "Invert A");
                                        ui.checkbox(&mut preset.invert_direction_b, "Invert B");
                                    }
                                });

                                ui.add_space(2.0);

                                // Row 3: Invert & Mirror toggles
                                ui.horizontal_wrapped(|ui| {
                                    ui.checkbox(&mut preset.invert_camera_yaw, "Reverse yaw")
                                        .on_hover_text("Reverse only camera rotation.");
                                    ui.checkbox(&mut preset.invert_camera_pitch, "Reverse pitch")
                                        .on_hover_text("Reverse only camera pitch angle.");
                                    if preset.orientation_source == EspOrientationSource::Angles {
                                        ui.checkbox(&mut preset.invert_direction_a, "Invert A (X)")
                                            .on_hover_text("Invert horizontal axis A (X). Use this when 2 opposite directions show ESP but 2 perpendicular directions disappear.");
                                        ui.checkbox(&mut preset.invert_direction_b, "Invert B (Y)")
                                            .on_hover_text("Invert horizontal axis B (Y).");
                                        ui.checkbox(&mut preset.swap_direction_pair, "Swap A/B")
                                            .on_hover_text("Swap horizontal axes A and B.");
                                    }
                                    ui.checkbox(&mut preset.invert_vertical, "Invert height (Z)")
                                        .on_hover_text("Invert target elevation difference.");
                                    ui.checkbox(&mut preset.invert_yaw, "Mirror X")
                                        .on_hover_text("Mirror only final left/right screen position.");
                                    ui.checkbox(&mut preset.invert_pitch, "Mirror Y")
                                        .on_hover_text("Mirror only final up/down screen position.");
                                });

                                ui.add_space(2.0);

                                // Row 4: Yaw offset & Direction B/A ratio
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Yaw offset:").on_hover_text("Use this when marker is consistently rotated left/right.");
                                    ui.add(
                                        DragValue::new(&mut preset.yaw_offset_degrees)
                                            .speed(1.0)
                                            .range(-360.0..=360.0)
                                            .suffix("°"),
                                    );
                                    for value in [-180.0, -90.0, 0.0, 90.0, 180.0] {
                                        if ui.small_button(format!("{value:+.0}")).clicked() {
                                            preset.yaw_offset_degrees = value;
                                        }
                                    }

                                    ui.add_space(4.0);
                                    ui.label("Dir B/A ratio:");
                                    ui.add(
                                        DragValue::new(&mut preset.direction_multiplier)
                                            .speed(0.001)
                                            .range(0.0001..=100.0),
                                    );
                                    if ui.small_button("Reset").clicked() {
                                        preset.direction_multiplier = 1.0;
                                    }
                                });

                                ui.add_space(2.0);

                                // Row 5: Pitch controls
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Pitch input:");
                                    ComboBox::from_id_salt(("esp_pitch_input", preset.id))
                                        .selected_text(match preset.pitch_input {
                                            EspPitchInput::Angle => "Angle",
                                            EspPitchInput::SineComponent => "Direction component (asin)",
                                            EspPitchInput::TangentComponent => "Slope component (atan)",
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut preset.pitch_input, EspPitchInput::Angle, "Angle");
                                            ui.selectable_value(
                                                &mut preset.pitch_input,
                                                EspPitchInput::SineComponent,
                                                "Direction component (asin)",
                                            );
                                            ui.selectable_value(
                                                &mut preset.pitch_input,
                                                EspPitchInput::TangentComponent,
                                                "Slope component (atan)",
                                            );
                                        });

                                    ui.add_space(4.0);
                                    ui.label("Pitch scale:");
                                    ui.add(
                                        DragValue::new(&mut preset.pitch_multiplier)
                                            .speed(0.01)
                                            .range(0.0..=10.0),
                                    );
                                    if ui.small_button("Reset").clicked() {
                                        preset.pitch_multiplier = 1.0;
                                    }

                                    ui.add_space(4.0);
                                    ui.label("Pitch offset:");
                                    ui.add(
                                        DragValue::new(&mut preset.pitch_offset_degrees)
                                            .speed(1.0)
                                            .range(-180.0..=180.0)
                                            .suffix("°"),
                                    );
                                    if ui.small_button("Reset").clicked() {
                                        preset.pitch_offset_degrees = 0.0;
                                    }
                                });

                                ui.add_space(2.0);

                                // Row 6: Vertical projection scale, Target height, World height scale
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Vertical proj scale:");
                                    ui.add(
                                        DragValue::new(&mut preset.vertical_projection_multiplier)
                                            .speed(0.01)
                                            .range(0.0..=10.0),
                                    );
                                    if ui.small_button("Reset").clicked() {
                                        preset.vertical_projection_multiplier = 1.0;
                                    }

                                    ui.add_space(4.0);
                                    ui.label("Target height:");
                                    ui.add(
                                        DragValue::new(&mut preset.target_vertical_offset)
                                            .speed(1.0)
                                            .range(-10000.0..=10000.0),
                                    );

                                    ui.add_space(4.0);
                                    ui.label("Height scale:");
                                    ui.add(
                                        DragValue::new(&mut preset.height_scale)
                                            .speed(0.001)
                                            .range(0.0001..=100.0),
                                    );
                                    if ui.small_button("Reset").clicked() {
                                        preset.height_scale = 1.0;
                                    }
                                });

                                ui.add_space(2.0);

                                // Row 7: Screen offset
                                ui.horizontal(|ui| {
                                    ui.label("Screen offset:");
                                    ui.label("X");
                                    ui.add(DragValue::new(&mut preset.screen_offset_x).speed(1.0).range(-10000.0..=10000.0).suffix(" px"));
                                    ui.label("Y");
                                    ui.add(DragValue::new(&mut preset.screen_offset_y).speed(1.0).range(-10000.0..=10000.0).suffix(" px"));
                                    if ui.small_button("Reset").clicked() {
                                        preset.screen_offset_x = 0.0;
                                        preset.screen_offset_y = 0.0;
                                    }
                                });
                            });

                        ui.add_space(4.0);

                        // --- BOX 2: Calibration & Combination Matrix ---
                        Frame::group(ui.style())
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .rounding(4.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Calibration & Helpers")
                                        .strong()
                                        .color(Color32::from_rgb(255, 200, 100)),
                                );
                                ui.add_space(2.0);

                                // Auto Calibration
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Auto Calib:");
                                    if ui.small_button("Apply suggested start").on_hover_text("Suggested: XY + vertical Z, yaw Degrees, pitch Radians, FOV 90. If sideways try ±90; if behind try 180.").clicked() {
                                        preset.yaw_unit = EspAngleUnit::Degrees;
                                        preset.pitch_unit = EspAngleUnit::Radians;
                                        preset.pitch_input = EspPitchInput::Angle;
                                        preset.pitch_multiplier = 1.0;
                                        preset.direction_multiplier = 1.0;
                                        preset.invert_camera_yaw = false;
                                        preset.invert_yaw = false;
                                        preset.invert_pitch = false;
                                        preset.yaw_offset_degrees = 0.0;
                                        preset.pitch_offset_degrees = 0.0;
                                        preset.target_vertical_offset = 0.0;
                                        preset.screen_offset_x = 0.0;
                                        preset.screen_offset_y = 0.0;
                                        preset.horizontal_fov = 90.0;
                                        preset.vertical_projection_multiplier = 1.0;
                                    }
                                    if ui.button("Capture direction").clicked() {
                                        self.esp_calibration_feedback
                                            .insert(preset.id, "Capturing direction...".to_owned());
                                        if self
                                            .overlay_tx
                                            .send(crate::overlay::OverlayCommand::CaptureEspCalibration(
                                                preset.clone(),
                                            ))
                                            .is_err()
                                        {
                                            self.esp_calibration_feedback.insert(
                                                preset.id,
                                                "Calibration unavailable: overlay worker stopped".to_owned(),
                                            );
                                        }
                                    }
                                    if ui.small_button("Clear captures").clicked() {
                                        self.esp_calibration_feedback
                                            .insert(preset.id, "Clearing captures...".to_owned());
                                        let _ = self
                                            .overlay_tx
                                            .send(crate::overlay::OverlayCommand::ClearEspCalibration(preset.id));
                                    }
                                    if let Some(feedback) = &calibration_feedback {
                                        ui.label(RichText::new(feedback).color(ui.visuals().weak_text_color()));
                                    }
                                });

                                ui.add_space(2.0);

                                // Combination Matrix
                                ui.horizontal_wrapped(|ui| {
                                    ui.checkbox(
                                        &mut preset.permutation_debug_mode,
                                        "Render combination matrix (Single target)",
                                    )
                                    .on_hover_text(
                                        "Simultaneously renders combination matrix with numbered labels. Look at target in-game to see which # lands on it, then apply! Tip: Tilt view slightly up/down in-game if numbers overlap at neutral pitch.",
                                    );

                                    if preset.permutation_debug_mode {
                                        let perms = crate::model::esp_debug_permutations();
                                        let selected_perm = self.esp_selected_permutation.entry(preset.id).or_insert(1);
                                        ui.label("Pick #:");
                                        ui.add(
                                            DragValue::new(selected_perm)
                                                .range(1..=perms.len())
                                                .speed(1.0),
                                        );
                                        if let Some(target_cfg) = perms.iter().find(|c| c.index == *selected_perm) {
                                            ui.label(
                                                RichText::new(&target_cfg.short_desc)
                                                    .color(Color32::from_rgb(
                                                        target_cfg.color[0],
                                                        target_cfg.color[1],
                                                        target_cfg.color[2],
                                                    ))
                                                    .strong(),
                                            );
                                            if ui
                                                .button(format!("Apply #{}", target_cfg.index))
                                                .clicked()
                                            {
                                                preset.swap_direction_pair = target_cfg.swap_direction_pair;
                                                preset.invert_direction_a = target_cfg.invert_direction_a;
                                                preset.invert_direction_b = target_cfg.invert_direction_b;
                                                preset.invert_camera_yaw = target_cfg.invert_camera_yaw;
                                                preset.yaw_offset_degrees = target_cfg.yaw_offset_degrees;
                                                preset.invert_camera_pitch = target_cfg.invert_camera_pitch;
                                                preset.invert_vertical = target_cfg.invert_vertical;
                                                preset.pitch_input = target_cfg.pitch_input;
                                                preset.pitch_unit = target_cfg.pitch_unit;
                                                preset.invert_yaw = false;
                                                preset.invert_pitch = false;
                                                preset.permutation_debug_mode = false;
                                            }
                                        }
                                    }
                                });
                            });
                    });

                    // --- Right Column: Box 3 (Marker & Styling) + Box 4 (Display & Overlay) + Box 5 (Spatial Target Audio) ---
                    columns[1].vertical(|ui| {
                        // --- BOX 3: Marker & Styling ---
                        Frame::group(ui.style())
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .rounding(4.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Marker & Styling")
                                        .strong()
                                        .color(Color32::from_rgb(180, 140, 255)),
                                );
                                ui.add_space(2.0);

                                // Marker source & Shape settings
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Source:");
                                    ComboBox::from_id_salt(("esp_marker_source", preset.id))
                                        .selected_text(match preset.marker_source {
                                            EspMarkerSource::Geometry => "Geometry",
                                            EspMarkerSource::Text => "Text",
                                            EspMarkerSource::Svg => "SVG",
                                            EspMarkerSource::Image => "Image",
                                            EspMarkerSource::None => "None",
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut preset.marker_source, EspMarkerSource::Geometry, "Geometry");
                                            ui.selectable_value(&mut preset.marker_source, EspMarkerSource::Text, "Text");
                                            ui.selectable_value(&mut preset.marker_source, EspMarkerSource::Svg, "SVG");
                                            ui.selectable_value(&mut preset.marker_source, EspMarkerSource::Image, "Image");
                                            ui.selectable_value(&mut preset.marker_source, EspMarkerSource::None, "None (Tracer / Audio only)");
                                        });

                                    if preset.marker_source == EspMarkerSource::Geometry {
                                        ui.add_space(4.0);
                                        ui.label("Shape:");
                                        ComboBox::from_id_salt(("esp_marker", preset.id))
                                            .selected_text(match preset.marker {
                                                EspMarkerKind::Dot => "Dot",
                                                EspMarkerKind::Box => "Box",
                                                EspMarkerKind::None => "None",
                                            })
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut preset.marker, EspMarkerKind::Dot, "Dot");
                                                ui.selectable_value(&mut preset.marker, EspMarkerKind::Box, "Box");
                                                ui.selectable_value(&mut preset.marker, EspMarkerKind::None, "None");
                                            });
                                        match preset.marker {
                                            EspMarkerKind::Dot => {
                                                ui.label("Radius:");
                                                ui.add(DragValue::new(&mut preset.dot_radius).speed(1.0).range(1.0..=100.0));
                                            }
                                            EspMarkerKind::Box => {
                                                ui.label("W:");
                                                ui.add(DragValue::new(&mut preset.box_width).speed(1.0).range(2.0..=1000.0));
                                                ui.label("H:");
                                                ui.add(DragValue::new(&mut preset.box_height).speed(1.0).range(2.0..=1000.0));
                                            }
                                            EspMarkerKind::None => {}
                                        }
                                        if preset.marker != EspMarkerKind::None {
                                            ui.label("Thick:");
                                            ui.add(DragValue::new(&mut preset.thickness).speed(1.0).range(1.0..=30.0));
                                            ui.checkbox(&mut preset.filled, "Fill");
                                        }
                                    } else if preset.marker_source == EspMarkerSource::Text {
                                        ui.add_space(4.0);
                                        ui.label("Off X:");
                                        ui.add(DragValue::new(&mut preset.text_offset_x).speed(1.0));
                                        ui.label("Off Y:");
                                        ui.add(DragValue::new(&mut preset.text_offset_y).speed(1.0));
                                        ui.label("Size:");
                                        ui.add(DragValue::new(&mut preset.text_font_size).speed(1.0).range(8.0..=256.0));
                                        ui.label("Opacity:");
                                        ui.add(DragValue::new(&mut preset.text_opacity).speed(0.01).range(0.0..=1.0));
                                    } else {
                                        ui.add_space(4.0);
                                        let label = if preset.marker_source == EspMarkerSource::Svg { "Choose SVG" } else { "Import image" };
                                        if ui.button(label).clicked() {
                                            let mut dialog = rfd::FileDialog::new();
                                            dialog = if preset.marker_source == EspMarkerSource::Svg {
                                                dialog.add_filter("SVG", &["svg"])
                                            } else {
                                                dialog.add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "ico"])
                                            };
                                            if let Some(path) = dialog.pick_file() {
                                                let path = path.to_string_lossy().into_owned();
                                                if preset.marker_source == EspMarkerSource::Svg {
                                                    preset.marker_svg_source = path;
                                                } else {
                                                    preset.marker_asset_path = path;
                                                }
                                            }
                                        }
                                        if preset.marker_source == EspMarkerSource::Image {
                                            let hint = RichText::new("Image file").color(ui.visuals().weak_text_color());
                                            let width = (ui.available_width() - 8.0).max(60.0);
                                            ui.add_sized([width, 21.0], TextEdit::singleline(&mut preset.marker_asset_path).hint_text(hint));
                                        }
                                        if preset.marker_source == EspMarkerSource::Svg {
                                            ui.label("W:");
                                            ui.add(DragValue::new(&mut preset.svg_width).speed(1.0).range(2.0..=1000.0));
                                            ui.label("H:");
                                            ui.add(DragValue::new(&mut preset.svg_height).speed(1.0).range(2.0..=1000.0));
                                        } else {
                                            ui.label("W:");
                                            ui.add(DragValue::new(&mut preset.image_width).speed(1.0).range(2.0..=1000.0));
                                            ui.label("H:");
                                            ui.add(DragValue::new(&mut preset.image_height).speed(1.0).range(2.0..=1000.0));
                                        }
                                        ui.checkbox(&mut preset.marker_billboard_3d, "Billboard 3D");
                                    }
                                });

                                // Text content editor (if Text source)
                                if preset.marker_source == EspMarkerSource::Text {
                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        ui.label("Text:");
                                        let text_id = ui.make_persistent_id(("esp_marker_text", preset.id));
                                        let text_width = (ui.available_width() - 8.0).max(60.0);
                                        Self::render_interpolated_text_edit(
                                            ui,
                                            &mut preset.marker_text,
                                            text_id,
                                            text_width,
                                            text_width,
                                            21.0,
                                            54.0,
                                            "Text with {variable}, e.g. Hunter: {health}",
                                            true,
                                        );
                                    });
                                }

                                // SVG source editor (if SVG source)
                                if preset.marker_source == EspMarkerSource::Svg {
                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        ui.label("SVG:");
                                        let hint = RichText::new("Paste <svg ...>...</svg> here")
                                            .color(ui.visuals().weak_text_color());
                                        egui::ScrollArea::vertical()
                                            .id_salt(("esp_svg_source_scroll", preset.id))
                                            .max_height(54.0)
                                            .show(ui, |ui| {
                                                ui.add_sized(
                                                    [(ui.available_width() - 8.0).max(60.0), 54.0],
                                                    TextEdit::multiline(&mut preset.marker_svg_source)
                                                        .desired_rows(2)
                                                        .hint_text(hint),
                                                );
                                            });
                                    });
                                }

                                ui.add_space(2.0);

                                // Marker screen offset & Distance scaling
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Marker off:");
                                    ui.label("X");
                                    ui.add(DragValue::new(&mut preset.marker_offset_x).speed(1.0).range(-10000.0..=10000.0).suffix(" px"));
                                    ui.label("Y");
                                    ui.add(DragValue::new(&mut preset.marker_offset_y).speed(1.0).range(-10000.0..=10000.0).suffix(" px"));
                                    if ui.small_button("Reset").clicked() {
                                        preset.marker_offset_x = 0.0;
                                        preset.marker_offset_y = 0.0;
                                    }

                                    ui.add_space(4.0);
                                    ui.checkbox(&mut preset.scale_with_distance, "Dist Scale");
                                    if preset.scale_with_distance {
                                        ui.label("Ref:");
                                        ui.add(DragValue::new(&mut preset.distance_reference).speed(1.0).range(0.01..=1_000_000.0));
                                        ui.label("Str:");
                                        ui.add(
                                            DragValue::new(&mut preset.distance_scale_strength_percent)
                                                .speed(1.0)
                                                .range(0.0..=100.0)
                                                .suffix("%"),
                                        );
                                        ui.label("Offset:");
                                        ui.add(DragValue::new(&mut preset.marker_size_offset_percent).speed(1.0).range(-95.0..=1000.0).suffix("%"));
                                    }
                                });
                            });

                        ui.add_space(4.0);

                        // --- BOX 4: Display & Overlay Effects ---
                        Frame::group(ui.style())
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .rounding(4.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Display & Overlay")
                                        .strong()
                                        .color(Color32::from_rgb(120, 230, 180)),
                                );
                                ui.add_space(2.0);

                                ui.horizontal_wrapped(|ui| {
                                    let mut color = Color32::from_rgba_unmultiplied(
                                        preset.color.r, preset.color.g, preset.color.b, preset.color.a,
                                    );
                                    ui.label("Color:");
                                    if ui.color_edit_button_srgba(&mut color).changed() {
                                        preset.color = RgbaColor { r: color.r(), g: color.g(), b: color.b(), a: color.a() };
                                    }

                                    ui.add_space(4.0);
                                    ui.checkbox(&mut preset.show_tracer, "Tracer");
                                    if preset.show_tracer {
                                        ui.checkbox(&mut preset.tracer_from_top, "From Top");
                                        ui.checkbox(&mut preset.clamp_offscreen_tracer, "Clamp off-screen");
                                    }

                                    ui.add_space(4.0);
                                    ui.checkbox(&mut preset.show_distance, "Distance");

                                    ui.add_space(4.0);
                                    ui.checkbox(&mut preset.debug_mode, "Debug Mode");

                                    ui.add_space(4.0);
                                    ui.label("Update:");
                                    ui.add(DragValue::new(&mut preset.update_interval_ms).speed(1.0).range(1..=1000).suffix(" ms"));

                                    ui.add_space(4.0);
                                    let mut smooth_enabled = preset.motion_smoothing_ms > 0;
                                    if ui.checkbox(&mut smooth_enabled, "Smooth").clicked() {
                                        preset.motion_smoothing_ms = if smooth_enabled { 1 } else { 0 };
                                    }
                                    if smooth_enabled {
                                        ui.add(
                                            DragValue::new(&mut preset.motion_smoothing_ms)
                                                .speed(1.0)
                                                .range(1..=500)
                                                .suffix(" ms"),
                                        );
                                    }
                                });
                            });

                        ui.add_space(4.0);

                        // --- BOX 5: Spatial Target Audio ---
                        Frame::group(ui.style())
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .rounding(4.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Spatial Target Audio")
                                        .strong()
                                        .color(Color32::from_rgb(255, 150, 150)),
                                );
                                ui.add_space(2.0);

                                ui.horizontal_wrapped(|ui| {
                                    ui.checkbox(&mut preset.target_audio_enabled, "Enable sound");

                                    if preset.target_audio_enabled {
                                        if ui.button("Choose sound").clicked()
                                            && let Some(path) = rfd::FileDialog::new()
                                                .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "m4a", "aac"])
                                                .pick_file()
                                        {
                                            preset.target_audio_path = path.to_string_lossy().into_owned();
                                        }
                                        let hint = RichText::new("Audio file").color(ui.visuals().weak_text_color());
                                        let audio_width = (ui.available_width() - 70.0).max(60.0);
                                        ui.add_sized([audio_width, 21.0], TextEdit::singleline(&mut preset.target_audio_path).hint_text(hint));
                                        ui.checkbox(&mut preset.target_audio_loop, "Loop");
                                    }
                                });

                                if preset.target_audio_enabled {
                                    ui.add_space(2.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Volume:");
                                        ui.add(DragValue::new(&mut preset.target_audio_volume).speed(0.01).range(0.0..=2.0));
                                        ui.add_space(4.0);
                                        ui.label("Full vol in:");
                                        ui.add(DragValue::new(&mut preset.target_audio_full_volume_distance).speed(1.0).range(0.0..=1_000_000.0));
                                        ui.add_space(4.0);
                                        ui.label("Silent after:");
                                        ui.add(DragValue::new(&mut preset.target_audio_max_distance).speed(1.0).range(0.01..=1_000_000.0));
                                    });
                                }
                            });
                    });
                });
            });
            if copy_preset_index == Some(index) {
                self.preset_clipboard = Some(crate::ui::PresetClipboard::Esp(preset.clone()));
            }
            if migrated_marker_source || preset != before {
                self.state.esp_presets[index] = preset;
                dirty = true;
            }
        }
        if let Some(id) = remove {
            #[cfg(windows)]
            if self
                .esp_entity_root_capture
                .as_ref()
                .is_some_and(|capture| capture.preset_id == id)
            {
                self.stop_esp_entity_root_capture(None);
            }
            self.state.esp_presets.retain(|preset| preset.id != id);
            self.esp_calibration_feedback.remove(&id);
            self.esp_entity_capture_feedback.remove(&id);
            dirty = true;
        }

        if let Some(index) = paste_after
            && let Some(crate::ui::PresetClipboard::Esp(mut preset)) = self.preset_clipboard.clone()
        {
            preset.id = Self::allocate_next_id(
                &self.state.esp_presets,
                &mut self.state.next_esp_preset_id,
                |item| item.id,
            );
            preset.name = format!("{} (Copy)", preset.name);
            self.state.esp_presets.insert(index + 1, preset);
            dirty = true;
        }
        if dirty {
            self.persist_esp_presets();
        }
        #[cfg(windows)]
        if let Some(id) = auto_capture_stop {
            self.stop_esp_entity_root_capture(Some("Stopped"));
            self.esp_entity_capture_feedback
                .insert(id, "Stopped".to_owned());
        } else if let Some(id) = auto_capture_start {
            self.start_esp_entity_root_capture(id, None);
        }
    }
}

fn memory_type_name(value_type: MemoryValueType) -> &'static str {
    match value_type {
        MemoryValueType::I8 => "Byte (1 Byte)",
        MemoryValueType::I16 => "2 Bytes",
        MemoryValueType::I32 => "4 Bytes",
        MemoryValueType::F32 => "Float (4 Bytes)",
        MemoryValueType::I64 => "8 Bytes",
        MemoryValueType::F64 => "Double (8 Bytes)",
    }
}

fn memory_expression_row(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(
        TextEdit::singleline(value).desired_width(420.0).hint_text(
            RichText::new("address / module+offset [offsets] / @alias")
                .color(ui.visuals().weak_text_color()),
        ),
    );
    ui.end_row();
}

fn angle_unit(ui: &mut egui::Ui, label: &str, id: u32, unit: &mut EspAngleUnit) {
    ui.label(label);
    ComboBox::from_id_salt(("esp_angle", label, id))
        .selected_text(match unit {
            EspAngleUnit::Degrees => "Degrees",
            EspAngleUnit::Radians => "Radians",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(unit, EspAngleUnit::Degrees, "Degrees");
            ui.selectable_value(unit, EspAngleUnit::Radians, "Radians");
        });
}

#[cfg(windows)]
fn find_nearest_entity_index(
    pid: u32,
    preset: &crate::model::EspPreset,
    addresses: &[usize],
) -> Option<usize> {
    if addresses.is_empty() {
        return None;
    }
    let cam_x = crate::overlay::evaluate_esp_expression_float(pid, &preset.camera_x, preset.value_type)?;
    let cam_y = crate::overlay::evaluate_esp_expression_float(pid, &preset.camera_y, preset.value_type)?;
    let cam_z = crate::overlay::evaluate_esp_expression_float(pid, &preset.camera_z, preset.value_type)?;

    let mut min_dist_sq = f32::MAX;
    let mut min_index = None;

    for (index, &entity_address) in addresses.iter().enumerate() {
        let Some(x_address) =
            crate::model::entity_field_address(entity_address, 0, 1, preset.entity_x_offset)
        else {
            continue;
        };
        let Some(y_address) =
            crate::model::entity_field_address(entity_address, 0, 1, preset.entity_y_offset)
        else {
            continue;
        };
        let Some(z_address) =
            crate::model::entity_field_address(entity_address, 0, 1, preset.entity_z_offset)
        else {
            continue;
        };

        let mut read_comp = |addr: usize| -> Option<f32> {
            let first = read_esp_f32_from_address(pid, addr, preset.value_type)?;
            if !preset.entity_aabb_center {
                return Some(first);
            }
            let second_addr = crate::model::entity_field_address(
                addr,
                0,
                1,
                preset.entity_aabb_pair_offset,
            )?;
            let second = read_esp_f32_from_address(pid, second_addr, preset.value_type)?;
            Some(crate::model::aabb_center_component(first, second))
        };

        let Some(x) = read_comp(x_address) else {
            continue;
        };
        let Some(y) = read_comp(y_address) else {
            continue;
        };
        let Some(z) = read_comp(z_address) else {
            continue;
        };

        let dx = x - cam_x;
        let dy = y - cam_y;
        let dz = z - cam_z;
        let dist_sq = dx * dx + dy * dy + dz * dz;

        if dist_sq < min_dist_sq {
            min_dist_sq = dist_sq;
            min_index = Some(index);
        }
    }

    min_index
}

#[cfg(windows)]
fn merge_entity_addresses_by_3d_proximity(
    pid: u32,
    preset: &crate::model::EspPreset,
    addresses: &[usize],
) -> Vec<usize> {
    if addresses.is_empty() {
        return Vec::new();
    }

    struct Sample {
        address: usize,
        pos: Option<[f32; 3]>,
    }

    let mut samples: Vec<Sample> = Vec::with_capacity(addresses.len());

    for &entity_address in addresses {
        let Some(x_address) =
            crate::model::entity_field_address(entity_address, 0, 1, preset.entity_x_offset)
        else {
            samples.push(Sample {
                address: entity_address,
                pos: None,
            });
            continue;
        };
        let Some(y_address) =
            crate::model::entity_field_address(entity_address, 0, 1, preset.entity_y_offset)
        else {
            samples.push(Sample {
                address: entity_address,
                pos: None,
            });
            continue;
        };
        let Some(z_address) =
            crate::model::entity_field_address(entity_address, 0, 1, preset.entity_z_offset)
        else {
            samples.push(Sample {
                address: entity_address,
                pos: None,
            });
            continue;
        };

        let mut read_comp = |addr: usize| -> Option<f32> {
            let first = read_esp_f32_from_address(pid, addr, preset.value_type)?;
            if !preset.entity_aabb_center {
                return Some(first);
            }
            let second_addr = crate::model::entity_field_address(
                addr,
                0,
                1,
                preset.entity_aabb_pair_offset,
            )?;
            let second = read_esp_f32_from_address(pid, second_addr, preset.value_type)?;
            Some(crate::model::aabb_center_component(first, second))
        };

        let (Some(x), Some(y), Some(z)) = (read_comp(x_address), read_comp(y_address), read_comp(z_address)) else {
            samples.push(Sample {
                address: entity_address,
                pos: None,
            });
            continue;
        };

        samples.push(Sample {
            address: entity_address,
            pos: Some([x, y, z]),
        });
    }

    // Merge samples whose 3D distance is within 5.0 game units
    let mut merged: Vec<Sample> = Vec::with_capacity(samples.len());
    for sample in samples {
        let mut found = false;
        if let Some(pos) = sample.pos {
            for existing in &mut merged {
                if let Some(existing_pos) = existing.pos {
                    let dx = pos[0] - existing_pos[0];
                    let dy = pos[1] - existing_pos[1];
                    let dz = pos[2] - existing_pos[2];
                    let dist_sq = dx * dx + dy * dy + dz * dz;
                    if dist_sq <= 25.0 {
                        if sample.address < existing.address {
                            existing.address = sample.address;
                        }
                        found = true;
                        break;
                    }
                }
            }
        }
        if !found {
            merged.push(sample);
        }
    }

    merged.into_iter().map(|s| s.address).collect()
}

#[cfg(windows)]
fn read_esp_f32_from_address(
    pid: u32,
    address: usize,
    value_type: crate::model::MemoryValueType,
) -> Option<f32> {
    let width = match value_type {
        crate::model::MemoryValueType::I8 => 1,
        crate::model::MemoryValueType::I16 => 2,
        crate::model::MemoryValueType::I32 | crate::model::MemoryValueType::F32 => 4,
        crate::model::MemoryValueType::I64 | crate::model::MemoryValueType::F64 => 8,
    };
    let bytes = crate::process_memory::read_memory_bytes(pid, address, width).ok()?;
    match value_type {
        crate::model::MemoryValueType::I8 => bytes.first().map(|&b| b as i8 as f32),
        crate::model::MemoryValueType::I16 => {
            bytes.get(0..2).and_then(|b| b.try_into().ok()).map(|b| i16::from_le_bytes(b) as f32)
        }
        crate::model::MemoryValueType::I32 => {
            bytes.get(0..4).and_then(|b| b.try_into().ok()).map(|b| i32::from_le_bytes(b) as f32)
        }
        crate::model::MemoryValueType::F32 => {
            bytes.get(0..4).and_then(|b| b.try_into().ok()).map(f32::from_le_bytes)
        }
        crate::model::MemoryValueType::I64 => {
            bytes.get(0..8).and_then(|b| b.try_into().ok()).map(|b| i64::from_le_bytes(b) as f32)
        }
        crate::model::MemoryValueType::F64 => {
            bytes.get(0..8).and_then(|b| b.try_into().ok()).map(|b| f64::from_le_bytes(b) as f32)
        }
    }
}
