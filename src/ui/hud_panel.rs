use crate::model::*;
use crate::overlay::OverlayCommand;
use crate::ui::CrosshairApp;
use eframe::egui::{self, Color32, RichText, Sense, Slider, TextBuffer, TextEdit, vec2};

impl CrosshairApp {
    pub(crate) fn render_hud_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(self.tr("+ Add HUD preset", "+ Add HUD preset"))
                .clicked()
            {
                self.add_toolbox_preset();
                self.persist_hud_presets();
            }
        });

        ui.add_space(16.0);
        ui.label(
            RichText::new(self.tr("Text Presets", "Text Presets"))
                .strong()
                .size(14.0),
        );
        ui.add_space(4.0);

        let mut remove_id = None;
        let mut changed = false;
        let mut active_preview: Option<HudPreset> = None;
        let mut preview_toggled_preset_id = None;
        let mut begin_hud_picker_preset_id = None;
        let mut copy_hud_preset = None;
        let mut paste_hud_after = None;
        let can_paste_hud = matches!(
            self.preset_clipboard,
            Some(crate::ui::PresetClipboard::Hud(_))
        );
        for index in 0..self.state.hud_presets.len() {
            let hud_snapshot = self.state.hud_presets[index].clone();
            let language = self.state.ui_language;
            let preset = &mut self.state.hud_presets[index];
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
                        let preview_active = preset.preview_enabled;
                        let preview_response = Self::sound_style_icon_button(
                            ui,
                            Self::material_icon_text(
                                if preview_active { 0xe047 } else { 0xe037 },
                                18.0,
                            ),
                        )
                        .on_hover_text(if preview_active {
                            Self::tr_lang(language, "Stop HUD preview", "Stop HUD preview")
                        } else {
                            Self::tr_lang(language, "Run HUD preview", "Run HUD preview")
                        });
                        if preview_response.clicked() {
                            preset.preview_enabled = !preset.preview_enabled;
                            if preset.preview_enabled {
                                preview_toggled_preset_id = Some(preset.id);
                            }
                            changed = true;
                        }
                        if ui
                            .add_enabled(
                                can_paste_hud,
                                egui::Button::new("Paste").min_size(egui::vec2(84.0, 24.0)),
                            )
                            .clicked()
                        {
                            paste_hud_after = Some(index);
                        }
                        if Self::sound_style_toggle_button(ui, "Copy").clicked() {
                            copy_hud_preset = Some(hud_snapshot.clone());
                        }

                        if Self::sound_style_remove_button(ui).clicked() {
                            remove_id = Some(preset.id);
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
                    });
                });
                if preset.collapsed {
                    if preset.preview_enabled {
                        preset.preview_enabled = false;
                        changed = true;
                    }
                    return;
                }

                egui::Grid::new((preset.id, "toolbox-preset-grid"))
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(Self::tr_lang(language, "Text", "Text"));
                        let response =
                            ui.add_sized([360.0, 21.0], TextEdit::singleline(&mut preset.text));
                        Self::apply_vietnamese_input_if_changed(
                            &response,
                            self.state.vietnamese_input_enabled,
                            self.state.vietnamese_input_mode,
                            &mut preset.text,
                        );
                        changed |= response.changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Font Size", "Font Size"));
                        changed |= ui
                            .add(
                                Slider::new(&mut preset.font_size, 1.0..=200.0)
                                    .text("px")
                                    .clamping(egui::SliderClamping::Always),
                            )
                            .changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Text Color", "Text Color"));
                        changed |= Self::edit_rgba_color(ui, &mut preset.text_color).changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(
                            language,
                            "Background Color",
                            "Background Color",
                        ));
                        changed |=
                            Self::edit_rgba_color(ui, &mut preset.background_color).changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(
                            language,
                            "Background Opacity",
                            "Background Opacity",
                        ));
                        changed |= ui
                            .add(
                                Slider::new(&mut preset.background_opacity, 0.0..=1.0)
                                    .text("")
                                    .clamping(egui::SliderClamping::Always),
                            )
                            .changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(
                            language,
                            "Rounded Background",
                            "Rounded Background",
                        ));
                        changed |= ui
                            .checkbox(
                                &mut preset.rounded_background,
                                Self::tr_lang(language, "Rounded corners", "Rounded corners"),
                            )
                            .changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Preview", "Preview"));
                        changed |= ui
                            .checkbox(
                                &mut preset.preview_enabled,
                                Self::tr_lang(
                                    language,
                                    "Stream preview in editor",
                                    "Stream preview in editor",
                                ),
                            )
                            .changed();
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.label(
                    RichText::new(Self::tr_lang(
                        language,
                        "Position Preview",
                        "Position Preview",
                    ))
                    .strong(),
                );
                changed |= Self::render_hud_rect_editor(ui, (preset.id, "toolbox-editor"), preset);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(Self::tr_lang(language, "Center X", "Center X"))
                        .clicked()
                    {
                        preset.x =
                            ((Self::screen_size().x as i32 - preset.width.max(1)) / 2).max(0);
                        changed = true;
                    }
                    if ui
                        .button(Self::tr_lang(language, "Center Y", "Center Y"))
                        .clicked()
                    {
                        preset.y =
                            ((Self::screen_size().y as i32 - preset.height.max(1)) / 2).max(0);
                        changed = true;
                    }
                    if ui
                        .button(Self::tr_lang(language, "Pick area", "Pick area"))
                        .clicked()
                    {
                        begin_hud_picker_preset_id = Some(preset.id);
                    }
                });

                if preset.preview_enabled {
                    active_preview = Some(preset.clone());
                }
            });
        }
        if let Some(pid) = begin_hud_picker_preset_id {
            self.begin_region_capture(
                ui.ctx(),
                crate::ui::VisionCaptureTarget::HudPresetRegion(pid),
            );
        }

        if let Some(preset) = copy_hud_preset {
            self.preset_clipboard = Some(crate::ui::PresetClipboard::Hud(preset));
        }
        if let Some(index) = paste_hud_after
            && let Some(crate::ui::PresetClipboard::Hud(mut preset)) =
                self.preset_clipboard.clone()
        {
            preset.id = Self::allocate_next_id(
                &self.state.hud_presets,
                &mut self.state.next_hud_preset_id,
                |item| item.id,
            );
            preset.name = format!("{} (Copy)", preset.name);
            self.state.hud_presets.insert(index + 1, preset);
            changed = true;
        }

        if let Some(id) = remove_id {
            self.state.hud_presets.retain(|preset| preset.id != id);
            changed = true;
        }
        if let Some(current_id) = preview_toggled_preset_id {
            for other_preset in &mut self.state.hud_presets {
                if other_preset.id != current_id {
                    other_preset.preview_enabled = false;
                }
            }
        }
        self.sync_hud_preview(active_preview.as_ref());
        if changed {
            self.persist_hud_presets_deferred(ui.ctx());
        }
    }

    pub(crate) fn render_timer_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        let mut remove_timer_id = None;
        let mut timer_changed = false;
        let mut active_timer_preview: Option<TimerPreset> = None;

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(self.tr("+ Add timer preset", "+ Add timer preset"))
                .clicked()
            {
                let id = Self::allocate_next_id(
                    &self.state.timer_presets,
                    &mut self.state.next_timer_preset_id,
                    |preset| preset.id,
                );
                let mut new_preset = TimerPreset::new(id);
                let mut suffix = 1;
                while self
                    .state
                    .timer_presets
                    .iter()
                    .any(|p| p.name == format!("Timer {}", suffix))
                {
                    suffix += 1;
                }
                new_preset.name = format!("Timer {}", suffix);
                self.state.timer_presets.push(new_preset);
                timer_changed = true;
            }
        });

        ui.add_space(16.0);
        ui.label(
            RichText::new(self.tr("Timer Presets", "Timer Presets"))
                .strong()
                .size(14.0),
        );
        ui.add_space(4.0);

        let mut copy_timer_preset = None;
        let mut paste_timer_after = None;
        let can_paste_timer = matches!(
            self.preset_clipboard,
            Some(crate::ui::PresetClipboard::Timer(_))
        );
        for index in 0..self.state.timer_presets.len() {
            let timer_snapshot = self.state.timer_presets[index].clone();
            let preset = &mut self.state.timer_presets[index];
            if preset.show_progress_bar || !preset.show_text {
                preset.show_progress_bar = false;
                preset.show_text = true;
                timer_changed = true;
            }
            if !preset.show_overlay && preset.preview_enabled {
                preset.preview_enabled = false;
                timer_changed = true;
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
                    timer_changed |= response.changed();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                can_paste_timer,
                                egui::Button::new("Paste").min_size(egui::vec2(84.0, 24.0)),
                            )
                            .clicked()
                        {
                            paste_timer_after = Some(index);
                        }
                        if Self::sound_style_toggle_button(ui, "Copy").clicked() {
                            copy_timer_preset = Some(timer_snapshot.clone());
                        }

                        if Self::sound_style_remove_button(ui).clicked() {
                            remove_timer_id = Some(preset.id);
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
                            timer_changed = true;
                        }
                    });
                });

                if preset.collapsed {
                    if preset.preview_enabled {
                        preset.preview_enabled = false;
                        timer_changed = true;
                    }
                    return;
                }

                egui::Grid::new((preset.id, "timer-preset-grid"))
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(Self::tr_lang(language, "Type", "Type"));
                        ui.horizontal(|ui| {
                            let mut selected_type = if preset.is_countdown { 1 } else { 0 };
                            let resp = egui::ComboBox::from_id_salt((preset.id, "timer-type-sel"))
                                .selected_text(if selected_type == 1 {
                                    Self::tr_lang(language, "Countdown", "Countdown")
                                } else {
                                    Self::tr_lang(language, "Stopwatch", "Stopwatch")
                                })
                                .show_ui(ui, |ui| {
                                    let mut changed = false;
                                    changed |= ui
                                        .selectable_value(
                                            &mut selected_type,
                                            0,
                                            Self::tr_lang(language, "Stopwatch", "Stopwatch"),
                                        )
                                        .clicked();
                                    changed |= ui
                                        .selectable_value(
                                            &mut selected_type,
                                            1,
                                            Self::tr_lang(language, "Countdown", "Countdown"),
                                        )
                                        .clicked();
                                    changed
                                });
                            if resp.inner.unwrap_or(false) {
                                preset.is_countdown = selected_type == 1;
                                timer_changed = true;
                            }
                        });
                        ui.end_row();

                        if preset.is_countdown {
                            ui.label(Self::tr_lang(language, "Duration", "Duration"));
                            timer_changed |= ui
                                .add(
                                    Slider::new(&mut preset.duration_secs, 1..=3600)
                                        .text(Self::tr_lang(language, "seconds", "seconds"))
                                        .clamping(egui::SliderClamping::Always),
                                )
                                .changed();
                            ui.end_row();
                        }

                        ui.label(Self::tr_lang(language, "Overlay", "Overlay"));
                        let overlay_changed = ui
                            .checkbox(
                                &mut preset.show_overlay,
                                Self::tr_lang(language, "Show overlay", "Hiện overlay"),
                            )
                            .changed();
                        if overlay_changed && !preset.show_overlay {
                            preset.preview_enabled = false;
                        }
                        timer_changed |= overlay_changed;
                        ui.end_row();

                        if preset.show_overlay && preset.show_text {
                            ui.label(Self::tr_lang(language, "Format", "Format"));
                            ui.horizontal(|ui| {
                                timer_changed |= ui
                                    .checkbox(
                                        &mut preset.show_minutes,
                                        Self::tr_lang(language, "Min", "Min"),
                                    )
                                    .changed();
                                timer_changed |= ui
                                    .checkbox(
                                        &mut preset.show_seconds,
                                        Self::tr_lang(language, "Sec", "Sec"),
                                    )
                                    .changed();
                                timer_changed |= ui
                                    .checkbox(
                                        &mut preset.show_ms,
                                        Self::tr_lang(language, "Ms/Ticks", "Ms/Ticks"),
                                    )
                                    .changed();
                            });
                            ui.end_row();
                        }

                        if preset.show_overlay && preset.show_text {
                            ui.label(Self::tr_lang(language, "Font Size", "Font Size"));
                            timer_changed |= ui
                                .add(
                                    Slider::new(&mut preset.font_size, 1.0..=200.0)
                                        .text("px")
                                        .clamping(egui::SliderClamping::Always),
                                )
                                .changed();
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Text Color", "Text Color"));
                            timer_changed |=
                                Self::edit_rgba_color(ui, &mut preset.text_color).changed();
                            ui.end_row();
                        }

                        if preset.show_overlay {
                            ui.label(Self::tr_lang(
                                language,
                                "Background Color",
                                "Background Color",
                            ));
                            timer_changed |=
                                Self::edit_rgba_color(ui, &mut preset.background_color).changed();
                            ui.end_row();

                            ui.label(Self::tr_lang(
                                language,
                                "Background Opacity",
                                "Background Opacity",
                            ));
                            timer_changed |= ui
                                .add(
                                    Slider::new(&mut preset.background_opacity, 0.0..=1.0)
                                        .text("")
                                        .clamping(egui::SliderClamping::Always),
                                )
                                .changed();
                            ui.end_row();

                            ui.label(Self::tr_lang(
                                language,
                                "Rounded Background",
                                "Rounded Background",
                            ));
                            timer_changed |= ui
                                .checkbox(
                                    &mut preset.rounded_background,
                                    Self::tr_lang(language, "Rounded corners", "Rounded corners"),
                                )
                                .changed();
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Preview", "Preview"));
                            timer_changed |= ui
                                .checkbox(
                                    &mut preset.preview_enabled,
                                    Self::tr_lang(
                                        language,
                                        "Stream preview in editor",
                                        "Stream preview in editor",
                                    ),
                                )
                                .changed();
                            ui.end_row();
                        }
                    });

                if preset.show_overlay {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(Self::tr_lang(
                            language,
                            "Position Preview",
                            "Position Preview",
                        ))
                        .strong(),
                    );
                    timer_changed |=
                        Self::render_timer_rect_editor(ui, (preset.id, "timer-editor"), preset);
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(Self::tr_lang(language, "Center X", "Center X"))
                            .clicked()
                        {
                            preset.x =
                                ((Self::screen_size().x as i32 - preset.width.max(1)) / 2).max(0);
                            timer_changed = true;
                        }
                        if ui
                            .button(Self::tr_lang(language, "Center Y", "Center Y"))
                            .clicked()
                        {
                            preset.y =
                                ((Self::screen_size().y as i32 - preset.height.max(1)) / 2).max(0);
                            timer_changed = true;
                        }
                    });
                }

                if preset.show_overlay && preset.preview_enabled {
                    active_timer_preview = Some(preset.clone());
                }
            });
        }

        if let Some(preset) = copy_timer_preset {
            self.preset_clipboard = Some(crate::ui::PresetClipboard::Timer(preset));
        }
        if let Some(index) = paste_timer_after
            && let Some(crate::ui::PresetClipboard::Timer(mut preset)) =
                self.preset_clipboard.clone()
        {
            preset.id = Self::allocate_next_id(
                &self.state.timer_presets,
                &mut self.state.next_timer_preset_id,
                |item| item.id,
            );
            preset.name = format!("{} (Copy)", preset.name);
            self.state.timer_presets.insert(index + 1, preset);
            timer_changed = true;
        }

        if let Some(id) = remove_timer_id {
            self.state.timer_presets.retain(|preset| preset.id != id);
            timer_changed = true;
        }

        self.sync_timer_preview(active_timer_preview.as_ref());

        if timer_changed {
            self.persist_timer_presets_deferred(ui.ctx());
        }
    }

    pub(crate) fn render_hud_rect_editor(
        ui: &mut egui::Ui,
        id_source: impl std::hash::Hash + Copy,
        preset: &mut HudPreset,
    ) -> bool {
        let mut changed = false;
        let screen_size = Self::screen_size();
        let desired = vec2(ui.available_width().max(560.0), 420.0);
        let (canvas_rect, response) =
            ui.allocate_exact_size(desired, Sense::drag().union(Sense::click()));

        let mut arrow_dx = 0;
        let mut arrow_dy = 0;
        if response.hovered() || response.has_focus() {
            ui.input(|i| {
                if i.key_pressed(egui::Key::ArrowLeft) {
                    arrow_dx -= 1;
                }
                if i.key_pressed(egui::Key::ArrowRight) {
                    arrow_dx += 1;
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    arrow_dy -= 1;
                }
                if i.key_pressed(egui::Key::ArrowDown) {
                    arrow_dy += 1;
                }
            });
            if arrow_dx != 0 || arrow_dy != 0 {
                preset.x = (preset.x + arrow_dx).clamp(0, screen_size.x.round() as i32);
                preset.y = (preset.y + arrow_dy).clamp(0, screen_size.y.round() as i32);
                changed = true;
            }
        }

        let draw_rect = canvas_rect.shrink(8.0);
        let scale = (draw_rect.width() / screen_size.x)
            .min(draw_rect.height() / screen_size.y)
            .max(0.0001);
        let preview_size = vec2(screen_size.x * scale, screen_size.y * scale);
        let preview_rect = egui::Rect::from_center_size(draw_rect.center(), preview_size);
        ui.painter().rect_filled(
            preview_rect,
            8.0,
            Color32::from_rgba_premultiplied(18, 24, 22, 220),
        );
        ui.painter().rect_stroke(
            preview_rect,
            8.0,
            egui::Stroke::new(1.0, Color32::from_rgb(104, 148, 124)),
            egui::StrokeKind::Outside,
        );

        let min_size = vec2(4.0, 4.0);
        let mut rect = egui::Rect::from_min_size(
            egui::pos2(
                preview_rect.left() + (preset.x as f32 * scale),
                preview_rect.top() + (preset.y as f32 * scale),
            ),
            vec2(
                preset.width.max(1) as f32 * scale,
                preset.height.max(1) as f32 * scale,
            ),
        )
        .intersect(preview_rect);
        if rect.width() < min_size.x {
            rect.max.x = (rect.min.x + min_size.x).min(preview_rect.right());
        }
        if rect.height() < min_size.y {
            rect.max.y = (rect.min.y + min_size.y).min(preview_rect.bottom());
        }

        let rect_id = ui.make_persistent_id((id_source, "toolbox-rect"));
        let drag_id = ui.make_persistent_id((id_source, "hud-selection-drag-handle"));
        let offset_id = ui.make_persistent_id((id_source, "hud-selection-drag-offset"));
        let anchor_id = ui.make_persistent_id((id_source, "hud-selection-drag-anchor"));

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum SelectionDragHandle {
            None,
            Center,
            TopLeft,
            TopRight,
            BottomLeft,
            BottomRight,
            Left,
            Right,
            Top,
            Bottom,
        }

        let mut active_handle: SelectionDragHandle =
            ui.data_mut(|d| d.get_temp(drag_id).unwrap_or(SelectionDragHandle::None));
        let mut drag_offset: egui::Vec2 =
            ui.data_mut(|d| d.get_temp(offset_id).unwrap_or(egui::Vec2::ZERO));
        let mut drag_anchor: egui::Pos2 =
            ui.data_mut(|d| d.get_temp(anchor_id).unwrap_or(egui::Pos2::ZERO));

        let pick_selection_drag_handle = |pointer_pos: egui::Pos2, rect: egui::Rect| {
            let dist_tl = pointer_pos.distance(rect.left_top());
            let dist_tr = pointer_pos.distance(rect.right_top());
            let dist_bl = pointer_pos.distance(rect.left_bottom());
            let dist_br = pointer_pos.distance(rect.right_bottom());
            let edge_threshold = 10.0;
            let vertical_hit_min = rect.top() - edge_threshold;
            let vertical_hit_max = rect.bottom() + edge_threshold;
            let horizontal_hit_min = rect.left() - edge_threshold;
            let horizontal_hit_max = rect.right() + edge_threshold;

            if dist_tl < 14.0 {
                SelectionDragHandle::TopLeft
            } else if dist_tr < 14.0 {
                SelectionDragHandle::TopRight
            } else if dist_bl < 14.0 {
                SelectionDragHandle::BottomLeft
            } else if dist_br < 14.0 {
                SelectionDragHandle::BottomRight
            } else if (pointer_pos.x - rect.left()).abs() < edge_threshold
                && pointer_pos.y >= vertical_hit_min
                && pointer_pos.y <= vertical_hit_max
            {
                SelectionDragHandle::Left
            } else if (pointer_pos.x - rect.right()).abs() < edge_threshold
                && pointer_pos.y >= vertical_hit_min
                && pointer_pos.y <= vertical_hit_max
            {
                SelectionDragHandle::Right
            } else if (pointer_pos.y - rect.top()).abs() < edge_threshold
                && pointer_pos.x >= horizontal_hit_min
                && pointer_pos.x <= horizontal_hit_max
            {
                SelectionDragHandle::Top
            } else if (pointer_pos.y - rect.bottom()).abs() < edge_threshold
                && pointer_pos.x >= horizontal_hit_min
                && pointer_pos.x <= horizontal_hit_max
            {
                SelectionDragHandle::Bottom
            } else if rect.contains(pointer_pos) {
                SelectionDragHandle::Center
            } else {
                SelectionDragHandle::None
            }
        };

        if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            if let Some(pointer_pos) = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos())
            {
                active_handle = pick_selection_drag_handle(pointer_pos, rect);
                ui.data_mut(|d| d.insert_temp(drag_id, active_handle));

                drag_offset = match active_handle {
                    SelectionDragHandle::Center => pointer_pos - rect.min,
                    SelectionDragHandle::Left
                    | SelectionDragHandle::TopLeft
                    | SelectionDragHandle::BottomLeft => {
                        let ox = pointer_pos.x - rect.min.x;
                        let oy = if active_handle == SelectionDragHandle::TopLeft {
                            pointer_pos.y - rect.min.y
                        } else if active_handle == SelectionDragHandle::BottomLeft {
                            pointer_pos.y - rect.max.y
                        } else {
                            0.0
                        };
                        egui::vec2(ox, oy)
                    }
                    SelectionDragHandle::Right
                    | SelectionDragHandle::TopRight
                    | SelectionDragHandle::BottomRight => {
                        let ox = pointer_pos.x - rect.max.x;
                        let oy = if active_handle == SelectionDragHandle::TopRight {
                            pointer_pos.y - rect.min.y
                        } else if active_handle == SelectionDragHandle::BottomRight {
                            pointer_pos.y - rect.max.y
                        } else {
                            0.0
                        };
                        egui::vec2(ox, oy)
                    }
                    SelectionDragHandle::Top => egui::vec2(0.0, pointer_pos.y - rect.min.y),
                    SelectionDragHandle::Bottom => egui::vec2(0.0, pointer_pos.y - rect.max.y),
                    SelectionDragHandle::None => egui::Vec2::ZERO,
                };
                ui.data_mut(|d| d.insert_temp(offset_id, drag_offset));

                drag_anchor = match active_handle {
                    SelectionDragHandle::Left | SelectionDragHandle::TopLeft => rect.max,
                    SelectionDragHandle::BottomLeft => egui::pos2(rect.max.x, rect.min.y),
                    SelectionDragHandle::Right | SelectionDragHandle::BottomRight => rect.min,
                    SelectionDragHandle::TopRight => egui::pos2(rect.min.x, rect.max.y),
                    SelectionDragHandle::Top => rect.max,
                    SelectionDragHandle::Bottom => rect.min,
                    _ => egui::Pos2::ZERO,
                };
                ui.data_mut(|d| d.insert_temp(anchor_id, drag_anchor));
            }
        }

        let pointer_primary_down = ui.input(|i| i.pointer.primary_down());
        if pointer_primary_down && active_handle != SelectionDragHandle::None {
            if let Some(pointer_pos) = ui
                .input(|i| i.pointer.latest_pos())
                .or_else(|| ui.input(|i| i.pointer.hover_pos()))
            {
                let shift_pressed = ui.input(|i| i.modifiers.shift);
                let original_aspect = if preset.height > 0 {
                    preset.width as f32 / preset.height as f32
                } else {
                    16.0 / 9.0
                };
                let lock_aspect = if shift_pressed { original_aspect } else { 0.0 };

                changed = true;

                let mut target_pos = pointer_pos - drag_offset;

                match active_handle {
                    SelectionDragHandle::Left
                    | SelectionDragHandle::TopLeft
                    | SelectionDragHandle::BottomLeft => {
                        target_pos.x = target_pos
                            .x
                            .clamp(preview_rect.left(), drag_anchor.x - min_size.x);
                    }
                    SelectionDragHandle::Right
                    | SelectionDragHandle::TopRight
                    | SelectionDragHandle::BottomRight => {
                        target_pos.x = target_pos
                            .x
                            .clamp(drag_anchor.x + min_size.x, preview_rect.right());
                    }
                    _ => {}
                }
                match active_handle {
                    SelectionDragHandle::Top
                    | SelectionDragHandle::TopLeft
                    | SelectionDragHandle::TopRight => {
                        target_pos.y = target_pos
                            .y
                            .clamp(preview_rect.top(), drag_anchor.y - min_size.y);
                    }
                    SelectionDragHandle::Bottom
                    | SelectionDragHandle::BottomLeft
                    | SelectionDragHandle::BottomRight => {
                        target_pos.y = target_pos
                            .y
                            .clamp(drag_anchor.y + min_size.y, preview_rect.bottom());
                    }
                    _ => {}
                }
                if active_handle == SelectionDragHandle::Center {
                    target_pos.x = target_pos
                        .x
                        .clamp(preview_rect.left(), preview_rect.right() - rect.width());
                    target_pos.y = target_pos
                        .y
                        .clamp(preview_rect.top(), preview_rect.bottom() - rect.height());
                }

                match active_handle {
                    SelectionDragHandle::Center => {
                        let size = rect.size();
                        rect.min = target_pos;
                        rect.max = rect.min + size;
                    }
                    SelectionDragHandle::Left => {
                        let new_left = target_pos.x.min(drag_anchor.x - min_size.x);
                        rect.min.x = new_left;
                        rect.max.x = drag_anchor.x;
                    }
                    SelectionDragHandle::Right => {
                        let new_right = target_pos.x.max(drag_anchor.x + min_size.x);
                        rect.min.x = drag_anchor.x;
                        rect.max.x = new_right;
                    }
                    SelectionDragHandle::Top => {
                        let new_top = target_pos.y.min(drag_anchor.y - min_size.y);
                        rect.min.y = new_top;
                        rect.max.y = drag_anchor.y;
                    }
                    SelectionDragHandle::Bottom => {
                        let new_bottom = target_pos.y.max(drag_anchor.y + min_size.y);
                        rect.min.y = drag_anchor.y;
                        rect.max.y = new_bottom;
                    }
                    SelectionDragHandle::TopLeft => {
                        let new_left = target_pos.x.min(drag_anchor.x - min_size.x);
                        let new_top = target_pos.y.min(drag_anchor.y - min_size.y);
                        rect.min = egui::pos2(new_left, new_top);
                        rect.max = drag_anchor;
                    }
                    SelectionDragHandle::TopRight => {
                        let new_right = target_pos.x.max(drag_anchor.x + min_size.x);
                        let new_top = target_pos.y.min(drag_anchor.y - min_size.y);
                        rect.min = egui::pos2(drag_anchor.x, new_top);
                        rect.max = egui::pos2(new_right, drag_anchor.y);
                    }
                    SelectionDragHandle::BottomLeft => {
                        let new_left = target_pos.x.min(drag_anchor.x - min_size.x);
                        let new_bottom = target_pos.y.max(drag_anchor.y + min_size.y);
                        rect.min = egui::pos2(new_left, drag_anchor.y);
                        rect.max = egui::pos2(drag_anchor.x, new_bottom);
                    }
                    SelectionDragHandle::BottomRight => {
                        let new_right = target_pos.x.max(drag_anchor.x + min_size.x);
                        let new_bottom = target_pos.y.max(drag_anchor.y + min_size.y);
                        rect.min = drag_anchor;
                        rect.max = egui::pos2(new_right, new_bottom);
                    }
                    SelectionDragHandle::None => {}
                }

                if lock_aspect > 0.0 {
                    match active_handle {
                        SelectionDragHandle::Right
                        | SelectionDragHandle::BottomRight
                        | SelectionDragHandle::TopRight => {
                            let new_h = rect.width() / lock_aspect;
                            if active_handle == SelectionDragHandle::TopRight {
                                rect.min.y = rect.max.y - new_h;
                            } else {
                                rect.max.y = rect.min.y + new_h;
                            }
                        }
                        SelectionDragHandle::Left
                        | SelectionDragHandle::TopLeft
                        | SelectionDragHandle::BottomLeft => {
                            let new_h = rect.width() / lock_aspect;
                            if active_handle == SelectionDragHandle::TopLeft {
                                rect.min.y = rect.max.y - new_h;
                            } else {
                                rect.max.y = rect.min.y + new_h;
                            }
                        }
                        SelectionDragHandle::Bottom => {
                            let new_w = rect.height() * lock_aspect;
                            rect.max.x = rect.min.x + new_w;
                        }
                        SelectionDragHandle::Top => {
                            let new_w = rect.height() * lock_aspect;
                            rect.min.x = rect.max.x - new_w;
                        }
                        _ => {}
                    }
                }

                if active_handle == SelectionDragHandle::Center {
                    if rect.left() < preview_rect.left() {
                        rect = rect.translate(egui::vec2(preview_rect.left() - rect.left(), 0.0));
                    }
                    if rect.top() < preview_rect.top() {
                        rect = rect.translate(egui::vec2(0.0, preview_rect.top() - rect.top()));
                    }
                    if rect.right() > preview_rect.right() {
                        rect = rect.translate(egui::vec2(preview_rect.right() - rect.right(), 0.0));
                    }
                    if rect.bottom() > preview_rect.bottom() {
                        rect =
                            rect.translate(egui::vec2(0.0, preview_rect.bottom() - rect.bottom()));
                    }
                }

                rect.min.x = rect
                    .min
                    .x
                    .clamp(preview_rect.left(), preview_rect.right() - min_size.x);
                rect.min.y = rect
                    .min
                    .y
                    .clamp(preview_rect.top(), preview_rect.bottom() - min_size.y);
                rect.max.x = rect
                    .max
                    .x
                    .clamp(rect.min.x + min_size.x, preview_rect.right());
                rect.max.y = rect
                    .max
                    .y
                    .clamp(rect.min.y + min_size.y, preview_rect.bottom());
            }
        }

        if ui.input(|i| i.pointer.any_released()) {
            active_handle = SelectionDragHandle::None;
            ui.data_mut(|d| d.insert_temp(drag_id, active_handle));
        }

        if response.hovered() || active_handle != SelectionDragHandle::None {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.hover_pos()) {
                let mut handle_to_use = if active_handle != SelectionDragHandle::None {
                    active_handle
                } else {
                    pick_selection_drag_handle(pointer_pos, rect)
                };
                if active_handle == SelectionDragHandle::None
                    && handle_to_use == SelectionDragHandle::Center
                    && !rect.contains(pointer_pos)
                {
                    handle_to_use = SelectionDragHandle::None;
                }

                match handle_to_use {
                    SelectionDragHandle::TopLeft | SelectionDragHandle::BottomRight => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                    }
                    SelectionDragHandle::TopRight | SelectionDragHandle::BottomLeft => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNeSw);
                    }
                    SelectionDragHandle::Left | SelectionDragHandle::Right => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    SelectionDragHandle::Top | SelectionDragHandle::Bottom => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    }
                    SelectionDragHandle::Center => {
                        if active_handle == SelectionDragHandle::Center {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        } else {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                    }
                    _ => {}
                }
            }
        }

        let size_text = format!("{}x{}", preset.width, preset.height);
        ui.painter().text(
            rect.left_top() + egui::vec2(0.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            size_text,
            egui::FontId::proportional(10.0),
            Color32::from_rgb(124, 240, 164),
        );

        let bg_alpha = (preset.background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let background = Color32::from_rgba_premultiplied(
            ((preset.background_color.r as u32 * bg_alpha as u32) / 255) as u8,
            ((preset.background_color.g as u32 * bg_alpha as u32) / 255) as u8,
            ((preset.background_color.b as u32 * bg_alpha as u32) / 255) as u8,
            bg_alpha,
        );
        let text_color = Color32::from_rgba_premultiplied(
            preset.text_color.r,
            preset.text_color.g,
            preset.text_color.b,
            preset.text_color.a,
        );
        let rounding = if preset.rounded_background { 12.0 } else { 0.0 };
        if bg_alpha > 0 {
            ui.painter().rect_filled(rect, rounding, background);
        }
        ui.painter().rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(2.0, Color32::from_rgb(124, 240, 164)),
            egui::StrokeKind::Outside,
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if preset.text.trim().is_empty() {
                "HUD preview"
            } else {
                preset.text.as_str()
            },
            egui::FontId::proportional((preset.font_size * scale).clamp(2.0, 200.0)),
            text_color,
        );

        if changed {
            preset.x = ((rect.left() - preview_rect.left()) / scale).round() as i32;
            preset.y = ((rect.top() - preview_rect.top()) / scale).round() as i32;
            preset.width = (rect.width() / scale).round().max(1.0) as i32;
            preset.height = (rect.height() / scale).round().max(1.0) as i32;
        }

        ui.label(
            RichText::new(format!(
                "X={} Y={} W={} H={}",
                preset.x, preset.y, preset.width, preset.height
            ))
            .small(),
        );
        changed
    }

    pub(crate) fn add_toolbox_preset(&mut self) {
        let id = Self::allocate_next_id(
            &self.state.hud_presets,
            &mut self.state.next_hud_preset_id,
            |preset| preset.id,
        );
        let mut new_preset = HudPreset::new(id);
        let mut suffix = 1;
        while self
            .state
            .hud_presets
            .iter()
            .any(|p| p.name == format!("HUD {}", suffix))
        {
            suffix += 1;
        }
        new_preset.name = format!("HUD {}", suffix);
        self.state.hud_presets.push(new_preset);
        self.sync_hud_presets();
        self.status = format!("Added HUD preset {id}.");
    }

    pub(crate) fn persist_hud_presets(&mut self) {
        self.persist_after_sync(Self::sync_hud_presets);
    }

    pub(crate) fn persist_hud_presets_deferred(&mut self, ctx: &egui::Context) {
        self.persist_deferred_after_sync(ctx, Self::sync_hud_presets);
    }

    pub(crate) fn sync_hud_presets(&mut self) {
        let presets = self.state.hud_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            presets,
            &mut self.last_synced_hud_presets,
            OverlayCommand::UpdateHudPresets,
        );
    }
}
