use crate::model::*;
use crate::ui::{CrosshairApp, CrosshairColorTarget, VisionCaptureTarget};
use eframe::egui::{self, *};
use std::time::Duration;

impl CrosshairApp {
    fn crosshair_asset_native_scale_for_paths(
        paths: &crate::storage::AppPaths,
        asset_name: &str,
    ) -> Option<f32> {
        let asset_path = paths.asset_path(asset_name);
        let ext = asset_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .unwrap_or_default();

        if ext == "svg" {
            let bytes = std::fs::read(&asset_path).ok()?;
            let tree = resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default())
                .ok()?;
            let size = tree.size();
            return Some(size.width().max(size.height()).round().clamp(16.0, 4096.0));
        }

        let (width, height) = image::image_dimensions(&asset_path).ok()?;
        Some((width.max(height) as f32).clamp(16.0, 4096.0))
    }

    pub(crate) fn render_crosshair_panel(&mut self, ui: &mut egui::Ui) {
        self.render_crosshair_presets_panel(ui);
    }

    fn render_crosshair_color_control(
        ui: &mut egui::Ui,
        color: &mut RgbaColor,
        target: VisionCaptureTarget,
        language: UiLanguage,
        active_color_pick_target: Option<VisionCaptureTarget>,
        pending_color_pick_target: &mut Option<VisionCaptureTarget>,
    ) -> (bool, bool) {
        let mut changed = false;
        let mut dragging = false;
        ui.horizontal(|ui| {
            let response = Self::edit_rgba_color(ui, color);
            changed |= response.changed();
            dragging |= response.dragged();
            ui.label(
                RichText::new("#")
                    .strong()
                    .color(ui.visuals().weak_text_color()),
            );
            changed |= Self::render_rgba_hex_input(
                ui,
                ui.make_persistent_id(color as *const RgbaColor as usize),
                color,
                egui::color_picker::Alpha::BlendOrAdditive,
                92.0,
            );

            let picking_active = active_color_pick_target == Some(target);
            if ui
                .add_sized(
                    [24.0, 21.0],
                    Button::new(Self::material_icon_text(0xe3b8, 16.0)).selected(picking_active),
                )
                .on_hover_text(Self::tr_lang(language, "Pick from screen", "Pick from screen"))
                .clicked()
            {
                *pending_color_pick_target = Some(target);
            }
        });
        (changed, dragging)
    }

    fn render_crosshair_style_editor<H: std::hash::Hash>(
        ui: &mut egui::Ui,
        language: UiLanguage,
        profile_index: usize,
        grid_id: H,
        style: &mut CrosshairStyle,
        link_lengths: &mut bool,
        active_color_pick_target: Option<VisionCaptureTarget>,
        pending_color_pick_target: &mut Option<VisionCaptureTarget>,
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
                ui.horizontal(|ui| {
                    let response = ui.add_sized(
                        [inline_field_width, 20.0],
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

                ui.label(Self::tr_lang(
                    language,
                    "Vertical length",
                    "Vertical length",
                ));
                ui.horizontal(|ui| {
                    let response = ui.add_sized(
                        [inline_field_width, 20.0],
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
                    ui.add_space(side_button_size[0]);
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

                ui.label(Self::tr_lang(language, "Circle", "Circle"));
                changed |= ui
                    .checkbox(
                        &mut style.ring_enabled,
                        Self::tr_lang(language, "Enabled", "Enabled"),
                    )
                    .changed();
                ui.end_row();

                if style.ring_enabled {
                    ui.label(Self::tr_lang(language, "Circle radius", "Circle radius"));
                    let response = ui.add_sized(
                        [340.0, 20.0],
                        DragValue::new(&mut style.ring_radius)
                            .range(0.0..=96.0)
                            .speed(0.1),
                    );
                    changed |= response.changed();
                    dragging |= response.dragged();
                    ui.end_row();

                    ui.label(Self::tr_lang(
                        language,
                        "Circle thickness",
                        "Circle thickness",
                    ));
                    let response = ui.add_sized(
                        [340.0, 20.0],
                        DragValue::new(&mut style.ring_thickness)
                            .range(0.0..=32.0)
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
                let (color_changed, color_dragging) = Self::render_crosshair_color_control(
                    ui,
                    &mut style.color,
                    VisionCaptureTarget::CrosshairProfileColor {
                        profile_index,
                        target: CrosshairColorTarget::Main,
                    },
                    language,
                    active_color_pick_target,
                    pending_color_pick_target,
                );
                changed |= color_changed;
                dragging |= color_dragging;
                ui.end_row();

                if style.outline_enabled {
                    ui.label(Self::tr_lang(language, "Outline color", "Outline color"));
                    let (color_changed, color_dragging) = Self::render_crosshair_color_control(
                        ui,
                        &mut style.outline_color,
                        VisionCaptureTarget::CrosshairProfileColor {
                            profile_index,
                            target: CrosshairColorTarget::Outline,
                        },
                        language,
                        active_color_pick_target,
                        pending_color_pick_target,
                    );
                    changed |= color_changed;
                    dragging |= color_dragging;
                    ui.end_row();
                }

                if style.ring_enabled {
                    ui.label(Self::tr_lang(language, "Circle color", "Circle color"));
                    let (color_changed, color_dragging) = Self::render_crosshair_color_control(
                        ui,
                        &mut style.ring_color,
                        VisionCaptureTarget::CrosshairProfileColor {
                            profile_index,
                            target: CrosshairColorTarget::Ring,
                        },
                        language,
                        active_color_pick_target,
                        pending_color_pick_target,
                    );
                    changed |= color_changed;
                    dragging |= color_dragging;
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
        let active_color_pick_target = if self.vision_capture_active
            && self.vision_capture_mode == Some(crate::ui::VisionCaptureMode::ColorSample)
        {
            self.vision_capture_target
        } else {
            None
        };
        let mut pending_color_pick_target = None;

        let mut copy_crosshair_profile = None;
        let mut paste_crosshair_profile_after = None;
        let mut pending_crosshair_draw_request: Option<(String, Option<String>, f32)> = None;
        let mut refresh_crosshair_profiles = false;
        let can_paste_crosshair = self.crosshair_profile_clipboard.is_some();
        let asset_paths = self.paths.clone();
        for index in 0..self.state.profiles.len() {
            let mut remove = false;
            let mut preset_changed = false;
            let mut pending_color_pick_target_for_preset = None;
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
                            if Self::sound_style_toggle_button(
                                ui,
                                &Self::tr_lang(language, "Copy", "Copy"),
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
                            index,
                            (index, "crosshair-style-grid"),
                            &mut preset.style,
                            &mut self.crosshair_link_lengths,
                            active_color_pick_target,
                            &mut pending_color_pick_target_for_preset,
                        );
                        preset_changed |= style_changed;
                        any_dragging |= style_dragging;

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button(Self::tr_lang(language, "Draw crosshair", "Draw crosshair"))
                                .clicked()
                            {
                                pending_crosshair_draw_request = Some((
                                    preset.name.clone(),
                                    preset.style.custom_asset.clone(),
                                    preset.style.custom_scale,
                                ));
                            }
                            if preset.style.custom_asset.is_some()
                                && ui
                                    .button(Self::tr_lang(
                                        language,
                                        "Clear custom draw",
                                        "Clear custom draw",
                                    ))
                                    .clicked()
                            {
                                preset.style.custom_asset = None;
                                preset_changed = true;
                            }
                        });

                        if preset.style.custom_asset.is_some() {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(Self::tr_lang(language, "Asset scale", "Asset scale"));
                                let response = ui.add_sized(
                                    [180.0, 20.0],
                                    DragValue::new(&mut preset.style.custom_scale)
                                        .range(16.0..=4096.0)
                                        .speed(1.0),
                                );
                                preset_changed |= response.changed();
                                any_dragging |= response.dragged();
                                if ui
                                    .button(Self::tr_lang(language, "Reset", "Reset"))
                                    .clicked()
                                    && let Some(asset_name) = preset.style.custom_asset.as_deref()
                                    && let Some(native_scale) =
                                        Self::crosshair_asset_native_scale_for_paths(
                                            &asset_paths,
                                            asset_name,
                                        )
                                {
                                    preset.style.custom_scale = native_scale;
                                    preset_changed = true;
                                }
                            });
                        }
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
            if pending_color_pick_target.is_none() {
                pending_color_pick_target = pending_color_pick_target_for_preset;
            }
        }

        if let Some(profile) = copy_crosshair_profile {
            self.copy_crosshair_profile(&profile);
        }
        if let Some((profile_name, asset_name, asset_scale)) = pending_crosshair_draw_request {
            let _ = self
                .overlay_tx
                .send(crate::overlay::OverlayCommand::BeginCrosshairDraw {
                    profile_name: profile_name.clone(),
                    asset_name,
                    asset_scale,
                });
            self.status = format!("Opened crosshair draw for {profile_name}.");
        }
        if let Some(index) = paste_crosshair_profile_after {
            self.paste_crosshair_profile_after(index);
        }
        if let Some(target) = pending_color_pick_target {
            self.begin_color_pick_capture(ui.ctx(), target);
        }
        if refresh_crosshair_profiles {
            self.persist_after_sync(Self::sync_crosshair);
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
            self.persist_after_sync(Self::sync_profiles);
            self.crosshair_editor_dirty = true;
        }

        if self.crosshair_editor_dirty {
            let pointer_down = ui.input(|i| i.pointer.any_down());
            self.flush_crosshair_profile_dirty(!pointer_down);
            if self.crosshair_editor_dirty {
                if any_dragging {
                    ui.ctx().request_repaint();
                } else {
                    ui.ctx().request_repaint_after(Duration::from_millis(16));
                }
            }
        }
    }
}
