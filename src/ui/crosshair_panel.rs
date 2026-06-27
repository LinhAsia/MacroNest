use crate::model::*;
use crate::ui::CrosshairApp;
use eframe::egui::{self, *};
use std::time::Duration;

impl CrosshairApp {
    pub(crate) fn render_crosshair_panel(&mut self, ui: &mut egui::Ui) {
        self.render_crosshair_presets_panel(ui);
    }

    fn render_crosshair_style_editor<H: std::hash::Hash>(
        ui: &mut egui::Ui,
        language: UiLanguage,
        grid_id: H,
        style: &mut CrosshairStyle,
        link_lengths: &mut bool,
    ) -> (bool, bool) {
        let mut changed = false;
        let mut dragging = false;
        let screen_size = Self::screen_size();
        let (offset_limit_x, offset_limit_y) = Self::crosshair_position_limits(screen_size);
        let inline_field_width = 280.0;
        let side_button_size = [60.0, 20.0];
        egui::Grid::new(grid_id)
            .num_columns(2)
            .spacing([14.0, 8.0])
            .show(ui, |ui| {
                ui.label(Self::tr_lang(
                    language,
                    "Horizontal length",
                    "Horizontal length",
                ));
                let response =
                    ui.add_sized(
                        [340.0, 20.0],
                        DragValue::new(&mut style.horizontal_length)
                            .range(0.0..=80.0)
                            .speed(0.1),
                    );
                let horizontal_changed = response.changed();
                changed |= horizontal_changed;
                dragging |= response.dragged();
                if horizontal_changed && *link_lengths {
                    style.vertical_length = style.horizontal_length;
                }
                ui.end_row();

                ui.label(Self::tr_lang(
                    language,
                    "Vertical length",
                    "Vertical length",
                ));
                let response =
                    ui.add_sized(
                        [340.0, 20.0],
                        DragValue::new(&mut style.vertical_length)
                            .range(0.0..=80.0)
                            .speed(0.1),
                    );
                let vertical_changed = response.changed();
                changed |= vertical_changed;
                dragging |= response.dragged();
                if vertical_changed && *link_lengths {
                    style.horizontal_length = style.vertical_length;
                }
                ui.end_row();

                ui.label(Self::tr_lang(language, "Link lengths", "Link lengths"));
                ui.horizontal(|ui| {
                    ui.add_space(inline_field_width);
                    if ui
                        .add_sized(
                            side_button_size,
                            Button::new(Self::tr_lang(language, "Link", "Link"))
                                .selected(*link_lengths),
                        )
                        .clicked()
                    {
                        *link_lengths = !*link_lengths;
                        if *link_lengths {
                            style.vertical_length = style.horizontal_length;
                        }
                        changed = true;
                    }
                });
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

                ui.label(Self::tr_lang(language, "X", "X"));
                ui.horizontal(|ui| {
                    let response = ui.add_sized(
                        [inline_field_width, 20.0],
                        DragValue::new(&mut style.x_offset)
                            .range(0..=offset_limit_x)
                            .speed(1.0),
                    );
                    changed |= response.changed();
                    dragging |= response.dragged();
                    if ui
                        .add_sized(
                            side_button_size,
                            Button::new(Self::tr_lang(language, "Center", "Center")),
                        )
                        .clicked()
                    {
                        style.x_offset = DEFAULT_CROSSHAIR_X_OFFSET;
                        changed = true;
                    }
                });
                ui.end_row();

                ui.label(Self::tr_lang(language, "Y", "Y"));
                ui.horizontal(|ui| {
                    let response = ui.add_sized(
                        [inline_field_width, 20.0],
                        DragValue::new(&mut style.y_offset)
                            .range(0..=offset_limit_y)
                            .speed(1.0),
                    );
                    changed |= response.changed();
                    dragging |= response.dragged();
                    if ui
                        .add_sized(
                            side_button_size,
                            Button::new(Self::tr_lang(language, "Center", "Center")),
                        )
                        .clicked()
                    {
                        style.y_offset = DEFAULT_CROSSHAIR_Y_OFFSET;
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
                        "Center dot size",
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

                ui.label(Self::tr_lang(
                    language,
                    "Crosshair color",
                    "Crosshair color",
                ));
                let response = Self::edit_rgba_color(ui, &mut style.color);
                changed |= response.changed();
                dragging |= response.dragged();
                ui.end_row();

                if style.outline_enabled {
                    ui.label(Self::tr_lang(language, "Outline color", "Outline color"));
                    let response = Self::edit_rgba_color(ui, &mut style.outline_color);
                    changed |= response.changed();
                    dragging |= response.dragged();
                    ui.end_row();
                }
                ui.label(Self::tr_lang(language, "Custom pixels", "Custom pixels"));
                ui.vertical(|ui| {
                    let grid_size = style.custom_pixels_grid_size.max(1).min(31) as i32;

                    // ---- Build grid from stored string ----
                    let mut grid = vec![vec!['.'; grid_size as usize]; grid_size as usize];
                    if let Some(ref pixels) = style.custom_pixels {
                        let lines: Vec<&str> = pixels.lines().collect();
                        for r in 0..grid_size.min(lines.len() as i32) {
                            let chars: Vec<char> = lines[r as usize].chars().collect();
                            for c in 0..grid_size.min(chars.len() as i32) {
                                grid[r as usize][c as usize] = chars[c as usize];
                            }
                        }
                    }
                    let mut grid_changed = false;

                    // ---- Size slider ----
                    let size_id = ui.id().with("grid-size");
                    let mut pending_size = ui
                        .data(|data| data.get_temp::<u8>(size_id))
                        .unwrap_or(style.custom_pixels_grid_size.max(1).min(31));
                    let mut size_changed = false;
                    let mut clear_clicked = false;
                    ui.horizontal(|ui| {
                        ui.label(Self::tr_lang(language, "Size", "Size"));
                        let size_response = ui.add(
                            egui::Slider::new(&mut pending_size, 3u8..=31u8)
                                .step_by(2.0)
                                .suffix(" px"),
                        );
                        size_changed = size_response.changed();
                        clear_clicked = ui
                            .button(Self::tr_lang(language, "Clear", "Clear"))
                            .clicked();
                    });
                    if size_changed {
                        let new_size = pending_size.max(3).min(31) as i32;
                        let mut new_grid = vec![vec!['.'; new_size as usize]; new_size as usize];
                        let copy_r = grid_size.min(new_size);
                        let copy_c = grid_size.min(new_size);
                        for r in 0..copy_r {
                            for c in 0..copy_c {
                                new_grid[r as usize][c as usize] = grid[r as usize][c as usize];
                            }
                        }
                        grid = new_grid;
                        style.custom_pixels_grid_size = pending_size;
                        grid_changed = true;
                    }
                    if clear_clicked {
                        grid = vec![vec!['.'; grid.len()]; grid.len()];
                        grid_changed = true;
                    }
                    let grid_size = grid.len() as i32;
                    ui.data_mut(|data| data.insert_temp(size_id, pending_size));

                    // ---- Paint color picker (edits style.color directly so the render matches) ----
                    let mut paint_rgba =
                        [style.color.r, style.color.g, style.color.b, style.color.a];
                    ui.horizontal(|ui| {
                        ui.label(Self::tr_lang(language, "Paint color", "Paint color"));
                        if ui
                            .color_edit_button_srgba_unmultiplied(&mut paint_rgba)
                            .changed()
                        {
                            style.color.r = paint_rgba[0];
                            style.color.g = paint_rgba[1];
                            style.color.b = paint_rgba[2];
                            style.color.a = paint_rgba[3];
                            changed = true;
                        }
                    });

                    let paint_egui_color = Color32::from_rgba_unmultiplied(
                        paint_rgba[0],
                        paint_rgba[1],
                        paint_rgba[2],
                        paint_rgba[3],
                    );

                    ui.add_space(6.0);

                    // ---- Canvas allocation ----
                    let cell_size = 16.0_f32;
                    let canvas_size =
                        vec2(grid_size as f32 * cell_size, grid_size as f32 * cell_size);
                    let (rect, response) =
                        ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

                    // ---- Handle mouse input ----
                    if response.hovered() || response.dragged() || response.clicked() {
                        let pointer_down = ui.input(|input| input.pointer.any_down());
                        if pointer_down {
                            if let Some(mouse_pos) = ui.input(|input| input.pointer.interact_pos())
                            {
                                if rect.contains(mouse_pos) {
                                    let relative_pos = mouse_pos - rect.min;
                                    let c = (relative_pos.x / cell_size).floor() as i32;
                                    let r = (relative_pos.y / cell_size).floor() as i32;
                                    if c >= 0 && c < grid_size && r >= 0 && r < grid_size {
                                        let r = r as usize;
                                        let c = c as usize;
                                        let is_right_click = ui.input(|input| {
                                            input
                                                .pointer
                                                .button_down(egui::PointerButton::Secondary)
                                        });
                                        let new_char = if is_right_click { '.' } else { '#' };
                                        if grid[r][c] != new_char {
                                            grid[r][c] = new_char;
                                            grid_changed = true;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ---- Render grid ----
                    let painter = ui.painter_at(rect);
                    let center_r = (grid_size / 2) as usize;
                    let center_c = (grid_size / 2) as usize;

                    // 1. Draw cells
                    for r in 0..grid_size as usize {
                        for c in 0..grid_size as usize {
                            let cell_rect = egui::Rect::from_min_size(
                                rect.min + vec2(c as f32 * cell_size, r as f32 * cell_size),
                                vec2(cell_size, cell_size),
                            );
                            let cell_char = grid[r][c];
                            let fill = match cell_char {
                                '#' | 'x' | 'X' | '1' => paint_egui_color,
                                '@' | 'o' | 'O' | '2' => Color32::from_rgba_unmultiplied(
                                    style.outline_color.r,
                                    style.outline_color.g,
                                    style.outline_color.b,
                                    style.outline_color.a,
                                ),
                                _ => {
                                    if (r + c) % 2 == 0 {
                                        Color32::from_gray(35)
                                    } else {
                                        Color32::from_gray(50)
                                    }
                                }
                            };
                            painter.rect_filled(cell_rect, 0.0, fill);
                        }
                    }

                    // 2. Center marker: small crosshair lines at center cell
                    {
                        let marker_color = Color32::from_rgba_unmultiplied(255, 80, 80, 200);
                        let stroke = egui::Stroke::new(1.0, marker_color);
                        let cx = rect.min.x + center_c as f32 * cell_size + cell_size * 0.5;
                        let cy = rect.min.y + center_r as f32 * cell_size + cell_size * 0.5;
                        let arm = cell_size * 0.35;
                        // Horizontal tick
                        painter.line_segment(
                            [egui::pos2(cx - arm, cy), egui::pos2(cx + arm, cy)],
                            stroke,
                        );
                        // Vertical tick
                        painter.line_segment(
                            [egui::pos2(cx, cy - arm), egui::pos2(cx, cy + arm)],
                            stroke,
                        );
                        // Highlight the center cell border
                        let center_cell_rect = egui::Rect::from_min_size(
                            rect.min
                                + vec2(center_c as f32 * cell_size, center_r as f32 * cell_size),
                            vec2(cell_size, cell_size),
                        );
                        painter.rect_stroke(
                            center_cell_rect,
                            0.0,
                            egui::Stroke::new(
                                1.0,
                                Color32::from_rgba_unmultiplied(255, 80, 80, 160),
                            ),
                            egui::StrokeKind::Inside,
                        );
                    }

                    // 3. Draw grid lines
                    let grid_color = Color32::from_gray(70);
                    for i in 0..=grid_size {
                        let offset = i as f32 * cell_size;
                        painter.line_segment(
                            [
                                rect.min + vec2(offset, 0.0),
                                rect.min + vec2(offset, canvas_size.y),
                            ],
                            egui::Stroke::new(0.5, grid_color),
                        );
                        painter.line_segment(
                            [
                                rect.min + vec2(0.0, offset),
                                rect.min + vec2(canvas_size.x, offset),
                            ],
                            egui::Stroke::new(0.5, grid_color),
                        );
                    }

                    // 4. Outer border
                    painter.rect_stroke(
                        rect,
                        0.0,
                        egui::Stroke::new(1.0, Color32::from_gray(100)),
                        egui::StrokeKind::Inside,
                    );

                    // ---- Hint text ----
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(Self::tr_lang(
                            language,
                            "Left-click: paint  |  Right-click: erase  |  Red marker = center",
                            "Left-click: paint  |  Right-click: erase  |  Red marker = center",
                        ))
                        .small()
                        .color(Color32::from_gray(140)),
                    );

                    // ---- Sync grid back to style ----
                    if grid_changed {
                        // Check if any pixel is non-empty
                        let has_pixels = grid.iter().any(|row| row.iter().any(|&ch| ch != '.'));
                        if has_pixels {
                            let lines: Vec<String> =
                                grid.iter().map(|row| row.iter().collect()).collect();
                            style.custom_pixels = Some(lines.join("\n"));
                        } else {
                            style.custom_pixels = None;
                        }
                        changed = true;
                    }
                });
                ui.end_row();
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
                    "+ Add crosshair preset",
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
            let preset_snapshot = self.state.profiles[index].clone();
            {
                let preset = &mut self.state.profiles[index];
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
                            "Crosshair Settings",
                        ));
                        let (style_changed, style_dragging) = Self::render_crosshair_style_editor(
                            ui,
                            language,
                            (index, "crosshair-style-grid"),
                            &mut preset.style,
                            &mut self.crosshair_link_lengths,
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
                if is_selected {
                    let preset = &self.state.profiles[index];
                    if preset.name != preset_snapshot.name {
                        self.state.selected_profile = Some(preset.name.clone());
                        self.save_name = preset.name.clone();
                    }
                    self.state.active_style = preset.style.clone();
                    self.state.active_style.enabled = preset.enabled;
                }
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
            let pointer_down = ui.input(|i| i.pointer.any_down());
            self.flush_crosshair_profile_dirty(!pointer_down);
            if self.crosshair_editor_dirty {
                ui.ctx().request_repaint_after(Duration::from_millis(16));
            }
        }
    }
}
