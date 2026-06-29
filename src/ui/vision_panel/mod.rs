use crate::hotkey;
use crate::model::*;
use crate::overlay::{OverlayCommand, UiCommand};
use crate::ui::{
    CrosshairApp, VisionCaptureMode, VisionCaptureTarget, VisionPreviewCache, VisionPreviewView,
};
use crate::window_list;
use crossbeam_channel::Sender;
use eframe::egui::{
    self, Button, Color32, ColorImage, DragValue, Frame, Margin, RichText, Sense, Slider,
    TextBuffer, TextEdit, TextureOptions, pos2, vec2,
};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[cfg(windows)]
use crate::ui::{GetAsyncKeyState, GetCursorPos, POINT};

impl CrosshairApp {
    pub(crate) fn native_selected_region_target_handles_cleanup(
        target: VisionCaptureTarget,
    ) -> bool {
        matches!(
            target,
            VisionCaptureTarget::Preset(_)
                | VisionCaptureTarget::OcrPreset(_)
                | VisionCaptureTarget::OcrStepRegion { .. }
        )
    }

    pub(crate) fn render_vision_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let language = self.state.ui_language;
        let capture_target_snapshot = self.capture_target.clone();
        let selected_steps_snapshot = self.selected_macro_steps.clone();
        let cancel_mouse_move_absolute_capture = false;
        let next_mouse_move_absolute_capture_target = None;
        ui.add_space(2.0);

        ui.horizontal(|ui| {
            if ui
                .button(Self::tr_lang(language, "+ Detect image", "+ Detect image"))
                .clicked()
            {
                let id = Self::allocate_next_id(
                    &self.state.vision_presets,
                    &mut self.state.next_vision_preset_id,
                    |preset| preset.id,
                );
                let mut preset = VisionPreset::new(id);
                preset.use_color_matching = false;
                self.state.vision_presets.push(preset);
                self.persist_vision_presets();
            }
            if ui
                .button(Self::tr_lang(language, "+ Detect color", "+ Detect color"))
                .clicked()
            {
                let id = Self::allocate_next_id(
                    &self.state.vision_presets,
                    &mut self.state.next_vision_preset_id,
                    |preset| preset.id,
                );
                let mut preset = VisionPreset::new(id);
                preset.name = format!("Color Search {id}");
                preset.use_color_matching = true;
                self.state.vision_presets.push(preset);
                self.persist_vision_presets();
            }
            if ui
                .button(Self::tr_lang(
                    language,
                    "+ Pixel counter",
                    "+ Pixel counter",
                ))
                .clicked()
            {
                let id = Self::allocate_next_id(
                    &self.state.vision_presets,
                    &mut self.state.next_vision_preset_id,
                    |preset| preset.id,
                );
                let mut preset = VisionPreset::new(id);
                preset.name = format!("Pixel Counter {id}");
                preset.use_color_matching = true;
                preset.is_pixel_counter = true;
                self.state.vision_presets.push(preset);
                self.persist_vision_presets();
            }
        });

        ui.add_space(8.0);
        let mut remove_id = None;
        let mut live_sync = false;
        let next_capture_target = None;
        let cancel_active_capture = false;
        let pending_custom_preset_save: Option<()> = None;

        let categories = [
            (
                Self::tr_lang(language, "Detect Image", "Detect Image"),
                false,
                false,
            ),
            (
                Self::tr_lang(language, "Detect Color", "Detect Color"),
                true,
                false,
            ),
            (
                Self::tr_lang(language, "Pixel Counter", "Pixel Counter"),
                true,
                true,
            ),
        ];

        for (title, filter_color, filter_counter) in categories {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(title).strong().size(14.0));
            ui.add_space(4.0);

            for index in 0..self.state.vision_presets.len() {
                let preset_snapshot = self.state.vision_presets[index].clone();
                if preset_snapshot.use_color_matching != filter_color
                    || preset_snapshot.is_pixel_counter != filter_counter
                {
                    continue;
                }
                let preview = if preset_snapshot.collapsed {
                    self.vision_preview_cache.remove(&preset_snapshot.id);
                    None
                } else {
                    self.image_search_preview_for_preset(ctx, &preset_snapshot)
                };
                let mut next_capture_local = None;
                let mut cancel_active_capture_local = false;
                let mut start_image_search_capture = None;
                let mut start_search_region_capture = None;
                let mut start_adjust_region_capture = None;
                let mut start_color_pick_capture = None;
                let mut start_color_priority_anchor_capture = None;
                let mut start_single_pixel_capture = None;
                let template_file = self.vision_template_file_for_preset(preset_snapshot.id);
                let preset = &mut self.state.vision_presets[index];
                if preset.click_after_move {
                    preset.click_after_move = false;
                    live_sync = true;
                }

                let capture_target = CaptureRequest::VisionPresetHotkey(preset.id);
                let active_capture_target = self.capture_target.clone();
                let pending_combo_keys = self.capture_hotkey_combo_keys.clone();

                preset.enabled = preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
                ui.add_space(6.0);
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
                                let bindings_labels: Vec<String> = Self::preset_trigger_bindings(
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
                                    cancel_active_capture_local = true;
                                } else {
                                    next_capture_local = Some((
                                        capture_target.clone(),
                                        format!(
                                            "Capturing image search hotkey for {}.",
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
                                    Self::tr_lang(language, "Show", "Show")
                                } else {
                                    Self::tr_lang(language, "Hide", "Hide")
                                },
                            )
                            .clicked()
                            {
                                preset.collapsed = !preset.collapsed;
                                if preset.collapsed {
                                    preset.show_search_region_overlay = false;
                                }
                                live_sync = true;
                            }
                        });
                    });

                    if preset.collapsed {
                        if preset.show_search_region_overlay {
                            preset.show_search_region_overlay = false;
                            live_sync = true;
                        }
                        return;
                    }

                    egui::Grid::new((preset.id, "image-search-grid"))
                        .num_columns(2)
                        .spacing([14.0, 8.0])
                        .min_col_width(110.0)
                        .show(ui, |ui| {

                            if !preset.use_color_matching && !self.opencv_installed {
                                ui.label("");
                                ui.label(egui::RichText::new(Self::tr_lang(language, "[!] OpenCV library not installed! Check Settings.", "[!] OpenCV library not installed! Check Settings.")).color(egui::Color32::from_rgb(255, 110, 110)));
                                ui.end_row();
                            }

                            if !preset.use_color_matching {
                                ui.label(Self::tr_lang(language, "Template", "Template"));
                                ui.horizontal_wrapped(|ui| {
                                    if ui
                                        .button(Self::tr_lang(language, "Pick from screen", "Pick from screen"))
                                        .clicked()
                                    {
                                        start_image_search_capture = Some(preset.id);
                                    }
                                    if ui
                                        .button(Self::tr_lang(language, "Clear template", "Clear template"))
                                        .clicked()
                                    {
                                        let _ = fs::remove_file(&template_file);
                                        let _ = fs::remove_file(&self.paths.vision_template_file);
                                        self.vision_preview_cache.remove(&preset.id);
                                        preset.enabled = false;
                                        live_sync = true;
                                    }
                                });
                                ui.end_row();
                            }

                            ui.label(Self::tr_lang(language, "Area", "Area"));
                            ui.horizontal_wrapped(|ui| {
                                ui.monospace(Self::image_search_search_area_text(preset));

                                let mut is_single = preset.search_region_is_single_pixel;
                                if preset.use_color_matching && !preset.is_pixel_counter {
                                    if ui
                                        .checkbox(&mut is_single, Self::tr_lang(language, "1 pixel", "1 pixel"))
                                        .changed()
                                    {
                                        preset.search_region_is_single_pixel = is_single;
                                        if is_single {
                                            preset.search_region_width = Some(1);
                                            preset.search_region_height = Some(1);
                                        } else {
                                            preset.search_region_width = None;
                                            preset.search_region_height = None;
                                            preset.search_region_screen_x = None;
                                            preset.search_region_screen_y = None;
                                        }
                                        live_sync = true;
                                    }
                                }

                                if preset.search_region_is_single_pixel {
                                    if ui
                                        .button(Self::tr_lang(language, "Pick pixel", "Pick pixel"))
                                        .clicked()
                                    {
                                        start_single_pixel_capture = Some(preset.id);
                                    }
                                } else {
                                    if ui
                                        .button(Self::tr_lang(language, "Pick area", "Pick area"))
                                        .clicked()
                                    {
                                        start_search_region_capture = Some(preset.id);
                                    }
                                    // Show Adjust button when there is an existing region
                                    let has_region = preset.search_region_screen_x.is_some()
                                        && preset.search_region_width.is_some();
                                    if has_region {
                                        if ui
                                            .button(Self::tr_lang(language, "Adjust", "Adjust"))
                                            .on_hover_text(Self::tr_lang(
                                                language,
                                                "Move/resize the region directly on screen",
                                                "Move/resize the region directly on screen",
                                            ))
                                            .clicked()
                                        {
                                            start_adjust_region_capture = Some(preset.id);
                                        }
                                    }
                                }

                                if ui
                                    .button(Self::tr_lang(language, "Clear area", "Clear area"))
                                    .clicked()
                                {
                                    preset.search_region_screen_x = None;
                                    preset.search_region_screen_y = None;
                                    preset.search_region_width = None;
                                    preset.search_region_height = None;
                                    live_sync = true;
                                }

                                if !preset.search_region_is_single_pixel {
                                    let preview_active = preset.show_search_region_overlay;
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
                                        preset.show_search_region_overlay = !preset.show_search_region_overlay;
                                        live_sync = true;
                                    }
                                }
                            });
                            ui.end_row();

                            if preset.use_color_matching && !preset.search_region_is_single_pixel {
                                ui.label(Self::tr_lang(language, "Color", "Color"));
                                ui.vertical(|ui| {
                                    let colors = Self::image_search_target_colors(&preset);
                                    let uses_legacy_single_color = preset.target_colors.is_empty()
                                        && preset.target_color.is_some();
                                    if colors.is_empty() {
                                        ui.monospace("None");
                                    } else {
                                        let mut remove_color_index = None;
                                        egui::Grid::new((preset.id, "image-search-color-grid"))
                                            .num_columns(8)
                                            .min_col_width(0.0)
                                            .spacing([ui.spacing().item_spacing.x, 4.0])
                                            .show(ui, |ui| {
                                                for (index, color) in colors.iter().copied().enumerate()
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
                                        if let Some(index) = remove_color_index {
                                            if uses_legacy_single_color && index == 0 {
                                                preset.target_color = None;
                                                live_sync = true;
                                            } else if !preset.target_colors.is_empty() {
                                                preset.target_colors = preset
                                                    .target_colors
                                                    .iter()
                                                    .copied()
                                                    .enumerate()
                                                    .filter_map(|(i, item)| {
                                                        (i != index).then_some(item)
                                                    })
                                                    .collect();
                                                preset.target_color =
                                                    preset.target_colors.first().copied();
                                                live_sync = true;
                                            }
                                        }
                                    }
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        if Self::image_search_add_color_button(ui, language).clicked() {
                                            start_color_pick_capture = Some(preset.id);
                                        }

                                        let popup_id = ui.make_persistent_id((preset.id, "vision-manual-color-popup"));
                                        let mut popup_open = ui
                                            .ctx()
                                            .data(|data| data.get_temp::<bool>(popup_id))
                                            .unwrap_or(false);

                                        let manual_button = ui.add_sized(
                                            [24.0, 21.0],
                                            Button::new(Self::material_icon_text(0xe40a, 18.0)),
                                        )
                                        .on_hover_text(Self::tr_lang(language, "Manual color input", "Manual color input"));

                                        if manual_button.clicked() {
                                            popup_open = true;
                                        }

                                        let mut added_color = false;

                                        let popup_response = egui::Popup::from_response(&manual_button)
                                            .id(popup_id)
                                            .open_bool(&mut popup_open)
                                            .align(egui::RectAlign::BOTTOM_START)
                                            .layout(egui::Layout::top_down_justified(egui::Align::Min))
                                            .width(220.0)
                                            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                                            .show(|ui| {
                                                ui.set_min_width(220.0);
                                                ui.label(Self::tr_lang(language, "Manual color", "Manual color"));
                                                ui.separator();

                                                let mut color32 = egui::Color32::from_rgba_unmultiplied(
                                                    self.vision_manual_color.r,
                                                    self.vision_manual_color.g,
                                                    self.vision_manual_color.b,
                                                    self.vision_manual_color.a,
                                                );
                                                if egui::color_picker::color_picker_color32(
                                                    ui,
                                                    &mut color32,
                                                    egui::color_picker::Alpha::Opaque,
                                                ) {
                                                    self.vision_manual_color.r = color32.r();
                                                    self.vision_manual_color.g = color32.g();
                                                    self.vision_manual_color.b = color32.b();
                                                    self.vision_manual_color.a = color32.a();
                                                    self.vision_manual_color_hex = format!(
                                                        "{:02X}{:02X}{:02X}",
                                                        self.vision_manual_color.r,
                                                        self.vision_manual_color.g,
                                                        self.vision_manual_color.b
                                                    );
                                                }

                                                ui.add_space(4.0);

                                                ui.horizontal(|ui| {
                                                    ui.label("#");
                                                    let hex_resp = ui.add(
                                                        TextEdit::singleline(&mut self.vision_manual_color_hex)
                                                            .hint_text("RRGGBB")
                                                    );
                                                    if hex_resp.changed() {
                                                        let hex = self.vision_manual_color_hex.trim().trim_start_matches('#');
                                                        if hex.len() == 6 {
                                                            if let Ok(color_val) = u32::from_str_radix(hex, 16) {
                                                                self.vision_manual_color.r = ((color_val >> 16) & 0xFF) as u8;
                                                                self.vision_manual_color.g = ((color_val >> 8) & 0xFF) as u8;
                                                                self.vision_manual_color.b = (color_val & 0xFF) as u8;
                                                            }
                                                        }
                                                    }
                                                });

                                                ui.add_space(8.0);

                                                if ui.button(Self::tr_lang(language, "Add color", "Add color")).clicked() {
                                                    added_color = true;
                                                }
                                            });

                                        if added_color {
                                            if preset.target_colors.is_empty() {
                                                if let Some(existing) = preset.target_color {
                                                    preset.target_colors.push(existing);
                                                }
                                            }
                                            preset.target_colors.push(self.vision_manual_color);
                                            preset.target_color = preset.target_colors.first().copied();
                                            live_sync = true;
                                            popup_open = false;
                                        }

                                        if popup_open
                                            && let Some(pointer_pos) = ui.ctx().pointer_hover_pos()
                                        {
                                            let mut keep_open_rect = manual_button.rect.expand(10.0);
                                            if let Some(popup) = &popup_response {
                                                keep_open_rect =
                                                    keep_open_rect.union(popup.response.rect.expand(10.0));
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
                            }

                            if !preset.use_color_matching {
                                ui.label(Self::tr_lang(language, "Accuracy", "Accuracy"));
                                ui.horizontal_wrapped(|ui| {
                                    live_sync |= ui
                                        .add(
                                            Slider::new(&mut preset.confidence_threshold, 0.35..=0.99)
                                                .fixed_decimals(2)
                                                .show_value(true),
                                        )
                                        .changed();
                                });
                                ui.end_row();
                            }

                            if !preset.is_pixel_counter && !preset.search_region_is_single_pixel {
                                ui.label(Self::tr_lang(language, "Mouse", "Mouse"));
                                ui.horizontal_wrapped(|ui| {
                                    if Self::sized_button(
                                        ui,
                                        96.0,
                                        Self::tr_lang(
                                            language,
                                            if preset.image_search_move_advanced_open {
                                                "Hide"
                                            } else {
                                                "Show"
                                            },
                                            if preset.image_search_move_advanced_open {
                                                "Hide"
                                            } else {
                                                "Show"
                                            },
                                        ),
                                    )
                                    .clicked()
                                    {
                                        preset.image_search_move_advanced_open =
                                            !preset.image_search_move_advanced_open;
                                        live_sync = true;
                                    }
                                });
                                ui.end_row();
                            }

                            if !preset.is_pixel_counter && !preset.search_region_is_single_pixel && preset.image_search_move_advanced_open {
                                ui.horizontal(|ui| {
                                    ui.label(Self::tr_lang(language, "Offset", "Offset"));
                                    let help_btn = ui.small_button("❓");
                                    if help_btn.hovered() {
                                        egui::show_tooltip_text(
                                            ui.ctx(),
                                            ui.layer_id(),
                                            help_btn.id,
                                            Self::tr_lang(language, "Click offset from the center of the detected object (image or color).\nThe cursor will move to the center plus this offset (X, Y) before clicking.", "Click offset from the center of the detected object (image or color).\nThe cursor will move to the center plus this offset (X, Y) before clicking.")
                                        );
                                    }
                                });
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("X");
                                    live_sync |= ui
                                        .add(DragValue::new(&mut preset.move_offset_x).range(-5000..=5000))
                                        .changed();
                                    ui.label("Y");
                                    live_sync |= ui
                                        .add(DragValue::new(&mut preset.move_offset_y).range(-5000..=5000))
                                        .changed();
                                });
                                ui.end_row();

                                ui.label(Self::tr_lang(language, "Move timing", "Move timing"));
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(Self::tr_lang(language, "Passes", "Passes"));
                                    live_sync |= ui
                                        .add(
                                            DragValue::new(&mut preset.non_interception_move_passes)
                                                .range(1..=10),
                                        )
                                        .changed();
                                    ui.add_space(8.0);
                                    ui.label(Self::tr_lang(language, "Delay", "Delay"));
                                    live_sync |= ui
                                        .add(
                                            DragValue::new(&mut preset.non_interception_move_delay_ms)
                                                .range(0..=100)
                                                .suffix(" ms"),
                                        )
                                        .changed();
                                });
                                ui.end_row();
                            }

                            if preset.use_color_matching && !preset.is_pixel_counter && !preset.search_region_is_single_pixel {
                                ui.label(Self::tr_lang(language, "Color scan", "Color scan"));
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(Self::tr_lang(language, "Tolerance", "Tolerance"));
                                    live_sync |= ui
                                        .add(
                                            DragValue::new(&mut preset.color_tolerance)
                                                .range(0..=96),
                                        )
                                        .changed();
                                    ui.label(Self::tr_lang(language, "Rate", "Rate"));
                                    live_sync |= ui
                                        .add(
                                            DragValue::new(&mut preset.color_scan_rate_hz)
                                                .range(1..=2000)
                                                .suffix(" Hz"),
                                        )
                                        .changed();
                                });
                                ui.end_row();

                                ui.label(Self::tr_lang(language, "Scan condition", "Điều kiện quét"));
                                ui.horizontal_wrapped(|ui| {
                                    if ui
                                        .checkbox(
                                            &mut preset.require_connected_target_colors,
                                            Self::tr_lang(language, "Connected colors", "Các màu phải chạm nhau"),
                                        )
                                        .changed()
                                    {
                                        if preset.require_connected_target_colors {
                                            preset.dual_color_scan_midpoint = false;
                                            preset.color_scan_average_centroid = false;
                                        }
                                        live_sync = true;
                                    }
                                    if preset.target_colors.len() < 2 {
                                        ui.weak(Self::tr_lang(language, "Needs 2+ colors", "Cần 2 màu trở lên"));
                                    }
                                });
                                ui.end_row();

                                ui.label(Self::tr_lang(language, "Target position", "Vị trí di chuột"));
                                ui.horizontal_wrapped(|ui| {
                                    if ui
                                        .checkbox(
                                            &mut preset.color_scan_average_centroid,
                                            Self::tr_lang(language, "Average centroid", "Lấy điểm ở giữa các pixel"),
                                        )
                                        .changed()
                                    {
                                        if preset.color_scan_average_centroid {
                                            preset.require_connected_target_colors = false;
                                            preset.dual_color_scan_midpoint = false;
                                        }
                                        live_sync = true;
                                    }

                                    if ui
                                        .checkbox(
                                            &mut preset.dual_color_scan_midpoint,
                                            Self::tr_lang(language, "Midpoint between colors", "Trung điểm các màu"),
                                        )
                                        .changed()
                                    {
                                        if preset.dual_color_scan_midpoint {
                                            preset.require_connected_target_colors = false;
                                            preset.color_scan_average_centroid = false;
                                        }
                                        live_sync = true;
                                    }
                                });
                                ui.end_row();

                                ui.label(Self::tr_lang(language, "Scan priority", "Ưu tiên quét"));
                                ui.horizontal_wrapped(|ui| {
                                    live_sync |= ui
                                        .checkbox(
                                            &mut preset.color_priority_from_anchor,
                                            Self::tr_lang(language, "Near target point", "Gần điểm chỉ định"),
                                        )
                                        .changed();
                                    let anchor = preset
                                        .color_priority_anchor_screen_x
                                        .zip(preset.color_priority_anchor_screen_y);
                                    if let Some((x, y)) = anchor {
                                        ui.monospace(format!("{x}, {y}"));
                                        if ui
                                            .small_button(Self::tr_lang(language, "x", "x"))
                                            .on_hover_text(Self::tr_lang(language, "Clear priority point", "Clear priority point"))
                                            .clicked()
                                        {
                                            preset.color_priority_anchor_screen_x = None;
                                            preset.color_priority_anchor_screen_y = None;
                                            live_sync = true;
                                        }
                                    }
                                    if preset.color_priority_from_anchor
                                        && ui
                                            .button(Self::tr_lang(language, "Pick point", "Pick point"))
                                            .clicked()
                                    {
                                        start_color_priority_anchor_capture = Some(preset.id);
                                    }
                                });
                                ui.end_row();
                            }

                            if preset.is_pixel_counter {
                                ui.label(Self::tr_lang(language, "Color scan", "Color scan"));
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(Self::tr_lang(language, "Tolerance", "Tolerance"));
                                    live_sync |= ui
                                        .add(
                                            DragValue::new(&mut preset.color_tolerance)
                                                .range(0..=96),
                                        )
                                        .changed();
                                });
                                ui.end_row();
                            }

                            if preset.is_pixel_counter || (preset.use_color_matching && preset.search_region_is_single_pixel) {
                                ui.label(Self::tr_lang(language, "Variable", "Variable"));
                                ui.horizontal_wrapped(|ui| {
                                    let is_dark_theme = self.state.ui_theme == UiThemeMode::Dark;
                                    let hint_color = if is_dark_theme {
                                        Color32::from_rgba_unmultiplied(140, 140, 140, 150)
                                    } else {
                                        Color32::from_rgba_unmultiplied(100, 100, 100, 150)
                                    };
                                    let hint_text = if preset.is_pixel_counter {
                                        format!("count_var (e.g. pixel_count_{})", preset.id)
                                    } else {
                                        format!("color_var (e.g. color_code_{})", preset.id)
                                    };
                                    let text_edit = egui::TextEdit::singleline(&mut preset.pixel_counter_variable_name)
                                        .desired_width(120.0)
                                        .hint_text(RichText::new(hint_text).color(hint_color).weak());
                                    live_sync |= ui.add(text_edit).changed();
                                });
                                ui.end_row();
                            }

                        });

                    if let Some(preview) = preview.as_ref() {
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            let base_scale = (320.0 / preview.width.max(1) as f32)
                                .min(180.0 / preview.height.max(1) as f32)
                                .min(1.0);
                            let scale = base_scale / ctx.pixels_per_point().max(1.0);
                            let size =
                                vec2(preview.width as f32 * scale, preview.height as f32 * scale);
                            ui.image((preview.texture.id(), size));
                        });
                    }
                });

                if let Some((target, status)) = next_capture_local {
                    self.begin_capture(target, status);
                }
                if let Some(preset_id) = start_image_search_capture {
                    self.begin_image_search_capture(
                        ctx,
                        VisionCaptureTarget::Preset(preset_id),
                        VisionCaptureMode::Template,
                    );
                }
                if let Some(preset_id) = start_search_region_capture {
                    self.begin_region_capture(ctx, VisionCaptureTarget::Preset(preset_id));
                }
                if let Some(preset_id) = start_adjust_region_capture {
                    self.begin_image_search_capture(
                        ctx,
                        VisionCaptureTarget::Preset(preset_id),
                        VisionCaptureMode::RegionAdjust,
                    );
                }
                if let Some(preset_id) = start_color_pick_capture {
                    self.begin_color_pick_capture(ctx, VisionCaptureTarget::Preset(preset_id));
                }
                if let Some(preset_id) = start_color_priority_anchor_capture {
                    self.begin_image_search_capture(
                        ctx,
                        VisionCaptureTarget::Preset(preset_id),
                        VisionCaptureMode::ColorPriorityAnchor,
                    );
                }
                if let Some(preset_id) = start_single_pixel_capture {
                    self.begin_single_pixel_capture(ctx, VisionCaptureTarget::Preset(preset_id));
                }
                if cancel_active_capture_local {
                    self.cancel_capture();
                }
            }
        }

        if let Some(remove_id) = remove_id {
            if let Some(preset) = self
                .state
                .vision_presets
                .iter()
                .find(|preset| preset.id == remove_id)
            {
                let template_file = self.vision_template_file_for_preset(preset.id);
                let _ = fs::remove_file(&template_file);
            }
            self.vision_preview_cache.remove(&remove_id);
            self.state
                .vision_presets
                .retain(|preset| preset.id != remove_id);
            live_sync = true;
        }

        if let Some((target, status)) = next_capture_target {
            self.begin_capture(target, status);
        }
        if cancel_active_capture {
            self.cancel_capture();
        }
        if cancel_mouse_move_absolute_capture {
            self.cancel_mouse_move_absolute_capture(ctx);
        } else if let Some(target) = next_mouse_move_absolute_capture_target {
            self.begin_mouse_move_absolute_capture(ctx, target);
        }

        if live_sync {
            self.persist_vision_presets();
        }
    }

    pub(crate) fn render_image_search_capture_overlay(&mut self, _ctx: &egui::Context) -> bool {
        false
    }

    pub(crate) fn sync_vision_presets(&mut self) {
        let presets = self.state.vision_presets.clone();
        if !Self::update_synced_state(presets.clone(), &mut self.last_synced_vision_presets) {
            return;
        }
        let preset_ids = presets.iter().map(|preset| preset.id).collect::<Vec<_>>();
        let _ = self
            .overlay_tx
            .send(OverlayCommand::InvalidateVisionWaits(preset_ids));
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateVisionPresets(presets));
    }

    pub(crate) fn persist_vision_presets(&mut self) {
        self.persist_after_sync(Self::sync_vision_presets);
    }

    pub(crate) fn image_search_preview_for_preset(
        &mut self,
        ctx: &egui::Context,
        preset: &VisionPreset,
    ) -> Option<VisionPreviewView> {
        let file_path = self.vision_template_file_for_preset(preset.id);
        let metadata = fs::metadata(&file_path).ok();
        let modified = metadata.and_then(|meta| meta.modified().ok());
        if let Some(cache) = self.vision_preview_cache.get(&preset.id)
            && cache.source_path == file_path
            && cache.source_modified == modified
        {
            return Some(cache.view.clone());
        }

        let image = image::open(&file_path).ok()?.to_rgba8();
        let width = image.width() as usize;
        let height = image.height() as usize;
        let color_image = ColorImage::from_rgba_unmultiplied([width, height], image.as_raw());
        let file_name = file_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("template.png")
            .to_owned();

        let view = if let Some(cache) = self.vision_preview_cache.get_mut(&preset.id) {
            cache.view.texture.set(color_image, TextureOptions::NEAREST);
            cache.updated_at = Instant::now();
            cache.source_path = file_path.clone();
            cache.source_modified = modified;
            cache.view.file_name = file_name.clone();
            cache.view.width = width;
            cache.view.height = height;
            cache.view.clone()
        } else {
            let texture = ctx.load_texture(
                format!("image-search-preview-{}", preset.id),
                color_image,
                TextureOptions::NEAREST,
            );
            let view = VisionPreviewView {
                texture,
                file_name,
                width,
                height,
            };
            self.vision_preview_cache.insert(
                preset.id,
                VisionPreviewCache {
                    updated_at: Instant::now(),
                    source_path: file_path,
                    source_modified: modified,
                    view: view.clone(),
                },
            );
            view
        };
        Some(view)
    }

    pub(crate) fn image_search_search_area_text(preset: &VisionPreset) -> String {
        if preset.search_region_is_single_pixel {
            match (preset.search_region_screen_x, preset.search_region_screen_y) {
                (Some(x), Some(y)) => format!("Pixel {x}, {y}"),
                _ => "Not selected".to_owned(),
            }
        } else {
            match (
                preset.search_region_screen_x,
                preset.search_region_screen_y,
                preset.search_region_width,
                preset.search_region_height,
            ) {
                (Some(x), Some(y), Some(width), Some(height)) if width > 0 && height > 0 => {
                    let shape = if preset.search_region_is_circle {
                        "Circle"
                    } else {
                        "Rect"
                    };
                    format!("{shape} {x}, {y}  {width}x{height}")
                }
                _ => "Any screen".to_owned(),
            }
        }
    }

    pub(crate) fn image_search_target_color_text(preset: &VisionPreset) -> String {
        let colors = Self::image_search_target_colors(preset);
        match colors.as_slice() {
            [] => "None".to_owned(),
            [color] => format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b),
            [first, rest @ ..] => format!(
                "#{:02X}{:02X}{:02X} +{}",
                first.r,
                first.g,
                first.b,
                rest.len()
            ),
        }
    }

    pub(crate) fn image_search_target_colors(preset: &VisionPreset) -> Vec<RgbaColor> {
        if !preset.target_colors.is_empty() {
            return preset.target_colors.clone();
        }
        preset.target_color.into_iter().collect()
    }

    pub(crate) fn image_search_target_color_swatch(
        ui: &mut egui::Ui,
        color: Option<RgbaColor>,
        size: egui::Vec2,
    ) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let fill = color.map_or(Color32::from_rgba_premultiplied(42, 48, 56, 220), |color| {
            Color32::from_rgba_unmultiplied(color.r, color.g, color.b, 255)
        });
        ui.painter().rect_filled(rect, 4.0, fill);
        if color.is_none() {
            let inset = 4.0;
            ui.painter().line_segment(
                [
                    egui::pos2(rect.left() + inset, rect.bottom() - inset),
                    egui::pos2(rect.right() - inset, rect.top() + inset),
                ],
                egui::Stroke::new(1.5, Color32::from_rgb(210, 220, 232)),
            );
        }
        ui.painter().rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, Color32::from_rgb(160, 174, 196)),
            egui::StrokeKind::Outside,
        );
    }

    pub(crate) fn image_search_color_tile(ui: &mut egui::Ui, color: RgbaColor) -> egui::Response {
        ui.add_sized(
            [24.0, 21.0],
            Button::new("")
                .fill(Color32::from_rgba_unmultiplied(
                    color.r, color.g, color.b, 255,
                ))
                .stroke(egui::Stroke::new(1.0, Color32::from_rgb(160, 174, 196))),
        )
        .on_hover_text(format!(
            "#{:02X}{:02X}{:02X}  rgba({}, {}, {}, {})",
            color.r, color.g, color.b, color.r, color.g, color.b, color.a
        ))
    }

    pub(crate) fn image_search_add_color_button(
        ui: &mut egui::Ui,
        language: UiLanguage,
    ) -> egui::Response {
        ui.add_sized(
            [24.0, 21.0],
            Button::new(Self::material_icon_text(0xe145, 18.0)),
        )
        .on_hover_text(Self::tr_lang(language, "Pick color", "Pick color"))
    }

    pub(crate) fn update_image_search_cursor_preview(
        &mut self,
        ctx: &egui::Context,
        screen_x: i32,
        screen_y: i32,
        sample_size: i32,
    ) -> Option<RgbaColor> {
        let sample_size = sample_size.max(3) | 1;
        let half = sample_size / 2;
        let left = screen_x - half;
        let top = screen_y - half;
        let (width, height, rgba) = if let Some(ref frame) = self.captured_freeze_frame {
            let mut buf = vec![0u8; (sample_size * sample_size * 4) as usize];
            for dy in 0..sample_size {
                let sy = top + dy;
                let ry = sy - frame.screen_y;
                for dx in 0..sample_size {
                    let sx = left + dx;
                    let rx = sx - frame.screen_x;
                    let dst_idx = ((dy * sample_size + dx) * 4) as usize;
                    if rx >= 0 && rx < frame.width as i32 && ry >= 0 && ry < frame.height as i32 {
                        let src_idx = ((ry as usize * frame.width) + rx as usize) * 4;
                        if src_idx + 3 < frame.rgba.len() {
                            buf[dst_idx..dst_idx + 4]
                                .copy_from_slice(&frame.rgba[src_idx..src_idx + 4]);
                        }
                    }
                }
            }
            (sample_size as usize, sample_size as usize, buf)
        } else {
            let capture =
                window_list::capture_virtual_screen_region(left, top, sample_size, sample_size)?;
            (capture.width, capture.height, capture.rgba)
        };
        if rgba.len() < 4 {
            return None;
        }

        let center_index = (((height / 2) * width) + (width / 2)) * 4;
        if center_index + 3 >= rgba.len() {
            return None;
        }
        let sampled = RgbaColor {
            r: rgba[center_index],
            g: rgba[center_index + 1],
            b: rgba[center_index + 2],
            a: 255,
        };
        let color_image = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
        if let Some(texture) = self.vision_color_pick_texture.as_mut() {
            texture.set(color_image, TextureOptions::NEAREST);
        } else {
            self.vision_color_pick_texture = Some(ctx.load_texture(
                "image-search-color-pick-preview",
                color_image,
                TextureOptions::NEAREST,
            ));
        }
        self.vision_color_pick_preview_color = Some(sampled);
        Some(sampled)
    }

    pub(crate) fn image_search_preview_panel_rect(
        viewport_rect: egui::Rect,
        pointer: Option<egui::Pos2>,
        panel_size: egui::Vec2,
    ) -> egui::Rect {
        let margin = 18.0;
        let Some(pointer) = pointer else {
            return egui::Rect::from_min_size(
                viewport_rect.right_top() - vec2(panel_size.x + margin, -margin),
                panel_size,
            );
        };
        let candidates = [
            egui::Rect::from_min_size(
                viewport_rect.right_top() - vec2(panel_size.x + margin, -margin),
                panel_size,
            ),
            egui::Rect::from_min_size(viewport_rect.left_top() + vec2(margin, margin), panel_size),
            egui::Rect::from_min_size(
                viewport_rect.right_bottom() - vec2(panel_size.x + margin, panel_size.y + margin),
                panel_size,
            ),
            egui::Rect::from_min_size(
                viewport_rect.left_bottom() + vec2(margin, -(panel_size.y + margin)),
                panel_size,
            ),
        ];
        let pointer_safe_zone = egui::Rect::from_center_size(pointer, vec2(54.0, 54.0));
        candidates
            .into_iter()
            .find(|candidate| !candidate.intersects(pointer_safe_zone))
            .unwrap_or(candidates[0])
    }

    pub(crate) fn render_image_search_cursor_preview_panel(
        &self,
        painter: &egui::Painter,
        viewport_rect: egui::Rect,
        pointer: Option<egui::Pos2>,
        sampled_color: Option<RgbaColor>,
        screen_point: Option<(i32, i32)>,
    ) {
        let Some(texture) = self.vision_color_pick_texture.as_ref() else {
            return;
        };
        let panel_size = vec2(200.0, 232.0);
        let panel_rect = Self::image_search_preview_panel_rect(viewport_rect, pointer, panel_size);
        let content_left = panel_rect.min.x + 28.0;
        painter.rect_filled(panel_rect, 10.0, Color32::from_rgb(12, 18, 28));
        painter.rect_stroke(
            panel_rect,
            10.0,
            egui::Stroke::new(1.0, Color32::from_rgb(110, 156, 210)),
            egui::StrokeKind::Outside,
        );
        let preview_rect = egui::Rect::from_min_size(
            pos2(content_left, panel_rect.min.y + 12.0),
            vec2(144.0, 144.0),
        );
        painter.image(
            texture.id(),
            preview_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(
            preview_rect,
            6.0,
            egui::Stroke::new(1.0, Color32::from_rgb(146, 192, 248)),
            egui::StrokeKind::Outside,
        );
        let cell_size = preview_rect.width() / 17.0;
        let center_rect =
            egui::Rect::from_center_size(preview_rect.center(), vec2(cell_size, cell_size));
        painter.rect_stroke(
            center_rect,
            0.0,
            egui::Stroke::new(2.0, Color32::from_rgb(120, 220, 255)),
            egui::StrokeKind::Outside,
        );

        if let Some(color) = sampled_color.or(self.vision_color_pick_preview_color) {
            let swatch_rect = egui::Rect::from_min_size(
                pos2(content_left, panel_rect.min.y + 168.0),
                vec2(24.0, 24.0),
            );
            painter.rect_filled(
                swatch_rect,
                6.0,
                Color32::from_rgb(color.r, color.g, color.b),
            );
            painter.rect_stroke(
                swatch_rect,
                6.0,
                egui::Stroke::new(1.0, Color32::WHITE),
                egui::StrokeKind::Outside,
            );
            painter.text(
                swatch_rect.right_center() + vec2(10.0, -8.0),
                egui::Align2::LEFT_TOP,
                format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b),
                egui::FontId::proportional(15.0),
                Color32::WHITE,
            );
        }
        if let Some((screen_x, screen_y)) = screen_point {
            painter.text(
                pos2(content_left, panel_rect.min.y + 198.0),
                egui::Align2::LEFT_TOP,
                format!("X:{screen_x}  Y:{screen_y}"),
                egui::FontId::proportional(12.0),
                Color32::from_rgb(188, 206, 230),
            );
        }
        painter.text(
            pos2(content_left, panel_rect.min.y + 214.0),
            egui::Align2::LEFT_TOP,
            "Center pixel",
            egui::FontId::proportional(12.0),
            Color32::from_rgb(188, 206, 230),
        );
    }

    pub(crate) fn precise_image_search_capture_pointer(
        &self,
        ctx: &egui::Context,
    ) -> Option<egui::Pos2> {
        let mut point = POINT::default();
        unsafe {
            if GetCursorPos(&mut point).is_err() {
                return None;
            }
        }
        let scale = ctx.pixels_per_point().max(0.5);
        let viewport_min = ctx
            .input(|input| input.viewport().inner_rect.map(|viewport| viewport.min))
            .unwrap_or_else(|| {
                let (left, top, _width, _height) = window_list::virtual_screen_bounds();
                egui::pos2(left as f32 / scale, top as f32 / scale)
            });
        Some(egui::pos2(
            point.x as f32 / scale - viewport_min.x,
            point.y as f32 / scale - viewport_min.y,
        ))
    }

    pub(crate) fn current_screen_cursor_pos() -> Option<(i32, i32)> {
        let mut point = POINT::default();
        unsafe {
            GetCursorPos(&mut point)
                .is_ok()
                .then_some((point.x, point.y))
        }
    }

    pub(crate) fn vision_template_file_for_preset(&self, preset_id: u32) -> PathBuf {
        self.paths.vision_template_file_for(preset_id)
    }

    pub(crate) fn begin_image_search_capture(
        &mut self,
        ctx: &egui::Context,
        target: VisionCaptureTarget,
        mode: VisionCaptureMode,
    ) {
        if self.vision_capture_active || self.native_capture_in_progress {
            return;
        }

        self.vision_capture_target = Some(target);
        self.vision_capture_mode = Some(mode);
        self.vision_capture_active = true;
        self.native_capture_in_progress = true;

        // Hide main app window natively
        #[cfg(windows)]
        unsafe {
            if let Some(hwnd) = crate::overlay::find_app_ui_window_for_ui_thread() {
                use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(false));
        crate::overlay::wake_command_queue();

        let ui_tx = self.ui_tx.clone();
        let egui_ctx = ctx.clone();
        let is_template = matches!(mode, VisionCaptureMode::Template);
        let is_point_click = matches!(
            mode,
            VisionCaptureMode::ColorSample
                | VisionCaptureMode::ColorPriorityAnchor
                | VisionCaptureMode::SinglePixel
        );
        let is_region_adjust = matches!(mode, VisionCaptureMode::RegionAdjust);
        let use_natural_point_click_preview = matches!(
            target,
            VisionCaptureTarget::QuickActionsCoordinates
                | VisionCaptureTarget::QuickActionsColor
                | VisionCaptureTarget::QuickActionsKeyDisplayPosition
                | VisionCaptureTarget::PinPresetColor(_)
                | VisionCaptureTarget::PinPresetRegion(_)
                | VisionCaptureTarget::PinPresetSourceCrop(_)
                | VisionCaptureTarget::HudPresetRegion(_)
        );
        let vietnamese = self.state.ui_language == crate::model::UiLanguage::Vietnamese;

        // For RegionAdjust, capture existing region coords
        let existing_region = if is_region_adjust {
            if let VisionCaptureTarget::Preset(preset_id) = target {
                self.state
                    .vision_presets
                    .iter()
                    .find(|p| p.id == preset_id)
                    .and_then(|p| {
                        Some((
                            p.search_region_screen_x?,
                            p.search_region_screen_y?,
                            p.search_region_width?,
                            p.search_region_height?,
                        ))
                    })
            } else {
                None
            }
        } else {
            None
        };

        std::thread::spawn(move || {
            // Sleep to let OS process window hide
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Capture virtual screen bounds
            let (left, top, width, height) = crate::window_list::virtual_screen_bounds();
            let (result, capture_frame) = if let Some(capture) =
                crate::window_list::capture_virtual_screen_region(left, top, width, height)
            {
                let native_mode = if is_region_adjust {
                    let (ix, iy, iw, ih) = existing_region.unwrap_or_else(|| {
                        // Default to center 1/4 of screen
                        (left + width / 4, top + height / 4, width / 2, height / 2)
                    });
                    crate::overlay::native_capture::NativeCaptureMode::RegionAdjust {
                        initial_x: ix,
                        initial_y: iy,
                        initial_w: iw,
                        initial_h: ih,
                        vietnamese,
                    }
                } else if is_point_click {
                    crate::overlay::native_capture::NativeCaptureMode::PointClick {
                        vietnamese,
                        dim_background: !use_natural_point_click_preview,
                    }
                } else {
                    crate::overlay::native_capture::NativeCaptureMode::RegionSelect {
                        is_template,
                        vietnamese,
                    }
                };

                let res = crate::overlay::native_capture::run_capture_overlay(
                    capture.clone(),
                    left,
                    top,
                    width,
                    height,
                    native_mode,
                );
                (res, Some(capture))
            } else {
                (
                    crate::overlay::native_capture::NativeCaptureResult::Cancelled,
                    None,
                )
            };

            // Restore main app window natively
            #[cfg(windows)]
            unsafe {
                if let Some(hwnd) = crate::overlay::find_app_ui_window_for_ui_thread() {
                    use windows::Win32::UI::WindowsAndMessaging::{
                        SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
                    };
                    let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
                    let _ = SetForegroundWindow(hwnd);
                }
            }

            // Sleep a tiny bit to let OS display the window so winit event loop is active
            std::thread::sleep(std::time::Duration::from_millis(50));

            let _ = ui_tx.send(UiCommand::NativeVisionCaptureFinished {
                target,
                mode,
                result,
                capture_frame,
            });
            egui_ctx.request_repaint();
        });
    }

    pub(crate) fn begin_region_capture(
        &mut self,
        ctx: &egui::Context,
        target: VisionCaptureTarget,
    ) {
        self.begin_image_search_capture(ctx, target, VisionCaptureMode::SearchRegion);
    }

    pub(crate) fn begin_color_pick_capture(
        &mut self,
        ctx: &egui::Context,
        target: VisionCaptureTarget,
    ) {
        self.begin_image_search_capture(ctx, target, VisionCaptureMode::ColorSample);
    }

    pub(crate) fn begin_single_pixel_capture(
        &mut self,
        ctx: &egui::Context,
        target: VisionCaptureTarget,
    ) {
        self.begin_image_search_capture(ctx, target, VisionCaptureMode::SinglePixel);
    }

    pub(crate) fn handle_image_search_capture_mouse_down(
        &mut self,
        ctx: &egui::Context,
        screen_x: i32,
        screen_y: i32,
    ) {
        if !self.vision_capture_active {
            return;
        }
        match self
            .vision_capture_mode
            .unwrap_or(VisionCaptureMode::Template)
        {
            VisionCaptureMode::ColorSample
            | VisionCaptureMode::ColorPriorityAnchor
            | VisionCaptureMode::SinglePixel
            | VisionCaptureMode::RegionAdjust => {
                // Do nothing on mouse down, wait for mouse up to capture!
            }
            VisionCaptureMode::Template | VisionCaptureMode::SearchRegion => {
                self.vision_capture_anchor = Some(egui::pos2(screen_x as f32, screen_y as f32));
                self.vision_capture_current = Some(egui::pos2(screen_x as f32, screen_y as f32));
                self.vision_capture_screen_region_preview = Some((screen_x, screen_y, 1, 1));
                self.status = match self
                    .vision_capture_mode
                    .unwrap_or(VisionCaptureMode::Template)
                {
                    VisionCaptureMode::Template => {
                        format!("Selecting template at {screen_x}, {screen_y}.")
                    }
                    VisionCaptureMode::SearchRegion => {
                        format!("Selecting area at {screen_x}, {screen_y}.")
                    }
                    _ => unreachable!(),
                };
                ctx.request_repaint();
            }
        }
    }

    pub(crate) fn handle_image_search_capture_mouse_move(
        &mut self,
        ctx: &egui::Context,
        screen_x: i32,
        screen_y: i32,
    ) {
        if !self.vision_capture_active {
            return;
        }
        match self
            .vision_capture_mode
            .unwrap_or(VisionCaptureMode::Template)
        {
            VisionCaptureMode::Template | VisionCaptureMode::SearchRegion => {
                if let Some(anchor) = self.vision_capture_anchor {
                    self.vision_capture_current =
                        Some(egui::pos2(screen_x as f32, screen_y as f32));
                    let start_x = anchor.x.round() as i32;
                    let start_y = anchor.y.round() as i32;
                    let x = start_x.min(screen_x);
                    let y = start_y.min(screen_y);
                    let width = (start_x - screen_x).abs().max(1);
                    let height = (start_y - screen_y).abs().max(1);
                    self.vision_capture_screen_region_preview = Some((x, y, width, height));
                    self.status = format!("Selecting area {width}x{height} at {x}, {y}.");
                    ctx.request_repaint();
                }
            }
            VisionCaptureMode::ColorSample
            | VisionCaptureMode::ColorPriorityAnchor
            | VisionCaptureMode::SinglePixel
            | VisionCaptureMode::RegionAdjust => {}
        }
    }

    pub(crate) fn handle_image_search_capture_mouse_up(
        &mut self,
        ctx: &egui::Context,
        screen_x: i32,
        screen_y: i32,
    ) {
        if !self.vision_capture_active {
            return;
        }
        match self
            .vision_capture_mode
            .unwrap_or(VisionCaptureMode::Template)
        {
            VisionCaptureMode::Template | VisionCaptureMode::SearchRegion => {
                let Some(anchor) = self.vision_capture_anchor else {
                    self.cancel_image_search_capture(ctx);
                    return;
                };
                let start_x = anchor.x.round() as i32;
                let start_y = anchor.y.round() as i32;
                let x = start_x.min(screen_x);
                let y = start_y.min(screen_y);
                let width = (start_x - screen_x).abs();
                let height = (start_y - screen_y).abs();
                if width >= 2 && height >= 2 {
                    let Some(target) = self.vision_capture_target else {
                        self.cancel_image_search_capture(ctx);
                        self.status = "No image search preset is active.".to_owned();
                        return;
                    };
                    match target {
                        VisionCaptureTarget::Preset(preset_id) => {
                            let template_mode = matches!(
                                self.vision_capture_mode,
                                Some(VisionCaptureMode::Template)
                            );
                            self.finish_image_search_region_capture_command(
                                ctx,
                                preset_id,
                                template_mode,
                                x,
                                y,
                                width,
                                height,
                            );
                        }
                        VisionCaptureTarget::OcrPreset(preset_id) => {
                            self.finish_ocr_region_capture_command(
                                ctx, preset_id, x, y, width, height,
                            );
                        }
                        VisionCaptureTarget::OcrStepRegion {
                            group_id,
                            preset_id,
                            step_index,
                        } => {
                            self.finish_ocr_step_region_capture_command(
                                ctx, group_id, preset_id, step_index, x, y, width, height,
                            );
                        }
                        VisionCaptureTarget::GeometryColor => {
                            self.cancel_image_search_capture(ctx);
                            self.status =
                                "Geometry color picking does not support area captures.".to_owned();
                        }
                        VisionCaptureTarget::MacroStepGeometryColor { .. } => {
                            self.cancel_image_search_capture(ctx);
                            self.status =
                                "Geometry color picking does not support area captures.".to_owned();
                        }
                        VisionCaptureTarget::QuickActionsCoordinates
                        | VisionCaptureTarget::QuickActionsColor
                        | VisionCaptureTarget::QuickActionsKeyDisplayPosition
                        | VisionCaptureTarget::PinPresetColor(_)
                        | VisionCaptureTarget::PinPresetRegion(_)
                        | VisionCaptureTarget::PinPresetSourceCrop(_)
                        | VisionCaptureTarget::HudPresetRegion(_) => {
                            self.cancel_image_search_capture(ctx);
                        }
                    }
                } else {
                    self.cancel_image_search_capture(ctx);
                    self.status = "Image area capture cancelled.".to_owned();
                }
            }
            VisionCaptureMode::ColorSample
            | VisionCaptureMode::ColorPriorityAnchor
            | VisionCaptureMode::SinglePixel => {
                let Some(target) = self.vision_capture_target else {
                    self.cancel_image_search_capture(ctx);
                    self.status = "No image search preset is active.".to_owned();
                    return;
                };
                match target {
                    VisionCaptureTarget::Preset(preset_id) => {
                        if matches!(
                            self.vision_capture_mode,
                            Some(VisionCaptureMode::SinglePixel)
                        ) {
                            self.finish_image_search_single_pixel_capture_from_screen(
                                ctx, preset_id, screen_x, screen_y,
                            );
                        } else {
                            let priority_anchor = matches!(
                                self.vision_capture_mode,
                                Some(VisionCaptureMode::ColorPriorityAnchor)
                            );
                            self.finish_image_search_point_capture_command_from_screen(
                                ctx,
                                preset_id,
                                priority_anchor,
                                screen_x,
                                screen_y,
                            );
                        }
                    }
                    VisionCaptureTarget::OcrPreset(_) => {
                        self.cancel_image_search_capture(ctx);
                        self.status = "OCR presets do not support color picking.".to_owned();
                    }
                    VisionCaptureTarget::OcrStepRegion { .. } => {
                        self.cancel_image_search_capture(ctx);
                        self.status = "OCR steps do not support color picking.".to_owned();
                    }
                    VisionCaptureTarget::GeometryColor => {
                        self.finish_image_search_color_pick_from_screen(ctx, screen_x, screen_y);
                    }
                    VisionCaptureTarget::MacroStepGeometryColor { .. } => {
                        self.finish_image_search_color_pick_from_screen(ctx, screen_x, screen_y);
                    }
                    VisionCaptureTarget::QuickActionsCoordinates => {
                        self.clear_image_search_capture_state();
                        let copy_x = self.state.quick_actions_copy_x;
                        let copy_y = self.state.quick_actions_copy_y;
                        let mut parts = Vec::new();
                        if copy_x {
                            parts.push(screen_x.to_string());
                        }
                        if copy_y {
                            parts.push(screen_y.to_string());
                        }
                        let formatted = parts.join(", ");
                        if !formatted.is_empty() {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                let _ = clipboard.set_text(formatted.clone());
                            }
                            self.status = match self.state.ui_language {
                                crate::model::UiLanguage::Vietnamese => {
                                    format!("Da sao chep toa do vao clipboard: {}", formatted)
                                }
                                _ => format!("Coordinates copied to clipboard: {}", formatted),
                            };
                        } else {
                            self.status = match self.state.ui_language {
                                crate::model::UiLanguage::Vietnamese => {
                                    format!("Toa do da chon: X={}, Y={}", screen_x, screen_y)
                                }
                                _ => {
                                    format!("Coordinates captured: X={}, Y={}", screen_x, screen_y)
                                }
                            };
                        }
                    }
                    VisionCaptureTarget::QuickActionsColor
                    | VisionCaptureTarget::PinPresetColor(_) => {
                        self.finish_image_search_color_pick_from_screen(ctx, screen_x, screen_y);
                    }
                    VisionCaptureTarget::QuickActionsKeyDisplayPosition => {
                        self.clear_image_search_capture_state();
                        self.state.quick_key_display_x = screen_x;
                        self.state.quick_key_display_y = screen_y;
                        self.sync_quick_key_display_config();
                        self.persist();
                        self.status = match self.state.ui_language {
                            crate::model::UiLanguage::Vietnamese => {
                                format!(
                                    "Da dat vi tri hien thi phim: X={}, Y={}",
                                    screen_x, screen_y
                                )
                            }
                            _ => {
                                format!("Key display position set: X={}, Y={}", screen_x, screen_y)
                            }
                        };
                    }
                    VisionCaptureTarget::PinPresetRegion(_)
                    | VisionCaptureTarget::PinPresetSourceCrop(_)
                    | VisionCaptureTarget::HudPresetRegion(_) => {
                        self.cancel_image_search_capture(ctx);
                        self.status = "This target does not support point captures.".to_owned();
                    }
                }
            }
            VisionCaptureMode::RegionAdjust => {}
        }
    }

    pub(crate) fn spawn_image_search_point_capture(
        ui_tx: Sender<UiCommand>,
        ctx: egui::Context,
        target: VisionCaptureTarget,
        priority_anchor: bool,
    ) {
        if let VisionCaptureTarget::Preset(preset_id) = target {
            Self::spawn_image_search_point_capture_thread(ui_tx, ctx, preset_id, priority_anchor);
        }
    }

    pub(crate) fn spawn_image_search_point_capture_thread(
        ui_tx: Sender<UiCommand>,
        ctx: egui::Context,
        preset_id: u32,
        priority_anchor: bool,
    ) {
        std::thread::spawn(move || {
            let is_down = |vk: i32| unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
            while is_down(0x01) {
                if is_down(0x1B) {
                    let _ = ui_tx.send(UiCommand::VisionPointCaptureCancelled(
                        "Image point capture cancelled.".to_owned(),
                    ));
                    ctx.request_repaint();
                    return;
                }
                std::thread::sleep(Duration::from_millis(6));
            }
            loop {
                if is_down(0x1B) {
                    let _ = ui_tx.send(UiCommand::VisionPointCaptureCancelled(
                        "Image point capture cancelled.".to_owned(),
                    ));
                    break;
                }
                if is_down(0x01) {
                    let mut point = POINT::default();
                    let got_point = unsafe { GetCursorPos(&mut point).is_ok() };
                    if got_point {
                        let color = if priority_anchor {
                            None
                        } else {
                            window_list::capture_virtual_screen_region(point.x, point.y, 1, 1)
                                .and_then(|frame| {
                                    (frame.rgba.len() >= 4).then(|| RgbaColor {
                                        r: frame.rgba[0],
                                        g: frame.rgba[1],
                                        b: frame.rgba[2],
                                        a: 255,
                                    })
                                })
                        };
                        let _ = ui_tx.send(UiCommand::VisionPointCaptured {
                            preset_id,
                            priority_anchor,
                            screen_x: point.x,
                            screen_y: point.y,
                            color,
                        });
                    } else {
                        let _ = ui_tx.send(UiCommand::VisionPointCaptureCancelled(
                            "Failed to read the selected screen point.".to_owned(),
                        ));
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(6));
            }
            ctx.request_repaint();
        });
    }

    pub(crate) fn spawn_image_search_region_capture(
        ui_tx: Sender<UiCommand>,
        ctx: egui::Context,
        target: VisionCaptureTarget,
        template_mode: bool,
    ) {
        if let VisionCaptureTarget::Preset(preset_id) = target {
            Self::spawn_image_search_region_capture_thread(ui_tx, ctx, preset_id, template_mode);
        }
    }

    pub(crate) fn spawn_image_search_region_capture_thread(
        ui_tx: Sender<UiCommand>,
        ctx: egui::Context,
        preset_id: u32,
        template_mode: bool,
    ) {
        std::thread::spawn(move || {
            let is_down = |vk: i32| unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
            let mut origin: Option<(i32, i32)> = None;
            while is_down(0x01) {
                if is_down(0x1B) {
                    let _ = ui_tx.send(UiCommand::VisionPointCaptureCancelled(
                        "Image area capture cancelled.".to_owned(),
                    ));
                    ctx.request_repaint();
                    return;
                }
                std::thread::sleep(Duration::from_millis(6));
            }
            loop {
                if is_down(0x1B) {
                    let _ = ui_tx.send(UiCommand::VisionPointCaptureCancelled(
                        "Image area capture cancelled.".to_owned(),
                    ));
                    break;
                }
                let mut point = POINT::default();
                let got_point = unsafe { GetCursorPos(&mut point).is_ok() };
                if got_point {
                    if is_down(0x01) {
                        let start = origin.get_or_insert((point.x, point.y));
                        let x = start.0.min(point.x);
                        let y = start.1.min(point.y);
                        let width = (start.0 - point.x).abs().max(1);
                        let height = (start.1 - point.y).abs().max(1);
                        let _ = ui_tx.send(UiCommand::VisionRegionPreview {
                            screen_x: x,
                            screen_y: y,
                            width,
                            height,
                        });
                        ctx.request_repaint();
                    } else if let Some(start) = origin {
                        let x = start.0.min(point.x);
                        let y = start.1.min(point.y);
                        let width = (start.0 - point.x).abs();
                        let height = (start.1 - point.y).abs();
                        if width >= 2 && height >= 2 {
                            let _ = ui_tx.send(UiCommand::VisionRegionCaptured {
                                preset_id,
                                template_mode,
                                screen_x: x,
                                screen_y: y,
                                width,
                                height,
                            });
                        } else {
                            let _ = ui_tx.send(UiCommand::VisionPointCaptureCancelled(
                                "Image area capture cancelled.".to_owned(),
                            ));
                        }
                        ctx.request_repaint();
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(16));
            }
        });
    }

    pub(crate) fn cancel_image_search_capture(&mut self, ctx: &egui::Context) {
        if !self.vision_capture_active {
            return;
        }
        let mode = self
            .vision_capture_mode
            .unwrap_or(VisionCaptureMode::Template);
        self.clear_image_search_capture_state();
        self.restore_image_search_viewport(ctx);
        self.status = match mode {
            VisionCaptureMode::Template => "Image template capture cancelled.".to_owned(),
            VisionCaptureMode::SearchRegion => "Image search area capture cancelled.".to_owned(),
            VisionCaptureMode::RegionAdjust => "Image search region adjust cancelled.".to_owned(),
            VisionCaptureMode::ColorSample => "Image color pick cancelled.".to_owned(),
            VisionCaptureMode::ColorPriorityAnchor => {
                "Image priority point capture cancelled.".to_owned()
            }
            VisionCaptureMode::SinglePixel => "Single pixel capture cancelled.".to_owned(),
        };
        ctx.request_repaint();
    }

    pub(crate) fn vision_capture_target_name(&self, target: VisionCaptureTarget) -> Option<String> {
        match target {
            VisionCaptureTarget::Preset(preset_id) => Self::named_item_name_by_id(
                &self.state.vision_presets,
                Some(preset_id),
                |preset| preset.id,
                |preset| &preset.name,
            ),
            VisionCaptureTarget::OcrPreset(preset_id) => Self::named_item_name_by_id(
                &self.state.ocr_presets,
                Some(preset_id),
                |preset| preset.id,
                |preset| &preset.name,
            ),
            VisionCaptureTarget::GeometryColor => Some("Geometry Color".to_owned()),
            VisionCaptureTarget::OcrStepRegion { .. } => Some("Custom OCR".to_owned()),
            VisionCaptureTarget::MacroStepGeometryColor { .. } => {
                Some("Macro Step Geometry Color".to_owned())
            }
            VisionCaptureTarget::QuickActionsCoordinates => {
                Some("Quick Actions Coordinates".to_owned())
            }
            VisionCaptureTarget::QuickActionsColor => Some("Quick Actions Color".to_owned()),
            VisionCaptureTarget::QuickActionsKeyDisplayPosition => {
                Some("Quick Actions Key Display Position".to_owned())
            }
            VisionCaptureTarget::PinPresetColor(preset_id) => {
                Some(format!("Pin Preset Color #{preset_id}"))
            }
            VisionCaptureTarget::PinPresetRegion(preset_id) => {
                Some(format!("Pin Preset Region #{preset_id}"))
            }
            VisionCaptureTarget::PinPresetSourceCrop(preset_id) => {
                Some(format!("Pin Preset Source Crop #{preset_id}"))
            }
            VisionCaptureTarget::HudPresetRegion(preset_id) => {
                Some(format!("HUD Preset Region #{preset_id}"))
            }
        }
    }

    pub(crate) fn vision_capture_target_is_circle(&self, target: VisionCaptureTarget) -> bool {
        match target {
            VisionCaptureTarget::Preset(preset_id) => Self::vision_preset_by_id(
                &self.state.vision_presets,
                Some(preset_id),
            )
            .is_some_and(|preset| preset.search_region_is_circle),
            VisionCaptureTarget::GeometryColor => false,
            VisionCaptureTarget::OcrPreset(_) => false,
            VisionCaptureTarget::OcrStepRegion { .. } => false,
            VisionCaptureTarget::MacroStepGeometryColor { .. } => false,
            VisionCaptureTarget::QuickActionsCoordinates => false,
            VisionCaptureTarget::QuickActionsColor => false,
            VisionCaptureTarget::QuickActionsKeyDisplayPosition => false,
            VisionCaptureTarget::PinPresetColor(_) => false,
            VisionCaptureTarget::PinPresetRegion(_) => false,
            VisionCaptureTarget::PinPresetSourceCrop(_) => false,
            VisionCaptureTarget::HudPresetRegion(_) => false,
        }
    }

    pub(crate) fn restore_image_search_viewport(&mut self, ctx: &egui::Context) {
        if let Some(size) = self.vision_restore_inner_size.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
        if let Some(pos) = self.vision_restore_outer_pos.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
    }

    pub(crate) fn finish_image_search_capture(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let Some(target) = self.vision_capture_target else {
            self.cancel_image_search_capture(ctx);
            self.status = "No image search preset is active.".to_owned();
            return;
        };
        let mode = self
            .vision_capture_mode
            .unwrap_or(VisionCaptureMode::Template);

        self.clear_image_search_capture_state();
        match mode {
            VisionCaptureMode::Template => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(false));
                std::thread::sleep(Duration::from_millis(70));
                let capture =
                    self.capture_screen_region_from_rect(ctx, rect, ctx.pixels_per_point());
                self.restore_image_search_viewport(ctx);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(true));

                let Some(capture) = capture else {
                    self.status = "Failed to capture the selected screen area.".to_owned();
                    ctx.request_repaint();
                    return;
                };

                let (status, sync_required) = match target {
                    VisionCaptureTarget::Preset(preset_id) => {
                        let template_file = self.vision_template_file_for_preset(preset_id);
                        if let Some(parent) = template_file.parent() {
                            let _ = fs::create_dir_all(parent);
                        }
                        let save_result = image::save_buffer(
                            &template_file,
                            &capture.rgba,
                            capture.width as u32,
                            capture.height as u32,
                            image::ColorType::Rgba8,
                        );

                        (
                            self.finish_vision_template_capture(
                                preset_id,
                                &capture,
                                save_result,
                            ),
                            false,
                        )
                    }
                    VisionCaptureTarget::GeometryColor => (
                        "Geometry color picking does not support template captures.".to_owned(),
                        false,
                    ),
                    VisionCaptureTarget::OcrPreset(_) => (
                        "OCR presets do not support template captures.".to_owned(),
                        false,
                    ),
                    VisionCaptureTarget::OcrStepRegion { .. } => (
                        "OCR steps do not support template captures.".to_owned(),
                        false,
                    ),
                    VisionCaptureTarget::MacroStepGeometryColor { .. } => (
                        "Geometry color picking does not support template captures.".to_owned(),
                        false,
                    ),
                    VisionCaptureTarget::QuickActionsCoordinates
                    | VisionCaptureTarget::QuickActionsColor
                    | VisionCaptureTarget::QuickActionsKeyDisplayPosition
                    | VisionCaptureTarget::PinPresetColor(_)
                    | VisionCaptureTarget::PinPresetRegion(_)
                    | VisionCaptureTarget::PinPresetSourceCrop(_)
                    | VisionCaptureTarget::HudPresetRegion(_) => (
                        "Pin/HUD presets do not support template captures.".to_owned(),
                        false,
                    ),
                };
                if sync_required {
                    self.persist_vision_presets();
                }
                self.status = status;
                ctx.request_repaint();
            }
            VisionCaptureMode::SearchRegion => {
                let region = self.screen_region_from_rect(ctx, rect, ctx.pixels_per_point());
                self.restore_image_search_viewport(ctx);
                if let Some((screen_x, screen_y, width, height)) = region {
                    self.status =
                        self.apply_image_search_region(ctx, target, screen_x, screen_y, width, height);
                } else {
                    self.status = "Failed to save the selected search area.".to_owned();
                }
                ctx.request_repaint();
            }
            VisionCaptureMode::ColorSample => {
                let center = rect.center();
                self.finish_image_search_color_pick(ctx, center);
            }
            VisionCaptureMode::ColorPriorityAnchor => {
                let center = rect.center();
                self.finish_image_search_color_priority_anchor_pick(ctx, center);
            }
            VisionCaptureMode::SinglePixel => {
                let center = rect.center();
                let screen_x = center.x.round() as i32;
                let screen_y = center.y.round() as i32;
                if let Some(VisionCaptureTarget::Preset(preset_id)) = self.vision_capture_target {
                    self.finish_image_search_single_pixel_capture_from_screen(
                        ctx, preset_id, screen_x, screen_y,
                    );
                }
            }
            VisionCaptureMode::RegionAdjust => {}
        }
    }

    pub(crate) fn capture_screen_region_from_rect(
        &self,
        ctx: &egui::Context,
        rect: egui::Rect,
        pixels_per_point: f32,
    ) -> Option<window_list::ScreenCaptureFrame> {
        let (capture_left, capture_top, capture_width, capture_height) =
            self.screen_region_from_rect(ctx, rect, pixels_per_point)?;
        window_list::capture_virtual_screen_region(
            capture_left,
            capture_top,
            capture_width,
            capture_height,
        )
    }

    pub(crate) fn screen_point_from_pos(
        &self,
        ctx: &egui::Context,
        pos: egui::Pos2,
        pixels_per_point: f32,
    ) -> Option<(i32, i32)> {
        let (left, top, _width, _height) = window_list::virtual_screen_bounds();
        let scale = pixels_per_point.max(0.5);
        let viewport_origin = ctx
            .input(|input| input.viewport().inner_rect.map(|viewport| viewport.min))
            .unwrap_or_else(|| egui::pos2(left as f32 / scale, top as f32 / scale));
        Some((
            ((viewport_origin.x + pos.x) * scale).round() as i32,
            ((viewport_origin.y + pos.y) * scale).round() as i32,
        ))
    }

    pub(crate) fn screen_region_from_rect(
        &self,
        ctx: &egui::Context,
        rect: egui::Rect,
        pixels_per_point: f32,
    ) -> Option<(i32, i32, i32, i32)> {
        let (left, top, _width, _height) = window_list::virtual_screen_bounds();
        let min = rect.min;
        let max = rect.max;
        let scale = pixels_per_point.max(0.5);
        let viewport_origin = ctx
            .input(|input| input.viewport().inner_rect.map(|viewport| viewport.min))
            .unwrap_or_else(|| egui::pos2(left as f32 / scale, top as f32 / scale));
        let capture_left = ((viewport_origin.x + min.x) * scale).round() as i32;
        let capture_top = ((viewport_origin.y + min.y) * scale).round() as i32;
        let capture_width = ((max.x - min.x).abs() * scale).round().max(1.0) as i32;
        let capture_height = ((max.y - min.y).abs() * scale).round().max(1.0) as i32;
        Some((capture_left, capture_top, capture_width, capture_height))
    }

    pub(crate) fn clear_image_search_capture_state(&mut self) {
        self.vision_capture_active = false;
        self.vision_capture_target = None;
        self.vision_capture_mode = None;
        self.vision_capture_anchor = None;
        self.vision_capture_current = None;
        self.vision_capture_screen_region_preview = None;
        self.vision_color_pick_preview_color = None;
        self.captured_freeze_texture = None;
        self.captured_freeze_frame = None;
        self.set_image_search_capture_mouse_blocked(false, false);
    }

    pub(crate) fn set_image_search_capture_mouse_blocked(
        &self,
        blocked: bool,
        is_region_mode: bool,
    ) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetVisionCaptureMouseBlocked {
                blocked,
                is_region_mode,
            });
        crate::overlay::wake_command_queue();
    }

    pub(crate) fn restore_image_search_capture_window(&mut self, ctx: &egui::Context) {
        self.restore_image_search_viewport(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(true));
        crate::overlay::wake_command_queue();
    }

    pub(crate) fn finish_vision_template_capture(
        &mut self,
        preset_id: u32,
        capture: &window_list::ScreenCaptureFrame,
        save_result: image::ImageResult<()>,
    ) -> String {
        if let Some(preset) = self
            .state
            .vision_presets
            .iter_mut()
            .find(|preset| preset.id == preset_id)
        {
            preset.collapsed = false;
            preset.last_capture_screen_x = Some(capture.screen_x);
            preset.last_capture_screen_y = Some(capture.screen_y);
        }
        self.vision_preview_cache.remove(&preset_id);
        self.persist_vision_presets();
        match save_result {
            Ok(()) => format!(
                "Saved template {}x{} for preset #{}.",
                capture.width, capture.height, preset_id
            ),
            Err(error) => format!("Captured template but could not save it: {error}"),
        }
    }

    pub(crate) fn apply_image_search_region(
        &mut self,
        ctx: &egui::Context,
        target: VisionCaptureTarget,
        screen_x: i32,
        screen_y: i32,
        width: i32,
        height: i32,
    ) -> String {
        match target {
            VisionCaptureTarget::Preset(preset_id) => {
                if let Some(preset) = self
                    .state
                    .vision_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.collapsed = false;
                    preset.search_region_screen_x = Some(screen_x);
                    preset.search_region_screen_y = Some(screen_y);
                    preset.search_region_width = Some(width);
                    preset.search_region_height = Some(height);
                }
                self.persist_vision_presets();
                format!(
                    "Saved search area {}x{} at {}, {} for preset #{}.",
                    width, height, screen_x, screen_y, preset_id
                )
            }
            VisionCaptureTarget::OcrPreset(preset_id) => {
                if let Some(preset) = self
                    .state
                    .ocr_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.collapsed = false;
                    preset.x = screen_x;
                    preset.y = screen_y;
                    preset.width = width;
                    preset.height = height;
                }
                self.persist_ocr_presets();
                format!(
                    "Saved OCR area {}x{} at {}, {} for preset #{}.",
                    width, height, screen_x, screen_y, preset_id
                )
            }
            VisionCaptureTarget::OcrStepRegion {
                group_id,
                preset_id,
                step_index,
            } => {
                self.finish_ocr_step_region_capture_command(
                    ctx, group_id, preset_id, step_index, screen_x, screen_y, width, height,
                );
                format!(
                    "Saved Custom OCR area {}x{} at {}, {} for step.",
                    width, height, screen_x, screen_y
                )
            }
            VisionCaptureTarget::GeometryColor => {
                "Geometry color picking does not support search regions.".to_owned()
            }
            VisionCaptureTarget::MacroStepGeometryColor { .. } => {
                "Geometry color picking does not support search regions.".to_owned()
            }
            VisionCaptureTarget::QuickActionsCoordinates
            | VisionCaptureTarget::QuickActionsColor
            | VisionCaptureTarget::QuickActionsKeyDisplayPosition
            | VisionCaptureTarget::PinPresetColor(_)
            | VisionCaptureTarget::PinPresetRegion(_)
            | VisionCaptureTarget::PinPresetSourceCrop(_)
            | VisionCaptureTarget::HudPresetRegion(_) => {
                "This target does not support search regions.".to_owned()
            }
        }
    }

    pub(crate) fn apply_image_search_color_pick(
        &mut self,
        target: VisionCaptureTarget,
        color: RgbaColor,
    ) -> String {
        match target {
            VisionCaptureTarget::Preset(preset_id) => {
                if let Some(preset) = self
                    .state
                    .vision_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.collapsed = false;
                    preset.use_color_matching = true;
                    if preset.target_colors.is_empty()
                        && let Some(existing) = preset.target_color
                    {
                        preset.target_colors.push(existing);
                    }
                    preset.target_colors.push(color);
                    preset.target_color = preset.target_colors.first().copied();
                }
                self.sync_vision_presets();
                format!(
                    "Picked color #{:02X}{:02X}{:02X} for preset #{}.",
                    color.r, color.g, color.b, preset_id
                )
            }
            VisionCaptureTarget::OcrPreset(_) => {
                "OCR presets do not support color picking.".to_owned()
            }
            VisionCaptureTarget::OcrStepRegion { .. } => {
                "OCR steps do not support color picking.".to_owned()
            }
            VisionCaptureTarget::GeometryColor => {
                self.vision_manual_color = color;
                self.vision_manual_color_hex = format!(
                    "{:02X}{:02X}{:02X}{:02X}",
                    color.r, color.g, color.b, color.a
                );
                let mut applied = false;
                if let Some((preset_id, object_id, is_fill)) =
                    self.geometry_color_pick_target.take()
                {
                    if let Some(preset) = self
                        .state
                        .geometry_presets
                        .iter_mut()
                        .find(|preset| preset.id == preset_id)
                    {
                        if let Some(object) = preset
                            .objects
                            .iter_mut()
                            .find(|object| object.id == object_id)
                        {
                            if is_fill {
                                object.spec.fill_color = color;
                                object.spec.fill_color_expr =
                                    Self::geometry_color_expr_literal(color);
                            } else {
                                object.spec.stroke_color = color;
                                object.spec.stroke_color_expr =
                                    Self::geometry_color_expr_literal(color);
                            }
                            applied = true;
                        }
                    }
                    if applied {
                        if self.geometry_preview_target == Some((preset_id, object_id)) {
                            let preview_spec = self
                                .state
                                .geometry_presets
                                .iter()
                                .find(|preset| preset.id == preset_id)
                                .and_then(|preset| {
                                    preset.objects.iter().find(|object| object.id == object_id)
                                })
                                .map(|object| object.spec.clone());
                            self.sync_geometry_spec_preview(preview_spec);
                        }
                        self.sync_geometry_presets();
                    }
                }
                format!(
                    "Picked geometry color #{:02X}{:02X}{:02X}.",
                    color.r, color.g, color.b
                )
            }
            VisionCaptureTarget::MacroStepGeometryColor {
                group_id,
                preset_id,
                step_index,
                is_fill,
                is_hold_stop,
            } => {
                self.vision_manual_color = color;
                self.vision_manual_color_hex = format!(
                    "{:02X}{:02X}{:02X}{:02X}",
                    color.r, color.g, color.b, color.a
                );
                let step = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|g| g.id == group_id)
                    .and_then(|g| g.presets.iter_mut().find(|p| p.id == preset_id))
                    .and_then(|p| {
                        if is_hold_stop {
                            Some(&mut *p.hold_stop_step)
                        } else {
                            p.steps.get_mut(step_index)
                        }
                    });
                if let Some(step) = step {
                    if is_fill {
                        step.geometry_spec.fill_color = color;
                        step.geometry_spec.fill_color_expr =
                            Self::geometry_color_expr_literal(color);
                    } else {
                        step.geometry_spec.stroke_color = color;
                        step.geometry_spec.stroke_color_expr =
                            Self::geometry_color_expr_literal(color);
                    }
                    if self.draw_geometry_step_preview_target
                        == Some((group_id, preset_id, step_index, is_hold_stop))
                    {
                        let _ = self.overlay_tx.send(
                            crate::overlay::OverlayCommand::PreviewGeometrySpec(Some(
                                step.geometry_spec.clone(),
                            )),
                        );
                    }
                    self.sync_macro_presets();
                }
                format!(
                    "Picked geometry color #{:02X}{:02X}{:02X}.",
                    color.r, color.g, color.b
                )
            }
            VisionCaptureTarget::QuickActionsCoordinates => {
                "Quick Actions Coordinates do not support color picking.".to_owned()
            }
            VisionCaptureTarget::PinPresetColor(preset_id) => {
                if let Some(preset) = self
                    .state
                    .pin_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.add_binary_target_color(color);
                }
                self.sync_window_presets();
                format!(
                    "Picked target color #{:02X}{:02X}{:02X} for pin preset #{}.",
                    color.r, color.g, color.b, preset_id
                )
            }
            VisionCaptureTarget::QuickActionsColor => {
                let hex_str = format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b);
                if self.state.quick_actions_copy_color {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(hex_str.clone());
                    }
                    match self.state.ui_language {
                        crate::model::UiLanguage::Vietnamese => {
                            format!("Da sao chep ma mau vao clipboard: {}", hex_str)
                        }
                        _ => format!("Color code copied to clipboard: {}", hex_str),
                    }
                } else {
                    match self.state.ui_language {
                        crate::model::UiLanguage::Vietnamese => format!("Mau da chon: {}", hex_str),
                        _ => format!("Color captured: {}", hex_str),
                    }
                }
            }
            VisionCaptureTarget::QuickActionsKeyDisplayPosition => {
                "Key display position does not support color picking.".to_owned()
            }
            VisionCaptureTarget::PinPresetRegion(_)
            | VisionCaptureTarget::PinPresetSourceCrop(_)
            | VisionCaptureTarget::HudPresetRegion(_) => {
                "This target does not support color picking.".to_owned()
            }
        }
    }

    pub(crate) fn apply_image_search_priority_anchor(
        &mut self,
        target: VisionCaptureTarget,
        screen_x: i32,
        screen_y: i32,
    ) -> String {
        match target {
            VisionCaptureTarget::Preset(preset_id) => {
                if let Some(preset) = self
                    .state
                    .vision_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.color_priority_from_anchor = true;
                    preset.color_priority_anchor_screen_x = Some(screen_x);
                    preset.color_priority_anchor_screen_y = Some(screen_y);
                    preset.collapsed = false;
                }
                self.sync_vision_presets();
                format!("Saved priority point at {screen_x}, {screen_y} for preset #{preset_id}.")
            }
            VisionCaptureTarget::OcrPreset(_) => {
                "OCR presets do not support priority anchors.".to_owned()
            }
            VisionCaptureTarget::OcrStepRegion { .. } => {
                "OCR steps do not support priority anchors.".to_owned()
            }
            VisionCaptureTarget::GeometryColor => {
                "Geometry color picking does not support priority anchors.".to_owned()
            }
            VisionCaptureTarget::MacroStepGeometryColor { .. } => {
                "Geometry color picking does not support priority anchors.".to_owned()
            }
            VisionCaptureTarget::QuickActionsCoordinates
            | VisionCaptureTarget::QuickActionsColor
            | VisionCaptureTarget::QuickActionsKeyDisplayPosition
            | VisionCaptureTarget::PinPresetColor(_)
            | VisionCaptureTarget::PinPresetRegion(_)
            | VisionCaptureTarget::PinPresetSourceCrop(_)
            | VisionCaptureTarget::HudPresetRegion(_) => {
                "This target does not support priority anchors.".to_owned()
            }
        }
    }

    pub(crate) fn finish_image_search_point_capture_command(
        &mut self,
        ctx: &egui::Context,
        preset_id: u32,
        priority_anchor: bool,
        screen_x: i32,
        screen_y: i32,
        color: Option<RgbaColor>,
    ) {
        let target = VisionCaptureTarget::Preset(preset_id);
        self.clear_image_search_capture_state();
        self.restore_image_search_capture_window(ctx);
        self.status = if priority_anchor {
            self.apply_image_search_priority_anchor(target, screen_x, screen_y)
        } else if let Some(color) = color {
            self.apply_image_search_color_pick(target, color)
        } else {
            "Failed to sample the selected screen color.".to_owned()
        };
        self.persist();
        ctx.request_repaint();
    }

    pub(crate) fn finish_image_search_point_capture_command_from_screen(
        &mut self,
        ctx: &egui::Context,
        preset_id: u32,
        priority_anchor: bool,
        screen_x: i32,
        screen_y: i32,
    ) {
        let color = if priority_anchor {
            None
        } else {
            if let Some(ref frame) = self.captured_freeze_frame {
                let rx = screen_x - frame.screen_x;
                let ry = screen_y - frame.screen_y;
                if rx >= 0 && rx < frame.width as i32 && ry >= 0 && ry < frame.height as i32 {
                    let index = ((ry as usize * frame.width) + rx as usize) * 4;
                    if index + 3 < frame.rgba.len() {
                        Some(RgbaColor {
                            r: frame.rgba[index],
                            g: frame.rgba[index + 1],
                            b: frame.rgba[index + 2],
                            a: 255,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                window_list::capture_virtual_screen_region(screen_x, screen_y, 1, 1).and_then(|f| {
                    (f.rgba.len() >= 4).then(|| RgbaColor {
                        r: f.rgba[0],
                        g: f.rgba[1],
                        b: f.rgba[2],
                        a: 255,
                    })
                })
            }
        };

        self.clear_image_search_capture_state();
        self.restore_image_search_capture_window(ctx);

        self.finish_image_search_point_capture_command(
            ctx,
            preset_id,
            priority_anchor,
            screen_x,
            screen_y,
            color,
        );
    }

    pub(crate) fn finish_image_search_color_pick_from_screen(
        &mut self,
        ctx: &egui::Context,
        screen_x: i32,
        screen_y: i32,
    ) {
        let Some(target) = self.vision_capture_target else {
            self.cancel_image_search_capture(ctx);
            self.status = "No image search preset is active.".to_owned();
            return;
        };

        let color = if let Some(ref frame) = self.captured_freeze_frame {
            let rx = screen_x - frame.screen_x;
            let ry = screen_y - frame.screen_y;
            if rx >= 0 && rx < frame.width as i32 && ry >= 0 && ry < frame.height as i32 {
                let index = ((ry as usize * frame.width) + rx as usize) * 4;
                if index + 3 < frame.rgba.len() {
                    Some(RgbaColor {
                        r: frame.rgba[index],
                        g: frame.rgba[index + 1],
                        b: frame.rgba[index + 2],
                        a: 255,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            let capture = window_list::capture_virtual_screen_region(screen_x, screen_y, 1, 1);
            capture.and_then(|f| {
                (f.rgba.len() >= 4).then(|| RgbaColor {
                    r: f.rgba[0],
                    g: f.rgba[1],
                    b: f.rgba[2],
                    a: 255,
                })
            })
        };

        self.clear_image_search_capture_state();
        self.restore_image_search_capture_window(ctx);

        let Some(color) = color else {
            self.status = "Failed to sample the selected screen color.".to_owned();
            ctx.request_repaint();
            return;
        };

        let status = self.apply_image_search_color_pick(target, color);
        self.persist();
        self.status = status;
        ctx.request_repaint();
    }

    pub(crate) fn finish_image_search_color_priority_anchor_pick_from_screen(
        &mut self,
        ctx: &egui::Context,
        screen_x: i32,
        screen_y: i32,
    ) {
        let Some(target) = self.vision_capture_target else {
            self.cancel_image_search_capture(ctx);
            self.status = "No image search preset is active.".to_owned();
            return;
        };

        self.clear_image_search_capture_state();
        self.restore_image_search_capture_window(ctx);

        self.status = self.apply_image_search_priority_anchor(target, screen_x, screen_y);
        self.persist();
        ctx.request_repaint();
    }

    pub(crate) fn finish_image_search_single_pixel_capture_from_screen(
        &mut self,
        ctx: &egui::Context,
        preset_id: u32,
        screen_x: i32,
        screen_y: i32,
    ) {
        self.clear_image_search_capture_state();
        self.restore_image_search_capture_window(ctx);

        if let Some(preset) = self
            .state
            .vision_presets
            .iter_mut()
            .find(|p| p.id == preset_id)
        {
            preset.search_region_screen_x = Some(screen_x);
            preset.search_region_screen_y = Some(screen_y);
            preset.search_region_width = Some(1);
            preset.search_region_height = Some(1);
            self.status = format!("Selected pixel at {screen_x}, {screen_y}.");
        }
        self.persist();
        ctx.request_repaint();
    }

    pub(crate) fn finish_image_search_region_capture_command(
        &mut self,
        ctx: &egui::Context,
        preset_id: u32,
        template_mode: bool,
        screen_x: i32,
        screen_y: i32,
        width: i32,
        height: i32,
    ) {
        let target = VisionCaptureTarget::Preset(preset_id);
        self.clear_image_search_capture_state();

        if template_mode {
            let capture = if let Some(ref frame) = self.captured_freeze_frame {
                let rx = screen_x - frame.screen_x;
                let ry = screen_y - frame.screen_y;
                let mut cropped_rgba = vec![0u8; (width * height * 4) as usize];
                for dy in 0..height {
                    let sy = ry + dy;
                    if sy >= 0 && sy < frame.height as i32 {
                        let src_start = ((sy as usize * frame.width) + rx.max(0) as usize) * 4;
                        let dst_start = (dy as usize * width as usize) * 4;
                        let copy_len = (width as usize * 4)
                            .min(frame.width * 4 - src_start % (frame.width * 4));
                        if src_start + copy_len <= frame.rgba.len()
                            && dst_start + copy_len <= cropped_rgba.len()
                        {
                            cropped_rgba[dst_start..dst_start + copy_len]
                                .copy_from_slice(&frame.rgba[src_start..src_start + copy_len]);
                        }
                    }
                }
                self.restore_image_search_capture_window(ctx);
                Some(window_list::ScreenCaptureFrame {
                    screen_x,
                    screen_y,
                    width: width as usize,
                    height: height as usize,
                    rgba: cropped_rgba,
                })
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(false));
                std::thread::sleep(Duration::from_millis(35));
                let capture =
                    window_list::capture_virtual_screen_region(screen_x, screen_y, width, height);
                self.restore_image_search_capture_window(ctx);
                capture
            };

            let Some(capture) = capture else {
                self.status = "Failed to capture the selected screen area.".to_owned();
                ctx.request_repaint();
                return;
            };

            let template_file = self.vision_template_file_for_preset(preset_id);
            if let Some(parent) = template_file.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let save_result = image::save_buffer(
                &template_file,
                &capture.rgba,
                capture.width as u32,
                capture.height as u32,
                image::ColorType::Rgba8,
            );
            self.status = self.finish_vision_template_capture(preset_id, &capture, save_result);
            ctx.request_repaint();
            return;
        }

        self.restore_image_search_capture_window(ctx);
        self.status = self.apply_image_search_region(ctx, target, screen_x, screen_y, width, height);
        ctx.request_repaint();
    }

    pub(crate) fn finish_image_search_color_pick(&mut self, ctx: &egui::Context, pos: egui::Pos2) {
        let Some(target) = self.vision_capture_target else {
            self.cancel_image_search_capture(ctx);
            self.status = "No image search preset is active.".to_owned();
            return;
        };

        self.clear_image_search_capture_state();
        let screen_point = self.screen_point_from_pos(ctx, pos, ctx.pixels_per_point());
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(false));
        std::thread::sleep(Duration::from_millis(70));
        let capture = screen_point.and_then(|(screen_x, screen_y)| {
            window_list::capture_virtual_screen_region(screen_x, screen_y, 1, 1)
        });
        self.restore_image_search_capture_window(ctx);

        let Some(capture) = capture else {
            self.status = "Failed to sample the selected screen color.".to_owned();
            ctx.request_repaint();
            return;
        };
        if capture.rgba.len() < 4 {
            self.status = "Failed to read the selected screen color.".to_owned();
            ctx.request_repaint();
            return;
        }

        let color = RgbaColor {
            r: capture.rgba[0],
            g: capture.rgba[1],
            b: capture.rgba[2],
            a: 255,
        };
        let status = self.apply_image_search_color_pick(target, color);
        self.persist();
        self.status = status;
        ctx.request_repaint();
    }

    pub(crate) fn finish_image_search_color_priority_anchor_pick(
        &mut self,
        ctx: &egui::Context,
        pos: egui::Pos2,
    ) {
        let Some(target) = self.vision_capture_target else {
            self.cancel_image_search_capture(ctx);
            self.status = "No image search preset is active.".to_owned();
            return;
        };

        self.clear_image_search_capture_state();
        let screen_point = self.screen_point_from_pos(ctx, pos, ctx.pixels_per_point());
        self.restore_image_search_capture_window(ctx);

        let Some((screen_x, screen_y)) = screen_point else {
            self.status = "Failed to read the selected priority point.".to_owned();
            ctx.request_repaint();
            return;
        };

        self.status = self.apply_image_search_priority_anchor(target, screen_x, screen_y);
        self.persist();
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_selected_region_cleanup_is_only_delegated_to_shared_finishers() {
        assert!(CrosshairApp::native_selected_region_target_handles_cleanup(
            VisionCaptureTarget::Preset(1)
        ));
        assert!(CrosshairApp::native_selected_region_target_handles_cleanup(
            VisionCaptureTarget::OcrPreset(1)
        ));
        assert!(CrosshairApp::native_selected_region_target_handles_cleanup(
            VisionCaptureTarget::OcrStepRegion {
                group_id: 1,
                preset_id: 2,
                step_index: 3,
            }
        ));
        assert!(
            !CrosshairApp::native_selected_region_target_handles_cleanup(
                VisionCaptureTarget::PinPresetRegion(1)
            )
        );
        assert!(
            !CrosshairApp::native_selected_region_target_handles_cleanup(
                VisionCaptureTarget::PinPresetSourceCrop(1)
            )
        );
        assert!(
            !CrosshairApp::native_selected_region_target_handles_cleanup(
                VisionCaptureTarget::HudPresetRegion(1)
            )
        );
    }
}
