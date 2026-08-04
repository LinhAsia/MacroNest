use eframe::egui::{self, Button, Color32, ComboBox, DragValue, Grid, RichText, TextEdit};

use crate::model::{
    EspAngleUnit, EspHorizontalPlane, EspMarkerKind, EspOrientationSource, EspPreset,
    MemoryValueType, RgbaColor,
};

use super::CrosshairApp;

impl CrosshairApp {
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
        let can_paste = matches!(
            self.preset_clipboard,
            Some(crate::ui::PresetClipboard::Esp(_))
        );
        for index in 0..self.state.esp_presets.len() {
            let mut preset = self.state.esp_presets[index].clone();
            let before = preset.clone();
            let snapshot = preset.clone();
            let calibration_feedback = self.esp_calibration_feedback.get(&preset.id).cloned();
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
                                let title = crate::window_list::selector_base_title(
                                    &preset.target_window,
                                );
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
                        for (label, value) in [
                            ("Target X", &mut preset.target_x),
                            ("Target Y", &mut preset.target_y),
                            ("Target Z", &mut preset.target_z),
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

                ui.horizontal_wrapped(|ui| {
                    ui.label("Value type");
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
                    ui.label("World plane");
                    ComboBox::from_id_salt(("esp_plane", preset.id))
                        .selected_text(match preset.horizontal_plane {
                            EspHorizontalPlane::Xy => "XY + vertical Z",
                            EspHorizontalPlane::Xz => "XZ + vertical Y",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut preset.horizontal_plane,
                                EspHorizontalPlane::Xy,
                                "XY + vertical Z",
                            );
                            ui.selectable_value(
                                &mut preset.horizontal_plane,
                                EspHorizontalPlane::Xz,
                                "XZ + vertical Y",
                            );
                        });
                });
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
                        .on_hover_text(
                            "Reverse only camera rotation. Use this when lateral movement is correct but rotating the camera moves ESP the wrong way.",
                        );
                    ui.checkbox(&mut preset.invert_yaw, "Mirror screen X")
                        .on_hover_text("Mirror only the final left/right screen position.");
                    ui.checkbox(&mut preset.invert_pitch, "Invert pitch");
                    ui.label("Horizontal FOV");
                    ui.add(DragValue::new(&mut preset.horizontal_fov).range(1.0..=179.0));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Yaw zero offset").on_hover_text(
                        "Use this when the marker is consistently rotated left/right. Try +90, -90, then 180.",
                    );
                    ui.add(
                        DragValue::new(&mut preset.yaw_offset_degrees)
                            .range(-360.0..=360.0)
                            .suffix(" deg"),
                    );
                    for value in [-180.0, -90.0, 0.0, 90.0, 180.0] {
                        if ui.small_button(format!("{value:+.0}")).clicked() {
                            preset.yaw_offset_degrees = value;
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Pitch zero offset").on_hover_text(
                        "Use only when every marker is consistently too high/low as the camera tilts.",
                    );
                    ui.add(
                        DragValue::new(&mut preset.pitch_offset_degrees)
                            .range(-180.0..=180.0)
                            .suffix(" deg"),
                    );
                    if ui.small_button("Reset pitch").clicked() {
                        preset.pitch_offset_degrees = 0.0;
                    }
                    ui.label("Target height").on_hover_text(
                        "World-unit correction for a target pivot at feet/waist. Start at 0; adjust only after yaw is correct.",
                    );
                    ui.add(
                        DragValue::new(&mut preset.target_vertical_offset)
                            .speed(0.1)
                            .range(-10000.0..=10000.0),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Screen offset").on_hover_text(
                        "Final pixel correction. It does not fix a wrong axis or angle convention.",
                    );
                    ui.label("X");
                    ui.add(
                        DragValue::new(&mut preset.screen_offset_x)
                            .speed(1.0)
                            .range(-10000.0..=10000.0)
                            .suffix(" px"),
                    );
                    ui.label("Y");
                    ui.add(
                        DragValue::new(&mut preset.screen_offset_y)
                            .speed(1.0)
                            .range(-10000.0..=10000.0)
                            .suffix(" px"),
                    );
                    if ui.small_button("Reset screen").clicked() {
                        preset.screen_offset_x = 0.0;
                        preset.screen_offset_y = 0.0;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(
                            "Suggested start for the shown values: XY + vertical Z, yaw Degrees, pitch Radians, FOV 90. If the marker is sideways try yaw +90/-90; if behind try 180.",
                        )
                        .color(ui.visuals().weak_text_color()),
                    );
                    if ui.small_button("Apply suggested start").clicked() {
                        preset.horizontal_plane = EspHorizontalPlane::Xy;
                        preset.yaw_unit = EspAngleUnit::Degrees;
                        preset.pitch_unit = EspAngleUnit::Radians;
                        preset.invert_camera_yaw = false;
                        preset.invert_yaw = false;
                        preset.invert_pitch = false;
                        preset.yaw_offset_degrees = 0.0;
                        preset.pitch_offset_degrees = 0.0;
                        preset.target_vertical_offset = 0.0;
                        preset.screen_offset_x = 0.0;
                        preset.screen_offset_y = 0.0;
                        preset.horizontal_fov = 90.0;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(
                            "Auto calibration: stand on four different sides, aim the screen center at the target, then capture once per side.",
                        )
                        .color(ui.visuals().weak_text_color()),
                    );
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
                });
                if let Some(feedback) = calibration_feedback {
                    ui.label(RichText::new(feedback).color(ui.visuals().weak_text_color()));
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Marker");
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
                            ui.add(DragValue::new(&mut preset.dot_radius).range(1.0..=100.0));
                        }
                        EspMarkerKind::Box => {
                            ui.label("Width");
                            ui.add(DragValue::new(&mut preset.box_width).range(2.0..=1000.0));
                            ui.label("Height");
                            ui.add(DragValue::new(&mut preset.box_height).range(2.0..=1000.0));
                        }
                    }
                    ui.label("Thickness");
                    ui.add(DragValue::new(&mut preset.thickness).range(1.0..=30.0));
                    ui.checkbox(&mut preset.filled, "Fill");
                });
                ui.horizontal_wrapped(|ui| {
                    let mut color = Color32::from_rgba_unmultiplied(
                        preset.color.r,
                        preset.color.g,
                        preset.color.b,
                        preset.color.a,
                    );
                    ui.label("Color");
                    if ui.color_edit_button_srgba(&mut color).changed() {
                        preset.color = RgbaColor {
                            r: color.r(),
                            g: color.g(),
                            b: color.b(),
                            a: color.a(),
                        };
                    }
                    ui.checkbox(&mut preset.show_tracer, "Tracer");
                    ui.checkbox(&mut preset.show_distance, "Distance");
                    ui.label("Update");
                    ui.add(
                        DragValue::new(&mut preset.update_interval_ms)
                            .range(1..=1000)
                            .suffix(" ms"),
                    );
                    ui.label("Smooth");
                    ui.add(
                        DragValue::new(&mut preset.motion_smoothing_ms)
                            .range(0..=500)
                            .suffix(" ms"),
                    )
                    .on_hover_text("Smooth marker movement between RAM samples. Set 0 to disable.");
                });
            });
            if preset != before {
                self.state.esp_presets[index] = preset;
                dirty = true;
            }
        }
        if let Some(id) = remove {
            self.state.esp_presets.retain(|preset| preset.id != id);
            self.esp_calibration_feedback.remove(&id);
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
