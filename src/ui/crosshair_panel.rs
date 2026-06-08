use crate::model::*;
use crate::ui::CrosshairApp;
use eframe::egui::{self, *};
use std::time::Duration;

impl CrosshairApp {
    pub(crate) fn render_crosshair_panel(&mut self, ui: &mut egui::Ui) {
        self.render_crosshair_presets_panel(ui);
        return;
        ui.spacing_mut().slider_width = 260.0;
        let mut changed = false;
        Self::show_preset_card(ui, self.state.active_style.enabled, |ui| {
            ui.horizontal(|ui| {
                changed |= ui
                    .checkbox(&mut self.state.active_style.enabled, "Enabled")
                    .changed();
                if Self::sound_style_toggle_button(
                    ui,
                    if self.crosshair_panel_collapsed {
                        "Show"
                    } else {
                        "Hide"
                    },
                )
                .clicked()
                {
                    self.crosshair_panel_collapsed = !self.crosshair_panel_collapsed;
                }
            });
        });
        if self.crosshair_panel_collapsed {
            if changed {
                self.sync_crosshair();
                self.persist();
            }
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.heading("Quick Controls");
                egui::Grid::new("crosshair-quick-controls")
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Actions");
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Center").clicked() {
                                let screen_size = Self::screen_size();
                                self.state.active_style.x_offset =
                                    (screen_size.x.round() as i32).saturating_sub(1) / 2;
                                self.state.active_style.y_offset =
                                    (screen_size.y.round() as i32).saturating_sub(1) / 2;
                                changed = true;
                            }
                        });
                        ui.end_row();
                    });

                ui.separator();
                ui.heading("Crosshair Presets");
                egui::Grid::new("crosshair-profiles")
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        let selected = self
                            .state
                            .selected_profile
                            .clone()
                            .unwrap_or_else(|| Self::tr_lang(self.state.ui_language, "No preset", "Chưa có preset").to_owned());
                        ui.label("Selected preset");
                        egui::ComboBox::from_id_salt("saved-crosshair-profiles")
                            .width(260.0)
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for profile in self.state.profiles.clone() {
                                    if ui
                                        .selectable_label(
                                            self.state.selected_profile.as_deref()
                                                == Some(&profile.name),
                                            &profile.name,
                                        )
                                        .clicked()
                                    {
                                        self.state.selected_profile = Some(profile.name.clone());
                                        self.state.active_style = profile.style.clone();
                                        self.state.active_style.enabled = profile.enabled;
                                        self.save_name = profile.name;
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Preset name");
                        ui.horizontal_wrapped(|ui| {
                            let response = ui.add_sized(
                                [220.0, 21.0],
                                TextEdit::singleline(&mut self.save_name),
                            );
                            Self::apply_vietnamese_input_if_changed(
                                &response,
                                self.state.vietnamese_input_enabled,
                                self.state.vietnamese_input_mode,
                                &mut self.save_name,
                            );

                            if ui.button("+ New Preset").clicked() {
                                self.add_profile();
                            }
                            if ui.button("Save").clicked() {
                                self.save_profile();
                            }
                            if Self::sound_style_remove_button(ui).clicked() {
                                self.delete_profile();
                            }
                        });
                        ui.end_row();
                    });

                ui.add_space(6.0);
                let dark_mode = self.state.ui_theme == UiThemeMode::Dark;
                let profiles_count = self.state.profiles.len();
                for index in 0..profiles_count {
                    let is_selected = self.state.selected_profile.as_deref()
                        == Some(self.state.profiles[index].name.as_str());
                    let mut activate = false;
                    let mut remove = false;
                    {
                        let preset = &mut self.state.profiles[index];
                        Self::show_preset_card(ui, preset.enabled, |ui| {
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut preset.enabled, "").changed() {
                                    preset.style.enabled = preset.enabled;
                                    if is_selected {
                                        self.state.active_style.enabled = preset.enabled;
                                    }
                                    changed = true;
                                }
                                ui.label(Self::preset_title_text(
                                    dark_mode,
                                    &preset.name,
                                    preset.enabled,
                                ));
                                if is_selected {
                                    ui.label(RichText::new("Active").strong());
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if Self::sound_style_remove_button(ui).clicked() {
                                            remove = true;
                                        }
                                        if ui
                                            .button(if is_selected { "Current" } else { "Apply" })
                                            .clicked()
                                        {
                                            activate = true;
                                        }
                                    },
                                );
                            });
                        });
                    }
                    if activate {
                        let preset = self.state.profiles[index].clone();
                        self.state.selected_profile = Some(preset.name.clone());
                        self.state.active_style = preset.style;
                        self.state.active_style.enabled = preset.enabled;
                        self.save_name = preset.name;
                        changed = true;
                    }
                    if remove {
                        let remove_name = self.state.profiles[index].name.clone();
                        self.state.profiles.remove(index);
                        self.status = format!("Deleted profile: {remove_name}");
                        if self.state.profiles.is_empty() {
                            self.state.profiles.push(ProfileRecord::default());
                        }
                        let next = self.state.profiles[0].clone();
                        self.state.selected_profile = Some(next.name.clone());
                        self.state.active_style = next.style;
                        self.state.active_style.enabled = next.enabled;
                        self.save_name = next.name;
                        self.sync_profiles();
                        changed = true;
                        break;
                    }
                    ui.add_space(4.0);
                }

                ui.separator();
                ui.heading("Crosshair Settings");
                egui::Grid::new("crosshair-settings-grid")
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Horizontal length");
                        changed |= ui
                            .add_sized(
                                [340.0, 20.0],
                                Slider::new(
                                    &mut self.state.active_style.horizontal_length,
                                    0.0..=80.0,
                                ),
                            )
                            .changed();
                        ui.end_row();

                        ui.label("Vertical length");
                        changed |= ui
                            .add_sized(
                                [340.0, 20.0],
                                Slider::new(
                                    &mut self.state.active_style.vertical_length,
                                    0.0..=80.0,
                                ),
                            )
                            .changed();
                        ui.end_row();

                        ui.label("Thickness");
                        changed |= ui
                            .add_sized(
                                [340.0, 20.0],
                                Slider::new(&mut self.state.active_style.thickness, 0.0..=32.0),
                            )
                            .changed();
                        ui.end_row();

                        ui.label("Gap");
                        changed |= ui
                            .add_sized(
                                [340.0, 20.0],
                                Slider::new(&mut self.state.active_style.gap, 0.0..=48.0),
                            )
                            .changed();
                        ui.end_row();

                        ui.label("Position X");
                        changed |= ui
                            .add_sized(
                                [340.0, 20.0],
                                Slider::new(&mut self.state.active_style.x_offset, -1000..=1000),
                            )
                            .changed();
                        ui.end_row();

                        ui.label("Position Y");
                        changed |= ui
                            .add_sized(
                                [340.0, 20.0],
                                Slider::new(&mut self.state.active_style.y_offset, -1000..=1000),
                            )
                            .changed();
                        ui.end_row();

                        ui.label("Opacity");
                        changed |= ui
                            .add_sized(
                                [340.0, 20.0],
                                Slider::new(&mut self.state.active_style.opacity, 0.05..=1.0),
                            )
                            .changed();
                        ui.end_row();
                    });

                ui.separator();
                ui.heading("Outline and Center Dot");
                egui::Grid::new("crosshair-outline-grid")
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Outline");
                        changed |= ui
                            .checkbox(&mut self.state.active_style.outline_enabled, "Enabled")
                            .changed();
                        ui.end_row();

                        if self.state.active_style.outline_enabled {
                            ui.label("Outline thickness");
                            changed |= ui
                                .add_sized(
                                    [340.0, 20.0],
                                    Slider::new(
                                        &mut self.state.active_style.outline_thickness,
                                        0.0..=16.0,
                                    ),
                                )
                                .changed();
                            ui.end_row();
                        }

                        ui.label("Center dot");
                        changed |= ui
                            .checkbox(&mut self.state.active_style.center_dot, "Enabled")
                            .changed();
                        ui.end_row();

                        if self.state.active_style.center_dot {
                            ui.label("Center dot size");
                            changed |= ui
                                .add_sized(
                                    [340.0, 20.0],
                                    Slider::new(
                                        &mut self.state.active_style.center_dot_size,
                                        0.0..=32.0,
                                    ),
                                )
                                .changed();
                            ui.end_row();
                        }
                    });

                ui.separator();
                ui.heading("Colors");
                egui::Grid::new("crosshair-colors-grid")
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Crosshair color");
                        let mut crosshair_rgba = [
                            self.state.active_style.color.r,
                            self.state.active_style.color.g,
                            self.state.active_style.color.b,
                            self.state.active_style.color.a,
                        ];
                        if ui
                            .color_edit_button_srgba_unmultiplied(&mut crosshair_rgba)
                            .changed()
                        {
                            self.state.active_style.color.r = crosshair_rgba[0];
                            self.state.active_style.color.g = crosshair_rgba[1];
                            self.state.active_style.color.b = crosshair_rgba[2];
                            self.state.active_style.color.a = crosshair_rgba[3];
                            changed = true;
                        }
                        ui.end_row();

                        if self.state.active_style.outline_enabled {
                            ui.label("Outline color");
                            let mut outline_rgba = [
                                self.state.active_style.outline_color.r,
                                self.state.active_style.outline_color.g,
                                self.state.active_style.outline_color.b,
                                self.state.active_style.outline_color.a,
                            ];
                            if ui
                                .color_edit_button_srgba_unmultiplied(&mut outline_rgba)
                                .changed()
                            {
                                self.state.active_style.outline_color.r = outline_rgba[0];
                                self.state.active_style.outline_color.g = outline_rgba[1];
                                self.state.active_style.outline_color.b = outline_rgba[2];
                                self.state.active_style.outline_color.a = outline_rgba[3];
                                changed = true;
                            }
                            ui.end_row();
                        }
                    });
            });

        if changed {
            self.sync_crosshair();
            self.persist();
        }
    }

    fn render_crosshair_style_editor<H: std::hash::Hash>(
        ui: &mut egui::Ui,
        language: UiLanguage,
        grid_id: H,
        style: &mut CrosshairStyle,
    ) -> (bool, bool) {
        let mut changed = false;
        let mut dragging = false;
        let screen_size = Self::screen_size();
        let (offset_limit_x, offset_limit_y) = Self::crosshair_position_limits(screen_size);
        egui::Grid::new(grid_id)
            .num_columns(2)
            .spacing([14.0, 8.0])
            .show(ui, |ui| {
                ui.label(Self::tr_lang(
                    language,
                    "Horizontal length",
                    "Horizontal length",
                ));
                let response = ui.add_sized(
                    [340.0, 20.0],
                    DragValue::new(&mut style.horizontal_length)
                        .range(0.0..=80.0)
                        .speed(0.1),
                );
                changed |= response.changed();
                dragging |= response.dragged();
                ui.end_row();

                ui.label(Self::tr_lang(
                    language,
                    "Vertical length",
                    "Vertical length",
                ));
                let response = ui.add_sized(
                    [340.0, 20.0],
                    DragValue::new(&mut style.vertical_length)
                        .range(0.0..=80.0)
                        .speed(0.1),
                );
                changed |= response.changed();
                dragging |= response.dragged();
                ui.end_row();

                ui.label(Self::tr_lang(language, "Thickness", "Thickness"));
                let response = ui.add_sized(
                    [340.0, 20.0],
                    DragValue::new(&mut style.thickness)
                        .range(0.0..=32.0)
                        .speed(0.1),
                );
                changed |= response.changed();
                dragging |= response.dragged();
                ui.end_row();

                ui.label(Self::tr_lang(language, "Gap", "Gap"));
                let response = ui.add_sized(
                    [340.0, 20.0],
                    DragValue::new(&mut style.gap).range(0.0..=48.0).speed(0.1),
                );
                changed |= response.changed();
                dragging |= response.dragged();
                ui.end_row();

                ui.label(Self::tr_lang(language, "X", "Độ lệch ngang"));
                ui.horizontal(|ui| {
                    let response = ui.add_sized(
                        [280.0, 20.0],
                        DragValue::new(&mut style.x_offset)
                            .range(0..=offset_limit_x)
                            .speed(1.0),
                    );
                    changed |= response.changed();
                    dragging |= response.dragged();
                    if ui
                        .button(Self::tr_lang(language, "Center", "Center"))
                        .clicked()
                    {
                        style.x_offset = (screen_size.x.round() as i32).saturating_sub(1) / 2;
                        changed = true;
                    }
                });
                ui.end_row();

                ui.label(Self::tr_lang(language, "Y", "Y"));
                ui.horizontal(|ui| {
                    let response = ui.add_sized(
                        [280.0, 20.0],
                        DragValue::new(&mut style.y_offset)
                            .range(0..=offset_limit_y)
                            .speed(1.0),
                    );
                    changed |= response.changed();
                    dragging |= response.dragged();
                    if ui
                        .button(Self::tr_lang(language, "Center", "Center"))
                        .clicked()
                    {
                        style.y_offset = (screen_size.y.round() as i32).saturating_sub(1) / 2;
                        changed = true;
                    }
                });
                ui.end_row();

                ui.label(Self::tr_lang(language, "Opacity", "Opacity"));
                let response = ui.add_sized(
                    [340.0, 20.0],
                    DragValue::new(&mut style.opacity)
                        .range(0.0..=1.0)
                        .speed(0.01),
                );
                changed |= response.changed();
                dragging |= response.dragged();
                ui.end_row();

                ui.label(Self::tr_lang(language, "Outline", "Outline"));
                changed |= ui
                    .checkbox(
                        &mut style.outline_enabled,
                        Self::tr_lang(language, "Enabled", "Enabled"),
                    )
                    .changed();
                ui.end_row();

                if style.outline_enabled {
                    ui.label(Self::tr_lang(
                        language,
                        "Outline thickness",
                        "Outline thickness",
                    ));
                    let response = ui.add_sized(
                        [340.0, 20.0],
                        DragValue::new(&mut style.outline_thickness)
                            .range(0.0..=16.0)
                            .speed(0.1),
                    );
                    changed |= response.changed();
                    dragging |= response.dragged();
                    ui.end_row();
                }

                ui.label(Self::tr_lang(language, "Center dot", "Center dot"));
                changed |= ui
                    .checkbox(
                        &mut style.center_dot,
                        Self::tr_lang(language, "Enabled", "Enabled"),
                    )
                    .changed();
                ui.end_row();

                if style.center_dot {
                    ui.label(Self::tr_lang(
                        language,
                        "Center dot size",
                        "Kích thước chấm giữa",
                    ));
                    let response = ui.add_sized(
                        [340.0, 20.0],
                        DragValue::new(&mut style.center_dot_size)
                            .range(0.0..=32.0)
                            .speed(0.1),
                    );
                    changed |= response.changed();
                    dragging |= response.dragged();
                    ui.end_row();
                }

                ui.label(Self::tr_lang(language, "Crosshair color", "Màu tâm ngắm"));
                let response = Self::edit_rgba_color(ui, &mut style.color);
                changed |= response.changed();
                dragging |= response.dragged();
                ui.end_row();

                if style.outline_enabled {
                    ui.label(Self::tr_lang(language, "Outline color", "Màu viền"));
                    let response = Self::edit_rgba_color(ui, &mut style.outline_color);
                    changed |= response.changed();
                    dragging |= response.dragged();
                    ui.end_row();
                }
            });
        (changed, dragging)
    }

    fn render_crosshair_presets_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        ui.spacing_mut().slider_width = 260.0;
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(Self::tr_lang(
                    language,
                    "+ Add crosshair preset",
                    "+ Thêm preset tâm ngắm",
                ))
                .clicked()
            {
                self.add_profile();
            }
        });

        ui.add_space(8.0);

        let mut any_dragging = false;
        let mut remove_index = None;

        let mut copy_crosshair_profile = None;
        let mut paste_crosshair_profile_after = None;
        let mut refresh_crosshair_profiles = false;
        let can_paste_crosshair = self.crosshair_profile_clipboard.is_some();
        for index in 0..self.state.profiles.len() {
            ui.add_space(6.0);
            let mut remove = false;
            let mut preset_changed = false;
            let is_selected = self.state.selected_profile.as_deref()
                == Some(self.state.profiles[index].name.as_str());
            {
                let preset = &mut self.state.profiles[index];
                let preset_snapshot = preset.clone();
                Self::show_preset_card(ui, preset.enabled, |ui| {
                    ui.horizontal(|ui| {
                        let name_width = Self::preset_header_name_width(ui);
                        let response = ui
                            .add_sized([name_width, 21.0], TextEdit::singleline(&mut preset.name));
                        Self::apply_vietnamese_input_if_changed(
                            &response,
                            self.state.vietnamese_input_enabled,
                            self.state.vietnamese_input_mode,
                            &mut preset.name,
                        );
                        preset_changed |= response.changed();
                        if Self::sound_style_toggle_button(
                            ui,
                            if preset.enabled {
                                Self::tr_lang(language, "Unapply", "Unapply")
                            } else {
                                Self::tr_lang(language, "Apply", "Apply")
                            },
                        )
                        .clicked()
                        {
                            preset.enabled = !preset.enabled;
                            preset.style.enabled = preset.enabled;
                            if is_selected {
                                self.state.active_style.enabled = preset.enabled;
                            }
                            refresh_crosshair_profiles = true;
                            preset_changed = true;
                        }
                        ui.add_space(6.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    can_paste_crosshair,
                                    Button::new(Self::tr_lang(language, "Paste", "Paste"))
                                        .min_size(vec2(84.0, 24.0)),
                                )
                                .clicked()
                            {
                                paste_crosshair_profile_after = Some(index);
                            }
                            if ui
                                .add_sized(
                                    [84.0, 21.0],
                                    Button::new(Self::tr_lang(language, "Copy", "Copy")),
                                )
                                .clicked()
                            {
                                copy_crosshair_profile = Some(preset_snapshot.clone());
                            }

                            if Self::sound_style_remove_button(ui).clicked() {
                                remove = true;
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
                                preset_changed = true;
                            }
                        });
                    });
                    if !preset.collapsed {
                        ui.add_space(4.0);
                        ui.label(Self::tr_lang(
                            language,
                            "Crosshair Settings",
                            "Cài đặt tâm ngắm",
                        ));
                        let (style_changed, style_dragging) = Self::render_crosshair_style_editor(
                            ui,
                            language,
                            (index, "crosshair-style-grid"),
                            &mut preset.style,
                        );
                        preset_changed |= style_changed;
                        any_dragging |= style_dragging;
                    }
                });
            }

            if remove {
                remove_index = Some(index);
                break;
            }
            if preset_changed {
                self.mark_crosshair_profile_dirty(index);
            }
        }

        if let Some(profile) = copy_crosshair_profile {
            self.copy_crosshair_profile(&profile);
        }
        if let Some(index) = paste_crosshair_profile_after {
            self.paste_crosshair_profile_after(index);
        }
        if refresh_crosshair_profiles {
            self.sync_crosshair();
            self.persist();
        }
        if let Some(index) = remove_index {
            self.flush_crosshair_profile_dirty(true);
            let remove_name = self.state.profiles[index].name.clone();
            self.state.profiles.remove(index);
            self.status = format!("Deleted crosshair preset: {remove_name}");
            if self.state.profiles.is_empty() {
                self.state.selected_profile = None;
                self.state.active_style = CrosshairStyle::default();
                self.state.active_style.enabled = false;
                self.save_name = String::new();
            } else {
                let next = self.state.profiles[0].clone();
                self.state.selected_profile = Some(next.name.clone());
                self.state.active_style = next.style;
                self.save_name = next.name;
            }
            self.sync_profiles();
            self.persist();
            self.crosshair_editor_dirty = true;
        }

        if self.crosshair_editor_dirty {
            self.flush_crosshair_profile_dirty(false);
            if self.crosshair_editor_dirty {
                ui.ctx().request_repaint_after(Duration::from_millis(16));
            }
        }
    }
}
