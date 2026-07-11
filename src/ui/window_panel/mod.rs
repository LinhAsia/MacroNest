use crate::hotkey;
use crate::model::*;
use crate::overlay::OverlayCommand;
use crate::ui::{CrosshairApp, VisionCaptureTarget, ZoomPreviewView};
use crate::window_list;
use eframe::egui::{self, Button, Color32, DragValue, RichText, Sense, TextBuffer, TextEdit, vec2};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct MonitorLayoutMetrics {
    monitor_width: f32,
    monitor_height: f32,
    work_left: f32,
    work_top: f32,
    work_width: f32,
    work_height: f32,
}

impl CrosshairApp {
    pub(crate) fn render_window_presets_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        let language = self.state.ui_language;

        ui.horizontal(|ui| {
            if ui
                .button(self.tr("+ Add resize preset", "+ Add resize preset"))
                .clicked()
            {
                self.add_window_preset();
                self.persist_window_presets();
            }
            if ui
                .button(self.tr("+ Add layout preset", "+ Add layout preset"))
                .clicked()
            {
                self.add_window_layout();
            }
        });

        ui.add_space(16.0);

        let mut remove_id = None;
        let mut live_sync = false;
        ui.label(
            RichText::new(Self::tr_lang(language, "Resize Presets", "Resize Presets"))
                .strong()
                .size(14.0),
        );
        ui.add_space(4.0);
        for index in 0..self.state.window_presets.len() {
            let mut next_capture_target = None;
            let mut cancel_active_capture = false;
            let mut run_resize_now = false;
            let active_capture_target = self.capture_target.clone();
            let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
            let preset_snapshot = self.state.window_presets[index].clone();
            let preview = if preset_snapshot.preview_enabled && !preset_snapshot.collapsed {
                self.window_preview_for_target(
                    ui.ctx(),
                    200_000 + preset_snapshot.id,
                    preset_snapshot.target_window_title.as_ref(),
                    &preset_snapshot.extra_target_window_titles,
                    preset_snapshot.match_duplicate_window_titles,
                )
            } else {
                self.zoom_preview_cache
                    .remove(&(200_000 + preset_snapshot.id));
                None
            };
            {
                let preset = &mut self.state.window_presets[index];
                preset.enabled = preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
                Self::show_preset_card(ui, preset.enabled, |ui| {
                    egui::Grid::new((preset.id, "window-preset-header"))
                        .num_columns(2)
                        .spacing([14.0, 8.0])
                        .show(ui, |ui| {
                            let capture_target = CaptureRequest::WindowPresetHotkey(preset.id);
                            ui.horizontal(|ui| {
                                let name_width = Self::preset_header_name_width(ui);
                                let response = ui.add_sized(
                                    [name_width, 21.0],
                                    TextEdit::singleline(&mut preset.name),
                                );
                                Self::apply_vietnamese_input_if_changed(
                                    &response,
                                    self.state.vietnamese_input_enabled,
                                    self.state.vietnamese_input_mode,
                                    &mut preset.name,
                                );
                                live_sync |= response.changed();

                                live_sync |= Self::render_preset_trigger_chips(
                                    ui,
                                    language,
                                    &mut preset.hotkey,
                                    &mut preset.trigger_keys,
                                    active_capture_target.as_ref(),
                                    &capture_target,
                                    pending_combo_keys.as_ref(),
                                );
                                preset.enabled = preset.hotkey.is_some()
                                    || !preset.trigger_keys.trim().is_empty();
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let capture_active =
                                        active_capture_target.as_ref() == Some(&capture_target);
                                    let capture_time = ui.ctx().input(|input| input.time) as f32;
                                    let pulse = if capture_active {
                                        0.5 + 0.5 * (capture_time * 6.0).sin().abs()
                                    } else {
                                        0.0
                                    };
                                    let has_keys = preset.hotkey.is_some()
                                        || !preset.trigger_keys.trim().is_empty();
                                    let fill = if capture_active {
                                        Color32::from_rgba_premultiplied(
                                            (88.0 + pulse * 28.0) as u8,
                                            (84.0 + pulse * 28.0) as u8,
                                            (44.0 + pulse * 10.0) as u8,
                                            255,
                                        )
                                    } else if has_keys {
                                        Color32::from_rgba_premultiplied(72, 156, 116, 120)
                                    } else {
                                        ui.visuals().faint_bg_color
                                    };
                                    let stroke = if capture_active {
                                        Color32::from_rgb(255, 232, 96)
                                    } else if has_keys {
                                        Color32::from_rgb(126, 224, 182)
                                    } else {
                                        ui.visuals().widgets.noninteractive.bg_stroke.color
                                    };

                                    let hover_text = if capture_active {
                                        Self::tr_lang(
                                            language,
                                            "Capturing... Press any key.",
                                            "Capturing... Press any key.",
                                        )
                                        .to_string()
                                    } else if has_keys {
                                        let bindings_labels: Vec<String> =
                                            Self::preset_trigger_bindings(
                                                &preset.hotkey,
                                                &preset.trigger_keys,
                                            )
                                            .iter()
                                            .map(|b| hotkey::format_binding(Some(b)))
                                            .collect();
                                        format!(
                                            "{} {}\n{}",
                                            Self::tr_lang(language, "Hotkey:", "Hotkey:"),
                                            bindings_labels.join(", "),
                                            Self::tr_lang(
                                                language,
                                                "Left click: rebind | Right click: clear",
                                                "Left click: rebind | Right click: clear"
                                            )
                                        )
                                    } else {
                                        Self::tr_lang(
                                            language,
                                            "Left click: bind hotkey",
                                            "Left click: bind hotkey",
                                        )
                                        .to_string()
                                    };

                                    let btn_text = if capture_active {
                                        RichText::new(Self::tr_lang(
                                            language,
                                            "Capturing...",
                                            "Capturing...",
                                        ))
                                        .strong()
                                        .color(Color32::from_rgb(255, 232, 96))
                                    } else {
                                        Self::material_icon_text(0xe312, 18.0)
                                    };
                                    let btn_width = if capture_active { 84.0 } else { 36.0 };
                                    let btn_response = ui
                                        .add_sized(
                                            [btn_width, 24.0],
                                            Button::new(btn_text)
                                                .fill(fill)
                                                .stroke(egui::Stroke::new(1.0, stroke)),
                                        )
                                        .on_hover_text(hover_text);

                                    if btn_response.clicked() {
                                        if capture_active {
                                            cancel_active_capture = true;
                                        } else {
                                            next_capture_target = Some((
                                                capture_target.clone(),
                                                format!(
                                                    "Capturing preset hotkey for {}.",
                                                    preset.name
                                                ),
                                            ));
                                        }
                                    }
                                    if btn_response.secondary_clicked() {
                                        preset.hotkey = None;
                                        preset.trigger_keys.clear();
                                        preset.enabled = false;
                                        live_sync = true;
                                    }

                                    let run_response = Self::sound_style_icon_button(
                                        ui,
                                        Self::material_icon_text(0xe037, 18.0),
                                    )
                                    .on_hover_text(Self::tr_lang(
                                        language,
                                        "Run this resize preset now",
                                        "Run this resize preset now",
                                    ));
                                    if run_response.clicked() {
                                        run_resize_now = true;
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
                                        if preset.collapsed {
                                            preset.preview_enabled = false;
                                        }
                                        live_sync = true;
                                    }
                                },
                            );
                            ui.end_row();
                        });
                    if preset.collapsed {
                        return;
                    }
                    if let Some((preview_x, preview_y)) =
                        Self::window_anchor_preview_position(preset)
                    {
                        if preset.x != preview_x {
                            preset.x = preview_x;
                            live_sync = true;
                        }
                        if preset.y != preview_y {
                            preset.y = preview_y;
                            live_sync = true;
                        }
                    }
                    egui::Grid::new((preset.id, "window-preset-grid"))
                        .num_columns(2)
                        .spacing([14.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(Self::tr_lang(language, "Size", "Size"));
                            ui.horizontal(|ui| {
                                ui.label(Self::tr_lang(language, "Width", "Width"));
                                live_sync |= ui
                                    .add(DragValue::new(&mut preset.width).range(1..=20000))
                                    .changed();
                                ui.label(Self::tr_lang(language, "Height", "Height"));
                                live_sync |= ui
                                    .add(DragValue::new(&mut preset.height).range(1..=20000))
                                    .changed();
                            });
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Anchor", "Anchor"));
                            live_sync |= Self::window_anchor_picker(ui, preset);
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Position", "Position"));
                            ui.horizontal(|ui| {
                                ui.add_enabled_ui(preset.anchor == WindowAnchor::Manual, |ui| {
                                    ui.label("X");
                                    live_sync |= ui
                                        .add(DragValue::new(&mut preset.x).range(-20000..=20000))
                                        .changed();
                                    ui.label("Y");
                                    live_sync |= ui
                                        .add(DragValue::new(&mut preset.y).range(-20000..=20000))
                                        .changed();
                                });
                            });
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Title", "Title"));
                            live_sync |= ui
                                .checkbox(
                                    &mut preset.remove_title_bar,
                                    Self::tr_lang(language, "Remove bar", "Remove bar"),
                                )
                                .on_hover_text(Self::tr_lang(
                                    language,
                                    "Remove title bar before apply. Off restores it.",
                                    "Remove title bar before apply. Off restores it.",
                                ))
                                .changed();
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Animated Apply", "Animated Apply"));
                            ui.horizontal_wrapped(|ui| {
                                live_sync |= ui
                                    .checkbox(
                                        &mut preset.animate_enabled,
                                        Self::tr_lang(language, "Enabled", "Enabled"),
                                    )
                                    .changed();
                                if preset.animate_enabled {
                                    ui.label(Self::tr_lang(language, "Duration", "Duration"));
                                    live_sync |= ui
                                        .add(
                                            DragValue::new(&mut preset.animate_duration_ms)
                                                .range(60..=10_000)
                                                .suffix(" ms"),
                                        )
                                        .changed();
                                }
                            });
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Target Window", "Target Window"));
                            live_sync |= Self::render_multi_window_targets_with_duplicate_mode(
                                ui,
                                language,
                                (preset.id, "window-target"),
                                Self::tr_lang(language, "Focus", "Focus"),
                                &mut preset.target_window_title,
                                &mut preset.extra_target_window_titles,
                                &mut preset.match_duplicate_window_titles,
                                &self.open_window_infos,
                            );
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Preview", "Preview"));
                            ui.horizontal_wrapped(|ui| {
                                live_sync |= ui
                                    .checkbox(
                                        &mut preset.preview_enabled,
                                        Self::tr_lang(
                                            language,
                                            "Stream preview in editor",
                                            "Stream preview in editor",
                                        ),
                                    )
                                    .changed();
                            });
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    Self::render_window_preset_preview(
                        ui,
                        language,
                        preset,
                        if preset.preview_enabled { preview.as_ref() } else { None },
                        &mut live_sync,
                    );
                    let screen_size = Self::screen_size();
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(Self::tr_lang(language, "Center X", "Center X"))
                            .clicked()
                        {
                            if preset.anchor != WindowAnchor::Manual {
                                if let Some((wx, wy)) = Self::window_anchor_preview_position(preset) {
                                    preset.x = wx;
                                    preset.y = wy;
                                }
                                preset.anchor = WindowAnchor::Manual;
                            }
                            preset.x = ((screen_size.x as i32 - preset.width.max(1)) / 2).max(0);
                            live_sync = true;
                        }
                        if ui
                            .button(Self::tr_lang(language, "Center Y", "Center Y"))
                            .clicked()
                        {
                            if preset.anchor != WindowAnchor::Manual {
                                if let Some((wx, wy)) = Self::window_anchor_preview_position(preset) {
                                    preset.x = wx;
                                    preset.y = wy;
                                }
                                preset.anchor = WindowAnchor::Manual;
                            }
                            preset.y = ((screen_size.y as i32 - preset.height.max(1)) / 2).max(0);
                            live_sync = true;
                        }
                    });
                });
            }
            if let Some((target, status)) = next_capture_target.take() {
                self.begin_capture(target, status);
            }
            if cancel_active_capture {
                self.cancel_capture();
            }
            if run_resize_now {
                let preset_id = self.state.window_presets[index].id.to_string();
                match crate::overlay::apply_window_preset_by_id(&preset_id) {
                    Ok(()) => {
                        self.status = format!(
                            "Applied resize preset {}.",
                            self.state.window_presets[index].name
                        );
                    }
                    Err(error) => {
                        self.status = format!(
                            "Failed to apply resize preset {}: {}",
                            self.state.window_presets[index].name, error
                        );
                    }
                }
            }
        }

        if live_sync {
            self.persist_window_presets_deferred(ui.ctx());
        }
        if let Some(id) = remove_id {
            self.state.window_presets.retain(|preset| preset.id != id);
            self.persist_window_presets();
        }

        self.render_layout_panel(ui);
    }

    pub(crate) fn render_pin_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(Self::tr_lang(
                    language,
                    "+ Add pin preset",
                    "+ Add pin preset",
                ))
                .clicked()
            {
                self.add_pin_preset();
                self.persist_window_presets();
            }
        });

        ui.add_space(8.0);

        let screen_size = Self::screen_size();
        let mut remove_id = None;
        let mut live_sync = false;
        let pin_preview_allowed = self.state.active_panel == AppPanel::Pin
            && ui
                .ctx()
                .input(|input| input.viewport().focused != Some(false));
        for index in 0..self.state.pin_presets.len() {
            let mut next_capture_target = None;
            let mut cancel_active_capture = false;
            let mut toggle_pin_now = false;
            let active_capture_target = self.capture_target.clone();
            let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
            let preset_snapshot = self.state.pin_presets[index].clone();
            let source_preview = if pin_preview_allowed && !preset_snapshot.collapsed {
                self.pin_preview_for_target(
                    ui.ctx(),
                    100_000 + preset_snapshot.id,
                    preset_snapshot.target_window_title.as_ref(),
                    &preset_snapshot.extra_target_window_titles,
                    preset_snapshot.match_duplicate_window_titles,
                )
            } else {
                None
            };
            let preview = if preset_snapshot.preview_enabled {
                source_preview.clone()
            } else {
                None
            };
            let vietnamese_input_enabled = self.state.vietnamese_input_enabled;
            let vietnamese_input_mode = self.state.vietnamese_input_mode;
            let mut begin_color_picker_preset_id = None;
            let mut begin_region_picker_preset_id = None;
            let mut begin_source_crop_picker_preset_id = None;
            let preset = &mut self.state.pin_presets[index];
            preset.use_source_crop = true;
            preset.enabled = preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
            Self::show_preset_card(ui, preset.enabled, |ui| {
                ui.horizontal(|ui| {
                    let name_width = Self::preset_header_name_width(ui);
                    let response =
                        ui.add_sized([name_width, 21.0], TextEdit::singleline(&mut preset.name));
                    Self::apply_vietnamese_input_if_changed(
                        &response,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                        &mut preset.name,
                    );
                    live_sync |= response.changed();

                    let capture_target = CaptureRequest::PinPresetHotkey(preset.id);
                    live_sync |= Self::render_preset_trigger_chips(
                        ui,
                        language,
                        &mut preset.hotkey,
                        &mut preset.trigger_keys,
                        active_capture_target.as_ref(),
                        &capture_target,
                        pending_combo_keys.as_ref(),
                    );
                    preset.enabled =
                        preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let capture_active =
                            active_capture_target.as_ref() == Some(&capture_target);
                        let capture_time = ui.ctx().input(|input| input.time) as f32;
                        let pulse = if capture_active {
                            0.5 + 0.5 * (capture_time * 6.0).sin().abs()
                        } else {
                            0.0
                        };
                        let has_keys =
                            preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
                        let fill = if capture_active {
                            Color32::from_rgba_premultiplied(
                                (88.0 + pulse * 28.0) as u8,
                                (84.0 + pulse * 28.0) as u8,
                                (44.0 + pulse * 10.0) as u8,
                                255,
                            )
                        } else if has_keys {
                            Color32::from_rgba_premultiplied(72, 156, 116, 120)
                        } else {
                            ui.visuals().faint_bg_color
                        };
                        let stroke = if capture_active {
                            Color32::from_rgb(255, 232, 96)
                        } else if has_keys {
                            Color32::from_rgb(126, 224, 182)
                        } else {
                            ui.visuals().widgets.noninteractive.bg_stroke.color
                        };

                        let hover_text = if capture_active {
                            Self::tr_lang(
                                language,
                                "Capturing... Press any key.",
                                "Capturing... Press any key.",
                            )
                            .to_string()
                        } else if has_keys {
                            let bindings_labels: Vec<String> =
                                Self::preset_trigger_bindings(&preset.hotkey, &preset.trigger_keys)
                                    .iter()
                                    .map(|b| hotkey::format_binding(Some(b)))
                                    .collect();
                            format!(
                                "{} {}\n{}",
                                Self::tr_lang(language, "Hotkey:", "Hotkey:"),
                                bindings_labels.join(", "),
                                Self::tr_lang(
                                    language,
                                    "Left click: rebind | Right click: clear",
                                    "Left click: rebind | Right click: clear"
                                )
                            )
                        } else {
                            Self::tr_lang(
                                language,
                                "Left click: bind hotkey",
                                "Left click: bind hotkey",
                            )
                            .to_string()
                        };

                        let btn_text = if capture_active {
                            RichText::new(Self::tr_lang(language, "Capturing...", "Capturing..."))
                                .strong()
                                .color(Color32::from_rgb(255, 232, 96))
                        } else {
                            Self::material_icon_text(0xe312, 18.0)
                        };
                        let btn_width = if capture_active { 84.0 } else { 36.0 };
                        let btn_response = ui
                            .add_sized(
                                [btn_width, 24.0],
                                Button::new(btn_text)
                                    .fill(fill)
                                    .stroke(egui::Stroke::new(1.0, stroke)),
                            )
                            .on_hover_text(hover_text);

                        if btn_response.clicked() {
                            if capture_active {
                                cancel_active_capture = true;
                            } else {
                                next_capture_target = Some((
                                    capture_target,
                                    format!("Capturing pin hotkey for {}.", preset.name),
                                ));
                            }
                        }
                        if btn_response.secondary_clicked() {
                            preset.hotkey = None;
                            preset.trigger_keys.clear();
                            preset.enabled = false;
                            live_sync = true;
                        }

                        let pin_active = crate::overlay::is_pin_active(&preset.id.to_string());
                        let run_response = Self::sound_style_icon_button(
                            ui,
                            Self::material_icon_text(
                                if pin_active { 0xe047 } else { 0xe037 },
                                18.0,
                            ),
                        )
                        .on_hover_text(if pin_active {
                            Self::tr_lang(language, "Stop this pin preset", "Stop this pin preset")
                        } else {
                            Self::tr_lang(
                                language,
                                "Run this pin preset now",
                                "Run this pin preset now",
                            )
                        });
                        if run_response.clicked() {
                            toggle_pin_now = true;
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
                            live_sync = true;
                        }
                    });
                });
                if preset.collapsed {
                    return;
                }

                egui::Grid::new((preset.id, "pin-grid"))
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(Self::tr_lang(language, "Target Window", "Target Window"));
                        let target_changed = Self::render_multi_window_targets_with_duplicate_mode(
                            ui,
                            language,
                            (preset.id, "pin-target-window"),
                            Self::tr_lang(language, "Focus", "Focus"),
                            &mut preset.target_window_title,
                            &mut preset.extra_target_window_titles,
                            &mut preset.match_duplicate_window_titles,
                            &self.open_window_infos,
                        );
                        live_sync |= target_changed;
                        ui.end_row();

                        preset.use_custom_bounds = true;
                        if preset.overlay_style != PinOverlayStyle::Rectangle {
                            preset.overlay_style = PinOverlayStyle::Rectangle;
                            live_sync = true;
                        }

                        ui.label(Self::tr_lang(language, "Preview", "Preview"));
                        live_sync |= ui
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

                        ui.label(Self::tr_lang(language, "Binarize", "Binarize"));
                        let binary_changed = ui
                            .checkbox(
                                &mut preset.binary_filter,
                                Self::tr_lang(
                                    language,
                                    "Binarize (Black & White)",
                                    "Binarize (Black & White)",
                                ),
                            )
                            .changed();
                        live_sync |= binary_changed;
                        ui.end_row();

                        if preset.binary_filter {
                            ui.label(Self::tr_lang(language, "Output Filter", "Output Filter"));
                            let hide_black_changed = ui
                                .checkbox(
                                    &mut preset.binary_transparent_black,
                                    Self::tr_lang(
                                        language,
                                        "Hide black pixels",
                                        "Hide black pixels",
                                    ),
                                )
                                .changed();
                            if hide_black_changed && preset.binary_transparent_black {
                                preset.binary_transparent_white = false;
                            }
                            let hide_white_changed = ui
                                .checkbox(
                                    &mut preset.binary_transparent_white,
                                    Self::tr_lang(
                                        language,
                                        "Hide white pixels",
                                        "Hide white pixels",
                                    ),
                                )
                                .changed();
                            if hide_white_changed && preset.binary_transparent_white {
                                preset.binary_transparent_black = false;
                            }
                            live_sync |= hide_black_changed || hide_white_changed;
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Binarize Mode", "Binarize Mode"));
                            let mode_changed =
                                egui::ComboBox::from_id_salt((preset.id, "bin-mode"))
                                    .selected_text(match preset.binary_mode {
                                        PinBinaryMode::Grayscale => {
                                            Self::tr_lang(language, "Grayscale", "Grayscale")
                                        }
                                        PinBinaryMode::ColorSimilarity => Self::tr_lang(
                                            language,
                                            "Color Similarity",
                                            "Color Similarity",
                                        ),
                                    })
                                    .show_ui(ui, |ui| {
                                        let mut m_changed = false;
                                        m_changed |= ui
                                            .selectable_value(
                                                &mut preset.binary_mode,
                                                PinBinaryMode::Grayscale,
                                                Self::tr_lang(language, "Grayscale", "Grayscale"),
                                            )
                                            .clicked();
                                        m_changed |= ui
                                            .selectable_value(
                                                &mut preset.binary_mode,
                                                PinBinaryMode::ColorSimilarity,
                                                Self::tr_lang(
                                                    language,
                                                    "Color Similarity",
                                                    "Color Similarity",
                                                ),
                                            )
                                            .clicked();
                                        m_changed
                                    })
                                    .inner
                                    .unwrap_or(false);
                            live_sync |= mode_changed;
                            ui.end_row();

                            match preset.binary_mode {
                                PinBinaryMode::Grayscale => {
                                    ui.label(Self::tr_lang(language, "Threshold", "Threshold"));
                                    live_sync |= ui
                                        .add(egui::Slider::new(
                                            &mut preset.binary_threshold,
                                            0..=255,
                                        ))
                                        .changed();
                                    ui.end_row();
                                }
                                PinBinaryMode::ColorSimilarity => {
                                    ui.label(Self::tr_lang(
                                        language,
                                        "Target Color",
                                        "Target Color",
                                    ));
                                    ui.vertical(|ui| {
                                        let colors = preset.binary_target_colors();
                                        if colors.is_empty() {
                                            ui.monospace("None");
                                        } else {
                                            let mut remove_color_index = None;
                                            egui::Grid::new((preset.id, "pin-color-grid"))
                                                .num_columns(8)
                                                .min_col_width(0.0)
                                                .spacing([ui.spacing().item_spacing.x, 4.0])
                                                .show(ui, |ui| {
                                                    for (index, color) in
                                                        colors.iter().copied().enumerate()
                                                    {
                                                        if Self::image_search_color_tile(ui, color)
                                                            .clicked()
                                                        {
                                                            remove_color_index = Some(index);
                                                        }
                                                        if (index + 1) % 8 == 0 {
                                                            ui.end_row();
                                                        }
                                                    }
                                                });
                                            if let Some(index) = remove_color_index
                                                && preset.remove_binary_target_color_at(index)
                                            {
                                                live_sync = true;
                                            }
                                        }

                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            if Self::image_search_add_color_button(ui, language)
                                                .clicked()
                                            {
                                                begin_color_picker_preset_id = Some(preset.id);
                                            }

                                            let popup_id = ui.make_persistent_id((
                                                preset.id,
                                                "pin-manual-color-popup",
                                            ));
                                            let mut popup_open = ui
                                                .ctx()
                                                .data(|data| data.get_temp::<bool>(popup_id))
                                                .unwrap_or(false);

                                            let manual_button = ui
                                                .add_sized(
                                                    [24.0, 21.0],
                                                    Button::new(Self::material_icon_text(
                                                        0xe40a, 18.0,
                                                    )),
                                                )
                                                .on_hover_text(Self::tr_lang(
                                                    language,
                                                    "Manual color input",
                                                    "Manual color input",
                                                ));

                                            if manual_button.clicked() {
                                                popup_open = true;
                                            }

                                            let mut added_color = false;

                                            let popup_response =
                                                egui::Popup::from_response(&manual_button)
                                                    .id(popup_id)
                                                    .open_bool(&mut popup_open)
                                                    .align(egui::RectAlign::BOTTOM_START)
                                                    .layout(egui::Layout::top_down_justified(
                                                        egui::Align::Min,
                                                    ))
                                                    .width(260.0)
                                                    .close_behavior(
                                                        egui::PopupCloseBehavior::IgnoreClicks,
                                                    )
                                                    .show(|ui| {
                                                        ui.set_min_width(260.0);
                                                        ui.label(Self::tr_lang(
                                                            language,
                                                            "Manual color",
                                                            "Manual color",
                                                        ));
                                                        ui.separator();

                                                        if Self::render_premium_color_picker(
                                                            ui,
                                                            &mut self.vision_manual_color,
                                                            egui::color_picker::Alpha::Opaque,
                                                        ) {
                                                            self.vision_manual_color_hex = format!(
                                                                "{:02X}{:02X}{:02X}",
                                                                self.vision_manual_color.r,
                                                                self.vision_manual_color.g,
                                                                self.vision_manual_color.b
                                                            );
                                                        }

                                                        ui.add_space(8.0);

                                                        if ui
                                                            .button(Self::tr_lang(
                                                                language,
                                                                "Add color",
                                                                "Add color",
                                                            ))
                                                            .clicked()
                                                        {
                                                            added_color = true;
                                                        }
                                                    });

                                            if added_color {
                                                preset.add_binary_target_color(
                                                    self.vision_manual_color,
                                                );
                                                live_sync = true;
                                                popup_open = false;
                                            }

                                            if popup_open
                                                && let Some(pointer_pos) =
                                                    ui.ctx().pointer_hover_pos()
                                            {
                                                let mut keep_open_rect =
                                                    manual_button.rect.expand(10.0);
                                                if let Some(popup) = &popup_response {
                                                    keep_open_rect = keep_open_rect
                                                        .union(popup.response.rect.expand(10.0));
                                                }
                                                if !keep_open_rect.contains(pointer_pos) {
                                                    popup_open = false;
                                                }
                                            }
                                            ui.ctx().data_mut(|data| {
                                                data.insert_temp(popup_id, popup_open)
                                            });
                                        });
                                    });
                                    ui.end_row();

                                    ui.label(Self::tr_lang(language, "Tolerance", "Tolerance"));
                                    live_sync |= ui
                                        .add(egui::Slider::new(
                                            &mut preset.binary_threshold,
                                            0..=255,
                                        ))
                                        .changed();
                                    ui.end_row();
                                }
                            }
                        }
                    });

                if preset.use_custom_bounds {
                    live_sync |= Self::render_zoom_rect_editor(
                        ui,
                        (preset.id, "pin-bounds"),
                        Self::tr_lang(language, "Pinned Region", "Pinned Region"),
                        &mut preset.x,
                        &mut preset.y,
                        &mut preset.width,
                        &mut preset.height,
                        screen_size,
                        preview.as_ref(),
                        None,
                        if preset.use_source_crop {
                            Some((
                                preset.source_x,
                                preset.source_y,
                                preset.source_width,
                                preset.source_height,
                            ))
                        } else {
                            None
                        },
                        None,
                        Some(
                            (preset.source_width.max(1) as f32)
                                / (preset.source_height.max(1) as f32),
                        ),
                        false,
                        true,
                        true,
                    );
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(Self::tr_lang(language, "Center X", "Center X"))
                            .clicked()
                        {
                            preset.x = ((screen_size.x as i32 - preset.width.max(1)) / 2).max(0);
                            live_sync = true;
                        }
                        if ui
                            .button(Self::tr_lang(language, "Center Y", "Center Y"))
                            .clicked()
                        {
                            preset.y = ((screen_size.y as i32 - preset.height.max(1)) / 2).max(0);
                            live_sync = true;
                        }
                        if ui
                            .button(Self::tr_lang(language, "Pick area", "Pick area"))
                            .clicked()
                        {
                            begin_region_picker_preset_id = Some(preset.id);
                        }
                    });
                } else {
                    ui.label(
                        RichText::new(Self::tr_lang(
                            language,
                            "Pinned view will keep the original window position and size.",
                            "Pinned view will keep the original window position and size.",
                        ))
                        .italics(),
                    );
                }

                if preset.use_source_crop {
                    let source_crop_metrics_id =
                        ui.make_persistent_id((preset.id, "pin-source-crop-preview-metrics"));
                    if let Some(preview_frame) = source_preview.as_ref() {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(
                                source_crop_metrics_id,
                                (
                                    preview_frame.screen_x,
                                    preview_frame.screen_y,
                                    preview_frame.logical_width.max(1),
                                    preview_frame.logical_height.max(1),
                                ),
                            );
                        });
                    }
                    let source_crop_preview_metrics = ui
                        .ctx()
                        .data(|data| data.get_temp::<(i32, i32, i32, i32)>(source_crop_metrics_id));
                    if (!preset.source_crop_initialized || preset.source_crop_fit_version < 1)
                        && let Some(preview_frame) = source_preview.as_ref()
                    {
                        preset.source_x = 0;
                        preset.source_y = 0;
                        preset.source_width = preview_frame.logical_width.max(1);
                        preset.source_height = preview_frame.logical_height.max(1);
                        preset.source_crop_initialized = true;
                        preset.source_crop_fit_version = 1;
                        live_sync = true;
                    }
                    if preset.source_crop_initialized
                        && preset.source_crop_fit_version < 2
                        && let Some(preview_frame) = source_preview.as_ref()
                    {
                        let logical_width = preview_frame.logical_width.max(1);
                        let logical_height = preview_frame.logical_height.max(1);
                        let looks_screen_relative = preset.source_x < 0
                            || preset.source_y < 0
                            || preset.source_x >= logical_width
                            || preset.source_y >= logical_height
                            || preset.source_x + preset.source_width > logical_width
                            || preset.source_y + preset.source_height > logical_height;
                        if looks_screen_relative {
                            preset.source_x -= preview_frame.screen_x;
                            preset.source_y -= preview_frame.screen_y;
                            preset.source_x =
                                preset.source_x.clamp(0, logical_width.saturating_sub(1));
                            preset.source_y =
                                preset.source_y.clamp(0, logical_height.saturating_sub(1));
                            preset.source_width = preset
                                .source_width
                                .max(1)
                                .min(logical_width.saturating_sub(preset.source_x).max(1));
                            preset.source_height = preset
                                .source_height
                                .max(1)
                                .min(logical_height.saturating_sub(preset.source_y).max(1));
                            live_sync = true;
                        }
                        preset.source_crop_fit_version = 2;
                    }
                    let crop_changed = Self::render_zoom_rect_editor(
                        ui,
                        (preset.id, "pin-source-crop"),
                        Self::tr_lang(language, "Source Crop", "Source Crop"),
                        &mut preset.source_x,
                        &mut preset.source_y,
                        &mut preset.source_width,
                        &mut preset.source_height,
                        screen_size,
                        source_preview.as_ref(),
                        source_crop_preview_metrics,
                        None,
                        None,
                        None,
                        true,
                        preset.preview_enabled,
                        true,
                    );
                    if crop_changed {
                        preset.source_crop_initialized = true;
                        preset.source_crop_fit_version = 2;
                    }
                    live_sync |= crop_changed;
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(Self::tr_lang(
                                language,
                                "Reset to Full Window",
                                "Reset to Full Window",
                            ))
                            .clicked()
                        {
                            let mut target_frame = None;
                            if let Some(preview_frame) = source_preview.as_ref() {
                                target_frame = Some((
                                    preview_frame.logical_width,
                                    preview_frame.logical_height,
                                ));
                            } else {
                                if let Some(frame) =
                                    window_list::capture_window_preview_with_candidates(
                                        preset.target_window_title.as_ref().map(|s| s.as_str()),
                                        &preset.extra_target_window_titles,
                                        preset.match_duplicate_window_titles,
                                        720,
                                    )
                                {
                                    target_frame =
                                        Some((frame.logical_width, frame.logical_height));
                                }
                            }

                            if let Some((w, h)) = target_frame {
                                preset.source_x = 0;
                                preset.source_y = 0;
                                preset.source_width = w.max(1);
                                preset.source_height = h.max(1);
                                preset.source_crop_initialized = true;
                                preset.source_crop_fit_version = 2;
                                live_sync = true;
                            }
                        }
                        if ui
                            .button(Self::tr_lang(language, "Pick area", "Pick area"))
                            .clicked()
                        {
                            begin_source_crop_picker_preset_id = Some(preset.id);
                        }
                    });
                }
            });
            if let Some(pid) = begin_color_picker_preset_id {
                self.begin_color_pick_capture(ui.ctx(), VisionCaptureTarget::PinPresetColor(pid));
            }
            if let Some(pid) = begin_region_picker_preset_id {
                self.begin_region_capture(ui.ctx(), VisionCaptureTarget::PinPresetRegion(pid));
            }
            if let Some(pid) = begin_source_crop_picker_preset_id {
                self.begin_region_capture(ui.ctx(), VisionCaptureTarget::PinPresetSourceCrop(pid));
            }
            if let Some((target, status)) = next_capture_target.take() {
                self.begin_capture(target, status);
            }
            if cancel_active_capture {
                self.cancel_capture();
            }
            if toggle_pin_now {
                let preset_id = self.state.pin_presets[index].id.to_string();
                let preset_name = self.state.pin_presets[index].name.clone();
                if crate::overlay::is_pin_active(&preset_id) {
                    crate::overlay::disable_pin_preset(&preset_id);
                    self.status = format!("Stopped pin preset {}.", preset_name);
                } else {
                    match crate::overlay::enable_pin_preset(&preset_id) {
                        Ok(()) => {
                            self.status = format!("Started pin preset {}.", preset_name);
                        }
                        Err(error) => {
                            self.status =
                                format!("Failed to start pin preset {}: {}", preset_name, error);
                        }
                    }
                }
            }
        }

        if let Some(id) = remove_id {
            self.state.pin_presets.retain(|preset| preset.id != id);
            live_sync = true;
        }
        if live_sync {
            self.persist_window_presets_deferred(ui.ctx());
        }
    }

    pub(crate) fn render_window_preset_preview(
        ui: &mut egui::Ui,
        language: UiLanguage,
        preset: &mut WindowPreset,
        preview: Option<&ZoomPreviewView>,
        live_sync: &mut bool,
    ) {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum DragHandle {
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

        let screen_size = Self::screen_size();
        let aspect_ratio = if screen_size.y > 0.0 {
            screen_size.x / screen_size.y
        } else {
            16.0 / 9.0
        };
        let width = ui.available_width();
        let height = width / aspect_ratio;
        let max_height = 400.0;
        let (desired_width, desired_height) = if height > max_height {
            (max_height * aspect_ratio, max_height)
        } else {
            (width, height)
        };
        let (canvas_rect, response) =
            ui.allocate_exact_size(vec2(width, desired_height), Sense::drag());
        let draw_rect =
            egui::Rect::from_center_size(canvas_rect.center(), vec2(desired_width, desired_height))
                .shrink(4.0);

        // Draw monitor screen background
        ui.painter().rect_filled(
            draw_rect,
            6.0,
            Color32::from_rgba_premultiplied(18, 24, 22, 220),
        );
        ui.painter().rect_stroke(
            draw_rect,
            6.0,
            egui::Stroke::new(1.5, Color32::from_rgb(104, 148, 124)),
            egui::StrokeKind::Outside,
        );

        // Calculate mapped window rect
        let scale_x = draw_rect.width() / screen_size.x.max(1.0);
        let scale_y = draw_rect.height() / screen_size.y.max(1.0);

        let (wx, wy) = if let Some(pos) = Self::window_anchor_preview_position(preset) {
            pos
        } else {
            (preset.x, preset.y)
        };
        let ww = preset.width;
        let wh = preset.height;

        let left = draw_rect.left() + wx as f32 * scale_x;
        let top = draw_rect.top() + wy as f32 * scale_y;
        let w = ww as f32 * scale_x;
        let h = wh as f32 * scale_y;

        let window_rect = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(w, h));

        // Interaction Handling
        let drag_id = ui.make_persistent_id((preset.id, "preview-drag-handle"));
        let mut active_handle: DragHandle =
            ui.data_mut(|d| d.get_temp(drag_id).unwrap_or(DragHandle::None));
        let pick_window_drag_handle = |pointer_pos: egui::Pos2, window_rect: egui::Rect| {
            let dist_tl = pointer_pos.distance(window_rect.left_top());
            let dist_tr = pointer_pos.distance(window_rect.right_top());
            let dist_bl = pointer_pos.distance(window_rect.left_bottom());
            let dist_br = pointer_pos.distance(window_rect.right_bottom());
            let edge_threshold = 8.0;
            let vertical_hit_min = window_rect.top() - edge_threshold;
            let vertical_hit_max = window_rect.bottom() + edge_threshold;
            let horizontal_hit_min = window_rect.left() - edge_threshold;
            let horizontal_hit_max = window_rect.right() + edge_threshold;

            if dist_tl < 12.0 {
                DragHandle::TopLeft
            } else if dist_tr < 12.0 {
                DragHandle::TopRight
            } else if dist_bl < 12.0 {
                DragHandle::BottomLeft
            } else if dist_br < 12.0 {
                DragHandle::BottomRight
            } else if (pointer_pos.x - window_rect.left()).abs() < edge_threshold
                && pointer_pos.y >= vertical_hit_min
                && pointer_pos.y <= vertical_hit_max
            {
                DragHandle::Left
            } else if (pointer_pos.x - window_rect.right()).abs() < edge_threshold
                && pointer_pos.y >= vertical_hit_min
                && pointer_pos.y <= vertical_hit_max
            {
                DragHandle::Right
            } else if (pointer_pos.y - window_rect.top()).abs() < edge_threshold
                && pointer_pos.x >= horizontal_hit_min
                && pointer_pos.x <= horizontal_hit_max
            {
                DragHandle::Top
            } else if (pointer_pos.y - window_rect.bottom()).abs() < edge_threshold
                && pointer_pos.x >= horizontal_hit_min
                && pointer_pos.x <= horizontal_hit_max
            {
                DragHandle::Bottom
            } else if window_rect.contains(pointer_pos) {
                DragHandle::Center
            } else {
                DragHandle::None
            }
        };

        if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            if let Some(pointer_pos) = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos())
            {
                active_handle = pick_window_drag_handle(pointer_pos, window_rect);
                ui.data_mut(|d| d.insert_temp(drag_id, active_handle));
            }
        }

        let wp_primary_down = ui.input(|i| i.pointer.primary_down());
        let wp_delta = ui.input(|i| i.pointer.delta());
        if wp_primary_down && active_handle != DragHandle::None {
            let delta = wp_delta;
            let delta_x = delta.x / scale_x;
            let delta_y = delta.y / scale_y;
            let shift_pressed = ui.input(|i| i.modifiers.shift);
            let ctrl_pressed = ui.input(|i| i.modifiers.ctrl);
            let original_aspect = if preset.height > 0 {
                preset.width as f32 / preset.height as f32
            } else {
                16.0 / 9.0
            };
            let target_aspect = if let Some(preview_frame) = preview {
                if preview_frame.logical_height > 0 {
                    preview_frame.logical_width as f32 / preview_frame.logical_height as f32
                } else {
                    16.0 / 9.0
                }
            } else {
                if screen_size.y > 0.0 {
                    screen_size.x / screen_size.y
                } else {
                    16.0 / 9.0
                }
            };
            let use_aspect = if ctrl_pressed {
                Some(target_aspect)
            } else if shift_pressed {
                Some(original_aspect)
            } else {
                None
            };

            if preset.anchor != WindowAnchor::Manual {
                if let Some((wx, wy)) = Self::window_anchor_preview_position(preset) {
                    preset.x = wx;
                    preset.y = wy;
                }
                preset.anchor = WindowAnchor::Manual;
            }

            *live_sync = true;

            match active_handle {
                DragHandle::Center => {
                    preset.x += delta_x.round() as i32;
                    preset.y += delta_y.round() as i32;
                }
                DragHandle::Right => {
                    let new_w = (preset.width as f32 + delta_x).max(10.0);
                    if let Some(aspect) = use_aspect {
                        let new_h = new_w / aspect;
                        preset.width = new_w.round() as i32;
                        preset.height = new_h.round() as i32;
                    } else {
                        preset.width = new_w.round() as i32;
                    }
                }
                DragHandle::Left => {
                    let new_w = (preset.width as f32 - delta_x).max(10.0);
                    let actual_w = new_w.round() as i32;
                    let dx = preset.width - actual_w;
                    if let Some(aspect) = use_aspect {
                        let new_h = new_w / aspect;
                        let actual_h = new_h.round() as i32;
                        let dy = preset.height - actual_h;
                        preset.x += dx;
                        preset.y += dy;
                        preset.width = actual_w;
                        preset.height = actual_h;
                    } else {
                        preset.x += dx;
                        preset.width = actual_w;
                    }
                }
                DragHandle::Bottom => {
                    let new_h = (preset.height as f32 + delta_y).max(10.0);
                    if let Some(aspect) = use_aspect {
                        let new_w = new_h * aspect;
                        preset.width = new_w.round() as i32;
                        preset.height = new_h.round() as i32;
                    } else {
                        preset.height = new_h.round() as i32;
                    }
                }
                DragHandle::Top => {
                    let new_h = (preset.height as f32 - delta_y).max(10.0);
                    let actual_h = new_h.round() as i32;
                    let dy = preset.height - actual_h;
                    if let Some(aspect) = use_aspect {
                        let new_w = new_h * aspect;
                        let actual_w = new_w.round() as i32;
                        let dx = preset.width - actual_w;
                        preset.x += dx;
                        preset.y += dy;
                        preset.width = actual_w;
                        preset.height = actual_h;
                    } else {
                        preset.y += dy;
                        preset.height = actual_h;
                    }
                }
                DragHandle::BottomRight => {
                    let new_w = (preset.width as f32 + delta_x).max(10.0);
                    if let Some(aspect) = use_aspect {
                        let new_h = new_w / aspect;
                        preset.width = new_w.round() as i32;
                        preset.height = new_h.round() as i32;
                    } else {
                        let new_h = (preset.height as f32 + delta_y).max(10.0);
                        preset.width = new_w.round() as i32;
                        preset.height = new_h.round() as i32;
                    }
                }
                DragHandle::TopLeft => {
                    let new_w = (preset.width as f32 - delta_x).max(10.0);
                    if let Some(aspect) = use_aspect {
                        let new_h = new_w / aspect;
                        let actual_w = new_w.round() as i32;
                        let actual_h = new_h.round() as i32;
                        preset.x += preset.width - actual_w;
                        preset.y += preset.height - actual_h;
                        preset.width = actual_w;
                        preset.height = actual_h;
                    } else {
                        let new_h = (preset.height as f32 - delta_y).max(10.0);
                        let actual_w = new_w.round() as i32;
                        let actual_h = new_h.round() as i32;
                        preset.x += preset.width - actual_w;
                        preset.y += preset.height - actual_h;
                        preset.width = actual_w;
                        preset.height = actual_h;
                    }
                }
                DragHandle::TopRight => {
                    let new_w = (preset.width as f32 + delta_x).max(10.0);
                    if let Some(aspect) = use_aspect {
                        let new_h = new_w / aspect;
                        let actual_w = new_w.round() as i32;
                        let actual_h = new_h.round() as i32;
                        preset.y += preset.height - actual_h;
                        preset.width = actual_w;
                        preset.height = actual_h;
                    } else {
                        let new_h = (preset.height as f32 - delta_y).max(10.0);
                        let actual_w = new_w.round() as i32;
                        let actual_h = new_h.round() as i32;
                        preset.y += preset.height - actual_h;
                        preset.width = actual_w;
                        preset.height = actual_h;
                    }
                }
                DragHandle::BottomLeft => {
                    let new_w = (preset.width as f32 - delta_x).max(10.0);
                    if let Some(aspect) = use_aspect {
                        let new_h = new_w / aspect;
                        let actual_w = new_w.round() as i32;
                        let actual_h = new_h.round() as i32;
                        preset.x += preset.width - actual_w;
                        preset.width = actual_w;
                        preset.height = actual_h;
                    } else {
                        let new_h = (preset.height as f32 + delta_y).max(10.0);
                        let actual_w = new_w.round() as i32;
                        let actual_h = new_h.round() as i32;
                        preset.x += preset.width - actual_w;
                        preset.width = actual_w;
                        preset.height = actual_h;
                    }
                }
                DragHandle::None => {}
            }
        }

        if ui.input(|i| i.pointer.any_released()) {
            active_handle = DragHandle::None;
            ui.data_mut(|d| d.insert_temp(drag_id, active_handle));
        }

        if response.hovered() || active_handle != DragHandle::None {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.hover_pos()) {
                let mut handle_to_use = if active_handle != DragHandle::None {
                    active_handle
                } else {
                    pick_window_drag_handle(pointer_pos, window_rect)
                };
                if active_handle == DragHandle::None
                    && handle_to_use == DragHandle::Center
                    && !window_rect.contains(pointer_pos)
                {
                    handle_to_use = DragHandle::None;
                }

                match handle_to_use {
                    DragHandle::TopLeft | DragHandle::BottomRight => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                    }
                    DragHandle::TopRight | DragHandle::BottomLeft => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNeSw);
                    }
                    DragHandle::Left | DragHandle::Right => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    DragHandle::Top | DragHandle::Bottom => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    }
                    DragHandle::Center => {
                        if active_handle == DragHandle::Center {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        } else {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Clip/intersect window rect with draw_rect
        let clipped_window_rect = window_rect.intersect(draw_rect);

        if !clipped_window_rect.is_negative() {
            if let Some(preview_view) = &preview {
                let uv_min_x = ((clipped_window_rect.left() - window_rect.left())
                    / window_rect.width().max(1.0))
                .clamp(0.0, 1.0);
                let uv_max_x = ((clipped_window_rect.right() - window_rect.left())
                    / window_rect.width().max(1.0))
                .clamp(0.0, 1.0);
                let uv_min_y = ((clipped_window_rect.top() - window_rect.top())
                    / window_rect.height().max(1.0))
                .clamp(0.0, 1.0);
                let uv_max_y = ((clipped_window_rect.bottom() - window_rect.top())
                    / window_rect.height().max(1.0))
                .clamp(0.0, 1.0);

                let uv = egui::Rect::from_min_max(
                    egui::pos2(uv_min_x, uv_min_y),
                    egui::pos2(uv_max_x, uv_max_y),
                );

                ui.painter().image(
                    preview_view.texture.id(),
                    clipped_window_rect,
                    uv,
                    Color32::WHITE,
                );
            } else {
                ui.painter().rect_filled(
                    clipped_window_rect,
                    4.0,
                    Color32::from_rgba_premultiplied(40, 52, 68, 200),
                );
                let display_text = if let Some(title) = &preset.target_window_title {
                    title.clone()
                } else {
                    Self::tr_lang(language, "Target Window", "Target Window").to_string()
                };
                let font_id = egui::FontId::proportional(12.0);
                ui.painter().text(
                    clipped_window_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    display_text,
                    font_id,
                    Color32::from_rgb(180, 200, 220),
                );
            }

            // Draw window borders
            ui.painter().rect_stroke(
                clipped_window_rect,
                4.0,
                egui::Stroke::new(2.0, Color32::from_rgb(0, 191, 255)),
                egui::StrokeKind::Outside,
            );

            // Size text label
            let size_text = format!("{}x{}", preset.width, preset.height);
            ui.painter().text(
                clipped_window_rect.left_top() + egui::vec2(4.0, 4.0),
                egui::Align2::LEFT_TOP,
                size_text,
                egui::FontId::proportional(10.0),
                Color32::from_rgb(0, 191, 255),
            );
        }
    }

    pub(crate) fn render_zoom_rect_editor(
        ui: &mut egui::Ui,
        id_source: impl std::hash::Hash + Copy,
        label: &str,
        x: &mut i32,
        y: &mut i32,
        width: &mut i32,
        height: &mut i32,
        screen_size: egui::Vec2,
        preview: Option<&ZoomPreviewView>,
        preview_metrics_fallback: Option<(i32, i32, i32, i32)>,
        target_preview_source: Option<(i32, i32, i32, i32)>,
        keep_aspect_ratio: Option<f32>,
        ctrl_aspect_ratio: Option<f32>,
        use_preview_local_coordinates: bool,
        show_preview_image: bool,
        allow_wheel_zoom: bool,
    ) -> bool {
        let mut changed = false;
        ui.label(RichText::new(label).strong());
        let desired = vec2(ui.available_width().max(420.0), 260.0);
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
                *x = (*x + arrow_dx).clamp(0, screen_size.x.round() as i32);
                *y = (*y + arrow_dy).clamp(0, screen_size.y.round() as i32);
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
            Color32::from_rgba_premultiplied(24, 36, 30, 220),
        );
        ui.painter().rect_stroke(
            preview_rect,
            8.0,
            egui::Stroke::new(1.0, Color32::from_rgb(112, 156, 128)),
            egui::StrokeKind::Outside,
        );

        let selection_bounds_rect = preview_rect;
        let has_fallback_preview_metrics =
            preview_metrics_fallback.is_some_and(|(_, _, logical_width, logical_height)| {
                logical_width > 0 && logical_height > 0
            });
        let (
            coord_width,
            coord_height,
            content_scale,
            preview_content_rect,
            coordinate_space_rect,
            coords_origin_x,
            coords_origin_y,
        ) = if let Some(preview_frame) = preview {
            let base_window_pos = egui::pos2(
                selection_bounds_rect.left() + (preview_frame.screen_x as f32 * scale),
                selection_bounds_rect.top() + (preview_frame.screen_y as f32 * scale),
            );
            let base_window_size = vec2(
                preview_frame.logical_width.max(1) as f32 * scale,
                preview_frame.logical_height.max(1) as f32 * scale,
            );
            let base_window_rect = egui::Rect::from_min_size(base_window_pos, base_window_size);
            let (
                content_scale,
                preview_content_rect,
                coordinate_space_rect,
                screen_coords_origin_x,
                screen_coords_origin_y,
            ) = if allow_wheel_zoom {
                let zoom_id = ui.make_persistent_id((id_source, "zoom-editor-view-scale"));
                let pan_id = ui.make_persistent_id((id_source, "zoom-editor-view-pan"));
                let mut view_zoom =
                    ui.data_mut(|d| d.get_temp::<f32>(zoom_id).unwrap_or(1.0).clamp(1.0, 16.0));
                let mut view_pan =
                    ui.data_mut(|d| d.get_temp::<egui::Vec2>(pan_id).unwrap_or_default());
                let base_view_rect = if use_preview_local_coordinates {
                    base_window_rect
                } else {
                    selection_bounds_rect
                };
                let zoom_hit_rect = if use_preview_local_coordinates {
                    base_window_rect.intersect(selection_bounds_rect)
                } else {
                    selection_bounds_rect
                };

                if let Some(pointer_pos) = ui.input(|input| input.pointer.hover_pos())
                    && zoom_hit_rect.contains(pointer_pos)
                {
                    let scroll_y = ui.input(|input| {
                        if input.modifiers.ctrl {
                            input.raw_scroll_delta.y
                        } else {
                            0.0
                        }
                    });
                    if scroll_y.abs() > 0.0 {
                        ui.ctx().input_mut(|input| {
                            input.raw_scroll_delta = egui::Vec2::ZERO;
                            input.smooth_scroll_delta = egui::Vec2::ZERO;
                        });
                        let old_zoom = view_zoom;
                        let factor = if scroll_y > 0.0 { 1.12 } else { 1.0 / 1.12 };
                        view_zoom = (view_zoom * factor).clamp(1.0, 16.0);
                        if (view_zoom - old_zoom).abs() > f32::EPSILON {
                            let old_size = base_view_rect.size() * old_zoom;
                            let old_min = base_view_rect.center() - old_size * 0.5 + view_pan;
                            let rel_x = if old_size.x > 0.0 {
                                ((pointer_pos.x - old_min.x) / old_size.x).clamp(0.0, 1.0)
                            } else {
                                0.5
                            };
                            let rel_y = if old_size.y > 0.0 {
                                ((pointer_pos.y - old_min.y) / old_size.y).clamp(0.0, 1.0)
                            } else {
                                0.5
                            };
                            let new_size = base_view_rect.size() * view_zoom;
                            let new_min = egui::pos2(
                                pointer_pos.x - rel_x * new_size.x,
                                pointer_pos.y - rel_y * new_size.y,
                            );
                            view_pan = new_min - (base_view_rect.center() - new_size * 0.5);
                        }
                    }
                }

                if view_zoom <= 1.0001 {
                    view_zoom = 1.0;
                    view_pan = egui::Vec2::ZERO;
                }
                ui.data_mut(|d| {
                    d.insert_temp(zoom_id, view_zoom);
                    d.insert_temp(pan_id, view_pan);
                });

                let zoomed_view_rect = egui::Rect::from_center_size(
                    base_view_rect.center() + view_pan,
                    base_view_rect.size() * view_zoom,
                );
                let content_scale = scale * view_zoom;
                let preview_content_rect = if use_preview_local_coordinates {
                    zoomed_view_rect
                } else {
                    egui::Rect::from_min_size(
                        egui::pos2(
                            zoomed_view_rect.left()
                                + (preview_frame.screen_x as f32 * content_scale),
                            zoomed_view_rect.top()
                                + (preview_frame.screen_y as f32 * content_scale),
                        ),
                        vec2(
                            preview_frame.logical_width.max(1) as f32 * content_scale,
                            preview_frame.logical_height.max(1) as f32 * content_scale,
                        ),
                    )
                };
                (
                    content_scale,
                    preview_content_rect,
                    if use_preview_local_coordinates {
                        preview_content_rect
                    } else {
                        zoomed_view_rect
                    },
                    zoomed_view_rect.left(),
                    zoomed_view_rect.top(),
                )
            } else {
                (
                    scale,
                    base_window_rect,
                    selection_bounds_rect,
                    selection_bounds_rect.left(),
                    selection_bounds_rect.top(),
                )
            };
            if use_preview_local_coordinates {
                (
                    preview_frame.logical_width.max(1) as f32,
                    preview_frame.logical_height.max(1) as f32,
                    content_scale,
                    preview_content_rect,
                    coordinate_space_rect,
                    preview_content_rect.left(),
                    preview_content_rect.top(),
                )
            } else {
                (
                    screen_size.x,
                    screen_size.y,
                    content_scale,
                    preview_content_rect,
                    coordinate_space_rect,
                    screen_coords_origin_x,
                    screen_coords_origin_y,
                )
            }
        } else if let Some((
            fallback_screen_x,
            fallback_screen_y,
            fallback_logical_width,
            fallback_logical_height,
        )) = preview_metrics_fallback.filter(|(_, _, logical_width, logical_height)| {
            *logical_width > 0 && *logical_height > 0
        }) {
            let base_window_rect = egui::Rect::from_min_size(
                egui::pos2(
                    selection_bounds_rect.left() + (fallback_screen_x as f32 * scale),
                    selection_bounds_rect.top() + (fallback_screen_y as f32 * scale),
                ),
                vec2(
                    fallback_logical_width.max(1) as f32 * scale,
                    fallback_logical_height.max(1) as f32 * scale,
                ),
            );
            let (
                content_scale,
                preview_content_rect,
                coordinate_space_rect,
                screen_coords_origin_x,
                screen_coords_origin_y,
            ) = if allow_wheel_zoom {
                let zoom_id = ui.make_persistent_id((id_source, "zoom-editor-view-scale"));
                let pan_id = ui.make_persistent_id((id_source, "zoom-editor-view-pan"));
                let mut view_zoom =
                    ui.data_mut(|d| d.get_temp::<f32>(zoom_id).unwrap_or(1.0).clamp(1.0, 16.0));
                let mut view_pan =
                    ui.data_mut(|d| d.get_temp::<egui::Vec2>(pan_id).unwrap_or_default());
                let base_view_rect = if use_preview_local_coordinates {
                    base_window_rect
                } else {
                    selection_bounds_rect
                };
                let zoom_hit_rect = if use_preview_local_coordinates {
                    base_window_rect.intersect(selection_bounds_rect)
                } else {
                    selection_bounds_rect
                };

                if let Some(pointer_pos) = ui.input(|input| input.pointer.hover_pos())
                    && zoom_hit_rect.contains(pointer_pos)
                {
                    let scroll_y = ui.input(|input| {
                        if input.modifiers.ctrl {
                            input.raw_scroll_delta.y
                        } else {
                            0.0
                        }
                    });
                    if scroll_y.abs() > 0.0 {
                        ui.ctx().input_mut(|input| {
                            input.raw_scroll_delta = egui::Vec2::ZERO;
                            input.smooth_scroll_delta = egui::Vec2::ZERO;
                        });
                        let old_zoom = view_zoom;
                        let factor = if scroll_y > 0.0 { 1.12 } else { 1.0 / 1.12 };
                        view_zoom = (view_zoom * factor).clamp(1.0, 16.0);
                        if (view_zoom - old_zoom).abs() > f32::EPSILON {
                            let old_size = base_view_rect.size() * old_zoom;
                            let old_min = base_view_rect.center() - old_size * 0.5 + view_pan;
                            let rel_x = if old_size.x > 0.0 {
                                ((pointer_pos.x - old_min.x) / old_size.x).clamp(0.0, 1.0)
                            } else {
                                0.5
                            };
                            let rel_y = if old_size.y > 0.0 {
                                ((pointer_pos.y - old_min.y) / old_size.y).clamp(0.0, 1.0)
                            } else {
                                0.5
                            };
                            let new_size = base_view_rect.size() * view_zoom;
                            let new_min = egui::pos2(
                                pointer_pos.x - rel_x * new_size.x,
                                pointer_pos.y - rel_y * new_size.y,
                            );
                            view_pan = new_min - (base_view_rect.center() - new_size * 0.5);
                        }
                    }
                }

                if view_zoom <= 1.0001 {
                    view_zoom = 1.0;
                    view_pan = egui::Vec2::ZERO;
                }
                ui.data_mut(|d| {
                    d.insert_temp(zoom_id, view_zoom);
                    d.insert_temp(pan_id, view_pan);
                });

                let zoomed_view_rect = egui::Rect::from_center_size(
                    base_view_rect.center() + view_pan,
                    base_view_rect.size() * view_zoom,
                );
                let content_scale = scale * view_zoom;
                let preview_content_rect = if use_preview_local_coordinates {
                    zoomed_view_rect
                } else {
                    egui::Rect::from_min_size(
                        egui::pos2(
                            zoomed_view_rect.left() + (fallback_screen_x as f32 * content_scale),
                            zoomed_view_rect.top() + (fallback_screen_y as f32 * content_scale),
                        ),
                        vec2(
                            fallback_logical_width.max(1) as f32 * content_scale,
                            fallback_logical_height.max(1) as f32 * content_scale,
                        ),
                    )
                };
                (
                    content_scale,
                    preview_content_rect,
                    if use_preview_local_coordinates {
                        preview_content_rect
                    } else {
                        zoomed_view_rect
                    },
                    zoomed_view_rect.left(),
                    zoomed_view_rect.top(),
                )
            } else {
                (
                    scale,
                    base_window_rect,
                    selection_bounds_rect,
                    selection_bounds_rect.left(),
                    selection_bounds_rect.top(),
                )
            };
            if use_preview_local_coordinates {
                (
                    fallback_logical_width.max(1) as f32,
                    fallback_logical_height.max(1) as f32,
                    content_scale,
                    preview_content_rect,
                    coordinate_space_rect,
                    preview_content_rect.left(),
                    preview_content_rect.top(),
                )
            } else {
                (
                    screen_size.x,
                    screen_size.y,
                    content_scale,
                    preview_content_rect,
                    coordinate_space_rect,
                    screen_coords_origin_x,
                    screen_coords_origin_y,
                )
            }
        } else {
            (
                screen_size.x,
                screen_size.y,
                scale,
                selection_bounds_rect,
                selection_bounds_rect,
                selection_bounds_rect.left(),
                selection_bounds_rect.top(),
            )
        };

        let show_preview_reference_frame = use_preview_local_coordinates
            && !show_preview_image
            && (preview.is_some() || has_fallback_preview_metrics);
        if show_preview_reference_frame {
            let painter = if allow_wheel_zoom {
                ui.painter().with_clip_rect(selection_bounds_rect)
            } else {
                ui.painter().clone()
            };
            painter.rect_filled(
                preview_content_rect,
                6.0,
                Color32::from_rgba_premultiplied(50, 82, 120, 28),
            );
            painter.rect_stroke(
                preview_content_rect,
                6.0,
                egui::Stroke::new(1.5, Color32::from_rgb(108, 176, 255)),
                egui::StrokeKind::Outside,
            );
            painter.text(
                preview_content_rect.left_top() + vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                preview
                    .map(|frame| frame.title.as_str())
                    .unwrap_or("Target window frame"),
                egui::TextStyle::Small.resolve(ui.style()),
                Color32::from_rgb(182, 220, 255),
            );
        }

        if show_preview_image && let Some(preview_frame) = preview {
            let painter = if allow_wheel_zoom {
                ui.painter().with_clip_rect(selection_bounds_rect)
            } else {
                ui.painter().clone()
            };
            painter.image(
                preview_frame.texture.id(),
                preview_content_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            painter.text(
                preview_content_rect.left_top() + vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                &preview_frame.title,
                egui::TextStyle::Small.resolve(ui.style()),
                Color32::WHITE,
            );
        } else if show_preview_image && has_fallback_preview_metrics {
            let painter = if allow_wheel_zoom {
                ui.painter().with_clip_rect(selection_bounds_rect)
            } else {
                ui.painter().clone()
            };
            painter.rect_filled(
                preview_content_rect,
                4.0,
                Color32::from_rgba_premultiplied(18, 24, 24, 120),
            );
            painter.rect_stroke(
                preview_content_rect,
                4.0,
                egui::Stroke::new(1.0, Color32::from_rgb(88, 110, 98)),
                egui::StrokeKind::Outside,
            );
            painter.text(
                preview_content_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Preview unavailable",
                egui::TextStyle::Small.resolve(ui.style()),
                Color32::from_gray(190),
            );
        }

        let min_size = vec2(6.0, 6.0);
        let mut rect = egui::Rect::from_min_size(
            egui::pos2(
                coords_origin_x + (*x as f32 * content_scale),
                coords_origin_y + (*y as f32 * content_scale),
            ),
            vec2(
                (*width).max(1) as f32 * content_scale,
                (*height).max(1) as f32 * content_scale,
            ),
        );
        let active_bounds_rect = if use_preview_local_coordinates
            && (preview.is_some() || has_fallback_preview_metrics)
        {
            preview_content_rect
        } else {
            coordinate_space_rect
        };
        rect = rect.intersect(active_bounds_rect);
        if rect.width() < min_size.x {
            rect.max.x = (rect.min.x + min_size.x).min(active_bounds_rect.right());
        }
        if rect.height() < min_size.y {
            rect.max.y = (rect.min.y + min_size.y).min(active_bounds_rect.bottom());
        }

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

        let drag_id = ui.make_persistent_id((id_source, "zoom-selection-drag-handle"));
        let mut active_handle: SelectionDragHandle =
            ui.data_mut(|d| d.get_temp(drag_id).unwrap_or(SelectionDragHandle::None));
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

        let offset_id = ui.make_persistent_id((id_source, "zoom-selection-drag-offset"));
        let mut drag_offset: egui::Vec2 =
            ui.data_mut(|d| d.get_temp(offset_id).unwrap_or(egui::Vec2::ZERO));

        let anchor_id = ui.make_persistent_id((id_source, "zoom-selection-drag-anchor"));
        let mut drag_anchor: egui::Pos2 =
            ui.data_mut(|d| d.get_temp(anchor_id).unwrap_or(egui::Pos2::ZERO));

        if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            if let Some(pointer_pos) = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos())
            {
                active_handle = pick_selection_drag_handle(pointer_pos, rect);
                ui.data_mut(|d| d.insert_temp(drag_id, active_handle));

                // Compute offsets and anchors for the handles
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

        // Use pointer.primary_down() + pointer.latest_pos() so dragging continues even when
        // the mouse moves outside the canvas bounds (important for small boxes).
        let pointer_primary_down = ui.input(|i| i.pointer.primary_down());
        if pointer_primary_down && active_handle != SelectionDragHandle::None {
            if let Some(pointer_pos) = ui
                .input(|i| i.pointer.latest_pos())
                .or_else(|| ui.input(|i| i.pointer.hover_pos()))
            {
                let shift_pressed = ui.input(|i| i.modifiers.shift);
                let ctrl_pressed = ui.input(|i| i.modifiers.ctrl);
                let aspect = if rect.height() > 0.0 {
                    rect.width() / rect.height()
                } else {
                    16.0 / 9.0
                };
                let target_aspect = if let Some(preview_frame) = preview {
                    if preview_frame.logical_height > 0 {
                        preview_frame.logical_width as f32 / preview_frame.logical_height as f32
                    } else {
                        16.0 / 9.0
                    }
                } else {
                    if screen_size.y > 0.0 {
                        screen_size.x / screen_size.y
                    } else {
                        16.0 / 9.0
                    }
                };
                let lock_aspect = if let Some(keep_aspect_ratio) = keep_aspect_ratio {
                    keep_aspect_ratio
                } else if ctrl_pressed {
                    ctrl_aspect_ratio.unwrap_or(target_aspect)
                } else if shift_pressed {
                    aspect
                } else {
                    0.0
                };

                changed = true;

                let mut target_pos = pointer_pos - drag_offset;

                match active_handle {
                    SelectionDragHandle::Left
                    | SelectionDragHandle::TopLeft
                    | SelectionDragHandle::BottomLeft => {
                        target_pos.x = target_pos
                            .x
                            .clamp(active_bounds_rect.left(), drag_anchor.x - min_size.x);
                    }
                    SelectionDragHandle::Right
                    | SelectionDragHandle::TopRight
                    | SelectionDragHandle::BottomRight => {
                        target_pos.x = target_pos
                            .x
                            .clamp(drag_anchor.x + min_size.x, active_bounds_rect.right());
                    }
                    _ => {}
                }
                match active_handle {
                    SelectionDragHandle::Top
                    | SelectionDragHandle::TopLeft
                    | SelectionDragHandle::TopRight => {
                        target_pos.y = target_pos
                            .y
                            .clamp(active_bounds_rect.top(), drag_anchor.y - min_size.y);
                    }
                    SelectionDragHandle::Bottom
                    | SelectionDragHandle::BottomLeft
                    | SelectionDragHandle::BottomRight => {
                        target_pos.y = target_pos
                            .y
                            .clamp(drag_anchor.y + min_size.y, active_bounds_rect.bottom());
                    }
                    _ => {}
                }
                if active_handle == SelectionDragHandle::Center {
                    target_pos.x = target_pos.x.clamp(
                        active_bounds_rect.left(),
                        active_bounds_rect.right() - rect.width(),
                    );
                    target_pos.y = target_pos.y.clamp(
                        active_bounds_rect.top(),
                        active_bounds_rect.bottom() - rect.height(),
                    );
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

                // Bound checking for translation (only when moving the whole rect)
                if active_handle == SelectionDragHandle::Center {
                    if rect.left() < active_bounds_rect.left() {
                        rect = rect
                            .translate(egui::vec2(active_bounds_rect.left() - rect.left(), 0.0));
                    }
                    if rect.top() < active_bounds_rect.top() {
                        rect =
                            rect.translate(egui::vec2(0.0, active_bounds_rect.top() - rect.top()));
                    }
                    if rect.right() > active_bounds_rect.right() {
                        rect = rect
                            .translate(egui::vec2(active_bounds_rect.right() - rect.right(), 0.0));
                    }
                    if rect.bottom() > active_bounds_rect.bottom() {
                        rect = rect.translate(egui::vec2(
                            0.0,
                            active_bounds_rect.bottom() - rect.bottom(),
                        ));
                    }
                }

                rect.min.x = rect.min.x.clamp(
                    active_bounds_rect.left(),
                    active_bounds_rect.right() - min_size.x,
                );
                rect.min.y = rect.min.y.clamp(
                    active_bounds_rect.top(),
                    active_bounds_rect.bottom() - min_size.y,
                );
                rect.max.x = rect
                    .max
                    .x
                    .clamp(rect.min.x + min_size.x, active_bounds_rect.right());
                rect.max.y = rect
                    .max
                    .y
                    .clamp(rect.min.y + min_size.y, active_bounds_rect.bottom());
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

        let selection_painter = if allow_wheel_zoom {
            ui.painter().with_clip_rect(selection_bounds_rect)
        } else {
            ui.painter().clone()
        };

        if let (Some(preview_frame), Some((src_x, src_y, src_w, src_h))) =
            (preview, target_preview_source)
        {
            let uv = egui::Rect::from_min_max(
                egui::pos2(
                    (src_x as f32 / preview_frame.logical_width.max(1) as f32).clamp(0.0, 1.0),
                    (src_y as f32 / preview_frame.logical_height.max(1) as f32).clamp(0.0, 1.0),
                ),
                egui::pos2(
                    ((src_x + src_w) as f32 / preview_frame.logical_width.max(1) as f32)
                        .clamp(0.0, 1.0),
                    ((src_y + src_h) as f32 / preview_frame.logical_height.max(1) as f32)
                        .clamp(0.0, 1.0),
                ),
            );
            if uv.width() > 0.0 && uv.height() > 0.0 {
                let texture = preview_frame
                    .filtered_texture
                    .as_ref()
                    .unwrap_or(&preview_frame.texture);
                selection_painter.image(texture.id(), rect, uv, Color32::WHITE);
            }
        }

        selection_painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(2.0, Color32::from_rgb(124, 240, 164)),
            egui::StrokeKind::Outside,
        );

        let size_text = format!("{}x{}", *width, *height);
        selection_painter.text(
            rect.left_top() + egui::vec2(0.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            size_text,
            egui::FontId::proportional(10.0),
            Color32::from_rgb(124, 240, 164),
        );

        if changed {
            *x = ((rect.left() - coords_origin_x) / content_scale).round() as i32;
            *y = ((rect.top() - coords_origin_y) / content_scale).round() as i32;
            *width = (rect.width() / content_scale).round().max(1.0) as i32;
            *height = (rect.height() / content_scale).round().max(1.0) as i32;
            *x = (*x).clamp(0, coord_width.round() as i32);
            *y = (*y).clamp(0, coord_height.round() as i32);
        }

        ui.label(RichText::new(format!("X={} Y={} W={} H={}", *x, *y, *width, *height)).small());
        changed
    }

    pub(crate) fn add_window_preset(&mut self) {
        let mut suffix = 1;
        while self
            .state
            .window_presets
            .iter()
            .any(|p| p.name == format!("Window Resize {}", suffix))
        {
            suffix += 1;
        }
        let id = Self::add_window_panel_preset(
            &mut self.state.window_presets,
            &mut self.state.next_preset_id,
            |preset| preset.id,
            WindowPreset::new,
        );
        if let Some(preset) = self.state.window_presets.iter_mut().find(|p| p.id == id) {
            preset.name = format!("Window Resize {}", suffix);
        }
        self.reconcile_master_presets();
        self.sync_window_presets();
        self.status = format!("Added window preset {id}.");
    }

    pub(crate) fn add_pin_preset(&mut self) {
        let mut suffix = 1;
        while self
            .state
            .pin_presets
            .iter()
            .any(|p| p.name == format!("Pin {}", suffix))
        {
            suffix += 1;
        }
        let id = Self::add_window_panel_preset(
            &mut self.state.pin_presets,
            &mut self.state.next_pin_preset_id,
            |preset| preset.id,
            PinPreset::new,
        );
        if let Some(preset) = self.state.pin_presets.iter_mut().find(|p| p.id == id) {
            preset.name = format!("Pin {}", suffix);
        }
        self.sync_window_presets();
        self.status = format!("Added pin preset {id}.");
    }

    fn add_window_panel_preset<T, IdOf, NewPreset>(
        presets: &mut Vec<T>,
        next_id: &mut u32,
        id_of: IdOf,
        new_preset: NewPreset,
    ) -> u32
    where
        IdOf: Fn(&T) -> u32,
        NewPreset: FnOnce(u32) -> T,
    {
        let id = Self::allocate_next_id(presets, next_id, id_of);
        presets.push(new_preset(id));
        id
    }

    pub(crate) fn persist_window_presets(&mut self) {
        self.persist_after_sync(Self::sync_window_presets);
    }

    pub(crate) fn persist_window_presets_deferred(&mut self, ctx: &egui::Context) {
        self.persist_deferred_after_sync(ctx, Self::sync_window_presets);
    }

    pub(crate) fn sync_window_presets(&mut self) {
        let window_presets = self.state.window_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            window_presets,
            &mut self.last_synced_window_presets,
            OverlayCommand::UpdateWindowPresets,
        );

        let focus_presets = self.state.window_focus_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            focus_presets,
            &mut self.last_synced_window_focus_presets,
            OverlayCommand::UpdateWindowFocusPresets,
        );

        let pin_presets = self.state.pin_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            pin_presets,
            &mut self.last_synced_pin_presets,
            OverlayCommand::UpdatePinPresets,
        );

        let mouse_path_presets = self.state.mouse_path_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            mouse_path_presets,
            &mut self.last_synced_mouse_path_presets,
            OverlayCommand::UpdateMousePathPresets,
        );
    }

    pub(crate) fn persist_window_layouts(&mut self) {
        self.persist_after_sync(Self::sync_window_layouts);
    }

    pub(crate) fn persist_window_layouts_deferred(&mut self, ctx: &egui::Context) {
        self.persist_deferred_after_sync(ctx, Self::sync_window_layouts);
    }

    pub(crate) fn sync_window_layouts(&mut self) {
        let layouts = self.state.window_layouts.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            layouts,
            &mut self.last_synced_window_layouts,
            OverlayCommand::UpdateWindowLayouts,
        );
    }

    pub(crate) fn add_window_layout(&mut self) {
        let id = Self::allocate_next_id(
            &self.state.window_layouts,
            &mut self.state.next_window_layout_id,
            |layout| layout.id,
        );
        let mut new_preset = WindowLayout::new(id);
        let mut suffix = 1;
        while self
            .state
            .window_layouts
            .iter()
            .any(|l| l.name == format!("Layout {}", suffix))
        {
            suffix += 1;
        }
        new_preset.name = format!("Layout {}", suffix);
        self.state.window_layouts.push(new_preset);
        self.persist_window_layouts();
        self.status = format!("Added layout {id}.");
    }

    fn sanitize_layout(layout: &mut WindowLayout) {
        let rows = layout.rows.max(1);
        let cols = layout.cols.max(1);

        layout.rows = rows;
        layout.cols = cols;

        if layout.row_ratios.len() != rows {
            layout.row_ratios = vec![1.0; rows];
        }
        for val in &mut layout.row_ratios {
            if *val <= 0.0 {
                *val = 0.1;
            }
        }

        if layout.col_ratios.len() != cols {
            layout.col_ratios = vec![1.0; cols];
        }
        for val in &mut layout.col_ratios {
            if *val <= 0.0 {
                *val = 0.1;
            }
        }

        layout
            .cells
            .retain(|cell| cell.row < rows && cell.col < cols);
        for cell in &mut layout.cells {
            cell.row_span = cell.row_span.max(1).min(rows - cell.row);
            cell.col_span = cell.col_span.max(1).min(cols - cell.col);
        }

        layout.cells.sort_by_key(|c| (c.row, c.col));

        let mut covered = vec![vec![false; cols]; rows];
        let mut sanitized_cells = Vec::new();

        for mut cell in layout.cells.drain(..) {
            if cell.row >= rows || cell.col >= cols {
                continue;
            }
            if covered[cell.row][cell.col] {
                continue;
            }
            let mut max_row_span = rows - cell.row;
            let mut max_col_span = cols - cell.col;

            for c in cell.col..(cell.col + cell.col_span).min(cols) {
                if covered[cell.row][c] {
                    max_col_span = c - cell.col;
                    break;
                }
            }
            cell.col_span = cell.col_span.min(max_col_span).max(1);

            'outer: for r in cell.row..(cell.row + cell.row_span).min(rows) {
                for c in cell.col..(cell.col + cell.col_span) {
                    if covered[r][c] {
                        max_row_span = r - cell.row;
                        break 'outer;
                    }
                }
            }
            cell.row_span = cell.row_span.min(max_row_span).max(1);

            for r in cell.row..(cell.row + cell.row_span) {
                for c in cell.col..(cell.col + cell.col_span) {
                    covered[r][c] = true;
                }
            }
            sanitized_cells.push(cell);
        }

        layout.cells = sanitized_cells;

        // Simplify to 1x1 if there is only one cell that covers the entire grid
        if layout.cells.len() == 1 {
            let cell = &layout.cells[0];
            if cell.row == 0 && cell.col == 0 && cell.row_span == rows && cell.col_span == cols {
                if rows > 1 || cols > 1 {
                    layout.rows = 1;
                    layout.cols = 1;
                    layout.row_ratios = vec![1.0];
                    layout.col_ratios = vec![1.0];
                    layout.cells[0].row_span = 1;
                    layout.cells[0].col_span = 1;
                }
            }
        }
    }

    fn get_monitor_layout_metrics() -> MonitorLayoutMetrics {
        #[cfg(windows)]
        unsafe {
            use std::mem::size_of;
            use windows::Win32::Foundation::POINT;
            use windows::Win32::Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
            };
            use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(monitor, &mut mi).as_bool() {
                let monitor_w = (mi.rcMonitor.right - mi.rcMonitor.left) as f32;
                let monitor_h = (mi.rcMonitor.bottom - mi.rcMonitor.top) as f32;
                if monitor_w > 0.0 && monitor_h > 0.0 {
                    let work_left = (mi.rcWork.left - mi.rcMonitor.left) as f32 / monitor_w;
                    let work_top = (mi.rcWork.top - mi.rcMonitor.top) as f32 / monitor_h;
                    let work_width = (mi.rcWork.right - mi.rcWork.left) as f32 / monitor_w;
                    let work_height = (mi.rcWork.bottom - mi.rcWork.top) as f32 / monitor_h;
                    MonitorLayoutMetrics {
                        monitor_width: monitor_w,
                        monitor_height: monitor_h,
                        work_left,
                        work_top,
                        work_width,
                        work_height,
                    }
                } else {
                    MonitorLayoutMetrics {
                        monitor_width: 1920.0,
                        monitor_height: 1080.0,
                        work_left: 0.0,
                        work_top: 0.0,
                        work_width: 1.0,
                        work_height: 1.0,
                    }
                }
            } else {
                MonitorLayoutMetrics {
                    monitor_width: 1920.0,
                    monitor_height: 1080.0,
                    work_left: 0.0,
                    work_top: 0.0,
                    work_width: 1.0,
                    work_height: 1.0,
                }
            }
        }
        #[cfg(not(windows))]
        {
            MonitorLayoutMetrics {
                monitor_width: 1920.0,
                monitor_height: 1080.0,
                work_left: 0.0,
                work_top: 0.0,
                work_width: 1.0,
                work_height: 1.0,
            }
        }
    }

    fn get_monitor_work_size(block_taskbar: bool) -> (f32, f32) {
        let metrics = Self::get_monitor_layout_metrics();
        if block_taskbar {
            (
                metrics.monitor_width * metrics.work_width,
                metrics.monitor_height * metrics.work_height,
            )
        } else {
            (metrics.monitor_width, metrics.monitor_height)
        }
    }

    fn draw_hatched_rect(
        painter: &egui::Painter,
        rect: egui::Rect,
        fill_color: egui::Color32,
        stroke: egui::Stroke,
        spacing: f32,
    ) {
        painter.rect_filled(rect, 0.0, fill_color);

        let x_min = rect.min.x;
        let x_max = rect.max.x;
        let y_min = rect.min.y;
        let y_max = rect.max.y;

        let start_c = x_min + y_min;
        let end_c = x_max + y_max;

        let mut c = start_c;
        while c <= end_c {
            let x_start = x_min.max(c - y_max);
            let x_end = x_max.min(c - y_min);
            if x_start < x_end {
                painter.line_segment(
                    [
                        egui::pos2(x_start, c - x_start),
                        egui::pos2(x_end, c - x_end),
                    ],
                    stroke,
                );
            }
            c += spacing;
        }
    }

    fn split_cell_vertical(layout: &mut WindowLayout, cell_idx: usize) {
        let cell = layout.cells[cell_idx].clone();
        let col = cell.col;
        let col_span = cell.col_span;
        let ratio_sum: f32 = layout.col_ratios.iter().sum();
        let starts: Vec<f32> = std::iter::once(0.0)
            .chain(layout.col_ratios.iter().scan(0.0, |sum, ratio| {
                *sum += *ratio / ratio_sum;
                Some(*sum)
            }))
            .collect();
        let end_col = (col + col_span).min(layout.cols);
        let actual_left = starts[col] + cell.adjust_left;
        let actual_right = starts[end_col] + cell.adjust_right;
        let actual_mid = (actual_left + actual_right) * 0.5;

        if col_span > 1 {
            let half = col_span / 2;
            let split_base = starts[col + half];
            let mut cell_b = cell.clone();
            layout.cells[cell_idx].adjust_right = actual_mid - split_base;
            cell_b.adjust_left = actual_mid - split_base;
            layout.cells[cell_idx].col_span = half;
            cell_b.col = col + half;
            cell_b.col_span = col_span - half;
            layout.cells.push(cell_b);
        } else {
            let split_col = col;
            let old_bounds: Vec<(f32, f32)> = layout.cells.iter().map(|item| {
                let end = (item.col + item.col_span).min(layout.cols);
                (starts[item.col] + item.adjust_left, starts[end] + item.adjust_right)
            }).collect();
            layout.cols += 1;

            let orig_ratio = layout.col_ratios[split_col];
            layout.col_ratios[split_col] = orig_ratio / 2.0;
            layout.col_ratios.insert(split_col + 1, orig_ratio / 2.0);

            let mut cell_b = cell.clone();
            for (idx, c) in layout.cells.iter_mut().enumerate() {
                if idx == cell_idx {
                    c.col_span = 1;
                } else if c.col > split_col {
                    c.col += 1;
                } else if c.col <= split_col && c.col + c.col_span > split_col {
                    c.col_span += 1;
                }
            }
            cell_b.col = split_col + 1;
            cell_b.col_span = 1;
            let new_sum: f32 = layout.col_ratios.iter().sum();
            let new_starts: Vec<f32> = std::iter::once(0.0)
                .chain(layout.col_ratios.iter().scan(0.0, |sum, ratio| {
                    *sum += *ratio / new_sum;
                    Some(*sum)
                }))
                .collect();
            for (item, (old_left, old_right)) in layout.cells.iter_mut().zip(old_bounds) {
                let end = (item.col + item.col_span).min(layout.cols);
                item.adjust_left = old_left - new_starts[item.col];
                item.adjust_right = old_right - new_starts[end];
            }
            let new_split_base = new_starts[split_col + 1];
            layout.cells[cell_idx].adjust_right = actual_mid - new_split_base;
            cell_b.adjust_left = actual_mid - new_split_base;
            cell_b.adjust_right = actual_right - new_starts[split_col + 2];
            layout.cells.push(cell_b);
        }
    }

    fn split_cell_horizontal(layout: &mut WindowLayout, cell_idx: usize) {
        let cell = layout.cells[cell_idx].clone();
        let row = cell.row;
        let row_span = cell.row_span;
        let ratio_sum: f32 = layout.row_ratios.iter().sum();
        let starts: Vec<f32> = std::iter::once(0.0)
            .chain(layout.row_ratios.iter().scan(0.0, |sum, ratio| {
                *sum += *ratio / ratio_sum;
                Some(*sum)
            }))
            .collect();
        let end_row = (row + row_span).min(layout.rows);
        let actual_top = starts[row] + cell.adjust_top;
        let actual_bottom = starts[end_row] + cell.adjust_bottom;
        let actual_mid = (actual_top + actual_bottom) * 0.5;

        if row_span > 1 {
            let half = row_span / 2;
            let split_base = starts[row + half];
            let mut cell_b = cell.clone();
            layout.cells[cell_idx].adjust_bottom = actual_mid - split_base;
            cell_b.adjust_top = actual_mid - split_base;
            layout.cells[cell_idx].row_span = half;
            cell_b.row = row + half;
            cell_b.row_span = row_span - half;
            layout.cells.push(cell_b);
        } else {
            let split_row = row;
            let old_bounds: Vec<(f32, f32)> = layout.cells.iter().map(|item| {
                let end = (item.row + item.row_span).min(layout.rows);
                (starts[item.row] + item.adjust_top, starts[end] + item.adjust_bottom)
            }).collect();
            layout.rows += 1;

            let orig_ratio = layout.row_ratios[split_row];
            layout.row_ratios[split_row] = orig_ratio / 2.0;
            layout.row_ratios.insert(split_row + 1, orig_ratio / 2.0);

            let mut cell_b = cell.clone();
            for (idx, c) in layout.cells.iter_mut().enumerate() {
                if idx == cell_idx {
                    c.row_span = 1;
                } else if c.row > split_row {
                    c.row += 1;
                } else if c.row <= split_row && c.row + c.row_span > split_row {
                    c.row_span += 1;
                }
            }
            cell_b.row = split_row + 1;
            cell_b.row_span = 1;
            let new_sum: f32 = layout.row_ratios.iter().sum();
            let new_starts: Vec<f32> = std::iter::once(0.0)
                .chain(layout.row_ratios.iter().scan(0.0, |sum, ratio| {
                    *sum += *ratio / new_sum;
                    Some(*sum)
                }))
                .collect();
            for (item, (old_top, old_bottom)) in layout.cells.iter_mut().zip(old_bounds) {
                let end = (item.row + item.row_span).min(layout.rows);
                item.adjust_top = old_top - new_starts[item.row];
                item.adjust_bottom = old_bottom - new_starts[end];
            }
            let new_split_base = new_starts[split_row + 1];
            layout.cells[cell_idx].adjust_bottom = actual_mid - new_split_base;
            cell_b.adjust_top = actual_mid - new_split_base;
            cell_b.adjust_bottom = actual_bottom - new_starts[split_row + 2];
            layout.cells.push(cell_b);
        }
    }

    fn draw_dashed_line(
        painter: &egui::Painter,
        from: egui::Pos2,
        to: egui::Pos2,
        stroke: egui::Stroke,
        dash_length: f32,
        gap_length: f32,
    ) {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 0.0 {
            return;
        }
        let dir_x = dx / len;
        let dir_y = dy / len;

        let mut dist = 0.0;
        while dist < len {
            let chunk_end = (dist + dash_length).min(len);
            painter.line_segment(
                [
                    egui::pos2(from.x + dir_x * dist, from.y + dir_y * dist),
                    egui::pos2(from.x + dir_x * chunk_end, from.y + dir_y * chunk_end),
                ],
                stroke,
            );
            dist += dash_length + gap_length;
        }
    }

    pub(crate) fn render_layout_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;

        let mut remove_id = None;
        let mut live_sync = false;

        ui.add_space(16.0);
        ui.label(
            RichText::new(Self::tr_lang(language, "Layout Presets", "Layout Presets"))
                .strong()
                .size(14.0),
        );
        ui.add_space(4.0);

        let layouts_count = self.state.window_layouts.len();
        for index in 0..layouts_count {
            let mut next_capture_target = None;
            let mut cancel_active_capture = false;
            let mut run_layout_now = false;
            let active_capture_target = self.capture_target.clone();
            let pending_combo_keys = self.capture_hotkey_combo_keys.clone();

            let layout = &mut self.state.window_layouts[index];
            Self::sanitize_layout(layout);

            let capture_target = CaptureRequest::WindowLayoutHotkey(layout.id);
            let id_source = layout.id;

            Self::show_preset_card(ui, layout.enabled, |ui| {
                egui::Grid::new((id_source, "window-layout-header"))
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let name_width = Self::preset_header_name_width(ui);
                            let response = ui.add_sized(
                                [name_width, 21.0],
                                TextEdit::singleline(&mut layout.name),
                            );
                            Self::apply_vietnamese_input_if_changed(
                                &response,
                                self.state.vietnamese_input_enabled,
                                self.state.vietnamese_input_mode,
                                &mut layout.name,
                            );
                            live_sync |= response.changed();

                            live_sync |= Self::render_preset_trigger_chips(
                                ui,
                                language,
                                &mut layout.hotkey,
                                &mut layout.trigger_keys,
                                active_capture_target.as_ref(),
                                &capture_target,
                                pending_combo_keys.as_ref(),
                            );
                            layout.enabled =
                                layout.hotkey.is_some() || !layout.trigger_keys.trim().is_empty();
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let capture_active =
                                active_capture_target.as_ref() == Some(&capture_target);
                            let capture_time = ui.ctx().input(|input| input.time) as f32;
                            let pulse = if capture_active {
                                0.5 + 0.5 * (capture_time * 6.0).sin().abs()
                            } else {
                                0.0
                            };
                            let has_keys =
                                layout.hotkey.is_some() || !layout.trigger_keys.trim().is_empty();
                            let fill = if capture_active {
                                Color32::from_rgba_premultiplied(
                                    (88.0 + pulse * 28.0) as u8,
                                    (84.0 + pulse * 28.0) as u8,
                                    (44.0 + pulse * 10.0) as u8,
                                    255,
                                )
                            } else if has_keys {
                                Color32::from_rgba_premultiplied(72, 156, 116, 120)
                            } else {
                                ui.visuals().faint_bg_color
                            };
                            let stroke = if capture_active {
                                Color32::from_rgb(255, 232, 96)
                            } else if has_keys {
                                Color32::from_rgb(126, 224, 182)
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            };

                            let hover_text = if capture_active {
                                Self::tr_lang(
                                    language,
                                    "Capturing... Press any key.",
                                    "Capturing... Press any key.",
                                )
                                .to_string()
                            } else if has_keys {
                                let bindings_labels: Vec<String> = Self::preset_trigger_bindings(
                                    &layout.hotkey,
                                    &layout.trigger_keys,
                                )
                                .iter()
                                .map(|b| hotkey::format_binding(Some(b)))
                                .collect();
                                format!(
                                    "{} {}\n{}",
                                    Self::tr_lang(language, "Hotkey:", "Hotkey:"),
                                    bindings_labels.join(", "),
                                    Self::tr_lang(
                                        language,
                                        "Left click: rebind | Right click: clear",
                                        "Left click: rebind | Right click: clear"
                                    )
                                )
                            } else {
                                Self::tr_lang(
                                    language,
                                    "Left click: bind hotkey",
                                    "Left click: bind hotkey",
                                )
                                .to_string()
                            };

                            let btn_text = if capture_active {
                                RichText::new(Self::tr_lang(
                                    language,
                                    "Capturing...",
                                    "Capturing...",
                                ))
                                .strong()
                                .color(Color32::from_rgb(255, 232, 96))
                            } else {
                                Self::material_icon_text(0xe312, 18.0)
                            };
                            let btn_width = if capture_active { 84.0 } else { 36.0 };
                            let btn_response = ui
                                .add_sized(
                                    [btn_width, 24.0],
                                    Button::new(btn_text)
                                        .fill(fill)
                                        .stroke(egui::Stroke::new(1.0, stroke)),
                                )
                                .on_hover_text(hover_text);

                            if btn_response.clicked() {
                                if capture_active {
                                    cancel_active_capture = true;
                                } else {
                                    next_capture_target = Some((
                                        capture_target.clone(),
                                        format!("Capturing preset hotkey for {}.", layout.name),
                                    ));
                                }
                            }
                            if btn_response.secondary_clicked() {
                                layout.hotkey = None;
                                layout.trigger_keys.clear();
                                layout.enabled = false;
                                live_sync = true;
                            }

                            let run_response = Self::sound_style_icon_button(
                                ui,
                                Self::material_icon_text(0xe037, 18.0),
                            )
                            .on_hover_text(Self::tr_lang(
                                language,
                                "Run this layout preset now",
                                "Run this layout preset now",
                            ));
                            if run_response.clicked() {
                                run_layout_now = true;
                            }

                            if Self::sound_style_remove_button(ui).clicked() {
                                remove_id = Some(layout.id);
                            }

                            if Self::sound_style_toggle_button(
                                ui,
                                if layout.collapsed {
                                    Self::tr_lang(language, "Show", "Show")
                                } else {
                                    Self::tr_lang(language, "Hide", "Hide")
                                },
                            )
                            .clicked()
                            {
                                layout.collapsed = !layout.collapsed;
                                live_sync = true;
                            }
                        });
                        ui.end_row();
                    });

                if layout.collapsed {
                    return;
                }

                egui::Grid::new((id_source, "window-layout-settings-grid"))
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(Self::tr_lang(language, "Focus on apply", "Focus on apply"));
                        live_sync |= ui.checkbox(&mut layout.focus_on_apply, "").changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Block taskbar", "Block taskbar"));
                        live_sync |= ui.checkbox(&mut layout.block_taskbar, "").changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(
                            language,
                            "Remove title bar",
                            "Remove title bar",
                        ));
                        live_sync |= ui.checkbox(&mut layout.remove_title_bar, "").changed();
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Animated apply", "Chuyển động"));
                        ui.horizontal_wrapped(|ui| {
                            live_sync |= ui
                                .checkbox(
                                    &mut layout.animate_enabled,
                                    Self::tr_lang(language, "Enabled", "Bật"),
                                )
                                .changed();
                            if layout.animate_enabled {
                                ui.label(Self::tr_lang(language, "Duration", "Thời gian"));
                                live_sync |= ui
                                    .add(
                                        DragValue::new(&mut layout.animate_duration_ms)
                                            .range(60..=10_000)
                                            .suffix(" ms"),
                                    )
                                    .changed();
                            }
                        });
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Grid size", "Grid size"));
                        ui.horizontal(|ui| {
                            ui.label(Self::tr_lang(language, "Rows", "Rows"));
                            let mut rows = layout.rows;
                            if ui.add(DragValue::new(&mut rows).range(1..=6)).changed() {
                                layout.rows = rows;
                                Self::sanitize_layout(layout);
                                live_sync = true;
                            }
                            ui.label(Self::tr_lang(language, "Cols", "Cols"));
                            let mut cols = layout.cols;
                            if ui.add(DragValue::new(&mut cols).range(1..=6)).changed() {
                                layout.cols = cols;
                                Self::sanitize_layout(layout);
                                live_sync = true;
                            }
                        });
                        ui.end_row();

                        ui.label(Self::tr_lang(language, "Visual Grid", "Visual Grid"));
                        ui.vertical(|ui| {
                            let r_sum: f32 = layout.row_ratios.iter().sum();
                            let c_sum: f32 = layout.col_ratios.iter().sum();

                            let mut row_starts = vec![0.0];
                            let mut acc = 0.0f32;
                            for r in &layout.row_ratios {
                                acc += r / r_sum;
                                row_starts.push(acc);
                            }

                            let mut col_starts = vec![0.0];
                            let mut acc = 0.0f32;
                            for c in &layout.col_ratios {
                                acc += c / c_sum;
                                col_starts.push(acc);
                            }

                            let metrics = Self::get_monitor_layout_metrics();
                            let aspect = if metrics.monitor_height > 0.0 {
                                metrics.monitor_width / metrics.monitor_height
                            } else {
                                16.0 / 9.0
                            };
                            let preview_w = (ui.available_width() - 24.0).clamp(320.0, 800.0);
                            let preview_h = preview_w / aspect;

                            let (rect, _response) = ui.allocate_exact_size(
                                vec2(preview_w, preview_h),
                                egui::Sense::hover(),
                            );

                            let monitor_grid_rect = if layout.block_taskbar {
                                egui::Rect::from_min_max(
                                    egui::pos2(
                                        rect.min.x + metrics.work_left * preview_w,
                                        rect.min.y + metrics.work_top * preview_h,
                                    ),
                                    egui::pos2(
                                        rect.min.x
                                            + (metrics.work_left + metrics.work_width) * preview_w,
                                        rect.min.y
                                            + (metrics.work_top + metrics.work_height) * preview_h,
                                    ),
                                )
                            } else {
                                rect
                            };
                            let grid_rect = monitor_grid_rect;
                            let grid_w = grid_rect.width();
                            let grid_h = grid_rect.height();

                            let split_hovered_or_dragged = false;

                            ui.painter()
                                .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);

                            // Draw hatched taskbar overlay regions if block_taskbar is checked
                            if layout.block_taskbar {
                                let hatch_bg =
                                    egui::Color32::from_rgba_unmultiplied(220, 50, 50, 40);
                                let hatch_stroke = egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgba_unmultiplied(220, 50, 50, 120),
                                );
                                let text_color =
                                    egui::Color32::from_rgba_unmultiplied(220, 50, 50, 180);

                                // Top taskbar
                                if monitor_grid_rect.min.y > rect.min.y {
                                    let r_top = egui::Rect::from_min_max(
                                        rect.min,
                                        egui::pos2(rect.max.x, monitor_grid_rect.min.y),
                                    );
                                    Self::draw_hatched_rect(
                                        ui.painter(),
                                        r_top,
                                        hatch_bg,
                                        hatch_stroke,
                                        12.0,
                                    );
                                    if r_top.height() > 10.0 {
                                        ui.painter().text(
                                            r_top.center(),
                                            egui::Align2::CENTER_CENTER,
                                            Self::tr_lang(language, "TASKBAR", "TASKBAR"),
                                            egui::FontId::proportional(10.0),
                                            text_color,
                                        );
                                    }
                                }
                                // Bottom taskbar
                                if monitor_grid_rect.max.y < rect.max.y {
                                    let r_bottom = egui::Rect::from_min_max(
                                        egui::pos2(rect.min.x, monitor_grid_rect.max.y),
                                        rect.max,
                                    );
                                    Self::draw_hatched_rect(
                                        ui.painter(),
                                        r_bottom,
                                        hatch_bg,
                                        hatch_stroke,
                                        12.0,
                                    );
                                    if r_bottom.height() > 10.0 {
                                        ui.painter().text(
                                            r_bottom.center(),
                                            egui::Align2::CENTER_CENTER,
                                            Self::tr_lang(language, "TASKBAR", "TASKBAR"),
                                            egui::FontId::proportional(10.0),
                                            text_color,
                                        );
                                    }
                                }
                                // Left taskbar
                                if monitor_grid_rect.min.x > rect.min.x {
                                    let r_left = egui::Rect::from_min_max(
                                        rect.min,
                                        egui::pos2(monitor_grid_rect.min.x, rect.max.y),
                                    );
                                    Self::draw_hatched_rect(
                                        ui.painter(),
                                        r_left,
                                        hatch_bg,
                                        hatch_stroke,
                                        12.0,
                                    );
                                    if r_left.width() > 10.0 {
                                        ui.painter().text(
                                            r_left.center(),
                                            egui::Align2::CENTER_CENTER,
                                            Self::tr_lang(language, "TASKBAR", "TASKBAR"),
                                            egui::FontId::proportional(10.0),
                                            text_color,
                                        );
                                    }
                                }
                                // Right taskbar
                                if monitor_grid_rect.max.x < rect.max.x {
                                    let r_right = egui::Rect::from_min_max(
                                        egui::pos2(monitor_grid_rect.max.x, rect.min.y),
                                        rect.max,
                                    );
                                    Self::draw_hatched_rect(
                                        ui.painter(),
                                        r_right,
                                        hatch_bg,
                                        hatch_stroke,
                                        12.0,
                                    );
                                    if r_right.width() > 10.0 {
                                        ui.painter().text(
                                            r_right.center(),
                                            egui::Align2::CENTER_CENTER,
                                            Self::tr_lang(language, "TASKBAR", "TASKBAR"),
                                            egui::FontId::proportional(10.0),
                                            text_color,
                                        );
                                    }
                                }
                            }

                            let mut delete_cell = None;
                            let mut snap_preview = None;
                            let mut merge_action = None;
                            let cells_to_draw = layout.cells.clone();
                            for cell in &cells_to_draw {
                                if cell.row >= layout.rows || cell.col >= layout.cols {
                                    continue;
                                }

                                let end_row = (cell.row + cell.row_span).min(layout.rows);
                                let end_col = (cell.col + cell.col_span).min(layout.cols);

                                let x1 = grid_rect.min.x
                                    + (col_starts[cell.col] + cell.adjust_left) * grid_w;
                                let y1 = grid_rect.min.y
                                    + (row_starts[cell.row] + cell.adjust_top) * grid_h;
                                let x2 = grid_rect.min.x
                                    + (col_starts[end_col] + cell.adjust_right) * grid_w;
                                let y2 = grid_rect.min.y
                                    + (row_starts[end_row] + cell.adjust_bottom) * grid_h;

                                let raw_cell_rect = egui::Rect::from_min_max(
                                    egui::pos2(x1, y1),
                                    egui::pos2(x2, y2),
                                );
                                let cell_rect = raw_cell_rect.shrink(2.0);

                                let is_selected = self.selected_layout_cell
                                    == Some((layout.id, cell.row, cell.col));
                                let is_merge_source = self.drag_start_layout_cell
                                    == Some((layout.id, cell.row, cell.col))
                                    && ui.input(|input| input.pointer.primary_down());
                                let is_merge_target = self.drag_start_layout_cell
                                    .is_some_and(|(drag_layout, drag_row, drag_col)| {
                                        drag_layout == layout.id
                                            && (drag_row, drag_col) != (cell.row, cell.col)
                                            && ui.input(|input| input.pointer.primary_down())
                                            && ui.input(|input| input.pointer.latest_pos())
                                                .is_some_and(|pointer| cell_rect.contains(pointer))
                                    });

                                let cell_id =
                                    ui.make_persistent_id((layout.id, "cell", cell.row, cell.col));
                                let cell_index = layout.cells.iter().position(|candidate| {
                                    candidate.row == cell.row && candidate.col == cell.col
                                });
                                let mut cell_edge_active = false;
                                if !split_hovered_or_dragged {
                                    let edge_handles = [
                                        (
                                            "left",
                                            egui::Rect::from_min_max(
                                                raw_cell_rect.left_top(),
                                                egui::pos2(
                                                    raw_cell_rect.min.x + 7.0,
                                                    raw_cell_rect.max.y,
                                                ),
                                            ),
                                            egui::CursorIcon::ResizeHorizontal,
                                        ),
                                        (
                                            "right",
                                            egui::Rect::from_min_max(
                                                egui::pos2(
                                                    raw_cell_rect.max.x - 7.0,
                                                    raw_cell_rect.min.y,
                                                ),
                                                raw_cell_rect.right_bottom(),
                                            ),
                                            egui::CursorIcon::ResizeHorizontal,
                                        ),
                                        (
                                            "top",
                                            egui::Rect::from_min_max(
                                                raw_cell_rect.left_top(),
                                                egui::pos2(
                                                    raw_cell_rect.max.x,
                                                    raw_cell_rect.min.y + 7.0,
                                                ),
                                            ),
                                            egui::CursorIcon::ResizeVertical,
                                        ),
                                        (
                                            "bottom",
                                            egui::Rect::from_min_max(
                                                egui::pos2(
                                                    raw_cell_rect.min.x,
                                                    raw_cell_rect.max.y - 7.0,
                                                ),
                                                raw_cell_rect.right_bottom(),
                                            ),
                                            egui::CursorIcon::ResizeVertical,
                                        ),
                                    ];
                                    for (edge, edge_rect, cursor) in edge_handles {
                                        let edge_response = ui.interact(
                                            edge_rect,
                                            ui.make_persistent_id((
                                                layout.id,
                                                "cell_edge",
                                                cell.row,
                                                cell.col,
                                                edge,
                                            )),
                                            egui::Sense::drag(),
                                        );
                                        if edge_response.hovered() || edge_response.dragged() {
                                            ui.ctx().set_cursor_icon(cursor);
                                            cell_edge_active = true;
                                        }
                                        if edge_response.dragged()
                                            && let Some(cell_index) = cell_index
                                        {
                                            let delta = ui.input(|input| input.pointer.delta());
                                            let mut candidate = layout.cells[cell_index].clone();
                                            let min_width = 24.0 / grid_w.max(1.0);
                                            let min_height = 24.0 / grid_h.max(1.0);
                                            let old_left = col_starts[cell.col] + candidate.adjust_left;
                                            let old_right = col_starts[end_col] + candidate.adjust_right;
                                            let old_top = row_starts[cell.row] + candidate.adjust_top;
                                            let old_bottom = row_starts[end_row] + candidate.adjust_bottom;
                                            match edge {
                                                "left" => {
                                                    let max_left = col_starts[end_col]
                                                        + candidate.adjust_right
                                                        - col_starts[cell.col]
                                                        - min_width;
                                                    candidate.adjust_left = (candidate.adjust_left
                                                        + delta.x / grid_w.max(1.0))
                                                        .min(max_left);
                                                }
                                                "right" => {
                                                    let min_right = col_starts[cell.col]
                                                        + candidate.adjust_left
                                                        + min_width
                                                        - col_starts[end_col];
                                                    candidate.adjust_right = (candidate.adjust_right
                                                        + delta.x / grid_w.max(1.0))
                                                        .max(min_right);
                                                }
                                                "top" => {
                                                    let max_top = row_starts[end_row]
                                                        + candidate.adjust_bottom
                                                        - row_starts[cell.row]
                                                        - min_height;
                                                    candidate.adjust_top = (candidate.adjust_top
                                                        + delta.y / grid_h.max(1.0))
                                                        .min(max_top);
                                                }
                                                "bottom" => {
                                                    let min_bottom = row_starts[cell.row]
                                                        + candidate.adjust_top
                                                        + min_height
                                                        - row_starts[end_row];
                                                    candidate.adjust_bottom = (candidate.adjust_bottom
                                                        + delta.y / grid_h.max(1.0))
                                                        .max(min_bottom);
                                                }
                                                _ => {}
                                            }
                                            let mut left = col_starts[candidate.col] + candidate.adjust_left;
                                            let mut right = col_starts[end_col] + candidate.adjust_right;
                                            let mut top = row_starts[candidate.row] + candidate.adjust_top;
                                            let mut bottom = row_starts[end_row] + candidate.adjust_bottom;
                                            let mut snapped = false;
                                            for (index, other) in layout.cells.iter().enumerate() {
                                                if index == cell_index { continue; }
                                                let other_end_row = (other.row + other.row_span).min(layout.rows);
                                                let other_end_col = (other.col + other.col_span).min(layout.cols);
                                                let other_left = col_starts[other.col] + other.adjust_left;
                                                let other_right = col_starts[other_end_col] + other.adjust_right;
                                                let other_top = row_starts[other.row] + other.adjust_top;
                                                let other_bottom = row_starts[other_end_row] + other.adjust_bottom;
                                                let vertical_overlap = bottom > other_top && top < other_bottom;
                                                let horizontal_overlap = right > other_left && left < other_right;
                                                match edge {
                                                    "right" if delta.x > 0.0 && vertical_overlap
                                                        && old_right <= other_left
                                                        && right >= other_left - 6.0 / grid_w.max(1.0) => {
                                                        candidate.adjust_right = other_left - col_starts[end_col];
                                                        right = other_left;
                                                        snapped = true;
                                                    }
                                                    "left" if delta.x < 0.0 && vertical_overlap
                                                        && old_left >= other_right
                                                        && left <= other_right + 6.0 / grid_w.max(1.0) => {
                                                        candidate.adjust_left = other_right - col_starts[candidate.col];
                                                        left = other_right;
                                                        snapped = true;
                                                    }
                                                    "bottom" if delta.y > 0.0 && horizontal_overlap
                                                        && old_bottom <= other_top
                                                        && bottom >= other_top - 6.0 / grid_h.max(1.0) => {
                                                        candidate.adjust_bottom = other_top - row_starts[end_row];
                                                        bottom = other_top;
                                                        snapped = true;
                                                    }
                                                    "top" if delta.y < 0.0 && horizontal_overlap
                                                        && old_top >= other_bottom
                                                        && top <= other_bottom + 6.0 / grid_h.max(1.0) => {
                                                        candidate.adjust_top = other_bottom - row_starts[candidate.row];
                                                        top = other_bottom;
                                                        snapped = true;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                            if edge == "left" && delta.x < 0.0 && left * grid_w <= 6.0 {
                                                candidate.adjust_left = -col_starts[candidate.col];
                                                left = 0.0;
                                                snapped = true;
                                            } else if edge == "right" && delta.x > 0.0 && (1.0 - right) * grid_w <= 6.0 {
                                                candidate.adjust_right = 1.0 - col_starts[end_col];
                                                right = 1.0;
                                                snapped = true;
                                            } else if edge == "top" && delta.y < 0.0 && top * grid_h <= 6.0 {
                                                candidate.adjust_top = -row_starts[candidate.row];
                                                top = 0.0;
                                                snapped = true;
                                            } else if edge == "bottom" && delta.y > 0.0 && (1.0 - bottom) * grid_h <= 6.0 {
                                                candidate.adjust_bottom = 1.0 - row_starts[end_row];
                                                bottom = 1.0;
                                                snapped = true;
                                            }
                                            if !snapped {
                                                snapped = match edge {
                                                    "left" => left.abs() * grid_w <= 0.5,
                                                    "right" => (1.0 - right).abs() * grid_w <= 0.5,
                                                    "top" => top.abs() * grid_h <= 0.5,
                                                    "bottom" => (1.0 - bottom).abs() * grid_h <= 0.5,
                                                    _ => false,
                                                };
                                            }
                                            if !snapped {
                                                snapped = layout.cells.iter().enumerate().any(|(index, other)| {
                                                    if index == cell_index { return false; }
                                                    let other_end_row = (other.row + other.row_span).min(layout.rows);
                                                    let other_end_col = (other.col + other.col_span).min(layout.cols);
                                                    let other_left = col_starts[other.col] + other.adjust_left;
                                                    let other_right = col_starts[other_end_col] + other.adjust_right;
                                                    let other_top = row_starts[other.row] + other.adjust_top;
                                                    let other_bottom = row_starts[other_end_row] + other.adjust_bottom;
                                                    match edge {
                                                        "left" => bottom > other_top && top < other_bottom
                                                            && (left - other_right).abs() * grid_w <= 0.5,
                                                        "right" => bottom > other_top && top < other_bottom
                                                            && (right - other_left).abs() * grid_w <= 0.5,
                                                        "top" => right > other_left && left < other_right
                                                            && (top - other_bottom).abs() * grid_h <= 0.5,
                                                        "bottom" => right > other_left && left < other_right
                                                            && (bottom - other_top).abs() * grid_h <= 0.5,
                                                        _ => false,
                                                    }
                                                });
                                            }
                                            let overlaps = layout.cells.iter().enumerate().any(|(index, other)| {
                                                if index == cell_index { return false; }
                                                let other_end_row = (other.row + other.row_span).min(layout.rows);
                                                let other_end_col = (other.col + other.col_span).min(layout.cols);
                                                let other_left = col_starts[other.col] + other.adjust_left;
                                                let other_right = col_starts[other_end_col] + other.adjust_right;
                                                let other_top = row_starts[other.row] + other.adjust_top;
                                                let other_bottom = row_starts[other_end_row] + other.adjust_bottom;
                                                left < other_right && right > other_left
                                                    && top < other_bottom && bottom > other_top
                                            });
                                            if left >= 0.0 && top >= 0.0
                                                && right <= 1.0 && bottom <= 1.0
                                                && !overlaps
                                            {
                                                layout.cells[cell_index] = candidate;
                                                live_sync = true;
                                                if snapped {
                                                    snap_preview = Some(egui::Rect::from_min_max(
                                                        egui::pos2(grid_rect.min.x + left * grid_w, grid_rect.min.y + top * grid_h),
                                                        egui::pos2(grid_rect.min.x + right * grid_w, grid_rect.min.y + bottom * grid_h),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                                let cell_sense = if split_hovered_or_dragged || cell_edge_active {
                                    egui::Sense::hover()
                                } else {
                                    egui::Sense::click_and_drag()
                                };
                                let cell_resp = ui.interact(cell_rect, cell_id, cell_sense);
                                let delete_rect = egui::Rect::from_min_size(
                                    egui::pos2(cell_rect.right() - 24.0, cell_rect.top() + 4.0),
                                    egui::vec2(20.0, 20.0),
                                );
                                let delete_enabled = layout.cells.len() > 1;
                                let delete_icon = Self::material_icon_text(0xe872, 14.0);
                                let delete_response = ui.put(
                                    delete_rect,
                                    egui::Button::new(if delete_enabled {
                                        delete_icon
                                    } else {
                                        delete_icon.color(ui.visuals().weak_text_color())
                                    }),
                                ).on_hover_text(if delete_enabled {
                                    Self::tr_lang(language, "Delete", "Xóa")
                                } else {
                                    Self::tr_lang(
                                        language,
                                        "The last window cannot be deleted",
                                        "Không thể xóa cửa sổ cuối cùng",
                                    )
                                });
                                let controls_active = delete_response.hovered()
                                    || delete_response.clicked();
                                if delete_enabled && delete_response.clicked() {
                                    delete_cell = cell_index;
                                }

                                if cell_resp.drag_started() && !controls_active {
                                    self.drag_start_layout_cell =
                                        Some((layout.id, cell.row, cell.col));
                                }

                                if cell_resp.dragged()
                                    && ui.input(|input| input.modifiers.alt)
                                    && !controls_active
                                    && let Some(cell_index) = cell_index
                                {
                                    let delta = ui.input(|input| input.pointer.delta());
                                    let dx = delta.x / grid_w.max(1.0);
                                    let dy = delta.y / grid_h.max(1.0);
                                    let mut candidate = layout.cells[cell_index].clone();
                                    let end_row = (candidate.row + candidate.row_span).min(layout.rows);
                                    let end_col = (candidate.col + candidate.col_span).min(layout.cols);
                                    let old_left = col_starts[candidate.col] + candidate.adjust_left;
                                    let old_right = col_starts[end_col] + candidate.adjust_right;
                                    let old_top = row_starts[candidate.row] + candidate.adjust_top;
                                    let old_bottom = row_starts[end_row] + candidate.adjust_bottom;
                                    let min_dx = -(col_starts[candidate.col] + candidate.adjust_left);
                                    let max_dx = 1.0 - (col_starts[end_col] + candidate.adjust_right);
                                    let min_dy = -(row_starts[candidate.row] + candidate.adjust_top);
                                    let max_dy = 1.0 - (row_starts[end_row] + candidate.adjust_bottom);
                                    let dx = dx.clamp(min_dx, max_dx);
                                    let dy = dy.clamp(min_dy, max_dy);
                                    candidate.adjust_left += dx;
                                    candidate.adjust_right += dx;
                                    candidate.adjust_top += dy;
                                    candidate.adjust_bottom += dy;
                                    let mut left = col_starts[candidate.col] + candidate.adjust_left;
                                    let mut right = col_starts[end_col] + candidate.adjust_right;
                                    let mut top = row_starts[candidate.row] + candidate.adjust_top;
                                    let mut bottom = row_starts[end_row] + candidate.adjust_bottom;
                                    let mut live_snap_x = 0.0;
                                    let mut live_snap_y = 0.0;
                                    for (index, other) in layout.cells.iter().enumerate() {
                                        if index == cell_index { continue; }
                                        let other_end_row = (other.row + other.row_span).min(layout.rows);
                                        let other_end_col = (other.col + other.col_span).min(layout.cols);
                                        let other_left = col_starts[other.col] + other.adjust_left;
                                        let other_right = col_starts[other_end_col] + other.adjust_right;
                                        let other_top = row_starts[other.row] + other.adjust_top;
                                        let other_bottom = row_starts[other_end_row] + other.adjust_bottom;
                                        if bottom > other_top && top < other_bottom {
                                            if dx > 0.0 && old_right <= other_left
                                                && right >= other_left - 6.0 / grid_w.max(1.0)
                                            {
                                                live_snap_x = other_left - right;
                                            } else if dx < 0.0 && old_left >= other_right
                                                && left <= other_right + 6.0 / grid_w.max(1.0)
                                            {
                                                live_snap_x = other_right - left;
                                            }
                                        }
                                        if right > other_left && left < other_right {
                                            if dy > 0.0 && old_bottom <= other_top
                                                && bottom >= other_top - 6.0 / grid_h.max(1.0)
                                            {
                                                live_snap_y = other_top - bottom;
                                            } else if dy < 0.0 && old_top >= other_bottom
                                                && top <= other_bottom + 6.0 / grid_h.max(1.0)
                                            {
                                                live_snap_y = other_bottom - top;
                                            }
                                        }
                                    }
                                    if dx < 0.0 && left * grid_w <= 6.0 {
                                        live_snap_x = -left;
                                    } else if dx > 0.0 && (1.0 - right) * grid_w <= 6.0 {
                                        live_snap_x = 1.0 - right;
                                    }
                                    if dy < 0.0 && top * grid_h <= 6.0 {
                                        live_snap_y = -top;
                                    } else if dy > 0.0 && (1.0 - bottom) * grid_h <= 6.0 {
                                        live_snap_y = 1.0 - bottom;
                                    }
                                    candidate.adjust_left += live_snap_x;
                                    candidate.adjust_right += live_snap_x;
                                    candidate.adjust_top += live_snap_y;
                                    candidate.adjust_bottom += live_snap_y;
                                    left += live_snap_x;
                                    right += live_snap_x;
                                    top += live_snap_y;
                                    bottom += live_snap_y;
                                    let overlaps = layout.cells.iter().enumerate().any(|(index, other)| {
                                        if index == cell_index { return false; }
                                        let other_end_row = (other.row + other.row_span).min(layout.rows);
                                        let other_end_col = (other.col + other.col_span).min(layout.cols);
                                        let other_left = col_starts[other.col] + other.adjust_left;
                                        let other_right = col_starts[other_end_col] + other.adjust_right;
                                        let other_top = row_starts[other.row] + other.adjust_top;
                                        let other_bottom = row_starts[other_end_row] + other.adjust_bottom;
                                        left < other_right && right > other_left
                                            && top < other_bottom && bottom > other_top
                                    });
                                    if !overlaps {
                                        layout.cells[cell_index] = candidate;
                                    }
                                    let snap_near_screen = left * grid_w <= 6.0
                                        || (1.0 - right) * grid_w <= 6.0
                                        || top * grid_h <= 6.0
                                        || (1.0 - bottom) * grid_h <= 6.0;
                                    let snap_near_window = layout.cells.iter().enumerate().any(|(index, other)| {
                                        if index == cell_index { return false; }
                                        let other_end_row = (other.row + other.row_span).min(layout.rows);
                                        let other_end_col = (other.col + other.col_span).min(layout.cols);
                                        let other_left = col_starts[other.col] + other.adjust_left;
                                        let other_right = col_starts[other_end_col] + other.adjust_right;
                                        let other_top = row_starts[other.row] + other.adjust_top;
                                        let other_bottom = row_starts[other_end_row] + other.adjust_bottom;
                                        let vertical_overlap = bottom > other_top && top < other_bottom;
                                        let horizontal_overlap = right > other_left && left < other_right;
                                        (vertical_overlap
                                            && ((left - other_right).abs() * grid_w <= 6.0
                                                || (right - other_left).abs() * grid_w <= 6.0))
                                            || (horizontal_overlap
                                                && ((top - other_bottom).abs() * grid_h <= 6.0
                                                    || (bottom - other_top).abs() * grid_h <= 6.0))
                                    });
                                    if live_snap_x != 0.0 || live_snap_y != 0.0
                                        || snap_near_screen || snap_near_window
                                    {
                                        let preview = egui::Rect::from_min_max(
                                            egui::pos2(
                                                grid_rect.min.x + left * grid_w,
                                                grid_rect.min.y + top * grid_h,
                                            ),
                                            egui::pos2(
                                                grid_rect.min.x + right * grid_w,
                                                grid_rect.min.y + bottom * grid_h,
                                            ),
                                        );
                                        snap_preview = Some(preview);
                                    }
                                    live_sync = true;
                                }

                                if cell_resp.drag_stopped()
                                    && let Some(cell_index) = cell_index
                                {
                                    let target = layout.cells[cell_index].clone();
                                    let target_end_row =
                                        (target.row + target.row_span).min(layout.rows);
                                    let target_end_col =
                                        (target.col + target.col_span).min(layout.cols);
                                    let target_rect = egui::Rect::from_min_max(
                                        egui::pos2(
                                            grid_rect.min.x
                                                + (col_starts[target.col]
                                                    + target.adjust_left)
                                                    * grid_w,
                                            grid_rect.min.y
                                                + (row_starts[target.row] + target.adjust_top)
                                                    * grid_h,
                                        ),
                                        egui::pos2(
                                            grid_rect.min.x
                                                + (col_starts[target_end_col]
                                                    + target.adjust_right)
                                                    * grid_w,
                                            grid_rect.min.y
                                                + (row_starts[target_end_row]
                                                    + target.adjust_bottom)
                                                    * grid_h,
                                        ),
                                    );
                                    let mut snap_x = [
                                        grid_rect.min.x - target_rect.min.x,
                                        grid_rect.max.x - target_rect.max.x,
                                    ]
                                    .into_iter()
                                    .min_by(|a, b| a.abs().total_cmp(&b.abs()));
                                    let mut snap_y = [
                                        grid_rect.min.y - target_rect.min.y,
                                        grid_rect.max.y - target_rect.max.y,
                                    ]
                                    .into_iter()
                                    .min_by(|a, b| a.abs().total_cmp(&b.abs()));
                                    for (other_index, other) in layout.cells.iter().enumerate() {
                                        if other_index == cell_index {
                                            continue;
                                        }
                                        let other_end_row =
                                            (other.row + other.row_span).min(layout.rows);
                                        let other_end_col =
                                            (other.col + other.col_span).min(layout.cols);
                                        let other_rect = egui::Rect::from_min_max(
                                            egui::pos2(
                                                grid_rect.min.x
                                                    + (col_starts[other.col]
                                                        + other.adjust_left)
                                                        * grid_w,
                                                grid_rect.min.y
                                                    + (row_starts[other.row] + other.adjust_top)
                                                        * grid_h,
                                            ),
                                            egui::pos2(
                                                grid_rect.min.x
                                                    + (col_starts[other_end_col]
                                                        + other.adjust_right)
                                                        * grid_w,
                                                grid_rect.min.y
                                                    + (row_starts[other_end_row]
                                                        + other.adjust_bottom)
                                                        * grid_h,
                                            ),
                                        );
                                        if target_rect.max.y > other_rect.min.y
                                            && target_rect.min.y < other_rect.max.y
                                        {
                                            for candidate in [
                                                other_rect.min.x - target_rect.max.x,
                                                other_rect.max.x - target_rect.min.x,
                                            ] {
                                                if snap_x.is_none_or(|best| {
                                                    candidate.abs() < best.abs()
                                                }) {
                                                    snap_x = Some(candidate);
                                                }
                                            }
                                        }
                                        if target_rect.max.x > other_rect.min.x
                                            && target_rect.min.x < other_rect.max.x
                                        {
                                            for candidate in [
                                                other_rect.min.y - target_rect.max.y,
                                                other_rect.max.y - target_rect.min.y,
                                            ] {
                                                if snap_y.is_none_or(|best| {
                                                    candidate.abs() < best.abs()
                                                }) {
                                                    snap_y = Some(candidate);
                                                }
                                            }
                                        }
                                    }
                                    let correction_x =
                                        snap_x.filter(|delta| delta.abs() < 0.0).unwrap_or(0.0)
                                            / grid_w.max(1.0);
                                    let correction_y =
                                        snap_y.filter(|delta| delta.abs() < 0.0).unwrap_or(0.0)
                                            / grid_h.max(1.0);
                                    if correction_x != 0.0 || correction_y != 0.0 {
                                        let target = &mut layout.cells[cell_index];
                                        target.adjust_left += correction_x;
                                        target.adjust_right += correction_x;
                                        target.adjust_top += correction_y;
                                        target.adjust_bottom += correction_y;
                                        for value in [
                                            &mut target.adjust_left,
                                            &mut target.adjust_right,
                                            &mut target.adjust_top,
                                            &mut target.adjust_bottom,
                                        ] {
                                            if value.abs() < 0.0005 {
                                                *value = 0.0;
                                            }
                                        }
                                    }
                                    if let Some((start_layout, start_row, start_col)) =
                                        self.drag_start_layout_cell.take()
                                    {
                                        if start_layout == layout.id {
                                            let pointer = ui.input(|input| input.pointer.latest_pos());
                                            if let Some(pointer) = pointer {
                                                for other in &layout.cells {
                                                    if (other.row, other.col) == (start_row, start_col) {
                                                        continue;
                                                    }
                                                    let other_end_row = (other.row + other.row_span).min(layout.rows);
                                                    let other_end_col = (other.col + other.col_span).min(layout.cols);
                                                    let other_rect = egui::Rect::from_min_max(
                                                        egui::pos2(
                                                            grid_rect.min.x + (col_starts[other.col] + other.adjust_left) * grid_w,
                                                            grid_rect.min.y + (row_starts[other.row] + other.adjust_top) * grid_h,
                                                        ),
                                                        egui::pos2(
                                                            grid_rect.min.x + (col_starts[other_end_col] + other.adjust_right) * grid_w,
                                                            grid_rect.min.y + (row_starts[other_end_row] + other.adjust_bottom) * grid_h,
                                                        ),
                                                    );
                                                    if other_rect.contains(pointer) {
                                                        merge_action = Some((start_row, start_col, other.row, other.col));
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                if cell_resp.clicked() && !controls_active {
                                    if self.selected_layout_cell
                                        == Some((layout.id, cell.row, cell.col))
                                    {
                                        self.selected_layout_cell = None;
                                    } else {
                                        self.selected_layout_cell =
                                            Some((layout.id, cell.row, cell.col));
                                    }
                                }

                                cell_resp.surrender_focus();

                                let fill_color = if is_selected {
                                    Color32::from_rgba_premultiplied(0, 120, 215, 80)
                                } else if is_merge_target {
                                    Color32::from_rgba_premultiplied(0, 180, 255, 100)
                                } else if is_merge_source {
                                    Color32::from_rgba_premultiplied(0, 140, 230, 70)
                                } else if cell_resp.hovered() {
                                    Color32::from_rgba_premultiplied(128, 128, 128, 40)
                                } else {
                                    Color32::from_rgba_premultiplied(128, 128, 128, 20)
                                };

                                let border_color = if is_selected {
                                    Color32::from_rgb(0, 120, 215)
                                } else if is_merge_target {
                                    Color32::from_rgb(0, 210, 255)
                                } else if is_merge_source {
                                    Color32::from_rgb(0, 160, 240)
                                } else {
                                    ui.visuals().widgets.noninteractive.bg_stroke.color
                                };

                                let stroke_width = if is_selected || is_merge_target || is_merge_source {
                                    2.0
                                } else {
                                    1.0
                                };

                                ui.painter().rect(
                                    cell_rect,
                                    2.0,
                                    fill_color,
                                    egui::Stroke::new(stroke_width, border_color),
                                    egui::StrokeKind::Outside,
                                );

                                if !split_hovered_or_dragged && cell_resp.hovered() {
                                    if let Some(pointer_pos) =
                                        ui.ctx().input(|i| i.pointer.hover_pos())
                                    {
                                        let norm_dx = (pointer_pos.x - cell_rect.center().x)
                                            / cell_rect.width();
                                        let norm_dy = (pointer_pos.y - cell_rect.center().y)
                                            / cell_rect.height();

                                        if norm_dx.abs() > norm_dy.abs() {
                                            Self::draw_dashed_line(
                                                ui.painter(),
                                                egui::pos2(cell_rect.center().x, cell_rect.min.y),
                                                egui::pos2(cell_rect.center().x, cell_rect.max.y),
                                                egui::Stroke::new(
                                                    2.5,
                                                    Color32::from_rgb(255, 69, 0),
                                                ),
                                                8.0,
                                                4.0,
                                            );

                                            if cell_resp.secondary_clicked() {
                                                if let Some(cell_idx) =
                                                    layout.cells.iter().position(|c| {
                                                        c.row == cell.row && c.col == cell.col
                                                    })
                                                {
                                                    Self::split_cell_vertical(layout, cell_idx);
                                                    Self::sanitize_layout(layout);
                                                    self.selected_layout_cell = None;
                                                    live_sync = true;
                                                }
                                            }
                                        } else {
                                            Self::draw_dashed_line(
                                                ui.painter(),
                                                egui::pos2(cell_rect.min.x, cell_rect.center().y),
                                                egui::pos2(cell_rect.max.x, cell_rect.center().y),
                                                egui::Stroke::new(
                                                    2.5,
                                                    Color32::from_rgb(255, 69, 0),
                                                ),
                                                8.0,
                                                4.0,
                                            );

                                            if cell_resp.secondary_clicked() {
                                                if let Some(cell_idx) =
                                                    layout.cells.iter().position(|c| {
                                                        c.row == cell.row && c.col == cell.col
                                                    })
                                                {
                                                    Self::split_cell_horizontal(layout, cell_idx);
                                                    Self::sanitize_layout(layout);
                                                    self.selected_layout_cell = None;
                                                    live_sync = true;
                                                }
                                            }
                                        }
                                    }
                                }

                                let label_text = if let Some(title) = &cell.target_window_title {
                                    let simplified = Self::simplify_window_title(title);
                                    Self::truncate_window_title(&simplified, 16)
                                } else {
                                    format!("{},{}", cell.row, cell.col)
                                };

                                let text_color = if is_selected {
                                    ui.visuals().widgets.active.text_color()
                                } else {
                                    ui.visuals().widgets.noninteractive.text_color()
                                };

                                ui.painter().text(
                                    cell_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    label_text,
                                    egui::FontId::proportional(11.0),
                                    text_color,
                                );
                            }

                            if let Some(preview) = snap_preview {
                                ui.painter().rect_stroke(
                                    preview,
                                    2.0,
                                    egui::Stroke::new(2.0, Color32::from_rgb(0, 180, 255)),
                                    egui::StrokeKind::Outside,
                                );
                            }

                            if let Some(cell_index) = delete_cell {
                                layout.cells.remove(cell_index);
                                self.selected_layout_cell = None;
                                live_sync = true;
                            }

                            if let Some((start_row, start_col, end_row, end_col)) = merge_action {
                                let cell_a = layout.cells.iter()
                                    .find(|cell| cell.row == start_row && cell.col == start_col)
                                    .cloned();
                                let cell_b = layout.cells.iter()
                                    .find(|cell| cell.row == end_row && cell.col == end_col)
                                    .cloned();
                                if let (Some(cell_a), Some(cell_b)) = (cell_a, cell_b) {
                                    let r1 = cell_a.row.min(cell_b.row);
                                    let c1 = cell_a.col.min(cell_b.col);
                                    let r2 = (cell_a.row + cell_a.row_span - 1)
                                        .max(cell_b.row + cell_b.row_span - 1);
                                    let c2 = (cell_a.col + cell_a.col_span - 1)
                                        .max(cell_b.col + cell_b.col_span - 1);
                                    layout.cells.insert(0, WindowLayoutCell {
                                        row: r1,
                                        col: c1,
                                        row_span: r2 - r1 + 1,
                                        col_span: c2 - c1 + 1,
                                        target_window_title: cell_a.target_window_title.clone()
                                            .or(cell_b.target_window_title.clone()),
                                        extra_target_window_titles: if cell_a.extra_target_window_titles.is_empty() {
                                            cell_b.extra_target_window_titles.clone()
                                        } else {
                                            cell_a.extra_target_window_titles.clone()
                                        },
                                        match_duplicate_window_titles: cell_a.match_duplicate_window_titles
                                            || cell_b.match_duplicate_window_titles,
                                        adjust_left: 0.0,
                                        adjust_right: 0.0,
                                        adjust_top: 0.0,
                                        adjust_bottom: 0.0,
                                    });
                                    Self::sanitize_layout(layout);
                                    self.selected_layout_cell = Some((layout.id, r1, c1));
                                    live_sync = true;
                                }
                            }

                        });
                        ui.end_row();
                    });

                let last_sel_id = ui.make_persistent_id("last_selected_layout_cell");
                let last_selected: Option<(u32, usize, usize)> = ui
                    .data_mut(|d| d.get_temp::<Option<(u32, usize, usize)>>(last_sel_id))
                    .flatten();
                let current_selected = self.selected_layout_cell;
                let selection_changed = current_selected != last_selected;
                if selection_changed {
                    ui.data_mut(|d| d.insert_temp(last_sel_id, current_selected));
                }

                if let Some((sel_layout_id, sel_row, sel_col)) = self.selected_layout_cell {
                    if sel_layout_id == layout.id {
                        if let Some(cell_idx) = layout
                            .cells
                            .iter()
                            .position(|c| c.row == sel_row && c.col == sel_col)
                        {
                            ui.add_space(8.0);
                            let header_resp = ui
                                .horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Cell ({}, {}) Settings",
                                            sel_row, sel_col
                                        ))
                                        .strong(),
                                    );
                                })
                                .response;

                            if selection_changed {
                                header_resp.scroll_to_me(Some(egui::Align::Center));
                            }

                            let mut cell_modified = false;

                            let mut row_span = layout.cells[cell_idx].row_span;
                            let mut col_span = layout.cells[cell_idx].col_span;
                            let mut target_window_title =
                                layout.cells[cell_idx].target_window_title.clone();
                            let mut extra_target_window_titles =
                                layout.cells[cell_idx].extra_target_window_titles.clone();
                            let mut match_duplicate_window_titles =
                                layout.cells[cell_idx].match_duplicate_window_titles;

                            egui::Grid::new((layout.id, "cell-settings-grid", sel_row, sel_col))
                                .num_columns(2)
                                .spacing([14.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label(Self::tr_lang(language, "Span", "Span"));
                                    ui.horizontal(|ui| {
                                        ui.label(Self::tr_lang(language, "Row span", "Row span"));
                                        let max_row_span = layout.rows - sel_row;
                                        if ui
                                            .add(
                                                DragValue::new(&mut row_span)
                                                    .range(1..=max_row_span),
                                            )
                                            .changed()
                                        {
                                            cell_modified = true;
                                        }
                                        ui.label(Self::tr_lang(language, "Col span", "Col span"));
                                        let max_col_span = layout.cols - sel_col;
                                        if ui
                                            .add(
                                                DragValue::new(&mut col_span)
                                                    .range(1..=max_col_span),
                                            )
                                            .changed()
                                        {
                                            cell_modified = true;
                                        }
                                    });
                                    ui.end_row();

                                    ui.label(Self::tr_lang(
                                        language,
                                        "Target Window",
                                        "Target Window",
                                    ));
                                    let dropdown_changed =
                                        Self::render_multi_window_targets_with_duplicate_mode(
                                            ui,
                                            language,
                                            (layout.id, "cell-target-picker", sel_row, sel_col),
                                            Self::tr_lang(language, "Focus", "Focus"),
                                            &mut target_window_title,
                                            &mut extra_target_window_titles,
                                            &mut match_duplicate_window_titles,
                                            &self.open_window_infos,
                                        );
                                    if dropdown_changed {
                                        cell_modified = true;
                                    }
                                    ui.end_row();

                                    // Render cell estimated resolution info
                                    ui.label(Self::tr_lang(language, "Resolution", "Resolution"));
                                    let (mon_w, mon_h) =
                                        Self::get_monitor_work_size(layout.block_taskbar);

                                    let r_sum: f32 = layout.row_ratios.iter().sum();
                                    let c_sum: f32 = layout.col_ratios.iter().sum();
                                    let mut row_starts = vec![0.0];
                                    let mut acc = 0.0f32;
                                    for r in &layout.row_ratios {
                                        acc += r / r_sum;
                                        row_starts.push(acc);
                                    }
                                    let mut col_starts = vec![0.0];
                                    let mut acc = 0.0f32;
                                    for c in &layout.col_ratios {
                                        acc += c / c_sum;
                                        col_starts.push(acc);
                                    }

                                    let cell_w_frac = col_starts[col_starts
                                        .len()
                                        .min(sel_col + layout.cells[cell_idx].col_span)]
                                        - col_starts[sel_col]
                                        + layout.cells[cell_idx].adjust_right
                                        - layout.cells[cell_idx].adjust_left;
                                    let cell_h_frac = row_starts[row_starts
                                        .len()
                                        .min(sel_row + layout.cells[cell_idx].row_span)]
                                        - row_starts[sel_row]
                                        + layout.cells[cell_idx].adjust_bottom
                                        - layout.cells[cell_idx].adjust_top;
                                    let cell_w = (cell_w_frac * mon_w).round() as i32;
                                    let cell_h = (cell_h_frac * mon_h).round() as i32;
                                    let info_text = format!(
                                        "{} x {} ({:.0}% x {:.0}%)",
                                        cell_w,
                                        cell_h,
                                        cell_w_frac * 100.0,
                                        cell_h_frac * 100.0
                                    );
                                    ui.label(RichText::new(info_text).strong());
                                    ui.end_row();
                                });

                            if cell_modified {
                                layout.cells[cell_idx].row_span = row_span;
                                layout.cells[cell_idx].col_span = col_span;
                                layout.cells[cell_idx].target_window_title = target_window_title;
                                layout.cells[cell_idx].extra_target_window_titles =
                                    extra_target_window_titles;
                                layout.cells[cell_idx].match_duplicate_window_titles =
                                    match_duplicate_window_titles;

                                Self::sanitize_layout(layout);
                                live_sync = true;
                            }
                        }
                    }
                }
            });

            if let Some((target, status)) = next_capture_target.take() {
                self.begin_capture(target, status);
            }
            if cancel_active_capture {
                self.cancel_capture();
            }
            if run_layout_now {
                let layout = self.state.window_layouts[index].clone();
                let layout_name = layout.name.clone();
                let _ = self
                    .overlay_tx
                    .send(OverlayCommand::ApplyWindowLayout(layout));
                self.status = format!("Applied layout preset {}.", layout_name);
            }
        }

        if live_sync {
            self.persist_window_layouts_deferred(ui.ctx());
        }
        if let Some(id) = remove_id {
            self.state.window_layouts.retain(|l| l.id != id);
            self.persist_window_layouts();
            if let Some((sel_layout_id, _, _)) = self.selected_layout_cell {
                if sel_layout_id == id {
                    self.selected_layout_cell = None;
                }
            }
        }
    }

    pub(crate) fn sync_hud_preview_presets(&mut self, presets: Vec<HudPreset>) {
        self.active_hud_preview_preset_id = presets.first().map(|preset| preset.id);
        let _ = self
            .overlay_tx
            .send(OverlayCommand::PreviewHudPreset(presets));
    }

    pub(crate) fn sync_hud_preview(&mut self, preset: Option<&HudPreset>) {
        let next_id = preset.map(|preset| preset.id);
        if self.active_hud_preview_preset_id == next_id {
            if let Some(preset) = preset {
                self.sync_hud_preview_presets(vec![preset.clone()]);
            }
            return;
        }
        self.sync_hud_preview_presets(preset.cloned().into_iter().collect());
    }

    pub(crate) fn clear_hud_preview(&mut self) {
        if self.active_hud_preview_preset_id.take().is_some() {
            self.sync_hud_preview_presets(Vec::new());
        }
    }

    pub(crate) fn clear_macro_visual_overlays(&mut self) {
        self.geometry_preview_target = None;
        self.geometry_preview_sent = None;
        self.draw_geometry_step_preview_target = None;
        self.draw_geometry_step_preview_sent = None;
        self.show_geometry_preset_preview_target = None;
        self.clear_geometry_spec_preview();
        self.clear_geometry_preset_preview();

        self.disable_hud_preview_modes();
        crate::overlay::hide_hud_now();

        self.disable_timer_preview_modes();
        crate::overlay::clear_timer_overlays_now();

        self.disable_pin_preview_modes();
        crate::overlay::clear_pin_overlay_now();

        crate::overlay::clear_geometry_overlay_now();
    }

    pub(crate) fn disable_pin_preview_modes(&mut self) -> bool {
        let mut changed = false;
        for preset in &mut self.state.pin_presets {
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
                self.zoom_preview_cache.remove(&(100_000 + preset.id));
            }
        }
        changed
    }

    pub(crate) fn disable_hud_preview_modes(&mut self) -> bool {
        let mut changed = false;
        for preset in &mut self.state.hud_presets {
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }
        if changed {
            self.clear_hud_preview();
        }
        changed
    }

    pub(crate) fn disable_window_presets_preview_modes(&mut self) -> bool {
        let mut changed = false;
        for preset in &mut self.state.window_presets {
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
                self.zoom_preview_cache.remove(&(200_000 + preset.id));
            }
        }
        changed
    }

    pub(crate) fn window_preview_for_target(
        &mut self,
        _ctx: &egui::Context,
        cache_id: u32,
        target_window_title: Option<&String>,
        extra_target_window_titles: &[String],
        match_duplicate_window_titles: bool,
    ) -> Option<ZoomPreviewView> {
        let refresh_every = Duration::from_millis(120);
        if let Some(cache) = self.zoom_preview_cache.get(&cache_id)
            && cache.source_window_key == target_window_title.cloned()
            && cache.source_window_extra_keys == extra_target_window_titles
            && cache.match_duplicate_window_titles == match_duplicate_window_titles
            && cache.updated_at.elapsed() < refresh_every
        {
            return Some(cache.view.clone());
        }

        let should_request = if let Some(last_req) = self.window_preview_requested.get(&cache_id) {
            last_req.elapsed() >= refresh_every
        } else {
            true
        };

        if should_request && !self.window_preview_loading.contains(&cache_id) {
            self.window_preview_requested
                .insert(cache_id, Instant::now());
            self.window_preview_loading.insert(cache_id);

            let ui_tx = self.ui_tx.clone();
            let target_title = target_window_title.cloned();
            let extra_titles = extra_target_window_titles.to_vec();

            std::thread::spawn(move || {
                let frame = crate::window_list::capture_window_preview_with_candidates(
                    target_title.as_deref(),
                    &extra_titles,
                    match_duplicate_window_titles,
                    720,
                );
                let _ = ui_tx.send(crate::overlay::UiCommand::WindowPreviewLoaded {
                    cache_id,
                    source_window_key: target_title,
                    source_window_extra_keys: extra_titles,
                    match_duplicate_window_titles,
                    frame,
                });
            });
        }

        self.zoom_preview_cache
            .get(&cache_id)
            .map(|cache| cache.view.clone())
    }

    pub(crate) fn pin_preview_for_target(
        &mut self,
        _ctx: &egui::Context,
        cache_id: u32,
        target_window_title: Option<&String>,
        extra_target_window_titles: &[String],
        match_duplicate_window_titles: bool,
    ) -> Option<ZoomPreviewView> {
        let refresh_every = Duration::from_millis(120);
        if let Some(cache) = self.zoom_preview_cache.get(&cache_id)
            && cache.source_window_key == target_window_title.cloned()
            && cache.source_window_extra_keys == extra_target_window_titles
            && cache.match_duplicate_window_titles == match_duplicate_window_titles
            && cache.updated_at.elapsed() < refresh_every
        {
            return Some(cache.view.clone());
        }

        let should_request = if let Some(last_req) = self.window_preview_requested.get(&cache_id) {
            last_req.elapsed() >= refresh_every
        } else {
            true
        };

        if should_request && !self.window_preview_loading.contains(&cache_id) {
            self.window_preview_requested
                .insert(cache_id, Instant::now());
            self.window_preview_loading.insert(cache_id);

            let ui_tx = self.ui_tx.clone();
            let target_title = target_window_title.cloned();
            let extra_titles = extra_target_window_titles.to_vec();

            std::thread::spawn(move || {
                let frame = crate::window_list::capture_window_client_preview_with_candidates(
                    target_title.as_deref(),
                    &extra_titles,
                    match_duplicate_window_titles,
                    720,
                );
                let _ = ui_tx.send(crate::overlay::UiCommand::WindowPreviewLoaded {
                    cache_id,
                    source_window_key: target_title,
                    source_window_extra_keys: extra_titles,
                    match_duplicate_window_titles,
                    frame,
                });
            });
        }

        self.zoom_preview_cache
            .get(&cache_id)
            .map(|cache| cache.view.clone())
    }

    pub(crate) fn clear_pin_preview_cache(&mut self) {
        for preset in &self.state.pin_presets {
            self.zoom_preview_cache.remove(&(100_000 + preset.id));
        }
    }
}
