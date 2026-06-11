use crate::model::{GeometryObject, GeometryPreset, GeometryShapeKind, GeometrySpec, VietnameseInputMode};
use crate::ui::{CrosshairApp, MouseCaptureKind, MouseMoveAbsoluteCaptureTarget, UiLanguage};
use eframe::egui::{self, Button, ComboBox, Frame, Grid, TextEdit};

impl CrosshairApp {
    const GEOMETRY_LABEL_COL_WIDTH: f32 = 48.0;
    const GEOMETRY_FIELD_WIDTH: f32 = 96.0;
    const GEOMETRY_FIELD_EXPANDED_WIDTH: f32 = 120.0;
    const GEOMETRY_GRID_SPACING_X: f32 = 2.0;

    pub(crate) fn render_geometry_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        let mut changed = false;
        let mut remove_preset_id = None;
        let mut request_screen_color_pick = false;
        let mut pending_screen_color_target: Option<(u32, u32, bool)> = None;
        let mut clear_preview_target = false;
        let mut begin_mouse_move_absolute_capture_target = None;


        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(Self::tr_lang(language, "+ Add geometry preset", "+ Thêm preset hình học"))
                .clicked()
            {
                let id = self.state.geometry_presets.iter().map(|p| p.id).max().unwrap_or(0) + 1;
                self.state.next_geometry_preset_id = id + 1;
                self.state
                    .geometry_presets
                    .push(GeometryPreset::new(id));
                changed = true;
            }
        });

        ui.add_space(8.0);

        for preset_index in 0..self.state.geometry_presets.len() {
            let preset = &mut self.state.geometry_presets[preset_index];
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
                            .on_hover_text(Self::tr_lang(language, "Delete preset", "Xoá preset"))
                            .clicked()
                        {
                            remove_preset_id = Some(preset.id);
                            if self.geometry_preview_target.is_some_and(|(preview_preset_id, _)| preview_preset_id == preset.id)
                            {
                                clear_preview_target = true;
                            }
                            if self.geometry_preset_preview_target == Some(preset.id) {
                                self.geometry_preset_preview_target = None;
                                let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometryPreset(None));
                            }
                        }
                        if Self::sound_style_toggle_button(
                            ui,
                            if preset.collapsed {
                                Self::tr_lang(language, "Show", "Hiện")
                            } else {
                                Self::tr_lang(language, "Hide", "Ẩn")
                            },
                        )
                        .clicked()
                        {
                            preset.collapsed = !preset.collapsed;
                            changed = true;
                        }
                        if ui
                            .add_sized(
                                [100.0, 24.0],
                                Button::new(Self::tr_lang(language, "+ New object", "+ Đối tượng mới")),
                            )
                            .clicked()
                        {
                            let object_id = preset.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
                            self.state.next_geometry_object_id = object_id + 1;
                            preset
                                .objects
                                .push(GeometryObject::new(object_id, GeometryShapeKind::Point));
                            preset.collapsed = false;
                            changed = true;
                        }

                        let preview_all_active = self.geometry_preset_preview_target == Some(preset.id);
                        let preview_all_btn = Button::new(Self::material_icon_text(
                            if preview_all_active { 0xe8f5 } else { 0xe8f4 },
                            18.0,
                        ));
                        if ui
                            .add_sized([36.0, 24.0], preview_all_btn)
                            .on_hover_text(if preview_all_active { Self::tr_lang(language, "Stop Preview All", "Dừng xem trước tất cả") } else { Self::tr_lang(language, "Preview All", "Xem trước tất cả") })
                            .clicked()
                        {
                            if preview_all_active {
                                self.geometry_preset_preview_target = None;
                                let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometryPreset(None));
                            } else {
                                self.geometry_preset_preview_target = Some(preset.id);
                                let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometryPreset(Some(preset.id)));
                            }
                        }
                    });
                });

                if preset.collapsed {
                    return;
                }

                let mut remove_object_id = None;
                for object in &mut preset.objects {
                    ui.add_space(6.0);
                    let mut frame = Frame::group(ui.style());
                    if object.enabled {
                        frame = frame
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 255, 170)))
                            .fill(egui::Color32::from_rgba_unmultiplied(0, 255, 170, 30));
                    }
                    let card_width = ui.available_width() - 16.0;
                    frame.inner_margin(8).show(ui, |ui| {
                        ui.set_min_width(card_width);
                        ui.horizontal(|ui| {
                            let preview_active =
                                self.geometry_preview_target == Some((preset.id, object.id));
                            let checkbox_response = {
                                let old_icon_width = ui.spacing().icon_width;
                                ui.spacing_mut().icon_width = 20.0;
                                let r = ui.checkbox(&mut object.enabled, "");
                                ui.spacing_mut().icon_width = old_icon_width;
                                r
                            };
                            if checkbox_response.changed() {
                                changed = true;
                            }
                            let response = ui.add_sized(
                                [180.0, 24.0],
                                TextEdit::singleline(&mut object.name),
                            );
                            Self::apply_vietnamese_input_if_changed(
                                &response,
                                self.state.vietnamese_input_enabled,
                                self.state.vietnamese_input_mode,
                                &mut object.name,
                            );
                            changed |= response.changed();

                            ComboBox::from_id_salt((preset.id, object.id, "shape"))
                                .width(132.0)
                                .selected_text(Self::geometry_shape_label(object.spec.shape, language))
                                .show_ui(ui, |ui| {
                                    for shape in Self::geometry_shapes() {
                                        let response = ui.selectable_value(
                                            &mut object.spec.shape,
                                            shape,
                                            Self::geometry_shape_label(shape, language),
                                        );
                                        if response.changed() {
                                            changed = true;
                                            if object.spec.shape == GeometryShapeKind::Svg {
                                                if object.spec.text == "Label" {
                                                    object.spec.text = String::new();
                                                }
                                                if object.spec.opacity_expr == "1" {
                                                    object.spec.opacity_expr = "100".to_owned();
                                                }
                                            }
                                        }
                                    }
                                });

                            let preview_btn = Button::new(Self::material_icon_text(
                                if preview_active { 0xe8f5 } else { 0xe8f4 },
                                16.0,
                            ));
                            let preview_response = ui.add_sized([24.0, 21.0], preview_btn);
                            if preview_response.on_hover_text(if preview_active { Self::tr_lang(language, "Stop preview", "Dừng xem trước") } else { Self::tr_lang(language, "Preview", "Xem trước") }).clicked() {
                                if preview_active {
                                    self.geometry_preview_target = None;
                                    self.geometry_preview_sent = None;
                                    let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometrySpec(None));
                                } else {
                                    self.geometry_preview_target = Some((preset.id, object.id));
                                    self.geometry_preview_sent = Some(object.spec.clone());
                                    let _ = self.overlay_tx.send(
                                        crate::overlay::OverlayCommand::PreviewGeometrySpec(
                                            Some(object.spec.clone()),
                                        ),
                                    );
                                }
                            }

                            if ui
                                .add_sized(
                                    [24.0, 21.0],
                                    Button::new(Self::material_icon_text(0xe5cd, 16.0)),
                                )
                                .on_hover_text(Self::tr_lang(language, "Delete object", "Xoá đối tượng"))
                                .clicked()
                            {
                                remove_object_id = Some(object.id);
                                if self.geometry_preview_target == Some((preset.id, object.id)) {
                                    clear_preview_target = true;
                                }
                            }
                        });

                            ui.add_space(6.0);
                            changed |= Self::render_geometry_spec_editor(
                                ui,
                                language,
                                preset.id,
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

                if let Some(object_id) = remove_object_id {
                    preset.objects.retain(|object| object.id != object_id);
                    changed = true;
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
            self.sync_geometry_presets();
            self.persist();
        }

        if clear_preview_target {
            self.geometry_preview_target = None;
            self.geometry_preview_sent = None;
            let _ = self
                .overlay_tx
                .send(crate::overlay::OverlayCommand::PreviewGeometrySpec(None));
        } else if let Some((preview_preset_id, preview_object_id)) = self.geometry_preview_target {
            let preview_spec = self
                .state
                .geometry_presets
                .iter()
                .find(|preset| preset.id == preview_preset_id)
                .and_then(|preset| preset.objects.iter().find(|object| object.id == preview_object_id))
                .map(|object| object.spec.clone());
            if preview_spec.is_none() {
                self.geometry_preview_target = None;
                self.geometry_preview_sent = None;
            }
            if self.geometry_preview_sent != preview_spec {
                self.geometry_preview_sent = preview_spec.clone();
                let _ = self
                    .overlay_tx
                    .send(crate::overlay::OverlayCommand::PreviewGeometrySpec(preview_spec));
            }
        }

        if let Some(preview_preset_id) = self.geometry_preset_preview_target {
            let exists = self
                .state
                .geometry_presets
                .iter()
                .any(|preset| preset.id == preview_preset_id);
            if !exists {
                self.geometry_preset_preview_target = None;
                let _ = self
                    .overlay_tx
                    .send(crate::overlay::OverlayCommand::PreviewGeometryPreset(None));
            }
        }

        if request_screen_color_pick {
            self.geometry_color_pick_target = pending_screen_color_target;
            self.begin_image_search_capture(
                ui.ctx(),
                crate::ui::VisionCaptureTarget::GeometryColor,
                crate::ui::VisionCaptureMode::ColorSample,
            );
        }

        if let Some(target) = begin_mouse_move_absolute_capture_target {
            self.begin_mouse_move_absolute_capture(ui.ctx(), target);
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

    pub(crate) fn geometry_shape_label(shape: GeometryShapeKind, language: crate::model::UiLanguage) -> &'static str {
        match language {
            crate::model::UiLanguage::Vietnamese => match shape {
                GeometryShapeKind::Point => "Điểm",
                GeometryShapeKind::Line => "Đường thẳng",
                GeometryShapeKind::Circle => "Hình tròn",
                GeometryShapeKind::Rectangle => "Hình chữ nhật",
                GeometryShapeKind::Label => "Văn bản/Nhãn",
                GeometryShapeKind::Ellipse => "Hình elip",
                GeometryShapeKind::Arrow => "Mũi tên",
                GeometryShapeKind::Polyline => "Đường gấp khúc",
                GeometryShapeKind::Polygon => "Đa giác",
                GeometryShapeKind::Arc => "Cung tròn",
                GeometryShapeKind::Svg => "Ảnh SVG",
            },
            _ => match shape {
                GeometryShapeKind::Point => "Point",
                GeometryShapeKind::Line => "Line",
                GeometryShapeKind::Circle => "Circle",
                GeometryShapeKind::Rectangle => "Rectangle",
                GeometryShapeKind::Label => "Label",
                GeometryShapeKind::Ellipse => "Ellipse",
                GeometryShapeKind::Arrow => "Arrow",
                GeometryShapeKind::Polyline => "Polyline",
                GeometryShapeKind::Polygon => "Polygon",
                GeometryShapeKind::Arc => "Arc",
                GeometryShapeKind::Svg => "SVG Image",
            }
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
        if matches!(spec.shape, GeometryShapeKind::Polyline | GeometryShapeKind::Polygon) {
            let mut points: Vec<(String, String)> = spec.points_expr
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
                            ui,
                            x_val,
                            x_id,
                            80.0,
                            120.0,
                            18.0,
                            18.0,
                            "",
                            false,
                        );
                        points_changed |= response_x.changed();
                        Self::apply_vietnamese_input_if_changed(
                            &response_x,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            x_val,
                        );

                        ui.add_sized([16.0, 18.0], egui::Label::new("Y"));
                        let y_id = ui.make_persistent_id((preset_id, object_id, idx, "poly-y"));
                        let response_y = Self::render_variable_text_edit(
                            ui,
                            y_val,
                            y_id,
                            80.0,
                            120.0,
                            18.0,
                            18.0,
                            "",
                            false,
                        );
                        points_changed |= response_y.changed();
                        Self::apply_vietnamese_input_if_changed(
                            &response_y,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            y_val,
                        );

                        if ui
                            .add_sized(
                                [24.0, 21.0],
                                Button::new(Self::material_icon_text(0xe55f, 16.0)),
                            )
                            .on_hover_text(Self::tr_lang(language, "Pick coordinates from screen", "Chon toa do tu man hinh"))
                            .clicked()
                        {
                            *begin_mouse_move_absolute_capture_target = Some(MouseMoveAbsoluteCaptureTarget {
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
                            .on_hover_text(Self::tr_lang(language, "Delete point", "Xoá điểm"))
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
            if ui.button(Self::tr_lang(language, "+ Add Point", "+ Thêm điểm")).clicked() {
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
                        preset_id,
                        object_id,
                        "thickness",
                        Self::tr_lang(language, "Thickness", "Độ dày"),
                        &mut spec.thickness_expr,
                        120.0,
                        120.0,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                    );
                    changed |= Self::geometry_expr_row(
                        ui,
                        preset_id,
                        object_id,
                        "opacity",
                        Self::tr_lang(language, "Opacity", "Độ trong suốt"),
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
                        preset_id,
                        object_id,
                        "rotation",
                        Self::tr_lang(language, "Rotate", "Xoay"),
                        &mut spec.rotation_expr,
                        120.0,
                        120.0,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                    );

                    let stroke_label = if spec.shape == GeometryShapeKind::Polygon {
                        Self::tr_lang(language, "Stroke", "Viền")
                    } else {
                        Self::tr_lang(language, "Color", "Màu sắc")
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
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                    );

                    if spec.filled && spec.shape == GeometryShapeKind::Polygon {
                        changed |= Self::geometry_color_row(
                        ui,
                        language,
                            preset_id,
                            object_id,
                            Self::tr_lang(language, "Fill", "Màu nền"),
                            &mut spec.fill_color,
                            &mut spec.fill_color_expr,
                            manual_color,
                            manual_color_hex,
                            allow_color_expression,
                            request_screen_color_pick,
                            pending_screen_color_target,
                            true,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                        );
                        changed |= Self::geometry_expr_row(
                            ui,
                            preset_id,
                            object_id,
                            "fill_opacity",
                            Self::tr_lang(language, "Fill Opacity", "Độ trong suốt nền"),
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
                .show(ui, |ui| {
                    match spec.shape {
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
                                Self::tr_lang(language, "Size", "Kích cỡ"),
                                &mut spec.radius_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Opacity", "Độ trong suốt"),
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
                                    Self::tr_lang(language, "Head", "Mũi tên"),
                                    &mut spec.arrow_head_size_expr,
                                    120.0,
                                    120.0,
                                    Self::tr_lang(language, "Thickness", "Độ dày"),
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
                                    Self::tr_lang(language, "Opacity", "Độ trong suốt"),
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
                                    Self::tr_lang(language, "Thickness", "Độ dày"),
                                    &mut spec.thickness_expr,
                                    120.0,
                                    120.0,
                                    Self::tr_lang(language, "Opacity", "Độ trong suốt"),
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
                                Self::tr_lang(language, "Radius", "Bán kính"),
                                &mut spec.radius_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Thickness", "Độ dày"),
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
                                    Self::tr_lang(language, "Opacity", "Độ trong suốt"),
                                    &mut spec.opacity_expr,
                                    120.0,
                                    120.0,
                                    Self::tr_lang(language, "Fill Opacity", "Độ trong suốt nền"),
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
                                    Self::tr_lang(language, "Opacity", "Độ trong suốt"),
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
                                Self::tr_lang(language, "Thickness", "Độ dày"),
                                &mut spec.thickness_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Opacity", "Độ trong suốt"),
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
                                    Self::tr_lang(language, "Fill Opacity", "Độ trong suốt nền"),
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
                                egui::Label::new(Self::tr_lang(language, "Text", "Văn bản")),
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
                                Self::tr_lang(language, "Size", "Kích cỡ"),
                                &mut spec.font_size_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Opacity", "Độ trong suốt"),
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
                                Self::tr_lang(language, "Thickness", "Độ dày"),
                                &mut spec.thickness_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Opacity", "Độ trong suốt"),
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
                                    Self::tr_lang(language, "Fill Opacity", "Độ trong suốt nền"),
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
                                Self::tr_lang(language, "Start", "Bắt đầu"),
                                &mut spec.start_angle_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "End", "Kết thúc"),
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
                                Self::tr_lang(language, "Thickness", "Độ dày"),
                                &mut spec.thickness_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Opacity", "Độ trong suốt"),
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
                                Self::tr_lang(language, "Width (0=auto)", "Chiều rộng (0=auto)"),
                                &mut spec.width_expr,
                                120.0,
                                120.0,
                                Self::tr_lang(language, "Height (0=auto)", "Chiều cao (0=auto)"),
                                &mut spec.height_expr,
                                120.0,
                                120.0,
                                begin_mouse_move_absolute_capture_target,
                                vietnamese_input_enabled,
                                vietnamese_input_mode,
                                group_id_override,
                            );
                            let op_label = if spec.shape == GeometryShapeKind::Svg {
                                Self::tr_lang(language, "Opacity (0-100)", "Do mo (0-100)")
                            } else {
                                Self::tr_lang(language, "Opacity", "Độ trong suốt")
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
                                Self::tr_lang(language, "Rotate", "Xoay"),
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
                    }

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
                            Self::tr_lang(language, "Rotate", "Xoay"),
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
                        Self::tr_lang(language, "Stroke", "Viền")
                    } else {
                        Self::tr_lang(language, "Color", "Màu sắc")
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
                            Self::tr_lang(language, "Fill", "Màu nền"),
                            &mut spec.fill_color,
                            &mut spec.fill_color_expr,
                            manual_color,
                            manual_color_hex,
                            allow_color_expression,
                            request_screen_color_pick,
                            pending_screen_color_target,
                            true,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                        );
                    }
                });
        }

        if spec.shape == GeometryShapeKind::Svg {
            ui.add_space(4.0);
            
            let svg_code_collapsed_id = ui.make_persistent_id((preset_id, object_id, "svg-code-collapsed"));
            let mut svg_code_collapsed = ui.data(|d| d.get_temp::<bool>(svg_code_collapsed_id)).unwrap_or(true);
            
            ui.horizontal(|ui| {
                let collapse_icon = if svg_code_collapsed { 0xe5cc } else { 0xe5cf }; // right or down arrow
                let collapse_btn = egui::Button::new(Self::material_icon_text(collapse_icon, 12.0));
                if ui.add_sized([18.0, 18.0], collapse_btn).clicked() {
                    svg_code_collapsed = !svg_code_collapsed;
                    ui.data_mut(|d| d.insert_temp(svg_code_collapsed_id, svg_code_collapsed));
                }
            });

            if !svg_code_collapsed {
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut spec.text)
                            .hint_text("<svg>...</svg>")
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(4)
                            .desired_width(450.0)
                    );
                    changed |= response.changed();
                    Self::apply_vietnamese_input_if_changed(
                        &response,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                        &mut spec.text,
                    );
                });
            }
        }

        changed
    }

    fn geometry_expr_row(
        ui: &mut egui::Ui,
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
        ui.add_sized([Self::GEOMETRY_LABEL_COL_WIDTH, 18.0], egui::Label::new(label));
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
        ui.end_row();
        changed
    }

    fn geometry_expr_pair_row(
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
            ui.add_sized([Self::GEOMETRY_LABEL_COL_WIDTH, 18.0], egui::Label::new(label_a));
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
        } else {
            ui.label("");
            ui.label("");
        }

        if !label_b.is_empty() {
            ui.add_sized([Self::GEOMETRY_LABEL_COL_WIDTH, 18.0], egui::Label::new(label_b));
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
                        .on_hover_text(Self::tr_lang(language, "Pick coordinates from screen", "Lấy toạ độ từ màn hình"))
                        .clicked()
                    {
                        *begin_mouse_move_absolute_capture_target = Some(MouseMoveAbsoluteCaptureTarget {
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

    fn geometry_fill_mode_row(
        ui: &mut egui::Ui,
        language: UiLanguage,
        filled: &mut bool,
    ) -> bool {
        let mut changed = false;
        ui.add_sized(
            [Self::GEOMETRY_LABEL_COL_WIDTH, 18.0],
            egui::Label::new(Self::tr_lang(language, "Mode", "Chế độ")),
        );
        ComboBox::from_id_salt(ui.next_auto_id())
            .width(Self::GEOMETRY_FIELD_WIDTH)
            .selected_text(if *filled {
                Self::tr_lang(language, "Filled", "Tô màu")
            } else {
                Self::tr_lang(language, "Outline", "Viền ngoài")
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(
                        filled,
                        false,
                        Self::tr_lang(language, "Outline", "Viền ngoài"),
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        filled,
                        true,
                        Self::tr_lang(language, "Filled", "Tô màu"),
                    )
                    .changed();
            });
        ui.end_row();
        changed
    }

    fn geometry_color_row(
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
        vietnamese_input_enabled: bool,
        vietnamese_input_mode: VietnameseInputMode,
    ) -> bool {
        let mut changed = false;
        let color_tooltip = format!(
            "#{:02X}{:02X}{:02X}{:02X} rgba({}, {}, {}, {})",
            color.r, color.g, color.b, color.a, color.r, color.g, color.b, color.a
        );
        ui.add_sized([Self::GEOMETRY_LABEL_COL_WIDTH, 18.0], egui::Label::new(label));
        ui.horizontal(|ui| {
            if allow_color_expression {
                let color_expr_id = ui.make_persistent_id((preset_id, object_id, label, "color-expr"));
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
                expr_response.on_hover_text(Self::tr_lang(language, "Optional color expression. Example: {A} or #BAD1C4", "Biểu thức màu tuỳ chọn. Ví dụ: {A} hoặc #BAD1C4"));
            }

            let _swatch_response = ui
                .scope(|ui| {
                    Self::image_search_target_color_swatch(ui, Some(*color), egui::vec2(24.0, 24.0));
                })
                .response
                .on_hover_text(color_tooltip.clone());

            let popup_id =
                ui.make_persistent_id((preset_id, object_id, label, "geometry-color-popup"));
            let mut popup_open = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(popup_id))
                .unwrap_or(false);

            let palette_button = ui
                .add_sized([24.0, 21.0], Button::new(Self::material_icon_text(0xe40a, 16.0)))
                .on_hover_text(Self::tr_lang(language, "Choose color", "Chọn màu"));
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
                .width(220.0)
                .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                .show(|ui| {
                    ui.set_min_width(220.0);
                    let mut color32 = egui::Color32::from_rgba_unmultiplied(
                        manual_color.r,
                        manual_color.g,
                        manual_color.b,
                        manual_color.a,
                    );
                    if egui::color_picker::color_picker_color32(
                        ui,
                        &mut color32,
                        egui::color_picker::Alpha::BlendOrAdditive,
                    ) {
                        manual_color.r = color32.r();
                        manual_color.g = color32.g();
                        manual_color.b = color32.b();
                        manual_color.a = color32.a();
                        *manual_color_hex = format!(
                            "{:02X}{:02X}{:02X}{:02X}",
                            manual_color.r, manual_color.g, manual_color.b, manual_color.a
                        );
                        *color = *manual_color;
                        expr.clear();
                        changed = true;
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("#");
                        let hex_response = ui.add(
                            TextEdit::singleline(manual_color_hex)
                                .hint_text("RRGGBB or RRGGBBAA")
                                .desired_width(132.0),
                        );
                        if hex_response.changed() {
                            let hex = manual_color_hex.trim().trim_start_matches('#');
                            if (hex.len() == 6 || hex.len() == 8)
                                && let Ok(color_value) = u32::from_str_radix(hex, 16)
                            {
                                let (r, g, b, a) = if hex.len() == 6 {
                                    (
                                        ((color_value >> 16) & 0xFF) as u8,
                                        ((color_value >> 8) & 0xFF) as u8,
                                        (color_value & 0xFF) as u8,
                                        255,
                                    )
                                } else {
                                    (
                                        ((color_value >> 24) & 0xFF) as u8,
                                        ((color_value >> 16) & 0xFF) as u8,
                                        ((color_value >> 8) & 0xFF) as u8,
                                        (color_value & 0xFF) as u8,
                                    )
                                };
                                *manual_color = crate::model::RgbaColor { r, g, b, a };
                                *color = *manual_color;
                                expr.clear();
                                changed = true;
                            }
                        }
                    });
                });

            if popup_open
                && let Some(pointer_pos) = ui.ctx().pointer_hover_pos()
            {
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
                .add_sized([24.0, 21.0], Button::new(Self::material_icon_text(0xe3b8, 16.0)))
                .on_hover_text(Self::tr_lang(language, "Pick from screen", "Chọn từ màn hình"));
            if screen_pick_response.clicked() {
                *request_screen_color_pick = true;
                *pending_screen_color_target = Some((preset_id, object_id, is_fill));
            }
        });
        ui.end_row();
        changed
    }
}
