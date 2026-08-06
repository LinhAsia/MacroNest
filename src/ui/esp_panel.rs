use eframe::egui::{self, Button, Color32, ComboBox, DragValue, Grid, RichText, TextEdit};

use crate::model::{
    EspAngleUnit, EspHorizontalPlane, EspMarkerKind, EspMarkerSource, EspOrientationSource,
    EspPreset, MemoryValueType, RgbaColor,
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
            let migrated_marker_source = preset.migrate_marker_sources();
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
                    ui.checkbox(&mut preset.invert_camera_pitch, "Reverse pitch value")
                        .on_hover_text(
                            "Reverse only camera pitch angle. Use this when looking down with camera moves ESP the wrong way.",
                        );
                    ui.checkbox(&mut preset.invert_vertical, "Invert elevation (height)")
                        .on_hover_text(
                            "Invert target elevation difference. Use this when moving player up/down moves ESP the wrong way.",
                        );
                    ui.checkbox(&mut preset.invert_yaw, "Mirror screen X")
                        .on_hover_text("Mirror only the final left/right screen position.");
                    ui.checkbox(&mut preset.invert_pitch, "Mirror screen Y")
                        .on_hover_text("Mirror only the final up/down screen position.");
                    ui.label("Horizontal FOV");
                    ui.add(
                        DragValue::new(&mut preset.horizontal_fov)
                            .speed(1.0)
                            .range(1.0..=179.0),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Yaw zero offset").on_hover_text(
                        "Use this when the marker is consistently rotated left/right. Try +90, -90, then 180.",
                    );
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
                    ui.label("Direction scale").on_hover_text("Multiplier for Direction A/B vector values");
                    ui.add(
                        DragValue::new(&mut preset.direction_multiplier)
                            .speed(0.001)
                            .range(0.0001..=100.0),
                    );
                    if ui.small_button("Reset scale").clicked() {
                        preset.direction_multiplier = 1.0;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Pitch zero offset").on_hover_text(
                        "Use only when every marker is consistently too high/low as the camera tilts.",
                    );
                    ui.add(
                        DragValue::new(&mut preset.pitch_offset_degrees)
                            .speed(1.0)
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
                            .speed(1.0)
                            .range(-10000.0..=10000.0),
                    );
                    ui.label("Height scale").on_hover_text(
                        "Multiplier for Z elevation distance (e.g., 0.01 if Z is in cm while X/Y are in m)",
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
                    ComboBox::from_id_salt(("esp_marker_source", preset.id))
                        .selected_text(match preset.marker_source {
                            EspMarkerSource::Geometry => "Geometry",
                            EspMarkerSource::Text => "Text",
                            EspMarkerSource::Svg => "SVG",
                            EspMarkerSource::Image => "Image",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut preset.marker_source,
                                EspMarkerSource::Geometry,
                                "Geometry",
                            );
                            ui.selectable_value(
                                &mut preset.marker_source,
                                EspMarkerSource::Text,
                                "Text",
                            );
                            ui.selectable_value(
                                &mut preset.marker_source,
                                EspMarkerSource::Svg,
                                "SVG",
                            );
                            ui.selectable_value(
                                &mut preset.marker_source,
                                EspMarkerSource::Image,
                                "Image",
                            );
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
                                ui.add(
                                    DragValue::new(&mut preset.dot_radius)
                                        .speed(1.0)
                                        .range(1.0..=100.0),
                                );
                            }
                            EspMarkerKind::Box => {
                                ui.label("Width");
                                ui.add(
                                    DragValue::new(&mut preset.box_width)
                                        .speed(1.0)
                                        .range(2.0..=1000.0),
                                );
                                ui.label("Height");
                                ui.add(
                                    DragValue::new(&mut preset.box_height)
                                        .speed(1.0)
                                        .range(2.0..=1000.0),
                                );
                            }
                        }
                        ui.label("Thickness");
                        ui.add(
                            DragValue::new(&mut preset.thickness)
                                .speed(1.0)
                                .range(1.0..=30.0),
                        );
                        ui.checkbox(&mut preset.filled, "Fill");
                    } else if preset.marker_source == EspMarkerSource::Text {
                        ui.label("Offset X");
                        ui.add(DragValue::new(&mut preset.text_offset_x).speed(1.0));
                        ui.label("Offset Y");
                        ui.add(DragValue::new(&mut preset.text_offset_y).speed(1.0));
                        ui.label("Size");
                        ui.add(
                            DragValue::new(&mut preset.text_font_size)
                                .speed(1.0)
                                .range(8.0..=256.0),
                        );
                        ui.label("Opacity");
                        ui.add(
                            DragValue::new(&mut preset.text_opacity)
                                .speed(0.01)
                                .range(0.0..=1.0),
                        );
                    } else {
                        let label = if preset.marker_source == EspMarkerSource::Svg {
                            "Choose SVG"
                        } else {
                            "Import image"
                        };
                        if ui.button(label).clicked() {
                            let mut dialog = rfd::FileDialog::new();
                            dialog = if preset.marker_source == EspMarkerSource::Svg {
                                dialog.add_filter("SVG", &["svg"])
                            } else {
                                dialog.add_filter(
                                    "Images",
                                    &["png", "jpg", "jpeg", "webp", "bmp", "ico"],
                                )
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
                            let hint = RichText::new("Image file")
                                .color(ui.visuals().weak_text_color());
                            ui.add_sized(
                                [260.0, 21.0],
                                TextEdit::singleline(&mut preset.marker_asset_path)
                                    .hint_text(hint),
                            );
                        }
                        if preset.marker_source == EspMarkerSource::Svg {
                            ui.label("SVG width");
                            ui.add(
                                DragValue::new(&mut preset.svg_width)
                                    .speed(1.0)
                                    .range(2.0..=1000.0),
                            );
                            ui.label("SVG height");
                            ui.add(
                                DragValue::new(&mut preset.svg_height)
                                    .speed(1.0)
                                    .range(2.0..=1000.0),
                            );
                        } else {
                            ui.label("Image width");
                            ui.add(
                                DragValue::new(&mut preset.image_width)
                                    .speed(1.0)
                                    .range(2.0..=1000.0),
                            );
                            ui.label("Image height");
                            ui.add(
                                DragValue::new(&mut preset.image_height)
                                    .speed(1.0)
                                    .range(2.0..=1000.0),
                            );
                        }
                        ui.checkbox(
                            &mut preset.marker_billboard_3d,
                            "World-space billboard",
                        )
                            .on_hover_text(
                                "Keep the sprite facing the camera and scale it by world distance. A billboard intentionally stays flat to the viewer; perspective size is its visible 3D effect.",
                            );
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Marker offset").on_hover_text(
                        "Move only the marker in screen pixels; this does not alter projection, FOV, or marker size.",
                    );
                    ui.label("X");
                    ui.add(
                        DragValue::new(&mut preset.marker_offset_x)
                            .speed(1.0)
                            .range(-10000.0..=10000.0)
                            .suffix(" px"),
                    );
                    ui.label("Y");
                    ui.add(
                        DragValue::new(&mut preset.marker_offset_y)
                            .speed(1.0)
                            .range(-10000.0..=10000.0)
                            .suffix(" px"),
                    );
                    if ui.small_button("Reset marker offset").clicked() {
                        preset.marker_offset_x = 0.0;
                        preset.marker_offset_y = 0.0;
                    }
                });
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
                }
                if preset.marker_source == EspMarkerSource::Svg {
                    ui.label("SVG file or inline SVG code");
                    let hint =
                        RichText::new("Paste <svg ...>...</svg> here, or choose an SVG file above")
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
                }
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut preset.scale_with_distance, "Scale with distance")
                        .on_hover_text(
                            "Scale every marker type from the existing camera-target distance; no target box address is needed.",
                        );
                    ui.label("Reference distance");
                    ui.add(
                        DragValue::new(&mut preset.distance_reference)
                            .speed(1.0)
                            .range(0.01..=1_000_000.0),
                    )
                    .on_hover_text("At this world distance, the marker uses its configured base size.");
                    ui.label("Size offset");
                    ui.add(
                        DragValue::new(&mut preset.marker_size_offset_percent)
                            .speed(1.0)
                            .range(-95.0..=1000.0)
                            .suffix("%"),
                    );
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
                            .speed(1.0)
                            .range(1..=1000)
                            .suffix(" ms"),
                    );
                    let mut smooth_enabled = preset.motion_smoothing_ms > 0;
                    if ui.checkbox(&mut smooth_enabled, "Smooth").clicked() {
                        preset.motion_smoothing_ms = if smooth_enabled { 16 } else { 0 };
                    }
                    if smooth_enabled {
                        ui.add(
                            DragValue::new(&mut preset.motion_smoothing_ms)
                                .speed(1.0)
                                .range(1..=100)
                                .suffix(" ms"),
                        )
                        .on_hover_text("Frame interpolation time for high-FPS sub-step motion. Lower = faster response.");
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut preset.target_audio_enabled, "Target sound")
                        .on_hover_text(
                            "Play spatial audio from the target: stereo follows its direction and volume fades with distance.",
                        );
                    if ui.button("Choose sound").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "Audio",
                                &["wav", "mp3", "flac", "ogg", "m4a", "aac"],
                            )
                            .pick_file()
                    {
                        preset.target_audio_path = path.to_string_lossy().into_owned();
                    }
                    let hint = RichText::new("Audio file").color(ui.visuals().weak_text_color());
                    ui.add_sized(
                        [260.0, 21.0],
                        TextEdit::singleline(&mut preset.target_audio_path).hint_text(hint),
                    );
                    ui.checkbox(&mut preset.target_audio_loop, "Loop");
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Volume");
                    ui.add(
                        DragValue::new(&mut preset.target_audio_volume)
                            .speed(0.01)
                            .range(0.0..=2.0),
                    )
                    .on_hover_text("1.0 is the original file volume; up to 2.0 boosts it.");
                    ui.label("Full volume within");
                    ui.add(
                        DragValue::new(&mut preset.target_audio_full_volume_distance)
                            .speed(1.0)
                            .range(0.0..=1_000_000.0),
                    );
                    ui.label("Silent after");
                    ui.add(
                        DragValue::new(&mut preset.target_audio_max_distance)
                            .speed(1.0)
                            .range(0.01..=1_000_000.0),
                    );
                });
            });
            if migrated_marker_source || preset != before {
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
