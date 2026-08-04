use eframe::egui::{self, Color32, ComboBox, DragValue, Grid, TextEdit};

use crate::model::{
    EspAngleUnit, EspHorizontalPlane, EspMarkerKind, EspPreset, MemoryValueType, RgbaColor,
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
        for index in 0..self.state.esp_presets.len() {
            let mut preset = self.state.esp_presets[index].clone();
            let before = preset.clone();
            Self::show_preset_card(ui, false, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [Self::preset_header_name_width(ui), 22.0],
                        TextEdit::singleline(&mut preset.name),
                    );
                    let toggle = if preset.enabled { "On" } else { "Off" };
                    if ui.selectable_label(preset.enabled, toggle).clicked() {
                        preset.enabled = !preset.enabled;
                    }
                    if ui
                        .button(if preset.collapsed { "Show" } else { "Hide" })
                        .clicked()
                    {
                        preset.collapsed = !preset.collapsed;
                    }
                    if Self::sound_style_remove_button(ui).clicked() {
                        remove = Some(preset.id);
                    }
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
                        let target_label = windows
                            .iter()
                            .find(|window| window.selector == preset.target_window)
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
                            ("Camera yaw", &mut preset.camera_yaw),
                            ("Camera pitch", &mut preset.camera_pitch),
                        ] {
                            ui.label(label);
                            ui.add(
                                TextEdit::singleline(value)
                                    .desired_width(420.0)
                                    .hint_text("address / module+offset [offsets] / @alias"),
                            );
                            ui.end_row();
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
                    angle_unit(ui, "Yaw", preset.id, &mut preset.yaw_unit);
                    angle_unit(ui, "Pitch", preset.id, &mut preset.pitch_unit);
                    ui.checkbox(&mut preset.invert_yaw, "Invert yaw");
                    ui.checkbox(&mut preset.invert_pitch, "Invert pitch");
                    ui.label("Horizontal FOV");
                    ui.add(DragValue::new(&mut preset.horizontal_fov).range(1.0..=179.0));
                });
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
                            .range(16..=1000)
                            .suffix(" ms"),
                    );
                });
            });
            if preset != before {
                self.state.esp_presets[index] = preset;
                dirty = true;
            }
        }
        if let Some(id) = remove {
            self.state.esp_presets.retain(|preset| preset.id != id);
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
