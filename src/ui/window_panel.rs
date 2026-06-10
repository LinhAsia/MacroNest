use crate::hotkey;
use crate::model::*;
use crate::overlay::OverlayCommand;
use crate::ui::{CrosshairApp, ZoomPreviewCache, ZoomPreviewView};
use crate::window_list;
use eframe::egui::{
    self, Button, Color32, ColorImage, DragValue, Frame, RichText, Sense, TextBuffer, TextEdit,
    TextureOptions, vec2,
};
use std::time::{Duration, Instant};

impl CrosshairApp {
    pub(crate) fn render_window_presets_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        let language = self.state.ui_language;

        ui.horizontal(|ui| {
            if ui
                .button(self.tr("+ Add resize preset", "+ Thêm preset kích thước"))
                .clicked()
            {
                self.add_window_preset();
                self.persist();
            }
            if ui
                .button(self.tr("+ Add layout preset", "+ Thêm preset bố cục"))
                .clicked()
            {
                self.add_window_layout();
            }
        });

        ui.add_space(16.0);

        let mut remove_id = None;
        let mut live_sync = false;
        ui.label(
            RichText::new(Self::tr_lang(language, "Resize Presets", "Kích thước"))
                .strong()
                .size(14.0),
        );
        ui.add_space(4.0);
        for index in 0..self.state.window_presets.len() {
            let mut next_capture_target = None;
            let mut cancel_active_capture = false;
            let active_capture_target = self.capture_target.clone();
            let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
            ui.add_space(6.0);
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
                                            "Đang ghi... Nhấn một phím bất kỳ.",
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
                                            Self::tr_lang(language, "Hotkey:", "Phím tắt:"),
                                            bindings_labels.join(", "),
                                            Self::tr_lang(
                                                language,
                                                "Left click: rebind | Right click: clear",
                                                "Chuột trái: đổi phím | Chuột phải: xóa phím"
                                            )
                                        )
                                    } else {
                                        Self::tr_lang(
                                            language,
                                            "Left click: bind hotkey",
                                            "Chuột trái: gán phím tắt",
                                        )
                                        .to_string()
                                    };

                                    let btn_text = if capture_active {
                                        RichText::new(Self::tr_lang(
                                            language,
                                            "Capturing...",
                                            "Đang bắt...",
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

                                    if Self::sound_style_remove_button(ui).clicked() {
                                        remove_id = Some(preset.id);
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
                                .checkbox(&mut preset.remove_title_bar, Self::tr_lang(language, "Remove bar", "Remove bar"))
                                .on_hover_text(
                                    Self::tr_lang(
                                        language,
                                        "Remove title bar before apply. Off restores it.",
                                        "Nếu bật, preset sẽ xóa thanh tiêu đề trước khi áp dụng kích thước và vị trí. Nếu tắt, thanh tiêu đề sẽ được giữ hoặc khôi phục.",
                                    ),
                                )
                                .changed();
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Animated Apply", "Animated Apply"));
                            ui.horizontal_wrapped(|ui| {
                                live_sync |= ui
                                    .checkbox(&mut preset.animate_enabled, Self::tr_lang(language, "Enabled", "Enabled"))
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
                                Self::tr_lang(language, "Focus", "Cửa sổ đang focus"),
                                &mut preset.target_window_title,
                                &mut preset.extra_target_window_titles,
                                &mut preset.match_duplicate_window_titles,
                                &self.open_windows,
                            );
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Preview", "Xem trước"));
                            ui.horizontal_wrapped(|ui| {
                                live_sync |= ui
                                    .checkbox(&mut preset.preview_enabled, Self::tr_lang(language, "Stream preview in editor", "Xem trước stream"))
                                    .changed();
                            });
                            ui.end_row();
                        });
                    if preset.preview_enabled {
                        ui.add_space(8.0);
                        Self::render_window_preset_preview(
                            ui,
                            language,
                            preset,
                            preview.as_ref(),
                            &mut live_sync,
                        );
                    }
                });
            }
            if let Some((target, status)) = next_capture_target.take() {
                self.begin_capture(target, status);
            }
            if cancel_active_capture {
                self.cancel_capture();
            }
        }

        if live_sync {
            self.persist_window_presets();
        }
        if let Some(id) = remove_id {
            self.state.window_presets.retain(|preset| preset.id != id);
            self.persist_window_presets();
        }

        self.render_layout_panel(ui);
    }

    pub(crate) fn render_zoom_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        ui.heading("Zoom");
        ui.label("Source -> target. Shift=ratio.");
        let screen_size = Self::screen_size();
        if ui.button("+ Add zoom preset").clicked() {
            self.add_zoom_preset();
            self.persist();
        }

        let mut remove_id = None;
        let mut live_sync = false;
        for index in 0..self.state.zoom_presets.len() {
            let mut next_capture_target = None;
            let mut cancel_active_capture = false;
            let active_capture_target = self.capture_target.clone();
            let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
            ui.add_space(6.0);
            let preset_snapshot = self.state.zoom_presets[index].clone();
            let preview = if preset_snapshot.preview_enabled && !preset_snapshot.collapsed {
                self.zoom_preview_for_preset(ui.ctx(), &preset_snapshot)
            } else {
                self.zoom_preview_cache.remove(&preset_snapshot.id);
                None
            };
            let preset = &mut self.state.zoom_presets[index];
            Self::show_preset_card(ui, preset.enabled, |ui| {
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
                    live_sync |= response.changed();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Self::enabled_icon_button(ui, preset.enabled)
                            .on_hover_text("Enable / disable preset")
                            .clicked()
                        {
                            preset.enabled = !preset.enabled;
                            live_sync = true;
                        }
                        if Self::sound_style_remove_button(ui).clicked() {
                            remove_id = Some(preset.id);
                        }
                        if Self::sound_style_toggle_button(
                            ui,
                            if preset.collapsed { "Show" } else { "Hide" },
                        )
                        .clicked()
                        {
                            preset.collapsed = !preset.collapsed;
                            if preset.collapsed {
                                self.zoom_preview_cache.remove(&preset.id);
                            }
                            live_sync = true;
                        }
                    });
                });
                if preset.collapsed {
                    return;
                }
                egui::Grid::new((preset.id, "zoom-grid"))
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Source");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            live_sync |= ui.add(DragValue::new(&mut preset.source_x)).changed();
                            ui.label("Y");
                            live_sync |= ui.add(DragValue::new(&mut preset.source_y)).changed();
                            ui.label("W");
                            live_sync |= ui
                                .add(DragValue::new(&mut preset.source_width).range(1..=8000))
                                .changed();
                            ui.label("H");
                            live_sync |= ui
                                .add(DragValue::new(&mut preset.source_height).range(1..=8000))
                                .changed();
                        });
                        ui.end_row();

                        ui.label("Target");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            live_sync |= ui.add(DragValue::new(&mut preset.target_x)).changed();
                            ui.label("Y");
                            live_sync |= ui.add(DragValue::new(&mut preset.target_y)).changed();
                            ui.label("W");
                            live_sync |= ui
                                .add(DragValue::new(&mut preset.target_width).range(1..=8000))
                                .changed();
                            ui.label("H");
                            live_sync |= ui
                                .add(DragValue::new(&mut preset.target_height).range(1..=8000))
                                .changed();
                        });
                        ui.end_row();

                        ui.label("FPS");
                        live_sync |= ui
                            .add(DragValue::new(&mut preset.fps).range(1..=240).suffix(" fps"))
                            .changed();
                        ui.end_row();

                        ui.label("Preview");
                        live_sync |= ui
                            .checkbox(&mut preset.preview_enabled, "Stream preview in editor")
                            .on_hover_text("Only stream the selected window into Source/Result when this is enabled.")
                            .changed();
                        if !preset.preview_enabled {
                            self.zoom_preview_cache.remove(&preset.id);
                        }
                        ui.end_row();

                        ui.label("Target Window");
                        live_sync |= Self::render_multi_window_targets(
                            ui,
                            language,
                            (preset.id, "zoom-target-window"),
                            Self::tr_lang(language, "Any focused window", "Cửa sổ đang focus"),
                            &mut preset.target_window_title,
                            &mut preset.extra_target_window_titles,
                            &self.open_windows,
                        );
                        ui.end_row();

                        ui.label("Hotkey");
                        ui.horizontal_wrapped(|ui| {
                            let capture_target = CaptureRequest::ZoomPresetHotkey(preset.id);
                            let (begin_capture, cancel_capture) =
                                Self::render_hotkey_capture_control(
                                    ui,
                                    language,
                                    &mut preset.hotkey,
                                    &capture_target,
                                    active_capture_target.as_ref(),
                                    pending_combo_keys.as_ref(),
                                    &mut live_sync,
                                );
                            if begin_capture {
                                next_capture_target = Some((
                                    capture_target,
                                    format!("Capturing zoom hotkey for {}.", preset.name),
                                ));
                            }
                            if cancel_capture {
                                cancel_active_capture = true;
                            }
                        });
                        ui.end_row();
                });
                ui.separator();
                live_sync |= Self::render_zoom_rect_editor(
                    ui,
                    (preset.id, "source"),
                    "Source Region",
                    &mut preset.source_x,
                    &mut preset.source_y,
                    &mut preset.source_width,
                    &mut preset.source_height,
                    screen_size,
                    preview.as_ref(),
                    None,
                    None,
                );
                ui.add_space(8.0);
                live_sync |= Self::render_zoom_rect_editor(
                    ui,
                    (preset.id, "target"),
                    "Result Region",
                    &mut preset.target_x,
                    &mut preset.target_y,
                    &mut preset.target_width,
                    &mut preset.target_height,
                    screen_size,
                    preview.as_ref(),
                    Some((
                        preset.source_x,
                        preset.source_y,
                        preset.source_width,
                        preset.source_height,
                    )),
                    Some(
                        (preset.source_width.max(1) as f32) / (preset.source_height.max(1) as f32),
                    ),
                );
            });
            if let Some((target, status)) = next_capture_target.take() {
                self.begin_capture(target, status);
            }
            if cancel_active_capture {
                self.cancel_capture();
            }
        }

        if live_sync {
            self.persist_window_presets();
        }
        if let Some(id) = remove_id {
            self.state.zoom_presets.retain(|preset| preset.id != id);
            self.zoom_preview_cache.remove(&id);
            self.reconcile_master_presets();
            self.persist_window_presets();
        }
    }

    pub(crate) fn render_pin_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(Self::tr_lang(
                    language,
                    "+ Add pin preset",
                    "+ Thêm preset ghim",
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
            let active_capture_target = self.capture_target.clone();
            let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
            ui.add_space(6.0);
            let preset_snapshot = self.state.pin_presets[index].clone();
            let preview = if pin_preview_allowed
                && preset_snapshot.preview_enabled
                && !preset_snapshot.collapsed
            {
                self.window_preview_for_target(
                    ui.ctx(),
                    100_000 + preset_snapshot.id,
                    preset_snapshot.target_window_title.as_ref(),
                    &preset_snapshot.extra_target_window_titles,
                    preset_snapshot.match_duplicate_window_titles,
                )
            } else {
                self.zoom_preview_cache
                    .remove(&(100_000 + preset_snapshot.id));
                None
            };
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
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
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
                                "Đang ghi... Nhấn một phím bất kỳ.",
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
                                Self::tr_lang(language, "Hotkey:", "Phím tắt:"),
                                bindings_labels.join(", "),
                                Self::tr_lang(
                                    language,
                                    "Left click: rebind | Right click: clear",
                                    "Chuột trái: đổi phím | Chuột phải: xóa phím"
                                )
                            )
                        } else {
                            Self::tr_lang(
                                language,
                                "Left click: bind hotkey",
                                "Chuột trái: gán phím tắt",
                            )
                            .to_string()
                        };

                        let btn_text = if capture_active {
                            RichText::new(Self::tr_lang(language, "Capturing...", "Đang bắt..."))
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
                        ui.label(Self::tr_lang(language, "Target Window", "Cửa sổ mục tiêu"));
                        let target_changed = Self::render_multi_window_targets_with_duplicate_mode(
                            ui,
                            language,
                            (preset.id, "pin-target-window"),
                            Self::tr_lang(language, "Focus", "Cửa sổ đang focus"),
                            &mut preset.target_window_title,
                            &mut preset.extra_target_window_titles,
                            &mut preset.match_duplicate_window_titles,
                            &self.open_windows,
                        );
                        if target_changed {
                            preset.source_crop_initialized = false;
                            preset.source_crop_fit_version = 0;
                        }
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
                                    "Phát xem trước trong trình chỉnh",
                                ),
                            )
                            .changed();
                        ui.end_row();
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
                    });
                } else {
                    ui.label(
                        RichText::new(Self::tr_lang(
                            language,
                            "Pinned view will keep the original window position and size.",
                            "Khung ghim sẽ giữ vị trí và kích thước gốc của cửa sổ.",
                        ))
                        .italics(),
                    );
                }

                if preset.use_source_crop {
                    if (!preset.source_crop_initialized || preset.source_crop_fit_version < 1)
                        && let Some(preview_frame) = preview.as_ref()
                    {
                        preset.source_x = 0;
                        preset.source_y = 0;
                        preset.source_width = preview_frame.logical_width.max(1);
                        preset.source_height = preview_frame.logical_height.max(1);
                        preset.source_crop_initialized = true;
                        preset.source_crop_fit_version = 1;
                        live_sync = true;
                    }
                    let crop_changed = Self::render_zoom_rect_editor(
                        ui,
                        (preset.id, "pin-source-crop"),
                        Self::tr_lang(language, "Source Crop", "Cắt vùng nguồn"),
                        &mut preset.source_x,
                        &mut preset.source_y,
                        &mut preset.source_width,
                        &mut preset.source_height,
                        screen_size,
                        preview.as_ref(),
                        None,
                        None,
                    );
                    if crop_changed {
                        preset.source_crop_initialized = true;
                        preset.source_crop_fit_version = 1;
                    }
                    live_sync |= crop_changed;
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(Self::tr_lang(
                                language,
                                "Reset to Full Window",
                                "Khôi phục toàn bộ cửa sổ",
                            ))
                            .clicked()
                        {
                            let mut target_frame = None;
                            if let Some(preview_frame) = preview.as_ref() {
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
                                preset.source_crop_fit_version = 1;
                                live_sync = true;
                            }
                        }
                    });
                }
            });
            if let Some((target, status)) = next_capture_target.take() {
                self.begin_capture(target, status);
            }
            if cancel_active_capture {
                self.cancel_capture();
            }
        }

        if let Some(id) = remove_id {
            self.state.pin_presets.retain(|preset| preset.id != id);
            live_sync = true;
        }
        if live_sync {
            self.persist_window_presets();
        }
    }

    pub(crate) fn render_modes_panel(&mut self, ui: &mut egui::Ui) {
        self.ensure_master_presets();
        self.reconcile_master_presets();
        ui.heading("Mode");
        ui.horizontal(|ui| {
            if ui.button("+ Capture").clicked() {
                self.add_master_preset_from_current();
            }
        });

        let mut remove_id = None;
        let mut apply_id = None;
        let mut update_from_current_id = None;
        let mut needs_persist = false;
        let selected_id = self.state.selected_master_preset_id;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for preset in &mut self.state.master_presets {
                    ui.separator();
                    let active = selected_id == Some(preset.id);
                    Self::show_preset_card(ui, active, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .radio(active, "")
                                .on_hover_text("Apply this mode right now.")
                                .clicked()
                            {
                                apply_id = Some(preset.id);
                            }
                            let response =
                                ui.add_sized([220.0, 21.0], TextEdit::singleline(&mut preset.name));
                            Self::apply_vietnamese_input_if_changed(
                                &response,
                                self.state.vietnamese_input_enabled,
                                self.state.vietnamese_input_mode,
                                &mut preset.name,
                            );
                            needs_persist |= response.changed();
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if Self::sound_style_remove_button(ui).clicked() {
                                        remove_id = Some(preset.id);
                                    }
                                    if ui.button("Update").clicked() {
                                        update_from_current_id = Some(preset.id);
                                    }
                                    if ui.button(if active { "Active" } else { "Apply" }).clicked()
                                    {
                                        apply_id = Some(preset.id);
                                    }
                                    if Self::sound_style_toggle_button(
                                        ui,
                                        if preset.collapsed { "Show" } else { "Hide" },
                                    )
                                    .clicked()
                                    {
                                        preset.collapsed = !preset.collapsed;
                                        needs_persist = true;
                                    }
                                },
                            );
                        });

                        if preset.collapsed {
                            return;
                        }

                        needs_persist |= ui
                            .checkbox(
                                &mut preset.macros_master_enabled,
                                "Enable all macros globally",
                            )
                            .changed();
                        ui.separator();
                        ui.label(RichText::new("Window Control").strong());
                        egui::Grid::new((preset.id, "mode-window-grid"))
                            .num_columns(4)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                ui.strong("Preset");
                                ui.strong("Apply");
                                ui.strong("Animate");
                                ui.strong("Restore");
                                ui.end_row();
                                for item in &mut preset.window_presets {
                                    let label = self
                                        .state
                                        .window_presets
                                        .iter()
                                        .find(|window_preset| window_preset.id == item.id)
                                        .map(|window_preset| window_preset.name.as_str())
                                        .unwrap_or("Missing preset");
                                    ui.label(label);
                                    needs_persist |= ui.checkbox(&mut item.enabled, "").changed();
                                    needs_persist |=
                                        ui.checkbox(&mut item.animate_enabled, "").changed();
                                    needs_persist |= ui
                                        .checkbox(&mut item.restore_titlebar_enabled, "")
                                        .changed();
                                    ui.end_row();
                                }
                            });

                        ui.separator();
                        ui.label(RichText::new("Zoom").strong());
                        for item in &mut preset.zoom_presets {
                            let label = self
                                .state
                                .zoom_presets
                                .iter()
                                .find(|zoom_preset| zoom_preset.id == item.id)
                                .map(|zoom_preset| zoom_preset.name.as_str())
                                .unwrap_or("Missing zoom");
                            needs_persist |= ui.checkbox(&mut item.enabled, label).changed();
                        }

                        ui.separator();
                        ui.label(RichText::new("Macro Groups").strong());
                        for group_state in &mut preset.macro_groups {
                            let Some(group) = self
                                .state
                                .macro_groups
                                .iter()
                                .find(|group| group.id == group_state.id)
                            else {
                                continue;
                            };
                            Frame::group(ui.style())
                                .inner_margin(egui::Margin::same(6))
                                .show(ui, |ui| {
                                    needs_persist |= ui
                                        .checkbox(&mut group_state.enabled, &group.name)
                                        .changed();
                                    ui.add_space(4.0);
                                    for preset_state in &mut group_state.presets {
                                        let label = group
                                            .presets
                                            .iter()
                                            .find(|macro_preset| macro_preset.id == preset_state.id)
                                            .map(|macro_preset| {
                                                hotkey::format_binding(macro_preset.hotkey.as_ref())
                                            })
                                            .unwrap_or_else(|| "Missing macro".to_owned());
                                        ui.indent(
                                            (group.id, preset_state.id, "mode-macro-indent"),
                                            |ui| {
                                                needs_persist |= ui
                                                    .checkbox(&mut preset_state.enabled, label)
                                                    .changed();
                                            },
                                        );
                                    }
                                });
                        }
                    });
                }
            });

        if let Some(id) = update_from_current_id {
            self.update_master_preset_from_current(id);
        }
        if let Some(id) = remove_id {
            self.state.master_presets.retain(|preset| preset.id != id);
            if self.state.selected_master_preset_id == Some(id) {
                self.state.selected_master_preset_id =
                    self.state.master_presets.first().map(|preset| preset.id);
            }
            self.ensure_master_presets();
            self.persist();
        }
        if let Some(id) = apply_id {
            self.apply_master_preset(id);
        } else if needs_persist {
            self.persist();
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

        if response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                let dist_tl = pointer_pos.distance(window_rect.left_top());
                let dist_tr = pointer_pos.distance(window_rect.right_top());
                let dist_bl = pointer_pos.distance(window_rect.left_bottom());
                let dist_br = pointer_pos.distance(window_rect.right_bottom());

                let nearest_on_box = egui::pos2(
                    pointer_pos.x.clamp(window_rect.left(), window_rect.right()),
                    pointer_pos.y.clamp(window_rect.top(), window_rect.bottom()),
                );
                let dist_to_box = pointer_pos.distance(nearest_on_box);

                active_handle = if dist_tl < 12.0 {
                    DragHandle::TopLeft
                } else if dist_tr < 12.0 {
                    DragHandle::TopRight
                } else if dist_bl < 12.0 {
                    DragHandle::BottomLeft
                } else if dist_br < 12.0 {
                    DragHandle::BottomRight
                } else if (pointer_pos.x - window_rect.left()).abs() < 8.0
                    && pointer_pos.y >= window_rect.top()
                    && pointer_pos.y <= window_rect.bottom()
                {
                    DragHandle::Left
                } else if (pointer_pos.x - window_rect.right()).abs() < 8.0
                    && pointer_pos.y >= window_rect.top()
                    && pointer_pos.y <= window_rect.bottom()
                {
                    DragHandle::Right
                } else if (pointer_pos.y - window_rect.top()).abs() < 8.0
                    && pointer_pos.x >= window_rect.left()
                    && pointer_pos.x <= window_rect.right()
                {
                    DragHandle::Top
                } else if (pointer_pos.y - window_rect.bottom()).abs() < 8.0
                    && pointer_pos.x >= window_rect.left()
                    && pointer_pos.x <= window_rect.right()
                {
                    DragHandle::Bottom
                } else if window_rect.contains(pointer_pos) {
                    DragHandle::Center
                } else if dist_to_box < 20.0 {
                    DragHandle::Center
                } else {
                    DragHandle::None
                };
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
                let dist_tl = pointer_pos.distance(window_rect.left_top());
                let dist_tr = pointer_pos.distance(window_rect.right_top());
                let dist_bl = pointer_pos.distance(window_rect.left_bottom());
                let dist_br = pointer_pos.distance(window_rect.right_bottom());

                let handle_to_use = if active_handle != DragHandle::None {
                    active_handle
                } else if dist_tl < 12.0 {
                    DragHandle::TopLeft
                } else if dist_tr < 12.0 {
                    DragHandle::TopRight
                } else if dist_bl < 12.0 {
                    DragHandle::BottomLeft
                } else if dist_br < 12.0 {
                    DragHandle::BottomRight
                } else if (pointer_pos.x - window_rect.left()).abs() < 8.0
                    && pointer_pos.y >= window_rect.top()
                    && pointer_pos.y <= window_rect.bottom()
                {
                    DragHandle::Left
                } else if (pointer_pos.x - window_rect.right()).abs() < 8.0
                    && pointer_pos.y >= window_rect.top()
                    && pointer_pos.y <= window_rect.bottom()
                {
                    DragHandle::Right
                } else if (pointer_pos.y - window_rect.top()).abs() < 8.0
                    && pointer_pos.x >= window_rect.left()
                    && pointer_pos.x <= window_rect.right()
                {
                    DragHandle::Top
                } else if (pointer_pos.y - window_rect.bottom()).abs() < 8.0
                    && pointer_pos.x >= window_rect.left()
                    && pointer_pos.x <= window_rect.right()
                {
                    DragHandle::Bottom
                } else if window_rect.contains(pointer_pos) {
                    DragHandle::Center
                } else {
                    DragHandle::None
                };

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
                    Self::tr_lang(language, "Target Window", "Cửa sổ mục tiêu").to_string()
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
        target_preview_source: Option<(i32, i32, i32, i32)>,
        keep_aspect_ratio: Option<f32>,
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
        let (coord_width, coord_height, content_scale, preview_content_rect) =
            if let Some(preview_frame) = preview {
                let window_pos = egui::pos2(
                    selection_bounds_rect.left() + (preview_frame.screen_x as f32 * scale),
                    selection_bounds_rect.top() + (preview_frame.screen_y as f32 * scale),
                );
                let window_size = vec2(
                    preview_frame.logical_width.max(1) as f32 * scale,
                    preview_frame.logical_height.max(1) as f32 * scale,
                );
                (
                    screen_size.x,
                    screen_size.y,
                    scale,
                    egui::Rect::from_min_size(window_pos, window_size),
                )
            } else {
                (screen_size.x, screen_size.y, scale, selection_bounds_rect)
            };

        if let Some(preview_frame) = preview {
            ui.painter().image(
                preview_frame.texture.id(),
                preview_content_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            ui.painter().text(
                preview_content_rect.left_top() + vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                &preview_frame.title,
                egui::TextStyle::Small.resolve(ui.style()),
                Color32::WHITE,
            );
        }

        let min_size = vec2(6.0, 6.0);
        let mut rect = egui::Rect::from_min_size(
            egui::pos2(
                selection_bounds_rect.left() + (*x as f32 * content_scale),
                selection_bounds_rect.top() + (*y as f32 * content_scale),
            ),
            vec2(
                (*width).max(1) as f32 * content_scale,
                (*height).max(1) as f32 * content_scale,
            ),
        );
        rect = rect.intersect(selection_bounds_rect);
        if rect.width() < min_size.x {
            rect.max.x = (rect.min.x + min_size.x).min(selection_bounds_rect.right());
        }
        if rect.height() < min_size.y {
            rect.max.y = (rect.min.y + min_size.y).min(selection_bounds_rect.bottom());
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

        if response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                let dist_tl = pointer_pos.distance(rect.left_top());
                let dist_tr = pointer_pos.distance(rect.right_top());
                let dist_bl = pointer_pos.distance(rect.left_bottom());
                let dist_br = pointer_pos.distance(rect.right_bottom());

                // Also allow drag start from near-but-outside the box (helps with tiny boxes)
                let nearest_on_box = egui::pos2(
                    pointer_pos.x.clamp(rect.left(), rect.right()),
                    pointer_pos.y.clamp(rect.top(), rect.bottom()),
                );
                let dist_to_box = pointer_pos.distance(nearest_on_box);

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
                } else if dist_to_box < 20.0 {
                    SelectionDragHandle::Center
                } else {
                    SelectionDragHandle::None
                };
                ui.data_mut(|d| d.insert_temp(drag_id, active_handle));
            }
        }

        // Use pointer.primary_down() + pointer.delta() so dragging continues even when
        // the mouse moves outside the canvas bounds (important for small boxes).
        let pointer_primary_down = ui.input(|i| i.pointer.primary_down());
        let pointer_delta = ui.input(|i| i.pointer.delta());
        if pointer_primary_down && active_handle != SelectionDragHandle::None {
            let delta = pointer_delta;
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
            let lock_aspect = keep_aspect_ratio.unwrap_or(if ctrl_pressed {
                target_aspect
            } else if shift_pressed {
                aspect
            } else {
                0.0
            });

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

            // Bound checking
            if rect.left() < selection_bounds_rect.left() {
                rect = rect.translate(vec2(selection_bounds_rect.left() - rect.left(), 0.0));
            }
            if rect.top() < selection_bounds_rect.top() {
                rect = rect.translate(vec2(0.0, selection_bounds_rect.top() - rect.top()));
            }
            if rect.right() > selection_bounds_rect.right() {
                rect = rect.translate(vec2(selection_bounds_rect.right() - rect.right(), 0.0));
            }
            if rect.bottom() > selection_bounds_rect.bottom() {
                rect = rect.translate(vec2(0.0, selection_bounds_rect.bottom() - rect.bottom()));
            }

            rect.min.x = rect.min.x.clamp(
                selection_bounds_rect.left(),
                selection_bounds_rect.right() - min_size.x,
            );
            rect.min.y = rect.min.y.clamp(
                selection_bounds_rect.top(),
                selection_bounds_rect.bottom() - min_size.y,
            );
            rect.max.x = rect
                .max
                .x
                .clamp(rect.min.x + min_size.x, selection_bounds_rect.right());
            rect.max.y = rect
                .max
                .y
                .clamp(rect.min.y + min_size.y, selection_bounds_rect.bottom());
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
                ui.painter()
                    .image(preview_frame.texture.id(), rect, uv, Color32::WHITE);
            }
        }

        ui.painter().rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(2.0, Color32::from_rgb(124, 240, 164)),
            egui::StrokeKind::Outside,
        );

        let size_text = format!("{}x{}", *width, *height);
        ui.painter().text(
            rect.left_top() + egui::vec2(0.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            size_text,
            egui::FontId::proportional(10.0),
            Color32::from_rgb(124, 240, 164),
        );

        if changed {
            *x = ((rect.left() - selection_bounds_rect.left()) / content_scale).round() as i32;
            *y = ((rect.top() - selection_bounds_rect.top()) / content_scale).round() as i32;
            *width = (rect.width() / content_scale).round().max(1.0) as i32;
            *height = (rect.height() / content_scale).round().max(1.0) as i32;
            *x = (*x).clamp(0, coord_width.round() as i32);
            *y = (*y).clamp(0, coord_height.round() as i32);
        }

        ui.label(RichText::new(format!("X={} Y={} W={} H={}", *x, *y, *width, *height)).small());
        changed
    }

    pub(crate) fn add_window_preset(&mut self) {
        let mut id = 1;
        while self.state.window_presets.iter().any(|p| p.id == id) {
            id += 1;
        }
        self.state.next_preset_id = (self
            .state
            .window_presets
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            + 1)
        .max(id + 1);
        self.state.window_presets.push(WindowPreset::new(id));
        self.reconcile_master_presets();
        self.sync_window_presets();
        self.status = format!("Added window preset {id}.");
    }

    pub(crate) fn add_window_focus_preset(&mut self) {
        let mut id = 1;
        while self.state.window_focus_presets.iter().any(|p| p.id == id) {
            id += 1;
        }
        self.state.next_window_focus_preset_id = (self
            .state
            .window_focus_presets
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            + 1)
        .max(id + 1);
        self.state
            .window_focus_presets
            .push(WindowFocusPreset::new(id));
        self.reconcile_master_presets();
        self.sync_window_presets();
        self.status = format!("Added window focus preset {id}.");
    }

    pub(crate) fn add_zoom_preset(&mut self) {
        let mut id = 1;
        while self.state.zoom_presets.iter().any(|p| p.id == id) {
            id += 1;
        }
        self.state.next_zoom_preset_id = (self
            .state
            .zoom_presets
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            + 1)
        .max(id + 1);
        self.state.zoom_presets.push(ZoomPreset::new(id));
        self.reconcile_master_presets();
        self.sync_window_presets();
        self.status = format!("Added zoom preset {id}.");
    }

    pub(crate) fn add_pin_preset(&mut self) {
        let mut id = 1;
        while self.state.pin_presets.iter().any(|p| p.id == id) {
            id += 1;
        }
        self.state.next_pin_preset_id = (self
            .state
            .pin_presets
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            + 1)
        .max(id + 1);
        self.state.pin_presets.push(PinPreset::new(id));
        self.sync_window_presets();
        self.status = format!("Added pin preset {id}.");
    }

    pub(crate) fn persist_window_presets(&mut self) {
        self.sync_window_presets();
        self.persist();
    }

    pub(crate) fn sync_window_presets(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateWindowPresets(
            self.state.window_presets.clone(),
        ));
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateWindowFocusPresets(
                self.state.window_focus_presets.clone(),
            ));
        let _ = self.overlay_tx.send(OverlayCommand::UpdatePinPresets(
            self.state.pin_presets.clone(),
        ));
        let _ = self.overlay_tx.send(OverlayCommand::UpdateMousePathPresets(
            self.state.mouse_path_presets.clone(),
        ));
    }

    pub(crate) fn persist_window_layouts(&mut self) {
        self.sync_window_layouts();
        self.persist();
    }

    pub(crate) fn sync_window_layouts(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateWindowLayouts(
            self.state.window_layouts.clone(),
        ));
    }

    pub(crate) fn add_window_layout(&mut self) {
        let mut id = 1;
        while self.state.window_layouts.iter().any(|l| l.id == id) {
            id += 1;
        }
        self.state.next_window_layout_id = (self
            .state
            .window_layouts
            .iter()
            .map(|l| l.id)
            .max()
            .unwrap_or(0)
            + 1)
        .max(id + 1);
        self.state.window_layouts.push(WindowLayout::new(id));
        self.sync_window_layouts();
        self.persist();
        self.status = format!("Added layout {id}.");
    }

    fn sanitize_layout(layout: &mut WindowLayout) {
        let rows = layout.rows.max(1);
        let cols = layout.cols.max(1);

        layout.rows = rows;
        layout.cols = cols;

        if layout.row_ratios.len() < rows {
            layout.row_ratios.resize(rows, 1.0);
        } else if layout.row_ratios.len() > rows {
            layout.row_ratios.truncate(rows);
        }
        for val in &mut layout.row_ratios {
            if *val <= 0.0 {
                *val = 0.1;
            }
        }

        if layout.col_ratios.len() < cols {
            layout.col_ratios.resize(cols, 1.0);
        } else if layout.col_ratios.len() > cols {
            layout.col_ratios.truncate(cols);
        }
        for val in &mut layout.col_ratios {
            if *val <= 0.0 {
                *val = 0.1;
            }
        }

        layout.cells.retain(|cell| cell.row < rows && cell.col < cols);
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

        for r in 0..rows {
            for c in 0..cols {
                if !covered[r][c] {
                    sanitized_cells.push(WindowLayoutCell {
                        row: r,
                        col: c,
                        row_span: 1,
                        col_span: 1,
                        ..Default::default()
                    });
                    covered[r][c] = true;
                }
            }
        }

        layout.cells = sanitized_cells;
    }

    pub(crate) fn render_layout_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;

        let mut remove_id = None;
        let mut live_sync = false;

        ui.add_space(16.0);
        ui.label(
            RichText::new(Self::tr_lang(language, "Layout Presets", "Bố cục"))
                .strong()
                .size(14.0),
        );
        ui.add_space(4.0);
        
        let layouts_count = self.state.window_layouts.len();
        for index in 0..layouts_count {
            let mut next_capture_target = None;
            let mut cancel_active_capture = false;
            let active_capture_target = self.capture_target.clone();
            let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
            ui.add_space(6.0);
            
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
                            layout.enabled = layout.hotkey.is_some()
                                || !layout.trigger_keys.trim().is_empty();
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
                                let has_keys = layout.hotkey.is_some()
                                    || !layout.trigger_keys.trim().is_empty();
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
                                        "Đang ghi... Nhấn một phím bất kỳ.",
                                    )
                                    .to_string()
                                } else if has_keys {
                                    let bindings_labels: Vec<String> =
                                        Self::preset_trigger_bindings(
                                            &layout.hotkey,
                                            &layout.trigger_keys,
                                        )
                                        .iter()
                                        .map(|b| hotkey::format_binding(Some(b)))
                                        .collect();
                                    format!(
                                        "{} {}\n{}",
                                        Self::tr_lang(language, "Hotkey:", "Phím tắt:"),
                                        bindings_labels.join(", "),
                                        Self::tr_lang(
                                            language,
                                            "Left click: rebind | Right click: clear",
                                            "Chuột trái: đổi phím | Chuột phải: xóa phím"
                                        )
                                    )
                                } else {
                                    Self::tr_lang(
                                        language,
                                        "Left click: bind hotkey",
                                        "Chuột trái: gán phím tắt",
                                    )
                                    .to_string()
                                };

                                let btn_text = if capture_active {
                                    RichText::new(Self::tr_lang(
                                        language,
                                        "Capturing...",
                                        "Đang bắt...",
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
                                                layout.name
                                            ),
                                        ));
                                    }
                                }
                                if btn_response.secondary_clicked() {
                                    layout.hotkey = None;
                                    layout.trigger_keys.clear();
                                    layout.enabled = false;
                                    live_sync = true;
                                }

                                if Self::sound_style_remove_button(ui).clicked() {
                                    remove_id = Some(layout.id);
                                }
                                
                                if Self::sound_style_toggle_button(
                                    ui,
                                    if layout.collapsed {
                                        Self::tr_lang(language, "Show", "Hiện")
                                    } else {
                                        Self::tr_lang(language, "Hide", "Ẩn")
                                    },
                                )
                                .clicked()
                                {
                                    layout.collapsed = !layout.collapsed;
                                    live_sync = true;
                                }
                                
                                if ui.button(Self::tr_lang(language, "Apply", "Áp dụng")).clicked() {
                                    let _ = self.overlay_tx.send(OverlayCommand::ApplyWindowLayout(layout.clone()));
                                }
                            },
                        );
                        ui.end_row();
                    });
                
                if layout.collapsed {
                    return;
                }
                
                egui::Grid::new((id_source, "window-layout-settings-grid"))
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(Self::tr_lang(language, "Focus on apply", "Focus khi áp dụng"));
                        live_sync |= ui.checkbox(&mut layout.focus_on_apply, "").changed();
                        ui.end_row();
                        
                        ui.label(Self::tr_lang(language, "Grid size", "Kích thước lưới"));
                        ui.horizontal(|ui| {
                            ui.label(Self::tr_lang(language, "Rows", "Hàng"));
                            let mut rows = layout.rows;
                            if ui.add(DragValue::new(&mut rows).range(1..=6)).changed() {
                                layout.rows = rows;
                                Self::sanitize_layout(layout);
                                live_sync = true;
                            }
                            ui.label(Self::tr_lang(language, "Cols", "Cột"));
                            let mut cols = layout.cols;
                            if ui.add(DragValue::new(&mut cols).range(1..=6)).changed() {
                                layout.cols = cols;
                                Self::sanitize_layout(layout);
                                live_sync = true;
                            }
                        });
                        ui.end_row();
                        
                        ui.label(Self::tr_lang(language, "Row ratios", "Tỷ lệ hàng"));
                        ui.horizontal(|ui| {
                            for r in 0..layout.rows {
                                if r < layout.row_ratios.len() {
                                    let mut val = layout.row_ratios[r];
                                    if ui.add(DragValue::new(&mut val).range(0.05..=10.0).speed(0.05).prefix(&format!("R{}: ", r + 1))).changed() {
                                        layout.row_ratios[r] = val;
                                        live_sync = true;
                                    }
                                }
                            }
                        });
                        ui.end_row();
                        
                        ui.label(Self::tr_lang(language, "Col ratios", "Tỷ lệ cột"));
                        ui.horizontal(|ui| {
                            for c in 0..layout.cols {
                                if c < layout.col_ratios.len() {
                                    let mut val = layout.col_ratios[c];
                                    if ui.add(DragValue::new(&mut val).range(0.05..=10.0).speed(0.05).prefix(&format!("C{}: ", c + 1))).changed() {
                                        layout.col_ratios[c] = val;
                                        live_sync = true;
                                    }
                                }
                            }
                        });
                        ui.end_row();
                        
                        ui.label(Self::tr_lang(language, "Visual Grid", "Xem trước lưới"));
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
                            
                            let preview_w = 320.0;
                            let preview_h = 160.0;
                            
                            let (rect, _response) = ui.allocate_exact_size(vec2(preview_w, preview_h), egui::Sense::hover());
                            
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                ui.visuals().extreme_bg_color
                            );
                            
                            let cells_to_draw = layout.cells.clone();
                            for cell in &cells_to_draw {
                                if cell.row >= layout.rows || cell.col >= layout.cols {
                                    continue;
                                }
                                
                                let end_row = (cell.row + cell.row_span).min(layout.rows);
                                let end_col = (cell.col + cell.col_span).min(layout.cols);
                                
                                let x1 = rect.min.x + col_starts[cell.col] * preview_w;
                                let y1 = rect.min.y + row_starts[cell.row] * preview_h;
                                let x2 = rect.min.x + col_starts[end_col] * preview_w;
                                let y2 = rect.min.y + row_starts[end_row] * preview_h;
                                
                                let cell_rect = egui::Rect::from_min_max(
                                    egui::pos2(x1, y1),
                                    egui::pos2(x2, y2)
                                ).shrink(2.0);
                                
                                let is_selected = self.selected_layout_cell == Some((layout.id, cell.row, cell.col));
                                
                                let cell_id = ui.make_persistent_id((layout.id, "cell", cell.row, cell.col));
                                let cell_resp = ui.interact(cell_rect, cell_id, egui::Sense::click());
                                
                                if cell_resp.clicked() {
                                    self.selected_layout_cell = Some((layout.id, cell.row, cell.col));
                                }
                                
                                let fill_color = if is_selected {
                                    Color32::from_rgba_premultiplied(0, 120, 215, 80)
                                } else if cell_resp.hovered() {
                                    Color32::from_rgba_premultiplied(128, 128, 128, 40)
                                } else {
                                    Color32::from_rgba_premultiplied(128, 128, 128, 20)
                                };
                                
                                let border_color = if is_selected {
                                    Color32::from_rgb(0, 120, 215)
                                } else {
                                    ui.visuals().widgets.noninteractive.bg_stroke.color
                                };
                                
                                let stroke_width = if is_selected { 2.0 } else { 1.0 };
                                
                                ui.painter().rect(
                                    cell_rect,
                                    2.0,
                                    fill_color,
                                    egui::Stroke::new(stroke_width, border_color),
                                    egui::StrokeKind::Inside,
                                );
                                
                                let label_text = if let Some(title) = &cell.target_window_title {
                                    Self::truncate_window_title(title, 12)
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
                                    text_color
                                );
                            }
                        });
                        ui.end_row();
                    });
                
                if let Some((sel_layout_id, sel_row, sel_col)) = self.selected_layout_cell {
                    if sel_layout_id == layout.id {
                        if let Some(cell_idx) = layout.cells.iter().position(|c| c.row == sel_row && c.col == sel_col) {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("Cell ({}, {}) Settings", sel_row, sel_col)).strong());
                            });
                            
                            let mut cell_modified = false;
                            
                            let mut row_span = layout.cells[cell_idx].row_span;
                            let mut col_span = layout.cells[cell_idx].col_span;
                            let mut target_window_title = layout.cells[cell_idx].target_window_title.clone();
                            let mut extra_target_window_titles = layout.cells[cell_idx].extra_target_window_titles.clone();
                            let mut match_duplicate_window_titles = layout.cells[cell_idx].match_duplicate_window_titles;
                            
                            egui::Grid::new((layout.id, "cell-settings-grid", sel_row, sel_col))
                                .num_columns(2)
                                .spacing([14.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label(Self::tr_lang(language, "Span", "Hợp nhất"));
                                    ui.horizontal(|ui| {
                                        ui.label(Self::tr_lang(language, "Row span", "Hợp dòng"));
                                        let max_row_span = layout.rows - sel_row;
                                        if ui.add(DragValue::new(&mut row_span).range(1..=max_row_span)).changed() {
                                            cell_modified = true;
                                        }
                                        ui.label(Self::tr_lang(language, "Col span", "Hợp cột"));
                                        let max_col_span = layout.cols - sel_col;
                                        if ui.add(DragValue::new(&mut col_span).range(1..=max_col_span)).changed() {
                                            cell_modified = true;
                                        }
                                    });
                                    ui.end_row();
                                    
                                    ui.label(Self::tr_lang(language, "Target Window", "Cửa sổ mục tiêu"));
                                    let dropdown_changed = Self::render_multi_window_targets_with_duplicate_mode(
                                        ui,
                                        language,
                                        (layout.id, "cell-target-picker", sel_row, sel_col),
                                        Self::tr_lang(language, "Focus", "Cửa sổ đang focus"),
                                        &mut target_window_title,
                                        &mut extra_target_window_titles,
                                        &mut match_duplicate_window_titles,
                                        &self.open_windows,
                                    );
                                    if dropdown_changed {
                                        cell_modified = true;
                                    }
                                    ui.end_row();
                                });
                            
                            if cell_modified {
                                layout.cells[cell_idx].row_span = row_span;
                                layout.cells[cell_idx].col_span = col_span;
                                layout.cells[cell_idx].target_window_title = target_window_title;
                                layout.cells[cell_idx].extra_target_window_titles = extra_target_window_titles;
                                layout.cells[cell_idx].match_duplicate_window_titles = match_duplicate_window_titles;
                                
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
        }
        
        if live_sync {
            self.persist_window_layouts();
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

    pub(crate) fn sync_hud_preview(&mut self, preset: Option<&HudPreset>) {
        let next_id = preset.map(|preset| preset.id);
        if self.active_hud_preview_preset_id == next_id {
            if let Some(preset) = preset {
                let _ = self
                    .overlay_tx
                    .send(OverlayCommand::PreviewHudPreset(vec![preset.clone()]));
            }
            return;
        }
        self.active_hud_preview_preset_id = next_id;
        let _ = self.overlay_tx.send(OverlayCommand::PreviewHudPreset(
            preset.cloned().into_iter().collect(),
        ));
    }

    pub(crate) fn clear_hud_preview(&mut self) {
        if self.active_hud_preview_preset_id.take().is_some() {
            let _ = self
                .overlay_tx
                .send(OverlayCommand::PreviewHudPreset(Vec::new()));
        }
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
        ctx: &egui::Context,
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

        if should_request {
            self.window_preview_requested.insert(cache_id, Instant::now());
            
            let ui_tx = self.ui_tx.clone();
            let target_title = target_window_title.cloned();
            let extra_titles = extra_target_window_titles.to_vec();
            
            std::thread::spawn(move || {
                if let Some(frame) = crate::window_list::capture_window_preview_with_candidates(
                    target_title.as_deref(),
                    &extra_titles,
                    match_duplicate_window_titles,
                    720,
                ) {
                    let _ = ui_tx.send(crate::overlay::UiCommand::WindowPreviewLoaded {
                        cache_id,
                        source_window_key: target_title,
                        source_window_extra_keys: extra_titles,
                        match_duplicate_window_titles,
                        frame,
                    });
                }
            });
        }

        self.zoom_preview_cache.get(&cache_id).map(|cache| cache.view.clone())
    }

    pub(crate) fn zoom_preview_for_preset(
        &mut self,
        ctx: &egui::Context,
        preset: &ZoomPreset,
    ) -> Option<ZoomPreviewView> {
        self.window_preview_for_target(
            ctx,
            preset.id,
            preset.target_window_title.as_ref(),
            &preset.extra_target_window_titles,
            false,
        )
    }

    pub(crate) fn clear_pin_preview_cache(&mut self) {
        for preset in &self.state.pin_presets {
            self.zoom_preview_cache.remove(&(100_000 + preset.id));
        }
    }

    pub(crate) fn apply_locked_aspect_ratio(
        handle: &str,
        aspect_ratio: f32,
        bounds: egui::Rect,
        min_size: egui::Vec2,
        rect: &mut egui::Rect,
    ) {
        if aspect_ratio <= 0.0 {
            return;
        }
        match handle {
            "nw" | "ne" | "se" | "sw" => {
                let anchor = match handle {
                    "nw" => rect.right_bottom(),
                    "ne" => rect.left_bottom(),
                    "se" => rect.left_top(),
                    "sw" => rect.right_top(),
                    _ => rect.right_bottom(),
                };
                let moving = match handle {
                    "nw" => rect.left_top(),
                    "ne" => rect.right_top(),
                    "se" => rect.right_bottom(),
                    "sw" => rect.left_bottom(),
                    _ => rect.left_top(),
                };
                let mut dx = moving.x - anchor.x;
                let mut dy = moving.y - anchor.y;
                let width = dx.abs().max(min_size.x);
                let height = dy.abs().max(min_size.y);
                let expected_height = width / aspect_ratio;
                let expected_width = height * aspect_ratio;
                if expected_height >= height {
                    dy = dy.signum() * expected_height.max(min_size.y);
                } else {
                    dx = dx.signum() * expected_width.max(min_size.x);
                }
                let new_corner = egui::pos2(anchor.x + dx, anchor.y + dy);
                *rect = egui::Rect::from_two_pos(anchor, new_corner).intersect(bounds);
            }
            "n" | "s" => {
                let center_x = rect.center().x;
                let anchor_y = if handle == "n" {
                    rect.bottom()
                } else {
                    rect.top()
                };
                let moving_y = if handle == "n" {
                    rect.top()
                } else {
                    rect.bottom()
                };
                let height = (moving_y - anchor_y).abs().max(min_size.y);
                let width = (height * aspect_ratio).max(min_size.x);
                let left = (center_x - width * 0.5).clamp(bounds.left(), bounds.right() - width);
                let right = left + width;
                let top = if handle == "n" {
                    (anchor_y - height).clamp(bounds.top(), bounds.bottom() - height)
                } else {
                    anchor_y.clamp(bounds.top(), bounds.bottom() - height)
                };
                let bottom = top + height;
                *rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));
            }
            "e" | "w" => {
                let center_y = rect.center().y;
                let anchor_x = if handle == "w" {
                    rect.right()
                } else {
                    rect.left()
                };
                let moving_x = if handle == "w" {
                    rect.left()
                } else {
                    rect.right()
                };
                let width = (moving_x - anchor_x).abs().max(min_size.x);
                let height = (width / aspect_ratio).max(min_size.y);
                let top = (center_y - height * 0.5).clamp(bounds.top(), bounds.bottom() - height);
                let bottom = top + height;
                let left = if handle == "w" {
                    (anchor_x - width).clamp(bounds.left(), bounds.right() - width)
                } else {
                    anchor_x.clamp(bounds.left(), bounds.right() - width)
                };
                let right = left + width;
                *rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));
            }
            _ => {}
        }
    }
}
