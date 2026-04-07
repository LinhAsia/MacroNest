use crate::model::*;
use crate::ocr::{OcrResult, perform_ocr};
use crate::overlay::OverlayCommand;
use crate::ui::{CrosshairApp, VisionCaptureMode, VisionCaptureTarget};
use crate::window_list::capture_virtual_screen_region;
use eframe::egui::{self, Color32, RichText, Sense, vec2};

impl CrosshairApp {
    pub(crate) fn render_ocr_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        let vietnamese_input_enabled = self.state.vietnamese_input_enabled;
        let vietnamese_input_mode = self.state.vietnamese_input_mode;

        let ocr_test_running = self.state.ocr_test_running;
        let ocr_test_x = self.state.ocr_test_x;
        let ocr_test_y = self.state.ocr_test_y;
        let ocr_test_width = self.state.ocr_test_width;
        let ocr_test_height = self.state.ocr_test_height;
        let ocr_test_error = self.state.ocr_test_error.clone();
        let ocr_test_result = self.state.ocr_test_result.clone();

        ui.add_space(2.0);

        // Add OCR preset button
        ui.horizontal(|ui| {
            if ui
                .button(Self::tr_lang(
                    language,
                    "+ Add OCR preset",
                    "+ Thêm preset OCR",
                ))
                .clicked()
            {
                let mut id = 1;
                while self.state.ocr_presets.iter().any(|p| p.id == id) {
                    id += 1;
                }
                self.state.next_ocr_preset_id = (self
                    .state
                    .ocr_presets
                    .iter()
                    .map(|p| p.id)
                    .max()
                    .unwrap_or(0)
                    + 1)
                .max(id + 1);
                let preset = OcrPreset::new(id);
                self.state.ocr_presets.push(preset);
                self.sync_ocr_presets();
                self.persist();
            }
        });

        ui.add_space(8.0);

        let mut remove_id = None;
        let mut live_sync = false;
        let mut run_test_preset_id = None;
        let mut preview_toggled_preset_id = None;
        let mut start_ocr_capture_preset_id = None;
        let mut pending_ocr_language_settings: Option<(String, String)> = None;

        // Render card-based presets list
        for index in 0..self.state.ocr_presets.len() {
            let preset = &mut self.state.ocr_presets[index];
            preset.enabled = true; // Always enabled for macros
            ui.add_space(6.0);

            Self::show_preset_card(ui, preset.enabled, |ui| {
                ui.horizontal(|ui| {
                    // Editable Name
                    let name_width = Self::preset_header_name_width(ui);
                    let response = ui.add_sized(
                        [name_width, 21.0],
                        egui::TextEdit::singleline(&mut preset.name),
                    );
                    Self::apply_vietnamese_input_if_changed(
                        &response,
                        vietnamese_input_enabled,
                        vietnamese_input_mode,
                        &mut preset.name,
                    );
                    live_sync |= response.changed();

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Delete Button
                        if Self::sound_style_remove_button(ui).clicked() {
                            remove_id = Some(preset.id);
                        }
                        // Collapse Button
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
                            live_sync = true;
                        }
                    });
                });

                if preset.collapsed {
                    if preset.preview_enabled {
                        preset.preview_enabled = false;
                        live_sync = true;
                    }
                    return;
                }

                ui.add_space(4.0);

                egui::Grid::new((preset.id, "ocr-preset-grid"))
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .min_col_width(110.0)
                    .show(ui, |ui| {
                        // Language Code - Dropdown with popular languages
                        ui.label(Self::tr_lang(language, "Language", "Ngon ngu OCR"));
                        {
                            let avail_langs = crate::ocr::available_ocr_languages();

                            let current_code = preset.lang.clone().unwrap_or_default();
                            let current_label: String = crate::ocr::OCR_SUPPORTED_LANGUAGE_CATALOG.iter()
                                .find(|(code, _, _)| crate::ocr::language_tag_matches(&[current_code.clone()], code))
                                .map(|(_, label, _)| label.to_string())
                                .unwrap_or_else(|| {
                                    if current_code.is_empty() {
                                        "Auto Detect".to_string()
                                    } else {
                                        current_code.clone()
                                    }
                                });

                            ui.horizontal(|ui| {
                                let cb = egui::ComboBox::from_id_salt((preset.id, "ocr-lang-combo"))
                                    .selected_text(current_label.as_str())
                                    .width(200.0)
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(
                                                current_code.is_empty(),
                                                "Auto Detect",
                                            )
                                            .on_hover_text(
                                                "Use Windows OCR automatic language detection",
                                            )
                                            .clicked()
                                        {
                                            preset.lang = None;
                                            live_sync = true;
                                        }
                                        for (code, label, hint) in crate::ocr::OCR_SUPPORTED_LANGUAGE_CATALOG {
                                            let is_selected = crate::ocr::language_tag_matches(
                                                &[current_code.clone()],
                                                code,
                                            );
                                            let has_ocr =
                                                crate::ocr::language_tag_matches(&avail_langs, code);
                                            let display = if has_ocr {
                                                label.to_string()
                                            } else {
                                                format!("{} [not installed]", label)
                                            };
                                            let hover_msg = if has_ocr {
                                                hint.to_string()
                                            } else {
                                                format!(
                                                    "{} - Windows OCR for this language is not installed on this PC. Click to install it now.",
                                                    hint
                                                )
                                            };
                                            if ui.selectable_label(is_selected, &display)
                                                .on_hover_text(hover_msg)
                                                .clicked()
                                            {
                                                preset.lang = Some(code.to_string());
                                                if !has_ocr {
                                                    pending_ocr_language_settings =
                                                        Some((code.to_string(), label.to_string()));
                                                }
                                                live_sync = true;
                                            }
                                        }
                                    });
                                let _ = cb;
                            });
                        }
                        ui.end_row();

                        // Scan Region (X, Y, W, H)
                        ui.label(Self::tr_lang(language, "Scan Region (X, Y, W, H)", "Vùng quét OCR (X, Y, W, H)"));
                        ui.horizontal(|ui| {
                            let mut changed = false;
                            ui.label("X:");
                            changed |= ui.add(egui::DragValue::new(&mut preset.x).range(0..=10000)).changed();
                            ui.add_space(6.0);
                            ui.label("Y:");
                            changed |= ui.add(egui::DragValue::new(&mut preset.y).range(0..=10000)).changed();
                            ui.add_space(6.0);
                            ui.label("W:");
                            changed |= ui.add(egui::DragValue::new(&mut preset.width).range(10..=5000)).changed();
                            ui.add_space(6.0);
                            ui.label("H:");
                            changed |= ui.add(egui::DragValue::new(&mut preset.height).range(10..=5000)).changed();

                             ui.add_space(10.0);
                             if ui
                                 .button(Self::tr_lang(language, "Pick area", "Chọn khu vực"))
                                 .clicked()
                             {
                                 start_ocr_capture_preset_id = Some(preset.id);
                             }

                            if changed {
                                live_sync = true;
                            }
                        });
                        ui.end_row();

                        // Preview checkbox
                        ui.label(Self::tr_lang(language, "Preview", "Preview"));
                        let prev_resp = ui.checkbox(
                            &mut preset.preview_enabled,
                            Self::tr_lang(
                                language,
                                "Stream preview in editor",
                                "Stream preview trong editor",
                            ),
                        );
                        if prev_resp.changed() {
                            live_sync = true;
                            if preset.preview_enabled {
                                preview_toggled_preset_id = Some(preset.id);
                            }
                        }
                        ui.end_row();
                    });

                ui.add_space(8.0);

                // Region Editor Preview
                ui.label(
                    RichText::new(Self::tr_lang(
                        language,
                        "Visual Region Adjuster",
                        "Điều chỉnh Vùng quét trực quan",
                    ))
                    .strong(),
                );
                ui.add_space(4.0);

                // Re-using the premium rect editor to adjust X, Y, W, H visually
                let changed = Self::render_zoom_rect_editor(
                    ui,
                    ("ocr-rect-editor", preset.id),
                    "",
                    &mut preset.x,
                    &mut preset.y,
                    &mut preset.width,
                    &mut preset.height,
                    Self::screen_size(),
                    None,
                    None,
                    None,
                );

                if changed {
                    live_sync = true;
                }

                ui.add_space(12.0);

                // Trigger Button
                ui.horizontal(|ui| {
                    if ui
                        .button(
                            RichText::new(Self::tr_lang(
                                language,
                                "⚡ Test Capture and OCR Scan",
                                "⚡ Chụp và Quét thử OCR",
                            ))
                            .strong()
                            .color(Color32::from_rgb(0, 255, 170)),
                        )
                        .clicked()
                    {
                        run_test_preset_id = Some(preset.id);
                    }

                    if ocr_test_running {
                        ui.spinner();
                        ui.label(Self::tr_lang(language, "Scanning...", "Đang nhận diện..."));
                    }
                });

                // Display scan results specifically for this preset
                let matches_current = ocr_test_x == preset.x
                    && ocr_test_y == preset.y
                    && ocr_test_width == preset.width
                    && ocr_test_height == preset.height;

                if matches_current {
                    if let Some(ref err) = ocr_test_error {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("❌ Error: {err}"))
                                .color(Color32::from_rgb(255, 85, 85))
                                .strong(),
                        );
                    } else if let Some(ref res) = ocr_test_result {
                        ui.add_space(8.0);
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(Self::tr_lang(
                                        language,
                                        "Full Extracted Text:",
                                        "Toàn bộ đoạn chữ quét được:",
                                    ))
                                    .strong(),
                                );
                            });
                            ui.add_space(4.0);

                            let mut text_val_str = if res.text.trim().is_empty() {
                                Self::tr_lang(
                                    language,
                                    "[No text found in region]",
                                    "[Không tìm thấy chữ nào trong vùng quét]",
                                )
                                .to_string()
                            } else {
                                res.text.clone()
                            };

                            ui.text_edit_multiline(&mut text_val_str.as_str());

                            // Number parsing test helper
                            if !res.text.trim().is_empty() {
                                let mut parsed_numbers = Vec::new();
                                let mut current_num = String::new();
                                for c in res.text.chars() {
                                    if c.is_ascii_digit() {
                                        current_num.push(c);
                                    } else if !current_num.is_empty() {
                                        if let Ok(n) = current_num.parse::<i32>() {
                                            parsed_numbers.push(n);
                                        }
                                        current_num.clear();
                                    }
                                }
                                if !current_num.is_empty() {
                                    if let Ok(n) = current_num.parse::<i32>() {
                                        parsed_numbers.push(n);
                                    }
                                }

                                if !parsed_numbers.is_empty() {
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(Self::tr_lang(
                                                language,
                                                "💡 Extracted Numeric values:",
                                                "💡 Các số trích xuất được:",
                                            ))
                                            .strong()
                                            .color(Color32::from_rgb(255, 232, 96)),
                                        );
                                        ui.label(format!("{:?}", parsed_numbers));
                                    });
                                }
                            }
                        });

                        if !res.words.is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(Self::tr_lang(
                                    language,
                                    "Detailed Words Coordinates:",
                                    "Tọa độ chi tiết các từ:",
                                ))
                                .strong(),
                            );
                            ui.add_space(4.0);

                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    egui::Grid::new("ocr-test-words-grid")
                                        .num_columns(4)
                                        .spacing([16.0, 6.0])
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("Word").strong());
                                            ui.label(RichText::new("Pos X, Y (Relative)").strong());
                                            ui.label(RichText::new("Absolute on Screen").strong());
                                            ui.label(RichText::new("Size (W x H)").strong());
                                            ui.end_row();

                                            for word in &res.words {
                                                ui.label(
                                                    RichText::new(&word.text)
                                                        .color(Color32::from_rgb(0, 255, 170)),
                                                );
                                                ui.label(format!("{:.0}, {:.0}", word.x, word.y));

                                                // Calc absolute screen position
                                                let abs_x = preset.x as f32 + word.x;
                                                let abs_y = preset.y as f32 + word.y;
                                                ui.label(format!("{:.0}, {:.0}", abs_x, abs_y));

                                                ui.label(format!(
                                                    "{:.0}x{:.0}",
                                                    word.width, word.height
                                                ));
                                                ui.end_row();
                                            }
                                        });
                                });
                        }
                    }
                }
            });
        }

        if let Some(id) = remove_id {
            self.state.ocr_presets.retain(|preset| preset.id != id);
            live_sync = true;
        }

        // Mutual exclusivity of preset previews
        if let Some(current_id) = preview_toggled_preset_id {
            for other_preset in &mut self.state.ocr_presets {
                if other_preset.id != current_id {
                    other_preset.preview_enabled = false;
                }
            }
        }

        if let Some(preset_id) = run_test_preset_id {
            if let Some(preset) = self.state.ocr_presets.iter().find(|p| p.id == preset_id) {
                self.state.ocr_test_x = preset.x;
                self.state.ocr_test_y = preset.y;
                self.state.ocr_test_width = preset.width;
                self.state.ocr_test_height = preset.height;
                self.state.ocr_test_lang = preset.lang.clone();
            }
            self.run_ocr_test();
        }

        // Live preview sync
        self.sync_ocr_preview();

        if let Some(preset_id) = start_ocr_capture_preset_id {
            self.begin_image_search_capture(
                ui.ctx(),
                VisionCaptureTarget::OcrPreset(preset_id),
                VisionCaptureMode::SearchRegion,
            );
        }

        if let Some((lang_code, display_name)) = pending_ocr_language_settings.take() {
            self.install_ocr_language_capability(&lang_code, &display_name);
        }

        if live_sync {
            self.sync_ocr_presets();
            self.persist();
        }
    }

    pub(crate) fn sync_ocr_presets(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateOcrPresets(
            self.state.ocr_presets.clone(),
        ));
    }

    pub(crate) fn sync_ocr_preview(&mut self) {
        let preview_preset = self.state.ocr_presets.iter().find(|p| p.preview_enabled);
        if let Some(preset) = preview_preset {
            let hud = HudPreset {
                id: 900_000 + preset.id,
                name: preset.name.clone(),
                collapsed: false,
                preview_enabled: true,
                text: format!("OCR Zone: {}", preset.name),
                font_size: 16.0,
                background_opacity: 0.15,
                rounded_background: true,
                text_color: RgbaColor {
                    r: 0,
                    g: 255,
                    b: 170,
                    a: 255,
                },
                background_color: RgbaColor {
                    r: 0,
                    g: 255,
                    b: 170,
                    a: 30,
                },
                x: preset.x,
                y: preset.y,
                width: preset.width,
                height: preset.height,
            };
            let _ = self
                .overlay_tx
                .send(OverlayCommand::PreviewHudPreset(vec![hud]));
        } else {
            if self.state.active_panel == AppPanel::Ocr {
                let _ = self
                    .overlay_tx
                    .send(OverlayCommand::PreviewHudPreset(Vec::new()));
            }
        }
    }

    pub(crate) fn disable_ocr_preview_modes(&mut self) -> bool {
        let mut changed = false;
        for preset in &mut self.state.ocr_presets {
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }
        if changed {
            let _ = self
                .overlay_tx
                .send(OverlayCommand::PreviewHudPreset(Vec::new()));
        }
        changed
    }

    fn run_ocr_test(&mut self) {
        self.state.ocr_test_running = true;
        self.state.ocr_test_error = None;
        self.state.ocr_test_result = None;

        let x = self.state.ocr_test_x;
        let y = self.state.ocr_test_y;
        let w = self.state.ocr_test_width;
        let h = self.state.ocr_test_height;
        let lang = self.state.ocr_test_lang.clone().unwrap_or_default();

        // Capture virtual screen region
        if let Some(frame) = capture_virtual_screen_region(x, y, w, h) {
            match perform_ocr(&frame.rgba, frame.width as u32, frame.height as u32, &lang) {
                Ok(res) => {
                    self.state.ocr_test_result = Some(res);
                }
                Err(err) => {
                    self.state.ocr_test_error = Some(err.to_string());
                }
            }
        } else {
            self.state.ocr_test_error = Some("Failed to capture screen region.".to_string());
        }

        self.state.ocr_test_running = false;
        self.persist();
    }

    pub(crate) fn finish_ocr_region_capture_command(
        &mut self,
        ctx: &egui::Context,
        preset_id: u32,
        screen_x: i32,
        screen_y: i32,
        width: i32,
        height: i32,
    ) {
        self.clear_image_search_capture_state();
        self.restore_image_search_capture_window(ctx);
        if let Some(preset) = self
            .state
            .ocr_presets
            .iter_mut()
            .find(|p| p.id == preset_id)
        {
            preset.x = screen_x;
            preset.y = screen_y;
            preset.width = width;
            preset.height = height;
            preset.collapsed = false;
        }
        self.sync_ocr_presets();
        self.persist();
        self.status = format!(
            "Saved OCR region {}x{} at {}, {} for preset #{}.",
            width, height, screen_x, screen_y, preset_id
        );
        ctx.request_repaint();
    }
    pub(crate) fn finish_ocr_step_region_capture_command(
        &mut self,
        ctx: &egui::Context,
        group_id: u32,
        preset_id: u32,
        step_index: usize,
        screen_x: i32,
        screen_y: i32,
        width: i32,
        height: i32,
    ) {
        self.clear_image_search_capture_state();
        self.restore_image_search_capture_window(ctx);
        // Find the matching macro step and update its custom OCR region fields
        'outer: for group in &mut self.state.macro_groups {
            if group.id == group_id {
                for preset in &mut group.presets {
                    if preset.id == preset_id {
                        if let Some(step) = preset.steps.get_mut(step_index) {
                            step.x = screen_x;
                            step.y = screen_y;
                            step.ocr_width = width;
                            step.ocr_height = height;
                        }
                        break 'outer;
                    }
                }
            }
        }
        self.persist();
        self.status = format!(
            "Saved custom OCR region {}x{} at {}, {} for step.",
            width, height, screen_x, screen_y
        );
        ctx.request_repaint();
    }
}
