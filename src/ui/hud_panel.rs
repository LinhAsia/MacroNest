use crate::model::*;
use crate::overlay::OverlayCommand;
use crate::ui::CrosshairApp;
use eframe::egui::{self, Color32, RichText, Sense, Slider, TextBuffer, TextEdit, vec2};

impl CrosshairApp {
    pub(crate) fn render_hud_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        let mut remove_timer_id = None;
        let mut timer_changed = false;
        let mut active_timer_preview: Option<TimerPreset> = None;

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(self.tr("+ Add HUD preset", "+ Thêm preset HUD"))
                .clicked()
            {
                self.add_toolbox_preset();
                self.persist_hud_presets();
            }
            if ui
                .button(self.tr("+ Add timer preset", "+ Thêm preset Timer"))
                .clicked()
            {
                let mut id = 1;
                while self.state.timer_presets.iter().any(|p| p.id == id) {
                    id += 1;
                }
                self.state.next_timer_preset_id = (self
                    .state
                    .timer_presets
                    .iter()
                    .map(|p| p.id)
                    .max()
                    .unwrap_or(0)
                    + 1)
                .max(id + 1);

                let mut new_preset = TimerPreset::new(id);
                new_preset.name = format!("Timer {id}");
                self.state.timer_presets.push(new_preset);
                timer_changed = true;
            }
        });

        ui.add_space(8.0);
        ui.label(
            RichText::new(self.tr("Text Presets", "Thiết lập Văn bản"))
                .strong()
                .size(14.0),
        );
        ui.add_space(4.0);

        let mut remove_id = None;
        let mut changed = false;
        let mut active_preview: Option<HudPreset> = None;
        for index in 0..self.state.hud_presets.len() {
            let language = self.state.ui_language;
            ui.add_space(6.0);
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
                        if Self::sound_style_remove_button(ui).clicked() {
                            remove_id = Some(preset.id);
                        }
                        if Self::sound_style_toggle_button(
                            ui,
                            if preset.collapsed {
                                Self::tr_lang(language, "Show", "Hiện")
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

                        ui.label(Self::tr_lang(language, "Font Size", "Cỡ chữ"));
                        changed |= ui
                            .add(
                                Slider::new(&mut preset.font_size, 1.0..=200.0)
                                    .text("px")
                                    .clamping(egui::SliderClamping::Always),
                            )
                            .changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Text Color", "Màu chữ"));
                        changed |= Self::edit_rgba_color(ui, &mut preset.text_color).changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Background Color", "Màu nền"));
                        changed |=
                            Self::edit_rgba_color(ui, &mut preset.background_color).changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Background Opacity", "Độ mờ nền"));
                        changed |= ui
                            .add(
                                Slider::new(&mut preset.background_opacity, 0.0..=1.0)
                                    .text("")
                                    .clamping(egui::SliderClamping::Always),
                            )
                            .changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Rounded Background", "Nền bo góc"));
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
                                    "Stream preview trong editor",
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
                        "Preview vị trí",
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
                });

                if preset.preview_enabled {
                    active_preview = Some(preset.clone());
                }
            });
        }

        if let Some(id) = remove_id {
            self.state.hud_presets.retain(|preset| preset.id != id);
            changed = true;
        }
        self.sync_hud_preview(active_preview.as_ref());
        if changed {
            self.persist_hud_presets();
        }

        ui.add_space(8.0);
        ui.label(
            RichText::new(self.tr("Timer Presets", "Thiết lập Hẹn giờ"))
                .strong()
                .size(14.0),
        );
        ui.add_space(4.0);

        for index in 0..self.state.timer_presets.len() {
            ui.add_space(6.0);
            let preset = &mut self.state.timer_presets[index];
            if preset.show_progress_bar || !preset.show_text {
                preset.show_progress_bar = false;
                preset.show_text = true;
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
                        if Self::sound_style_remove_button(ui).clicked() {
                            remove_timer_id = Some(preset.id);
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
                        ui.label(Self::tr_lang(language, "Type", "Loại"));
                        ui.horizontal(|ui| {
                            let mut selected_type = if preset.is_countdown { 1 } else { 0 };
                            let resp = egui::ComboBox::from_id_salt((preset.id, "timer-type-sel"))
                                .selected_text(if selected_type == 1 {
                                    Self::tr_lang(language, "Countdown", "Đếm ngược (Hẹn giờ)")
                                } else {
                                    Self::tr_lang(language, "Stopwatch", "Đếm xuôi (Bấm giờ)")
                                })
                                .show_ui(ui, |ui| {
                                    let mut changed = false;
                                    changed |= ui
                                        .selectable_value(
                                            &mut selected_type,
                                            0,
                                            Self::tr_lang(
                                                language,
                                                "Stopwatch",
                                                "Đếm xuôi (Bấm giờ)",
                                            ),
                                        )
                                        .clicked();
                                    changed |= ui
                                        .selectable_value(
                                            &mut selected_type,
                                            1,
                                            Self::tr_lang(
                                                language,
                                                "Countdown",
                                                "Đếm ngược (Hẹn giờ)",
                                            ),
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
                            ui.label(Self::tr_lang(language, "Duration", "Thời lượng"));
                            timer_changed |= ui
                                .add(
                                    Slider::new(&mut preset.duration_secs, 1..=3600)
                                        .text(Self::tr_lang(language, "seconds", "giây"))
                                        .clamping(egui::SliderClamping::Always),
                                )
                                .changed();
                            ui.end_row();
                        }

                        if preset.show_text {
                            ui.label(Self::tr_lang(language, "Format", "Định dạng"));
                            ui.horizontal(|ui| {
                                timer_changed |= ui
                                    .checkbox(
                                        &mut preset.show_minutes,
                                        Self::tr_lang(language, "Min", "Phút"),
                                    )
                                    .changed();
                                timer_changed |= ui
                                    .checkbox(
                                        &mut preset.show_seconds,
                                        Self::tr_lang(language, "Sec", "Giây"),
                                    )
                                    .changed();
                                timer_changed |= ui
                                    .checkbox(
                                        &mut preset.show_ms,
                                        Self::tr_lang(language, "Ms/Ticks", "Khắc (Ms)"),
                                    )
                                    .changed();
                            });
                            ui.end_row();
                        }

                        if preset.show_text {
                            ui.label(Self::tr_lang(language, "Font Size", "Cỡ chữ"));
                            timer_changed |= ui
                                .add(
                                    Slider::new(&mut preset.font_size, 1.0..=200.0)
                                        .text("px")
                                        .clamping(egui::SliderClamping::Always),
                                )
                                .changed();
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Text Color", "Màu chữ"));
                            timer_changed |=
                                Self::edit_rgba_color(ui, &mut preset.text_color).changed();
                            ui.end_row();
                        }

                        ui.label(Self::tr_lang(language, "Background Color", "Màu nền"));
                        timer_changed |=
                            Self::edit_rgba_color(ui, &mut preset.background_color).changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Background Opacity", "Độ mờ nền"));
                        timer_changed |= ui
                            .add(
                                Slider::new(&mut preset.background_opacity, 0.0..=1.0)
                                    .text("")
                                    .clamping(egui::SliderClamping::Always),
                            )
                            .changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Rounded Background", "Nền bo góc"));
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
                                    "Stream preview trong editor",
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
                        "Preview vị trí",
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

                if preset.preview_enabled {
                    active_timer_preview = Some(preset.clone());
                }
            });
        }

        if let Some(id) = remove_timer_id {
            self.state.timer_presets.retain(|preset| preset.id != id);
            timer_changed = true;
        }

        self.sync_timer_preview(active_timer_preview.as_ref());

        if timer_changed {
            self.persist_timer_presets();
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

        if response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                let dist_tl = pointer_pos.distance(rect.left_top());
                let dist_tr = pointer_pos.distance(rect.right_top());
                let dist_bl = pointer_pos.distance(rect.left_bottom());
                let dist_br = pointer_pos.distance(rect.right_bottom());

                active_handle = if dist_tl < 14.0 {
                    SelectionDragHandle::TopLeft
                } else if dist_tr < 14.0 {
                    SelectionDragHandle::TopRight
                } else if dist_bl < 14.0 {
                    SelectionDragHandle::BottomLeft
                } else if dist_br < 14.0 {
                    SelectionDragHandle::BottomRight
                } else if (pointer_pos.x - rect.left()).abs() < 10.0
                    && pointer_pos.y >= rect.top()
                    && pointer_pos.y <= rect.bottom()
                {
                    SelectionDragHandle::Left
                } else if (pointer_pos.x - rect.right()).abs() < 10.0
                    && pointer_pos.y >= rect.top()
                    && pointer_pos.y <= rect.bottom()
                {
                    SelectionDragHandle::Right
                } else if (pointer_pos.y - rect.top()).abs() < 10.0
                    && pointer_pos.x >= rect.left()
                    && pointer_pos.x <= rect.right()
                {
                    SelectionDragHandle::Top
                } else if (pointer_pos.y - rect.bottom()).abs() < 10.0
                    && pointer_pos.x >= rect.left()
                    && pointer_pos.x <= rect.right()
                {
                    SelectionDragHandle::Bottom
                } else if rect.contains(pointer_pos) {
                    SelectionDragHandle::Center
                } else {
                    SelectionDragHandle::None
                };
                ui.data_mut(|d| d.insert_temp(drag_id, active_handle));
            }
        }

        if response.dragged() && active_handle != SelectionDragHandle::None {
            let delta = response.drag_delta();
            let shift_pressed = ui.input(|i| i.modifiers.shift);
            let original_aspect = if preset.height > 0 {
                preset.width as f32 / preset.height as f32
            } else {
                16.0 / 9.0
            };
            let lock_aspect = if shift_pressed { original_aspect } else { 0.0 };

            changed = true;

            match active_handle {
                SelectionDragHandle::Center => {
                    rect = rect.translate(delta);
                }
                SelectionDragHandle::Right => {
                    let new_w = (rect.width() + delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.max.x = rect.min.x + new_w;
                        rect.max.y = rect.min.y + new_h;
                    } else {
                        rect.max.x = rect.min.x + new_w;
                    }
                }
                SelectionDragHandle::Left => {
                    let new_w = (rect.width() - delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.min.x = rect.max.x - new_w;
                        rect.min.y = rect.max.y - new_h;
                    } else {
                        rect.min.x = rect.max.x - new_w;
                    }
                }
                SelectionDragHandle::Bottom => {
                    let new_h = (rect.height() + delta.y).max(min_size.y);
                    if lock_aspect > 0.0 {
                        let new_w = new_h * lock_aspect;
                        rect.max.x = rect.min.x + new_w;
                        rect.max.y = rect.min.y + new_h;
                    } else {
                        rect.max.y = rect.min.y + new_h;
                    }
                }
                SelectionDragHandle::Top => {
                    let new_h = (rect.height() - delta.y).max(min_size.y);
                    if lock_aspect > 0.0 {
                        let new_w = new_h * lock_aspect;
                        rect.min.x = rect.max.x - new_w;
                        rect.min.y = rect.max.y - new_h;
                    } else {
                        rect.min.y = rect.max.y - new_h;
                    }
                }
                SelectionDragHandle::BottomRight => {
                    let new_w = (rect.width() + delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.max.x = rect.min.x + new_w;
                        rect.max.y = rect.min.y + new_h;
                    } else {
                        let new_h = (rect.height() + delta.y).max(min_size.y);
                        rect.max.x = rect.min.x + new_w;
                        rect.max.y = rect.min.y + new_h;
                    }
                }
                SelectionDragHandle::TopLeft => {
                    let new_w = (rect.width() - delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.min.x = rect.max.x - new_w;
                        rect.min.y = rect.max.y - new_h;
                    } else {
                        let new_h = (rect.height() - delta.y).max(min_size.y);
                        rect.min.x = rect.max.x - new_w;
                        rect.min.y = rect.max.y - new_h;
                    }
                }
                SelectionDragHandle::TopRight => {
                    let new_w = (rect.width() + delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.max.x = rect.min.x + new_w;
                        rect.min.y = rect.max.y - new_h;
                    } else {
                        let new_h = (rect.height() - delta.y).max(min_size.y);
                        rect.max.x = rect.min.x + new_w;
                        rect.min.y = rect.max.y - new_h;
                    }
                }
                SelectionDragHandle::BottomLeft => {
                    let new_w = (rect.width() - delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.min.x = rect.max.x - new_w;
                        rect.max.y = rect.min.y + new_h;
                    } else {
                        let new_h = (rect.height() + delta.y).max(min_size.y);
                        rect.min.x = rect.max.x - new_w;
                        rect.max.y = rect.min.y + new_h;
                    }
                }
                SelectionDragHandle::None => {}
            }

            if rect.left() < preview_rect.left() {
                rect = rect.translate(vec2(preview_rect.left() - rect.left(), 0.0));
            }
            if rect.top() < preview_rect.top() {
                rect = rect.translate(vec2(0.0, preview_rect.top() - rect.top()));
            }
            if rect.right() > preview_rect.right() {
                rect = rect.translate(vec2(preview_rect.right() - rect.right(), 0.0));
            }
            if rect.bottom() > preview_rect.bottom() {
                rect = rect.translate(vec2(0.0, preview_rect.bottom() - rect.bottom()));
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

        if ui.input(|i| i.pointer.any_released()) {
            active_handle = SelectionDragHandle::None;
            ui.data_mut(|d| d.insert_temp(drag_id, active_handle));
        }

        if response.hovered() || active_handle != SelectionDragHandle::None {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.hover_pos()) {
                let dist_tl = pointer_pos.distance(rect.left_top());
                let dist_tr = pointer_pos.distance(rect.right_top());
                let dist_bl = pointer_pos.distance(rect.left_bottom());
                let dist_br = pointer_pos.distance(rect.right_bottom());

                let handle_to_use = if active_handle != SelectionDragHandle::None {
                    active_handle
                } else if dist_tl < 14.0 {
                    SelectionDragHandle::TopLeft
                } else if dist_tr < 14.0 {
                    SelectionDragHandle::TopRight
                } else if dist_bl < 14.0 {
                    SelectionDragHandle::BottomLeft
                } else if dist_br < 14.0 {
                    SelectionDragHandle::BottomRight
                } else if (pointer_pos.x - rect.left()).abs() < 10.0
                    && pointer_pos.y >= rect.top()
                    && pointer_pos.y <= rect.bottom()
                {
                    SelectionDragHandle::Left
                } else if (pointer_pos.x - rect.right()).abs() < 10.0
                    && pointer_pos.y >= rect.top()
                    && pointer_pos.y <= rect.bottom()
                {
                    SelectionDragHandle::Right
                } else if (pointer_pos.y - rect.top()).abs() < 10.0
                    && pointer_pos.x >= rect.left()
                    && pointer_pos.x <= rect.right()
                {
                    SelectionDragHandle::Top
                } else if (pointer_pos.y - rect.bottom()).abs() < 10.0
                    && pointer_pos.x >= rect.left()
                    && pointer_pos.x <= rect.right()
                {
                    SelectionDragHandle::Bottom
                } else if rect.contains(pointer_pos) {
                    SelectionDragHandle::Center
                } else {
                    SelectionDragHandle::None
                };

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
        let mut id = 1;
        while self.state.hud_presets.iter().any(|p| p.id == id) {
            id += 1;
        }
        self.state.next_hud_preset_id = (self
            .state
            .hud_presets
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            + 1)
        .max(id + 1);
        self.state.hud_presets.push(HudPreset::new(id));
        self.sync_hud_presets();
        self.status = format!("Added HUD preset {id}.");
    }

    pub(crate) fn persist_hud_presets(&mut self) {
        self.sync_hud_presets();
        self.persist();
    }

    pub(crate) fn sync_hud_presets(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateHudPresets(
            self.state.hud_presets.clone(),
        ));
    }
}
