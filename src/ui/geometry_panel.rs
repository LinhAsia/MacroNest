use crate::model::{GeometryPreset, GeometryShapeKind, GeometrySpec, VietnameseInputMode};
use crate::ui::{CrosshairApp, MouseCaptureKind, MouseMoveAbsoluteCaptureTarget, UiLanguage};
use eframe::egui::{self, Button, ComboBox, Frame, Grid, TextEdit};

impl CrosshairApp {
    pub(crate) const GEOMETRY_LABEL_COL_WIDTH: f32 = 110.0;
    pub(crate) const GEOMETRY_FIELD_WIDTH: f32 = 96.0;
    pub(crate) const GEOMETRY_FIELD_EXPANDED_WIDTH: f32 = 120.0;
    pub(crate) const GEOMETRY_GRID_SPACING_X: f32 = 2.0;

    pub(crate) fn render_geometry_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        let mut changed = false;
        let mut remove_preset_id = None;
        let mut request_screen_color_pick = false;
        let mut pending_screen_color_target: Option<(u32, u32, bool)> = None;
        let mut clear_preview_target = false;
        let mut clear_preset_preview_target = false;
        let mut next_preset_preview_target: Option<u32> = None;
        let mut next_geometry_preview_spec: Option<GeometrySpec> = None;
        let mut begin_mouse_move_absolute_capture_target = None;

        // Initialize autocomplete suggestions for geometry panel
        let timer_names: Vec<String> = self
            .state
            .timer_presets
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mut suggestion_names = std::collections::HashSet::new();
        for preset in &self.state.timer_presets {
            suggestion_names.insert(preset.name.clone());
        }
        for (idx, _name) in timer_names.iter().enumerate() {
            suggestion_names.insert(format!("Timer{}", idx + 1));
        }
        for name in self.collect_all_macro_referenced_variables() {
            if !name.contains('.') {
                suggestion_names.insert(name);
            }
        }
        {
            let vars = crate::overlay::RUNTIME_VARIABLES.lock();
            for name in vars.keys() {
                if !name.contains('.') {
                    suggestion_names.insert(name.clone());
                }
            }
        }
        let mut suggestion_names: Vec<String> = suggestion_names.into_iter().collect();
        suggestion_names.sort();
        let mut all_vars = suggestion_names.clone();
        for (const_name, _) in &self.state.global_constants {
            if !all_vars.contains(const_name) {
                all_vars.push(const_name.clone());
            }
        }
        all_vars.sort();

        ui.memory_mut(|mem| {
            mem.data.insert_temp(
                egui::Id::new("macro_variable_suggestion_names"),
                suggestion_names,
            );
            mem.data
                .insert_temp(egui::Id::new("macro_timer_suggestion_names"), timer_names);
        });

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(Self::tr_lang(
                    language,
                    "+ Add geometry preset",
                    "+ Add geometry preset",
                ))
                .clicked()
            {
                let id = Self::allocate_next_id(
                    &self.state.geometry_presets,
                    &mut self.state.next_geometry_preset_id,
                    |preset| preset.id,
                );
                let mut new_preset = GeometryPreset::new(id);
                let mut suffix = 1;
                while self
                    .state
                    .geometry_presets
                    .iter()
                    .any(|p| p.name == format!("Geometry {}", suffix))
                {
                    suffix += 1;
                }
                new_preset.name = format!("Geometry {}", suffix);
                self.state.geometry_presets.push(new_preset);
                changed = true;
            }
        });

        ui.add_space(8.0);

        for preset_index in 0..self.state.geometry_presets.len() {
            let preset = &mut self.state.geometry_presets[preset_index];
            if preset.objects.is_empty() {
                preset.objects.push(crate::model::GeometryObject::new(
                    preset.id,
                    GeometryShapeKind::Point,
                ));
                changed = true;
            }
            Self::show_preset_card(ui, false, |ui| {
                ui.horizontal(|ui| {
                    let name_width = Self::preset_header_name_width(ui);
                    let response =
                        ui.add_sized([name_width, 21.0], TextEdit::singleline(&mut preset.name));
                    Self::apply_vietnamese_input_if_changed(
                        &response,
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
                        &mut preset.name,
                    );
                    changed |= response.changed();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Self::sound_style_remove_button(ui)
                            .on_hover_text(Self::tr_lang(
                                language,
                                "Delete preset",
                                "Delete preset",
                            ))
                            .clicked()
                        {
                            remove_preset_id = Some(preset.id);
                            if self
                                .geometry_preview_target
                                .is_some_and(|(preview_preset_id, _)| {
                                    preview_preset_id == preset.id
                                })
                            {
                                clear_preview_target = true;
                            }
                            if self.geometry_preset_preview_target == Some(preset.id) {
                                clear_preset_preview_target = true;
                            }
                        }
                        if Self::sound_style_toggle_button(
                            ui,
                            if preset.collapsed {
                                Self::tr_lang(language, "Show", "Show")
                            } else {
                                Self::tr_lang(language, "Hide", "Hide")
                            },
                        )
                        .clicked()
                        {
                            preset.collapsed = !preset.collapsed;
                            changed = true;
                        }

                        let preview_all_active =
                            self.geometry_preset_preview_target == Some(preset.id);
                        let preview_all_btn = Button::new(Self::material_icon_text(
                            if preview_all_active { 0xe8f5 } else { 0xe8f4 },
                            18.0,
                        ));
                        if ui
                            .add_sized([36.0, 24.0], preview_all_btn)
                            .on_hover_text(if preview_all_active {
                                Self::tr_lang(language, "Stop Preview All", "Stop Preview All")
                            } else {
                                Self::tr_lang(language, "Preview All", "Preview All")
                            })
                            .clicked()
                        {
                            if preview_all_active {
                                clear_preset_preview_target = true;
                            } else {
                                next_preset_preview_target = Some(preset.id);
                            }
                        }
                    });
                });

                if preset.collapsed {
                    return;
                }

                let preset_id = preset.id;
                if let Some(object) = preset.object_mut() {
                    ui.add_space(6.0);
                    let card_width = ui.available_width() - 16.0;
                    Frame::group(ui.style()).inner_margin(8).show(ui, |ui| {
                        ui.set_min_width(card_width);
                        ui.horizontal(|ui| {
                            let preview_active =
                                self.geometry_preview_target == Some((preset_id, object.id));

                            ComboBox::from_id_salt((preset_id, object.id, "shape"))
                                .width(132.0)
                                .selected_text(Self::geometry_shape_label(
                                    object.spec.shape,
                                    language,
                                ))
                                .show_ui(ui, |ui| {
                                    for shape in Self::geometry_shapes() {
                                        let response = ui.selectable_value(
                                            &mut object.spec.shape,
                                            shape,
                                            Self::geometry_shape_label(shape, language),
                                        );
                                        if response.changed() {
                                            changed = true;
                                            object.spec.apply_shape_defaults();
                                        }
                                    }
                                });

                            let preview_btn = Button::new(Self::material_icon_text(
                                if preview_active { 0xe8f5 } else { 0xe8f4 },
                                16.0,
                            ));
                            let preview_response = ui.add_sized([24.0, 21.0], preview_btn);
                            if preview_response
                                .on_hover_text(if preview_active {
                                    Self::tr_lang(language, "Stop preview", "Stop preview")
                                } else {
                                    Self::tr_lang(language, "Preview", "Preview")
                                })
                                .clicked()
                            {
                                if preview_active {
                                    clear_preview_target = true;
                                } else {
                                    self.geometry_preview_target = Some((preset_id, object.id));
                                    next_geometry_preview_spec = Some(object.spec.clone());
                                }
                            }
                        });

                        ui.add_space(6.0);
                        changed |= Self::render_geometry_spec_editor(
                            ui,
                            language,
                            preset_id,
                            object.id,
                            false,
                            &mut object.spec,
                            &mut self.vision_manual_color,
                            &mut self.vision_manual_color_hex,
                            &mut request_screen_color_pick,
                            &mut pending_screen_color_target,
                            &mut begin_mouse_move_absolute_capture_target,
                            self.state.vietnamese_input_enabled,
                            self.state.vietnamese_input_mode,
                            None,
                        );
                    });
                }
            });
        }

        if let Some(preset_id) = remove_preset_id {
            self.state
                .geometry_presets
                .retain(|preset| preset.id != preset_id);
            changed = true;
        }

        if changed {
            self.persist_geometry_presets();
        }

        if clear_preview_target {
            self.clear_geometry_spec_preview();
        } else if let Some(spec) = next_geometry_preview_spec.take() {
            self.sync_geometry_spec_preview(Some(spec));
        } else if let Some((preview_preset_id, preview_object_id)) = self.geometry_preview_target {
            let preview_spec = self
                .state
                .geometry_presets
                .iter()
                .find(|preset| preset.id == preview_preset_id)
                .and_then(|preset| {
                    preset
                        .objects
                        .iter()
                        .find(|object| object.id == preview_object_id)
                })
                .map(|object| object.spec.clone());
            if preview_spec.is_none() {
                self.geometry_preview_target = None;
                self.geometry_preview_sent = None;
            }
            if self.geometry_preview_sent != preview_spec {
                self.sync_geometry_spec_preview(preview_spec);
            }
        }

        if let Some(preview_preset_id) = self.geometry_preset_preview_target {
            let exists = self
                .state
                .geometry_presets
                .iter()
                .any(|preset| preset.id == preview_preset_id);
            if !exists {
                clear_preset_preview_target = true;
            }
        }

        if clear_preset_preview_target {
            self.clear_geometry_preset_preview();
        } else if let Some(preset_id) = next_preset_preview_target {
            self.sync_geometry_preset_preview(Some(preset_id));
        }

        if request_screen_color_pick {
            self.geometry_color_pick_target = pending_screen_color_target;
            self.begin_color_pick_capture(ui.ctx(), crate::ui::VisionCaptureTarget::GeometryColor);
        }

        if let Some(target) = begin_mouse_move_absolute_capture_target {
            self.begin_mouse_move_absolute_capture(ui.ctx(), target);
        }
    }

    pub(crate) fn geometry_color_expr_literal(color: crate::model::RgbaColor) -> String {
        if color.a == 255 {
            format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
        } else {
            format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                color.r, color.g, color.b, color.a
            )
        }
    }

    pub(crate) fn geometry_shapes() -> [GeometryShapeKind; 11] {
        [
            GeometryShapeKind::Point,
            GeometryShapeKind::Line,
            GeometryShapeKind::Circle,
            GeometryShapeKind::Rectangle,
            GeometryShapeKind::Label,
            GeometryShapeKind::Ellipse,
            GeometryShapeKind::Arrow,
            GeometryShapeKind::Polyline,
            GeometryShapeKind::Polygon,
            GeometryShapeKind::Arc,
            GeometryShapeKind::Svg,
        ]
    }

    pub(crate) fn geometry_shape_label(
        shape: GeometryShapeKind,
        language: crate::model::UiLanguage,
    ) -> &'static str {
        let (translation_key, english) = match shape {
            GeometryShapeKind::Point => ("geometry_shape_label.point", "Point"),
            GeometryShapeKind::Line => ("geometry_shape_label.line", "Line"),
            GeometryShapeKind::Circle => ("geometry_shape_label.circle", "Circle"),
            GeometryShapeKind::Rectangle => ("geometry_shape_label.rectangle", "Rectangle"),
            GeometryShapeKind::Label => ("geometry_shape_label.label", "Label"),
            GeometryShapeKind::Ellipse => ("geometry_shape_label.ellipse", "Ellipse"),
            GeometryShapeKind::Arrow => ("geometry_shape_label.arrow", "Arrow"),
            GeometryShapeKind::Polyline => ("geometry_shape_label.polyline", "Polyline"),
            GeometryShapeKind::Polygon => ("geometry_shape_label.polygon", "Polygon"),
            GeometryShapeKind::Arc => ("geometry_shape_label.arc", "Arc"),
            GeometryShapeKind::Svg => ("geometry_shape_label.svg", "SVG Image"),
        };
        match language {
            crate::model::UiLanguage::Vietnamese => {
                crate::lang::translate(language, translation_key).unwrap_or(english)
            }
            _ => english,
        }
    }

    pub(crate) fn render_geometry_spec_editor(
        ui: &mut egui::Ui,
        language: crate::model::UiLanguage,
        preset_id: u32,
        object_id: u32,
        allow_color_expression: bool,
        spec: &mut GeometrySpec,
        manual_color: &mut crate::model::RgbaColor,
        manual_color_hex: &mut String,
        request_screen_color_pick: &mut bool,
        pending_screen_color_target: &mut Option<(u32, u32, bool)>,
        begin_mouse_move_absolute_capture_target: &mut Option<MouseMoveAbsoluteCaptureTarget>,
        vietnamese_input_enabled: bool,
        vietnamese_input_mode: VietnameseInputMode,
        group_id_override: Option<u32>,
    ) -> bool {
        let mut changed = false;

        if matches!(
            spec.shape,
            GeometryShapeKind::Polyline | GeometryShapeKind::Polygon
        ) {
            let mut points: Vec<(String, String)> = spec
                .points_expr
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|pair| {
                    if let Some((x, y)) = pair.split_once(',') {
                        (x.trim().to_owned(), y.trim().to_owned())
                    } else {
                        (pair.trim().to_owned(), String::new())
                    }
                })
                .collect();

            let mut points_changed = false;
            let mut remove_point_idx = None;

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (idx, (x_val, y_val)) in points.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.add_sized([24.0, 18.0], egui::Label::new(format!("P{}", idx + 1)));
                        let x_id = ui.make_persistent_id((preset_id, object_id, idx, "poly-x"));
                        let response_x = Self::render_variable_text_edit(
                            ui, x_val, x_id, 80.0, 120.0, 18.0, 18.0, "", false,
                        );
                        points_changed |= response_x.changed();
                        Self::apply_vietnamese_input_if_changed(
                            &response_x,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            x_val,
                        );
                        Self::render_variable_suggestions(ui, &response_x, x_val, &[], language);

                        ui.add_sized([16.0, 18.0], egui::Label::new("Y"));
                        let y_id = ui.make_persistent_id((preset_id, object_id, idx, "poly-y"));
                        let response_y = Self::render_variable_text_edit(
                            ui, y_val, y_id, 80.0, 120.0, 18.0, 18.0, "", false,
                        );
                        points_changed |= response_y.changed();
                        Self::apply_vietnamese_input_if_changed(
                            &response_y,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            y_val,
                        );
                        Self::render_variable_suggestions(ui, &response_y, y_val, &[], language);

                        if ui
                            .add_sized(
                                [24.0, 21.0],
                                Button::new(Self::material_icon_text(0xe55f, 16.0)),
                            )
                            .on_hover_text(Self::tr_lang(
                                language,
                                "Pick coordinates from screen",
                                "Pick coordinates from screen",
                            ))
                            .clicked()
                        {
                            *begin_mouse_move_absolute_capture_target =
                                Some(MouseMoveAbsoluteCaptureTarget {
                                    group_id: group_id_override,
                                    preset_id,
                                    step_index: object_id as usize,
                                    capture_kind: MouseCaptureKind::GeometryPrimaryPos,
                                    extra_cond_index: Some(idx),
                                    is_hold_stop: false,
                                });
                        }
                        if ui
                            .add_sized(
                                [24.0, 21.0],
                                Button::new(Self::material_icon_text(0xe5cd, 16.0)),
                            )
                            .on_hover_text(Self::tr_lang(language, "Delete point", "Delete point"))
                            .clicked()
                        {
                            remove_point_idx = Some(idx);
                        }
                    });
                }
            });

            if let Some(idx) = remove_point_idx {
                points.remove(idx);
                points_changed = true;
            }

            ui.add_space(2.0);
            if ui
                .button(Self::tr_lang(language, "+ Add Point", "+ Add Point"))
                .clicked()
            {
                points.push(("960".to_owned(), "540".to_owned()));
                points_changed = true;
            }

            if points_changed {
                spec.points_expr = points
                    .iter()
                    .map(|(x, y)| format!("{},{}", x, y))
                    .collect::<Vec<_>>()
                    .join(";");
                changed = true;
            }

            ui.add_space(6.0);

            Grid::new((preset_id, object_id, "geometry-spec-grid"))
                .num_columns(2)
                .spacing([Self::GEOMETRY_GRID_SPACING_X, 6.0])
                .show(ui, |ui| {
                    changed |= Self::geometry_expr_row(
                        ui,
                        language,
                        preset_id,
                        object_id,
                        "thickness",
                        Self::tr_lang(language, "Thickness", "Thickness"),
                        &mut spec.thickness_expr,
                        120.0,
                        120.0,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                    );
                    changed |= Self::geometry_expr_row(
                        ui,
                        language,
                        preset_id,
                        object_id,
                        "opacity",
                        Self::tr_lang(language, "Opacity", "Opacity"),
                        &mut spec.opacity_expr,
                        120.0,
                        120.0,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                    );
                    if spec.shape == GeometryShapeKind::Polygon {
                        changed |= Self::geometry_fill_mode_row(ui, language, &mut spec.filled);
                    }

                    changed |= Self::geometry_expr_row(
                        ui,
                        language,
                        preset_id,
                        object_id,
                        "rotation",
                        Self::tr_lang(language, "Rotate", "Rotate"),
                        &mut spec.rotation_expr,
                        120.0,
                        120.0,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                    );

                    let stroke_label = if spec.shape == GeometryShapeKind::Polygon {
                        Self::tr_lang(language, "Stroke", "Stroke")
                    } else {
                        Self::tr_lang(language, "Color", "Color")
                    };

                    changed |= Self::geometry_color_row(
                        ui,
                        language,
                        preset_id,
                        object_id,
                        stroke_label,
                        &mut spec.stroke_color,
                        &mut spec.stroke_color_expr,
                        manual_color,
                        manual_color_hex,
                        allow_color_expression,
                        request_screen_color_pick,
                        pending_screen_color_target,
                        false,
                        false,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                    );

                    if spec.filled && spec.shape == GeometryShapeKind::Polygon {
                        changed |= Self::geometry_color_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            Self::tr_lang(language, "Fill", "Fill"),
                            &mut spec.fill_color,
                            &mut spec.fill_color_expr,
                            manual_color,
                            manual_color_hex,
                            allow_color_expression,
                            request_screen_color_pick,
                            pending_screen_color_target,
                            true,
                            false,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                        );
                        changed |= Self::geometry_expr_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "fill_opacity",
                            Self::tr_lang(language, "Fill Opacity", "Fill Opacity"),
                            &mut spec.fill_opacity_expr,
                            120.0,
                            120.0,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                        );
                    }
                });
        } else {
            Grid::new((preset_id, object_id, "geometry-spec-grid"))
                .num_columns(4)
                .spacing([Self::GEOMETRY_GRID_SPACING_X, 6.0])
                .min_col_width(0.0)
                .show(ui, |ui| match spec.shape {
                    GeometryShapeKind::Point => {
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "pos",
                            0,
                            "X",
                            &mut spec.x1_expr,
                            120.0,
                            120.0,
                            "Y",
                            &mut spec.y1_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "styling",
                            255,
                            Self::tr_lang(language, "Size", "Size"),
                            &mut spec.radius_expr,
                            120.0,
                            120.0,
                            Self::tr_lang(language, "Opacity", "Opacity"),
                            &mut spec.opacity_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                    }
                    GeometryShapeKind::Line | GeometryShapeKind::Arrow => {
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "pos1",
                            0,
                            "X1",
                            &mut spec.x1_expr,
                            120.0,
                            120.0,
                            "Y1",
                            &mut spec.y1_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "pos2",
                            1,
                            "X2",
                            &mut spec.x2_expr,
                            120.0,
                            120.0,
                            "Y2",
                            &mut spec.y2_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        if spec.shape == GeometryShapeKind::Arrow {
                            changed |= Self::geometry_expr_pair_row(
                                ui,
                                language,
                                preset_id,
                                object_id,
                                "arrow_styling",
                                255,
                                Self::tr_lang(language, "Head", "Head"),
                                &mut spec.arrow_head_size_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Thickness", "Thickness"),
                                &mut spec.thickness_expr,
                                120.0,
                                120.0,
                                begin_mouse_move_absolute_capture_target,
                                vietnamese_input_enabled,
                                vietnamese_input_mode,
                                group_id_override,
                            );
                            changed |= Self::geometry_expr_pair_row(
                                ui,
                                language,
                                preset_id,
                                object_id,
                                "opacity",
                                255,
                                Self::tr_lang(language, "Opacity", "Opacity"),
                                &mut spec.opacity_expr,
                                120.0,
                                120.0,
                                "",
                                &mut String::new(),
                                0.0,
                                0.0,
                                begin_mouse_move_absolute_capture_target,
                                vietnamese_input_enabled,
                                vietnamese_input_mode,
                                group_id_override,
                            );
                        } else {
                            changed |= Self::geometry_expr_pair_row(
                                ui,
                                language,
                                preset_id,
                                object_id,
                                "styling",
                                255,
                                Self::tr_lang(language, "Thickness", "Thickness"),
                                &mut spec.thickness_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Opacity", "Opacity"),
                                &mut spec.opacity_expr,
                                120.0,
                                120.0,
                                begin_mouse_move_absolute_capture_target,
                                vietnamese_input_enabled,
                                vietnamese_input_mode,
                                group_id_override,
                            );
                        }
                    }
                    GeometryShapeKind::Circle => {
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "pos",
                            0,
                            "CX",
                            &mut spec.x1_expr,
                            120.0,
                            120.0,
                            "CY",
                            &mut spec.y1_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "styling",
                            255,
                            Self::tr_lang(language, "Radius", "Radius"),
                            &mut spec.radius_expr,
                            120.0,
                            120.0,
                            Self::tr_lang(language, "Thickness", "Thickness"),
                            &mut spec.thickness_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        if spec.filled {
                            changed |= Self::geometry_expr_pair_row(
                                ui,
                                language,
                                preset_id,
                                object_id,
                                "opacity",
                                255,
                                Self::tr_lang(language, "Opacity", "Opacity"),
                                &mut spec.opacity_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Fill Opacity", "Fill Opacity"),
                                &mut spec.fill_opacity_expr,
                                120.0,
                                120.0,
                                begin_mouse_move_absolute_capture_target,
                                vietnamese_input_enabled,
                                vietnamese_input_mode,
                                group_id_override,
                            );
                        } else {
                            changed |= Self::geometry_expr_pair_row(
                                ui,
                                language,
                                preset_id,
                                object_id,
                                "opacity",
                                255,
                                Self::tr_lang(language, "Opacity", "Opacity"),
                                &mut spec.opacity_expr,
                                120.0,
                                120.0,
                                "",
                                &mut String::new(),
                                0.0,
                                0.0,
                                begin_mouse_move_absolute_capture_target,
                                vietnamese_input_enabled,
                                vietnamese_input_mode,
                                group_id_override,
                            );
                        }
                        changed |= Self::geometry_fill_mode_row(ui, language, &mut spec.filled);
                    }
                    GeometryShapeKind::Rectangle => {
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "pos",
                            0,
                            "X",
                            &mut spec.x1_expr,
                            120.0,
                            120.0,
                            "Y",
                            &mut spec.y1_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "dims",
                            255,
                            "W",
                            &mut spec.width_expr,
                            120.0,
                            120.0,
                            "H",
                            &mut spec.height_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "styling",
                            255,
                            Self::tr_lang(language, "Thickness", "Thickness"),
                            &mut spec.thickness_expr,
                            120.0,
                            120.0,
                            Self::tr_lang(language, "Opacity", "Opacity"),
                            &mut spec.opacity_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        if spec.filled {
                            changed |= Self::geometry_expr_pair_row(
                                ui,
                                language,
                                preset_id,
                                object_id,
                                "fill_opacity",
                                255,
                                Self::tr_lang(language, "Fill Opacity", "Fill Opacity"),
                                &mut spec.fill_opacity_expr,
                                120.0,
                                120.0,
                                "",
                                &mut String::new(),
                                0.0,
                                0.0,
                                begin_mouse_move_absolute_capture_target,
                                vietnamese_input_enabled,
                                vietnamese_input_mode,
                                group_id_override,
                            );
                        }
                        changed |= Self::geometry_fill_mode_row(ui, language, &mut spec.filled);
                    }
                    GeometryShapeKind::Label => {
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "pos",
                            0,
                            "X",
                            &mut spec.x1_expr,
                            120.0,
                            120.0,
                            "Y",
                            &mut spec.y1_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        ui.add_sized(
                            [Self::GEOMETRY_LABEL_COL_WIDTH, 18.0],
                            egui::Label::new(Self::tr_lang(language, "Text", "Text")),
                        );
                        let text_id = ui.make_persistent_id((preset_id, object_id, "label-text"));
                        let response = Self::render_interpolated_text_edit(
                            ui,
                            &mut spec.text,
                            text_id,
                            120.0,
                            120.0,
                            18.0,
                            18.0,
                            "Text",
                            false,
                        );
                        changed |= response.changed();
                        Self::apply_vietnamese_input_if_changed(
                            &response,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut spec.text,
                        );
                        ui.label("");
                        ui.label("");
                        ui.add_space(24.0);
                        ui.end_row();
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "styling",
                            255,
                            Self::tr_lang(language, "Size", "Size"),
                            &mut spec.font_size_expr,
                            120.0,
                            120.0,
                            Self::tr_lang(language, "Opacity", "Opacity"),
                            &mut spec.opacity_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                    }
                    GeometryShapeKind::Ellipse => {
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "pos",
                            0,
                            "CX",
                            &mut spec.x1_expr,
                            120.0,
                            120.0,
                            "CY",
                            &mut spec.y1_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "dims",
                            255,
                            "RX",
                            &mut spec.radius_x_expr,
                            120.0,
                            120.0,
                            "RY",
                            &mut spec.radius_y_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "styling",
                            255,
                            Self::tr_lang(language, "Thickness", "Thickness"),
                            &mut spec.thickness_expr,
                            120.0,
                            120.0,
                            Self::tr_lang(language, "Opacity", "Opacity"),
                            &mut spec.opacity_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        if spec.filled {
                            changed |= Self::geometry_expr_pair_row(
                                ui,
                                language,
                                preset_id,
                                object_id,
                                "fill_opacity",
                                255,
                                Self::tr_lang(language, "Fill Opacity", "Fill Opacity"),
                                &mut spec.fill_opacity_expr,
                                120.0,
                                120.0,
                                "",
                                &mut String::new(),
                                0.0,
                                0.0,
                                begin_mouse_move_absolute_capture_target,
                                vietnamese_input_enabled,
                                vietnamese_input_mode,
                                group_id_override,
                            );
                        }
                        changed |= Self::geometry_fill_mode_row(ui, language, &mut spec.filled);
                    }
                    GeometryShapeKind::Arc => {
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "pos",
                            0,
                            "CX",
                            &mut spec.x1_expr,
                            120.0,
                            120.0,
                            "CY",
                            &mut spec.y1_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "dims",
                            255,
                            "RX",
                            &mut spec.radius_x_expr,
                            120.0,
                            120.0,
                            "RY",
                            &mut spec.radius_y_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "angles",
                            255,
                            Self::tr_lang(language, "Start", "Start"),
                            &mut spec.start_angle_expr,
                            120.0,
                            120.0,
                            Self::tr_lang(language, "End", "End"),
                            &mut spec.end_angle_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "styling",
                            255,
                            Self::tr_lang(language, "Thickness", "Thickness"),
                            &mut spec.thickness_expr,
                            120.0,
                            120.0,
                            Self::tr_lang(language, "Opacity", "Opacity"),
                            &mut spec.opacity_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                    }
                    GeometryShapeKind::Svg => {
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "pos",
                            0,
                            "X",
                            &mut spec.x1_expr,
                            120.0,
                            120.0,
                            "Y",
                            &mut spec.y1_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "size",
                            255,
                            Self::tr_lang(language, "Width (0=auto)", "Width (0=auto)"),
                            &mut spec.width_expr,
                            120.0,
                            120.0,
                            Self::tr_lang(language, "Height (0=auto)", "Height (0=auto)"),
                            &mut spec.height_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                        let op_label = if spec.shape == GeometryShapeKind::Svg {
                            Self::tr_lang(language, "Opacity (0-100)", "Opacity (0-100)")
                        } else {
                            Self::tr_lang(language, "Opacity", "Opacity")
                        };
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "transform",
                            255,
                            op_label,
                            &mut spec.opacity_expr,
                            120.0,
                            120.0,
                            Self::tr_lang(language, "Rotate", "Rotate"),
                            &mut spec.rotation_expr,
                            120.0,
                            120.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                    }
                    GeometryShapeKind::Polyline | GeometryShapeKind::Polygon => unreachable!(),
                });
        }

        if spec.shape == GeometryShapeKind::Svg {
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = Self::GEOMETRY_GRID_SPACING_X;
                let label_text = Self::tr_lang(language, "SVG Code", "SVG Code");
                ui.add_sized(
                    [Self::GEOMETRY_LABEL_COL_WIDTH, 18.0],
                    egui::Label::new(label_text),
                );

                let id = ui.make_persistent_id((preset_id, object_id, "svg-text-edit"));
                let text_edit_response = Self::render_plain_text_edit(
                    ui,
                    &mut spec.text,
                    id,
                    450.0,
                    450.0,
                    18.0,
                    72.0,
                    "<svg>...</svg>",
                    true,
                );
                changed |= text_edit_response.changed();
                Self::apply_vietnamese_input_if_changed(
                    &text_edit_response,
                    vietnamese_input_enabled,
                    vietnamese_input_mode,
                    &mut spec.text,
                );
            });
        }

        {
            Grid::new((preset_id, object_id, "geometry-spec-grid-2"))
                .num_columns(4)
                .spacing([Self::GEOMETRY_GRID_SPACING_X, 6.0])
                .min_col_width(0.0)
                .show(ui, |ui| {
                    if matches!(
                        spec.shape,
                        GeometryShapeKind::Line
                            | GeometryShapeKind::Rectangle
                            | GeometryShapeKind::Label
                            | GeometryShapeKind::Ellipse
                            | GeometryShapeKind::Arrow
                            | GeometryShapeKind::Arc
                    ) {
                        changed |= Self::geometry_expr_pair_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            "rotation",
                            255,
                            Self::tr_lang(language, "Rotate", "Rotate"),
                            &mut spec.rotation_expr,
                            120.0,
                            120.0,
                            "",
                            &mut String::new(),
                            0.0,
                            0.0,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            group_id_override,
                        );
                    }

                    let stroke_label = if matches!(
                        spec.shape,
                        GeometryShapeKind::Circle
                            | GeometryShapeKind::Rectangle
                            | GeometryShapeKind::Ellipse
                    ) {
                        Self::tr_lang(language, "Stroke", "Stroke")
                    } else {
                        Self::tr_lang(language, "Color", "Color")
                    };

                    changed |= Self::geometry_color_row(
                        ui,
                        language,
                        preset_id,
                        object_id,
                        stroke_label,
                        &mut spec.stroke_color,
                        &mut spec.stroke_color_expr,
                        manual_color,
                        manual_color_hex,
                        allow_color_expression,
                        request_screen_color_pick,
                        pending_screen_color_target,
                        false,
                        false,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                    );

                    if spec.filled
                        && matches!(
                            spec.shape,
                            GeometryShapeKind::Circle
                                | GeometryShapeKind::Rectangle
                                | GeometryShapeKind::Ellipse
                        )
                    {
                        changed |= Self::geometry_color_row(
                            ui,
                            language,
                            preset_id,
                            object_id,
                            Self::tr_lang(language, "Fill", "Fill"),
                            &mut spec.fill_color,
                            &mut spec.fill_color_expr,
                            manual_color,
                            manual_color_hex,
                            allow_color_expression,
                            request_screen_color_pick,
                            pending_screen_color_target,
                            true,
                            false,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                        );
                    }
                });
        }

        changed
    }

    pub(crate) fn geometry_expr_row(
        ui: &mut egui::Ui,
        language: UiLanguage,
        preset_id: u32,
        object_id: u32,
        row_id: &str,
        label: &str,
        expr: &mut String,
        width: f32,
        expanded_width: f32,
        vietnamese_input_enabled: bool,
        vietnamese_input_mode: VietnameseInputMode,
    ) -> bool {
        let mut changed = false;
        let width = width.min(Self::GEOMETRY_FIELD_WIDTH);
        let expanded_width = expanded_width.min(Self::GEOMETRY_FIELD_EXPANDED_WIDTH);
        ui.add_sized(
            [Self::GEOMETRY_LABEL_COL_WIDTH, 18.0],
            egui::Label::new(label),
        );
        let id = ui.make_persistent_id((preset_id, object_id, row_id, "expr"));
        let response = Self::render_variable_text_edit(
            ui,
            expr,
            id,
            width,
            expanded_width,
            18.0,
            18.0,
            "",
            false,
        );
        changed |= response.changed();
        Self::apply_vietnamese_input_if_changed(
            &response,
            vietnamese_input_enabled,
            vietnamese_input_mode,
            expr,
        );
        Self::render_variable_suggestions(ui, &response, expr, &[], language);
        ui.end_row();
        changed
    }

    pub(crate) fn geometry_expr_pair_row(
        ui: &mut egui::Ui,
        language: UiLanguage,
        preset_id: u32,
        object_id: u32,
        row_id: &str,
        pair_index: u8,
        label_a: &str,
        expr_a: &mut String,
        width_a: f32,
        expanded_width_a: f32,
        label_b: &str,
        expr_b: &mut String,
        width_b: f32,
        expanded_width_b: f32,
        begin_mouse_move_absolute_capture_target: &mut Option<MouseMoveAbsoluteCaptureTarget>,
        vietnamese_input_enabled: bool,
        vietnamese_input_mode: VietnameseInputMode,
        group_id_override: Option<u32>,
    ) -> bool {
        let mut changed = false;
        let width_a = width_a.min(Self::GEOMETRY_FIELD_WIDTH);
        let expanded_width_a = expanded_width_a.min(Self::GEOMETRY_FIELD_EXPANDED_WIDTH);
        let width_b = width_b.min(Self::GEOMETRY_FIELD_WIDTH);
        let expanded_width_b = expanded_width_b.min(Self::GEOMETRY_FIELD_EXPANDED_WIDTH);

        if !label_a.is_empty() {
            ui.add_sized(
                [Self::GEOMETRY_LABEL_COL_WIDTH, 18.0],
                egui::Label::new(label_a),
            );
            let id_a = ui.make_persistent_id((preset_id, object_id, row_id, "expr-a"));
            let response_a = Self::render_variable_text_edit(
                ui,
                expr_a,
                id_a,
                width_a,
                expanded_width_a,
                18.0,
                18.0,
                "",
                false,
            );
            changed |= response_a.changed();
            Self::apply_vietnamese_input_if_changed(
                &response_a,
                vietnamese_input_enabled,
                vietnamese_input_mode,
                expr_a,
            );
            Self::render_variable_suggestions(ui, &response_a, expr_a, &[], language);
        } else {
            ui.label("");
            ui.label("");
        }

        if !label_b.is_empty() {
            ui.add_sized(
                [Self::GEOMETRY_LABEL_COL_WIDTH, 18.0],
                egui::Label::new(label_b),
            );
            let id_b = ui.make_persistent_id((preset_id, object_id, row_id, "expr-b"));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = Self::GEOMETRY_GRID_SPACING_X;
                let response_b = Self::render_variable_text_edit(
                    ui,
                    expr_b,
                    id_b,
                    width_b,
                    expanded_width_b,
                    18.0,
                    18.0,
                    "",
                    false,
                );
                changed |= response_b.changed();
                Self::apply_vietnamese_input_if_changed(
                    &response_b,
                    vietnamese_input_enabled,
                    vietnamese_input_mode,
                    expr_b,
                );
                Self::render_variable_suggestions(ui, &response_b, expr_b, &[], language);

                if pair_index != 255 {
                    let capture_kind = if pair_index == 1 {
                        MouseCaptureKind::GeometrySecondaryPos
                    } else {
                        MouseCaptureKind::GeometryPrimaryPos
                    };
                    if ui
                        .add_sized(
                            [24.0, 21.0],
                            Button::new(Self::material_icon_text(0xe55f, 16.0)),
                        )
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Pick coordinates from screen",
                            "Pick coordinates from screen",
                        ))
                        .clicked()
                    {
                        *begin_mouse_move_absolute_capture_target =
                            Some(MouseMoveAbsoluteCaptureTarget {
                                group_id: group_id_override,
                                preset_id,
                                step_index: object_id as usize,
                                capture_kind,
                                extra_cond_index: None,
                                is_hold_stop: false,
                            });
                    }
                }
            });
        } else {
            ui.label("");
            ui.label("");
        }

        ui.end_row();
        changed
    }

    pub(crate) fn geometry_fill_mode_row(
        ui: &mut egui::Ui,
        language: UiLanguage,
        filled: &mut bool,
    ) -> bool {
        let mut changed = false;
        ui.add_sized(
            [Self::GEOMETRY_LABEL_COL_WIDTH, 18.0],
            egui::Label::new(Self::tr_lang(language, "Mode", "Mode")),
        );
        ComboBox::from_id_salt(ui.next_auto_id())
            .width(Self::GEOMETRY_FIELD_WIDTH)
            .selected_text(if *filled {
                Self::tr_lang(language, "Filled", "Filled")
            } else {
                Self::tr_lang(language, "Outline", "Outline")
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(filled, false, Self::tr_lang(language, "Outline", "Outline"))
                    .changed();
                changed |= ui
                    .selectable_value(filled, true, Self::tr_lang(language, "Filled", "Filled"))
                    .changed();
            });
        ui.end_row();
        changed
    }

    pub(crate) fn geometry_color_row(
        ui: &mut egui::Ui,
        language: UiLanguage,
        preset_id: u32,
        object_id: u32,
        label: &str,
        color: &mut crate::model::RgbaColor,
        expr: &mut String,
        manual_color: &mut crate::model::RgbaColor,
        manual_color_hex: &mut String,
        allow_color_expression: bool,
        request_screen_color_pick: &mut bool,
        pending_screen_color_target: &mut Option<(u32, u32, bool)>,
        is_fill: bool,
        empty_expr_means_unset: bool,
        vietnamese_input_enabled: bool,
        vietnamese_input_mode: VietnameseInputMode,
    ) -> bool {
        let mut changed = false;
        if allow_color_expression && !empty_expr_means_unset && expr.trim().is_empty() {
            *expr = Self::geometry_color_expr_literal(*color);
            changed = true;
        }
        let display_color = if empty_expr_means_unset && expr.trim().is_empty() {
            None
        } else {
            Some(*color)
        };
        let color_tooltip = format!(
            "#{:02X}{:02X}{:02X}{:02X} rgba({}, {}, {}, {})",
            color.r, color.g, color.b, color.a, color.r, color.g, color.b, color.a
        );
        let empty_tooltip = Self::tr_lang(
            language,
            "No override color set yet.",
            "No override color set yet.",
        );
        ui.add_sized(
            [Self::GEOMETRY_LABEL_COL_WIDTH, 18.0],
            egui::Label::new(label),
        );
        ui.horizontal(|ui| {
            if allow_color_expression {
                let color_expr_id =
                    ui.make_persistent_id((preset_id, object_id, label, "color-expr"));
                let expr_response = Self::render_variable_text_edit(
                    ui,
                    expr,
                    color_expr_id,
                    Self::GEOMETRY_FIELD_WIDTH,
                    Self::GEOMETRY_FIELD_EXPANDED_WIDTH,
                    18.0,
                    18.0,
                    "{A} or #RRGGBB",
                    false,
                );
                changed |= expr_response.changed();
                Self::apply_vietnamese_input_if_changed(
                    &expr_response,
                    vietnamese_input_enabled,
                    vietnamese_input_mode,
                    expr,
                );
                Self::render_variable_suggestions(ui, &expr_response, expr, &[], language);
                expr_response.on_hover_text(Self::tr_lang(
                    language,
                    "Optional color expression. Example: {A} or #BAD1C4",
                    "Optional color expression. Example: {A} or #BAD1C4",
                ));
            }

            let _swatch_response = ui
                .scope(|ui| {
                    Self::image_search_target_color_swatch(
                        ui,
                        display_color,
                        egui::vec2(24.0, 24.0),
                    );
                })
                .response
                .on_hover_text(if display_color.is_some() {
                    color_tooltip.clone()
                } else {
                    empty_tooltip.to_owned()
                });

            let popup_id =
                ui.make_persistent_id((preset_id, object_id, label, "geometry-color-popup"));
            let mut popup_open = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(popup_id))
                .unwrap_or(false);

            let palette_button = ui
                .add_sized(
                    [24.0, 21.0],
                    Button::new(Self::material_icon_text(0xe40a, 16.0)),
                )
                .on_hover_text(Self::tr_lang(language, "Choose color", "Choose color"));
            if palette_button.clicked() {
                *manual_color = *color;
                *manual_color_hex = format!(
                    "{:02X}{:02X}{:02X}{:02X}",
                    color.r, color.g, color.b, color.a
                );
                popup_open = true;
            }

            let popup_response = egui::Popup::from_response(&palette_button)
                .id(popup_id)
                .open_bool(&mut popup_open)
                .align(egui::RectAlign::BOTTOM_START)
                .layout(egui::Layout::top_down_justified(egui::Align::Min))
                .width(260.0)
                .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                .show(|ui| {
                    ui.set_min_width(260.0);
                    if Self::render_premium_color_picker(
                        ui,
                        manual_color,
                        egui::color_picker::Alpha::BlendOrAdditive,
                    ) {
                        *manual_color_hex = format!(
                            "{:02X}{:02X}{:02X}{:02X}",
                            manual_color.r, manual_color.g, manual_color.b, manual_color.a
                        );
                        *color = *manual_color;
                        *expr = Self::geometry_color_expr_literal(*manual_color);
                        changed = true;
                    }
                });

            if popup_open && let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
                let mut keep_open_rect = palette_button.rect.expand(10.0);
                if let Some(popup) = &popup_response {
                    keep_open_rect = keep_open_rect.union(popup.response.rect.expand(10.0));
                }
                if !keep_open_rect.contains(pointer_pos) {
                    popup_open = false;
                }
            }
            ui.ctx()
                .data_mut(|data| data.insert_temp(popup_id, popup_open));

            let screen_pick_response = ui
                .add_sized(
                    [24.0, 21.0],
                    Button::new(Self::material_icon_text(0xe3b8, 16.0)),
                )
                .on_hover_text(Self::tr_lang(
                    language,
                    "Pick from screen",
                    "Pick from screen",
                ));
            if screen_pick_response.clicked() {
                *request_screen_color_pick = true;
                *pending_screen_color_target = Some((preset_id, object_id, is_fill));
            }
        });
        ui.end_row();
        changed
    }
}
