use eframe::egui::{self, Button, Color32, ComboBox, DragValue, Grid, RichText, TextEdit};

use crate::model::{
    EspAngleUnit, EspHorizontalPlane, EspMarkerKind, EspMarkerSource, EspOrientationSource,
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
    required: usize,
    hit_order: bool,
    addresses: Vec<usize>,
    rx: std::sync::mpsc::Receiver<WatchEvent>,
    active: Option<AccessWatch>,
    hud_preset_id: Option<u32>,
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
    pub(crate) fn stop_esp_entity_root_capture(&mut self, status: Option<&str>) {
        let Some(mut capture) = self.esp_entity_root_capture.take() else {
            return;
        };
        if let Some(mut active) = capture.active.take() {
            active.stop();
        }
        if capture.hud_preset_id.is_some() {
            let _ = self
                .overlay_tx
                .send(crate::overlay::OverlayCommand::PreviewHudPreset(Vec::new()));
        }
        self.esp_entity_capture_hud_hide_at = None;
        if let Some(status) = status {
            self.esp_entity_capture_feedback
                .insert(capture.preset_id, status.to_owned());
        }
    }

    #[cfg(windows)]
    fn start_esp_entity_root_capture(&mut self, preset_id: u32) {
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
                .insert(preset_id, "Select a Code-list instruction".to_owned());
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
        let hit_order = preset.entity_auto_hit_order;
        let (tx, rx) = std::sync::mpsc::channel();
        let started = AccessWatch::start_unique(
            pid,
            instruction_address,
            self.state.memory_debugger_architecture,
            if hit_order {
                required
            } else {
                // ponytail: bound menu/loading noise; raise this cap if a game legitimately
                // touches more unique entity addresses before the wanted group appears.
                ESP_ENTITY_ROOT_CAPTURE_LIMIT
            },
            move |event| {
                let _ = tx.send(event);
            },
        );
        match started {
            Ok(active) => {
                self.esp_entity_root_capture = Some(EspEntityRootCapture {
                    preset_id,
                    required,
                    hit_order,
                    addresses: Vec::with_capacity(required),
                    rx,
                    active: Some(active),
                    hud_preset_id,
                });
                self.esp_entity_capture_feedback
                    .insert(
                        preset_id,
                        format!(
                            "{} 0/{required}",
                            if hit_order { "Captured" } else { "Matched" }
                        ),
                    );
                self.esp_entity_capture_hud_hide_at = None;
                self.show_esp_entity_capture_hud(
                    hud_preset_id,
                    format!("Entity scan 0/{required}"),
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
                WatchEvent::AccessHit { data_address } => {
                    capture.addresses.push(data_address);
                    changed = true;
                }
                WatchEvent::CaptureLimitReached(limit) => {
                    if !capture.hit_order {
                        stopped = Some(format!(
                            "Stopped after {limit} unique addresses without a matching Stride group"
                        ));
                    }
                }
                WatchEvent::Error(error) => stopped = Some(format!("Debugger stopped: {error}")),
                WatchEvent::Stopped if stopped.is_none() => {
                    stopped = Some(if capture.hit_order {
                        "Debugger stopped before enough unique addresses were captured".to_owned()
                    } else {
                        "Debugger stopped before a complete Stride group was found".to_owned()
                    })
                }
                WatchEvent::Stopped | WatchEvent::AddressHit { .. } => {}
            }
        }
        let ordered = capture
            .hit_order
            .then(|| {
                crate::model::entity_hits_in_capture_order(
                    &capture.addresses,
                    capture.required as u32,
                )
            })
            .flatten();
        let (candidate, matched) = if capture.hit_order {
            (
                ordered
                    .as_ref()
                    .and_then(|addresses| addresses.first())
                    .copied(),
                capture.addresses.len().min(capture.required),
            )
        } else {
            self.state
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
                .unwrap_or((None, 0))
        };
        if changed {
            self.esp_entity_capture_feedback.insert(
                capture.preset_id,
                format!(
                    "{} {matched}/{}",
                    if capture.hit_order { "Captured" } else { "Matched" },
                    capture.required
                ),
            );
            self.show_esp_entity_capture_hud(
                capture.hud_preset_id,
                format!("Entity scan {matched}/{}", capture.required),
            );
        }
        if let Some(candidate) = candidate {
            if let Some(mut active) = capture.active.take() {
                active.stop();
            }
            if capture.hit_order && let Some(addresses) = ordered {
                if let Some(preset) = self
                    .state
                    .esp_presets
                    .iter_mut()
                    .find(|preset| preset.id == capture.preset_id)
                {
                    preset.entity_root = format!("0x{candidate:X}");
                    preset.entity_hit_order_addresses = addresses;
                    preset.entity_count = capture.required as u32;
                    preset.entity_list_enabled = true;
                }
                self.esp_entity_capture_feedback.insert(
                    capture.preset_id,
                    format!("Captured {} entities in hit order", capture.required),
                );
                self.show_esp_entity_capture_hud(
                    capture.hud_preset_id,
                    format!("Entity scan {}/{} - order saved", capture.required, capture.required),
                );
                if capture.hud_preset_id.is_some() {
                    self.esp_entity_capture_hud_hide_at =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                }
                self.persist_esp_presets();
                return;
            }
            let result = self
                .state
                .esp_presets
                .iter()
                .find(|preset| preset.id == capture.preset_id)
                .map(|preset| {
                    crate::model::entity_root_from_instruction_hits(
                        &capture.addresses,
                        capture.required as u32,
                        preset.entity_stride,
                    )
                });
            match result {
                Some(Ok(root)) => {
                    if let Some(preset) = self
                        .state
                        .esp_presets
                        .iter_mut()
                        .find(|preset| preset.id == capture.preset_id)
                    {
                        preset.entity_root = format!("0x{root:X}");
                        preset.entity_list_enabled = true;
                    }
                    self.esp_entity_capture_feedback.insert(
                        capture.preset_id,
                        format!("Root updated: 0x{root:X}"),
                    );
                    self.show_esp_entity_capture_hud(
                        capture.hud_preset_id,
                        format!("Entity scan {}/{} - root found", capture.required, capture.required),
                    );
                    if capture.hud_preset_id.is_some() {
                        self.esp_entity_capture_hud_hide_at =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                    }
                    self.persist_esp_presets();
                }
                Some(Err(error)) => {
                    self.esp_entity_capture_feedback.insert(
                        capture.preset_id,
                        format!("Matched {matched}/{}, but {error}", capture.required),
                    );
                    if capture.hud_preset_id.is_some() {
                        let _ = self
                            .overlay_tx
                            .send(crate::overlay::OverlayCommand::PreviewHudPreset(Vec::new()));
                    }
                }
                None => {}
            }
            return;
        }
        if let Some(status) = stopped {
            if let Some(mut active) = capture.active.take() {
                active.stop();
            }
            self.esp_entity_capture_feedback
                .insert(capture.preset_id, status);
            if capture.hud_preset_id.is_some() {
                let _ = self
                    .overlay_tx
                    .send(crate::overlay::OverlayCommand::PreviewHudPreset(Vec::new()));
            }
            return;
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
        let mut copy_preset = None;
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
            let snapshot = preset.clone();
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
                            copy_preset = Some(snapshot.clone());
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
                        let target_label = matched_window
                            .as_ref()
                            .map(|window| format!("{} [PID {}]", window.title, window.process_id))
                            .unwrap_or_else(|| {
                                if preset.target_window.trim().is_empty() {
                                    "Select window".to_string()
                                } else {
                                    "Window is not running".to_string()
                                }
                            });
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
                                    .on_hover_text("Replace the raw root address with root - Step")
                                    .clicked()
                                {
                                    if let Some(root) = crate::model::shift_raw_entity_root(
                                        &preset.entity_root,
                                        root_step,
                                        -1,
                                    ) {
                                        preset.entity_root = root;
                                    }
                                }
                                if ui
                                    .add_enabled(raw_root, egui::Button::new("▼"))
                                    .on_hover_text("Replace the raw root address with root + Step")
                                    .clicked()
                                {
                                    if let Some(root) = crate::model::shift_raw_entity_root(
                                        &preset.entity_root,
                                        root_step,
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
                            ui.horizontal_wrapped(|ui| {
                                let selected_code = self
                                    .state
                                    .memory_code_list
                                    .iter()
                                    .find(|code| {
                                        code.module.eq_ignore_ascii_case(
                                            &preset.entity_auto_code_module,
                                        ) && code.offset == preset.entity_auto_code_offset
                                    });
                                ComboBox::from_id_salt(("esp_auto_root_code", preset.id))
                                    .selected_text(selected_code.map_or(
                                        "Select instruction".to_owned(),
                                        |code| {
                                            format!(
                                                "{}+{:X}  {}",
                                                code.module, code.offset, code.instruction
                                            )
                                        },
                                    ))
                                    .width(280.0)
                                    .show_ui(ui, |ui| {
                                        for code in &self.state.memory_code_list {
                                            let selected = code.module.eq_ignore_ascii_case(
                                                &preset.entity_auto_code_module,
                                            ) && code.offset == preset.entity_auto_code_offset;
                                            if ui
                                                .selectable_label(
                                                    selected,
                                                    format!(
                                                        "{}+{:X}  {}",
                                                        code.module, code.offset, code.instruction
                                                    ),
                                                )
                                                .clicked()
                                            {
                                                preset.entity_auto_code_module =
                                                    code.module.clone();
                                                preset.entity_auto_code_offset = code.offset;
                                            }
                                        }
                                    });
                                ui.checkbox(&mut preset.entity_auto_hit_order, "Hit order")
                                    .on_hover_text(
                                        "Keep unique instruction addresses in first-hit order; ignore Stride grouping.",
                                    );
                                ui.label("Need");
                                ui.add(
                                    DragValue::new(&mut preset.entity_auto_capture_count)
                                        .range(1..=512),
                                )
                                .on_hover_text(if preset.entity_auto_hit_order {
                                    "Stop after this many unique addresses are captured."
                                } else {
                                    "Stop only after this many addresses form one group at the configured Stride."
                                });
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
                                        .width(120.0)
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
                        for (label, value) in [
                            ("Camera X", &mut preset.camera_x),
                            ("Camera Y", &mut preset.camera_y),
                            ("Camera Z", &mut preset.camera_z),
                        ] {
                            ui.label(label);
                            ui.add(
                                TextEdit::singleline(value).desired_width(420.0).hint_text(
                                    RichText::new("address / module+offset [offsets] / @alias")
                                        .color(ui.visuals().weak_text_color()),
                                ),
                            );
                            ui.end_row();
                        }
                        ui.label("Orientation source");
                        ComboBox::from_id_salt(("esp_orientation_source", preset.id))
                            .selected_text(match preset.orientation_source {
                                EspOrientationSource::Angles => "Yaw + pitch angles",
                                EspOrientationSource::DirectionPairPitch => {
                                    "Horizontal direction pair + pitch"
                                }
                            })
                            .width(240.0)
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
                                    memory_expression_row(ui, label, value);
                                }
                            }
                            EspOrientationSource::DirectionPairPitch => {
                                for (label, value) in [
                                    ("Camera direction A", &mut preset.camera_direction_a),
                                    ("Camera pitch", &mut preset.camera_pitch),
                                    ("Camera direction B", &mut preset.camera_direction_b),
                                ] {
                                    memory_expression_row(ui, label, value);
                                }
                            }
                        }
                    });

                Grid::new(("esp_settings", preset.id))
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        // --- Value type & Height axis ---
                        ui.label("Value type");
                        ui.horizontal(|ui| {
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
                            ui.label("Height axis");
                            ComboBox::from_id_salt(("esp_plane", preset.id))
                                .selected_text(match preset.horizontal_plane {
                                    EspHorizontalPlane::Xz => "Y is Height (NeoX / IdentityV)",
                                    EspHorizontalPlane::Xy => "Z is Height (Unreal)",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut preset.horizontal_plane,
                                        EspHorizontalPlane::Xz,
                                        "Y is Height (NeoX / IdentityV)",
                                    );
                                    ui.selectable_value(
                                        &mut preset.horizontal_plane,
                                        EspHorizontalPlane::Xy,
                                        "Z is Height (Unreal)",
                                    );
                                });
                        });
                        ui.end_row();

                        // --- Angle / Orientation options ---
                        ui.label("Angle options");
                        ui.horizontal_wrapped(|ui| {
                            if preset.orientation_source == EspOrientationSource::Angles {
                                angle_unit(ui, "Yaw", preset.id, &mut preset.yaw_unit);
                                angle_unit(ui, "Pitch", preset.id, &mut preset.pitch_unit);
                            } else {
                                ui.label("Yaw = atan2(Direction B, Direction A)");
                                angle_unit(ui, "Pitch", preset.id, &mut preset.pitch_unit);
                                ui.checkbox(&mut preset.swap_direction_pair, "Swap direction A/B");
                                ui.checkbox(&mut preset.invert_direction_a, "Invert direction A");
                                ui.checkbox(&mut preset.invert_direction_b, "Invert direction B");
                            }
                            ui.checkbox(&mut preset.invert_camera_yaw, "Reverse yaw value")
                                .on_hover_text("Reverse only camera rotation. Use this when lateral movement is correct but rotating the camera moves ESP the wrong way.");
                            ui.checkbox(&mut preset.invert_camera_pitch, "Reverse pitch value")
                                .on_hover_text("Reverse only camera pitch angle. Use this when looking down with camera moves ESP the wrong way.");
                        });
                        ui.end_row();

                        // --- Invert / Mirror ---
                        ui.label("Invert / Mirror");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut preset.invert_vertical, "Invert elevation (height)")
                                .on_hover_text("Invert target elevation difference. Use this when moving player up/down moves ESP the wrong way.");
                            ui.checkbox(&mut preset.invert_yaw, "Mirror screen X")
                                .on_hover_text("Mirror only the final left/right screen position.");
                            ui.checkbox(&mut preset.invert_pitch, "Mirror screen Y")
                                .on_hover_text("Mirror only the final up/down screen position.");
                            ui.label("Horizontal FOV");
                            ui.add(DragValue::new(&mut preset.horizontal_fov).speed(1.0).range(1.0..=179.0));
                        });
                        ui.end_row();

                        // --- Yaw zero offset ---
                        ui.label("Yaw zero offset").on_hover_text("Use this when the marker is consistently rotated left/right. Try +90, -90, then 180.");
                        ui.horizontal(|ui| {
                            ui.add(
                                DragValue::new(&mut preset.yaw_offset_degrees)
                                    .speed(1.0)
                                    .range(-360.0..=360.0)
                                    .suffix(" deg"),
                            );
                            for value in [-180.0, -90.0, 0.0, 90.0, 180.0] {
                                if ui.small_button(format!("{value:+.0}")).clicked() {
                                    preset.yaw_offset_degrees = value;
                                }
                            }
                            ui.label("Direction B/A ratio").on_hover_text(
                                "Scales Direction B relative to A before atan2. Use this when horizontal ESP is correct near one direction but increasingly wrong while rotating.",
                            );
                            ui.add(
                                DragValue::new(&mut preset.direction_multiplier)
                                    .speed(0.001)
                                    .range(0.0001..=100.0),
                            );
                            if ui.small_button("Reset scale").clicked() {
                                preset.direction_multiplier = 1.0;
                            }
                        });
                        ui.end_row();

                        ui.label("Pitch input");
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
                        ui.end_row();

                        ui.label("Pitch scale").on_hover_text(
                            "Scale only camera pitch response; values below 1 slow vertical ESP movement without changing horizontal FOV.",
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                DragValue::new(&mut preset.pitch_multiplier)
                                    .speed(0.01)
                                    .range(0.0..=10.0),
                            );
                            if ui.small_button("Reset scale").clicked() {
                                preset.pitch_multiplier = 1.0;
                            }
                        });
                        ui.end_row();

                        ui.label("Vertical projection scale").on_hover_text(
                            "Scales only final screen Y. Lower this when every vertical movement is too large while horizontal alignment is already correct.",
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                DragValue::new(&mut preset.vertical_projection_multiplier)
                                    .speed(0.01)
                                    .range(0.0..=10.0),
                            );
                            if ui.small_button("Reset vertical").clicked() {
                                preset.vertical_projection_multiplier = 1.0;
                            }
                        });
                        ui.end_row();

                        // --- Pitch zero offset ---
                        ui.label("Pitch zero offset").on_hover_text("Use only when every marker is consistently too high/low as the camera tilts.");
                        ui.horizontal(|ui| {
                            ui.add(
                                DragValue::new(&mut preset.pitch_offset_degrees)
                                    .speed(1.0)
                                    .range(-180.0..=180.0)
                                    .suffix(" deg"),
                            );
                            if ui.small_button("Reset pitch").clicked() {
                                preset.pitch_offset_degrees = 0.0;
                            }
                            ui.label("Target height").on_hover_text("World-unit correction for a target pivot at feet/waist. Start at 0; adjust only after yaw is correct.");
                            ui.add(
                                DragValue::new(&mut preset.target_vertical_offset)
                                    .speed(1.0)
                                    .range(-10000.0..=10000.0),
                            );
                            ui.label("World height scale").on_hover_text(
                                "Scales Target height - Camera height in world coordinates. It does not change camera pitch response.",
                            );
                            ui.add(
                                DragValue::new(&mut preset.height_scale)
                                    .speed(0.001)
                                    .range(0.0001..=100.0),
                            );
                            if ui.small_button("Reset height scale").clicked() {
                                preset.height_scale = 1.0;
                            }
                        });
                        ui.end_row();

                        // --- Screen offset ---
                        ui.label("Screen offset").on_hover_text("Final pixel correction. It does not fix a wrong axis or angle convention.");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(DragValue::new(&mut preset.screen_offset_x).speed(1.0).range(-10000.0..=10000.0).suffix(" px"));
                            ui.label("Y");
                            ui.add(DragValue::new(&mut preset.screen_offset_y).speed(1.0).range(-10000.0..=10000.0).suffix(" px"));
                            if ui.small_button("Reset screen").clicked() {
                                preset.screen_offset_x = 0.0;
                                preset.screen_offset_y = 0.0;
                            }
                        });
                        ui.end_row();

                        // --- Calibration ---
                        ui.label("Calibration").on_hover_text("Auto calibration: stand on four different sides, aim the screen center at the target, then capture once per side.");
                        ui.horizontal(|ui| {
                            if ui.small_button("Apply suggested start").on_hover_text("Suggested: XY + vertical Z, yaw Degrees, pitch Radians, FOV 90. If sideways try ±90; if behind try 180.").clicked() {
                                preset.horizontal_plane = EspHorizontalPlane::Xy;
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
                        ui.end_row();

                        // --- Marker ---
                        ui.label("Marker");
                        ui.horizontal_wrapped(|ui| {
                            ComboBox::from_id_salt(("esp_marker_source", preset.id))
                                .selected_text(match preset.marker_source {
                                    EspMarkerSource::Geometry => "Geometry",
                                    EspMarkerSource::Text => "Text",
                                    EspMarkerSource::Svg => "SVG",
                                    EspMarkerSource::Image => "Image",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut preset.marker_source, EspMarkerSource::Geometry, "Geometry");
                                    ui.selectable_value(&mut preset.marker_source, EspMarkerSource::Text, "Text");
                                    ui.selectable_value(&mut preset.marker_source, EspMarkerSource::Svg, "SVG");
                                    ui.selectable_value(&mut preset.marker_source, EspMarkerSource::Image, "Image");
                                });
                            if preset.marker_source == EspMarkerSource::Geometry {
                                ComboBox::from_id_salt(("esp_marker", preset.id))
                                    .selected_text(match preset.marker {
                                        EspMarkerKind::Dot => "Dot",
                                        EspMarkerKind::Box => "Box",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut preset.marker, EspMarkerKind::Dot, "Dot");
                                        ui.selectable_value(&mut preset.marker, EspMarkerKind::Box, "Box");
                                    });
                                match preset.marker {
                                    EspMarkerKind::Dot => {
                                        ui.label("Radius");
                                        ui.add(DragValue::new(&mut preset.dot_radius).speed(1.0).range(1.0..=100.0));
                                    }
                                    EspMarkerKind::Box => {
                                        ui.label("Width");
                                        ui.add(DragValue::new(&mut preset.box_width).speed(1.0).range(2.0..=1000.0));
                                        ui.label("Height");
                                        ui.add(DragValue::new(&mut preset.box_height).speed(1.0).range(2.0..=1000.0));
                                    }
                                }
                                ui.label("Thickness");
                                ui.add(DragValue::new(&mut preset.thickness).speed(1.0).range(1.0..=30.0));
                                ui.checkbox(&mut preset.filled, "Fill");
                            } else if preset.marker_source == EspMarkerSource::Text {
                                ui.label("Offset X");
                                ui.add(DragValue::new(&mut preset.text_offset_x).speed(1.0));
                                ui.label("Offset Y");
                                ui.add(DragValue::new(&mut preset.text_offset_y).speed(1.0));
                                ui.label("Size");
                                ui.add(DragValue::new(&mut preset.text_font_size).speed(1.0).range(8.0..=256.0));
                                ui.label("Opacity");
                                ui.add(DragValue::new(&mut preset.text_opacity).speed(0.01).range(0.0..=1.0));
                            } else {
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
                                    ui.add_sized([260.0, 21.0], TextEdit::singleline(&mut preset.marker_asset_path).hint_text(hint));
                                }
                                if preset.marker_source == EspMarkerSource::Svg {
                                    ui.label("SVG width");
                                    ui.add(DragValue::new(&mut preset.svg_width).speed(1.0).range(2.0..=1000.0));
                                    ui.label("SVG height");
                                    ui.add(DragValue::new(&mut preset.svg_height).speed(1.0).range(2.0..=1000.0));
                                } else {
                                    ui.label("Image width");
                                    ui.add(DragValue::new(&mut preset.image_width).speed(1.0).range(2.0..=1000.0));
                                    ui.label("Image height");
                                    ui.add(DragValue::new(&mut preset.image_height).speed(1.0).range(2.0..=1000.0));
                                }
                                ui.checkbox(&mut preset.marker_billboard_3d, "World-space billboard")
                                    .on_hover_text("Keep the sprite facing the camera and scale it by world distance. A billboard intentionally stays flat to the viewer; perspective size is its visible 3D effect.");
                            }
                        });
                        ui.end_row();

                        // --- Marker offset ---
                        ui.label("Marker offset").on_hover_text("Move only the marker in screen pixels; this does not alter projection, FOV, or marker size.");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(DragValue::new(&mut preset.marker_offset_x).speed(1.0).range(-10000.0..=10000.0).suffix(" px"));
                            ui.label("Y");
                            ui.add(DragValue::new(&mut preset.marker_offset_y).speed(1.0).range(-10000.0..=10000.0).suffix(" px"));
                            if ui.small_button("Reset marker offset").clicked() {
                                preset.marker_offset_x = 0.0;
                                preset.marker_offset_y = 0.0;
                            }
                        });
                        ui.end_row();

                        // --- Text (only when marker is Text) ---
                        if preset.marker_source == EspMarkerSource::Text {
                            ui.label("Text");
                            let text_id = ui.make_persistent_id(("esp_marker_text", preset.id));
                            let text_width = ui.available_width();
                            Self::render_interpolated_text_edit(
                                ui,
                                &mut preset.marker_text,
                                text_id,
                                text_width,
                                text_width,
                                21.0,
                                72.0,
                                "Text with {variable}, e.g. Hunter: {health}",
                                true,
                            );
                            ui.end_row();
                        }

                        // --- SVG inline editor ---
                        if preset.marker_source == EspMarkerSource::Svg {
                            ui.label("SVG source");
                            let hint = RichText::new("Paste <svg ...>...</svg> here, or choose an SVG file above")
                                .color(ui.visuals().weak_text_color());
                            egui::ScrollArea::vertical()
                                .id_salt(("esp_svg_source_scroll", preset.id))
                                .max_height(72.0)
                                .show(ui, |ui| {
                                    ui.add_sized(
                                        [ui.available_width(), 72.0],
                                        TextEdit::multiline(&mut preset.marker_svg_source)
                                            .desired_rows(3)
                                            .hint_text(hint),
                                    );
                                });
                            ui.end_row();
                        }

                        // --- Scale with distance ---
                        ui.label("Scale");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut preset.scale_with_distance, "Scale with distance")
                                .on_hover_text("Scale every marker type from the existing camera-target distance; no target box address is needed.");
                            ui.label("Reference distance");
                            ui.add(DragValue::new(&mut preset.distance_reference).speed(1.0).range(0.01..=1_000_000.0))
                                .on_hover_text("At this world distance, the marker uses its configured base size.");
                            ui.label("Strength");
                            ui.add(
                                DragValue::new(&mut preset.distance_scale_strength_percent)
                                    .speed(1.0)
                                    .range(0.0..=100.0)
                                    .suffix("%"),
                            )
                            .on_hover_text("100% uses full inverse-distance scaling; lower values make size changes gentler.");
                            ui.label("Size offset");
                            ui.add(DragValue::new(&mut preset.marker_size_offset_percent).speed(1.0).range(-95.0..=1000.0).suffix("%"));
                        });
                        ui.end_row();

                        // --- Color / Tracer / Update / Smooth ---
                        ui.label("Display");
                        ui.horizontal(|ui| {
                            let mut color = Color32::from_rgba_unmultiplied(
                                preset.color.r, preset.color.g, preset.color.b, preset.color.a,
                            );
                            ui.label("Color");
                            if ui.color_edit_button_srgba(&mut color).changed() {
                                preset.color = RgbaColor { r: color.r(), g: color.g(), b: color.b(), a: color.a() };
                            }
                            ui.checkbox(&mut preset.show_tracer, "Tracer");
                            ui.checkbox(&mut preset.show_distance, "Distance");
                            ui.checkbox(&mut preset.debug_mode, "Debug Mode")
                                .on_hover_text("Clicking any Box marker displays its memory address & X Y Z coordinates and copies the address to clipboard.");
                            ui.label("Update");
                            ui.add(DragValue::new(&mut preset.update_interval_ms).speed(1.0).range(1..=1000).suffix(" ms"));
                            let mut smooth_enabled = preset.motion_smoothing_ms > 0;
                            if ui.checkbox(&mut smooth_enabled, "Smooth").clicked() {
                                preset.motion_smoothing_ms = if smooth_enabled { 40 } else { 0 };
                            }
                            if smooth_enabled {
                                ui.add(
                                    DragValue::new(&mut preset.motion_smoothing_ms)
                                        .speed(1.0)
                                        .range(1..=500)
                                        .suffix(" ms"),
                                )
                                .on_hover_text("Thời gian trôi mượt ESP (smooth kiểu cũ). Số ms càng cao trôi càng đằm.");
                            }
                        });
                        ui.end_row();

                        // --- Target sound ---
                        ui.label("Target sound");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut preset.target_audio_enabled, "Enable")
                                .on_hover_text("Play spatial audio from the target: stereo follows its direction and volume fades with distance.");
                            if ui.button("Choose sound").clicked()
                                && let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "m4a", "aac"])
                                    .pick_file()
                            {
                                preset.target_audio_path = path.to_string_lossy().into_owned();
                            }
                            let hint = RichText::new("Audio file").color(ui.visuals().weak_text_color());
                            ui.add_sized([220.0, 21.0], TextEdit::singleline(&mut preset.target_audio_path).hint_text(hint));
                            ui.checkbox(&mut preset.target_audio_loop, "Loop");
                        });
                        ui.end_row();

                        // --- Volume ---
                        ui.label("Volume");
                        ui.horizontal(|ui| {
                            ui.add(DragValue::new(&mut preset.target_audio_volume).speed(0.01).range(0.0..=2.0))
                                .on_hover_text("1.0 is the original file volume; up to 2.0 boosts it.");
                            ui.label("Full volume within");
                            ui.add(DragValue::new(&mut preset.target_audio_full_volume_distance).speed(1.0).range(0.0..=1_000_000.0));
                            ui.label("Silent after");
                            ui.add(DragValue::new(&mut preset.target_audio_max_distance).speed(1.0).range(0.01..=1_000_000.0));
                        });
                        ui.end_row();
                    });
            });
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
        if let Some(preset) = copy_preset {
            self.preset_clipboard = Some(crate::ui::PresetClipboard::Esp(preset));
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
            self.start_esp_entity_root_capture(id);
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
