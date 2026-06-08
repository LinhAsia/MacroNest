use eframe::egui;

use crate::model::{MacroStep, UiLanguage, VietnameseInputMode};

use super::CrosshairApp;

impl CrosshairApp {
    pub(crate) fn render_ocr_outputs_selector(
        ui: &mut egui::Ui,
        language: UiLanguage,
        vietnamese_input_enabled: bool,
        vietnamese_input_mode: VietnameseInputMode,
        group_id: u32,
        preset_id: u32,
        step_index: usize,
        step: &mut MacroStep,
        live_sync: &mut bool,
    ) {
        let outputs_label = Self::tr_lang(language, "Outputs", "Đầu ra").to_owned();

        egui::ComboBox::from_id_salt((group_id, preset_id, step_index, "ocr-outputs"))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(110.0)
            .selected_text(outputs_label)
            .show_ui(ui, |ui| {
                ui.set_min_width(200.0);

                egui::Grid::new("ocr_outputs_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        let found_label = ui.label(Self::tr_lang(
                            language,
                            "Found Var:",
                            "Biến kết quả:",
                        ));
                        found_label.on_hover_text(Self::tr_lang(
                            language,
                            "Assigns 1 if the target text was found (or if OCR succeeded when no target is set), 0 otherwise",
                            "Gán 1 nếu tìm thấy từ khóa (hoặc nếu quét OCR thành công khi không đặt từ tìm), ngược lại là 0",
                        ));
                        let found_resp = ui.add(
                            egui::TextEdit::singleline(&mut step.ocr_success_var)
                                .hint_text("found_var"),
                        );
                        Self::apply_vietnamese_input_if_changed(
                            &found_resp,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut step.ocr_success_var,
                        );
                        *live_sync |= found_resp.changed();
                        ui.end_row();

                        let pos_x_label = ui.label("Pos X:");
                        pos_x_label.on_hover_text(Self::tr_lang(
                            language,
                            "Assigns the absolute X coordinate of the center of found text",
                            "Gán tọa độ X tuyệt đối ở chính giữa từ tìm thấy",
                        ));
                        let pos_x_resp =
                            ui.add(
                                egui::TextEdit::singleline(&mut step.ocr_pos_var_x)
                                    .hint_text("result_x_var"),
                            );
                        Self::apply_vietnamese_input_if_changed(
                            &pos_x_resp,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut step.ocr_pos_var_x,
                        );
                        *live_sync |= pos_x_resp.changed();
                        ui.end_row();

                        let pos_y_label = ui.label("Pos Y:");
                        pos_y_label.on_hover_text(Self::tr_lang(
                            language,
                            "Assigns the absolute Y coordinate of the center of found text",
                            "Gán tọa độ Y tuyệt đối ở chính giữa từ tìm thấy",
                        ));
                        let pos_y_resp =
                            ui.add(
                                egui::TextEdit::singleline(&mut step.ocr_pos_var_y)
                                    .hint_text("result_y_var"),
                            );
                        Self::apply_vietnamese_input_if_changed(
                            &pos_y_resp,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut step.ocr_pos_var_y,
                        );
                        *live_sync |= pos_y_resp.changed();
                        ui.end_row();

                        let text_var_label =
                            ui.label(Self::tr_lang(language, "Text Var:", "Text Var:"));
                        text_var_label.on_hover_text(Self::tr_lang(
                            language,
                            "Stores ALL recognized text into this variable, regardless of the Target Text filter",
                            "Stores all OCR text into this variable regardless of Target Text filter",
                        ));
                        let text_var_resp = ui.add(
                            egui::TextEdit::singleline(&mut step.ocr_text_var)
                                .hint_text("text_var"),
                        );
                        Self::apply_vietnamese_input_if_changed(
                            &text_var_resp,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut step.ocr_text_var,
                        );
                        *live_sync |= text_var_resp.changed();
                        ui.end_row();
                    });
            });
    }

    pub(crate) fn render_custom_ocr_inline_controls(
        ui: &mut egui::Ui,
        language: UiLanguage,
        vietnamese_input_enabled: bool,
        vietnamese_input_mode: VietnameseInputMode,
        group_id: u32,
        preset_id: u32,
        step_index: usize,
        step: &mut MacroStep,
        live_sync: &mut bool,
        pending_ocr_step_capture: &mut Option<(u32, u32, usize)>,
        pending_ocr_language_settings: &mut Option<(String, String)>,
    ) {
        let ctrl_height = ui.spacing().interact_size.y;

        let pick_btn = egui::Button::new("⛶");
        if ui
            .add_sized([ctrl_height, ctrl_height], pick_btn)
            .on_hover_text(Self::tr_lang(
                language,
                "Pick area - Drag on screen to select the OCR scan region",
                "Chọn vùng - Kéo trên màn hình để chọn vùng quét OCR",
            ))
            .clicked()
        {
            *pending_ocr_step_capture = Some((group_id, preset_id, step_index));
        }

        let available_languages = crate::ocr::available_ocr_languages();
        let current_language = step.ocr_lang.clone().unwrap_or_default();
        let language_label = crate::ocr::OCR_SUPPORTED_LANGUAGE_CATALOG
            .iter()
            .find(|(code, _, _)| {
                crate::ocr::language_tag_matches(&[current_language.clone()], code)
            })
            .map(|(_, label, _)| *label)
            .unwrap_or(if current_language.is_empty() {
                "Auto"
            } else {
                current_language.as_str()
            });

        let short_label = match current_language.as_str() {
            "" => "Auto",
            "en" | "en-US" => "EN",
            "zh-Hans" | "zh-CN" => "ZH",
            "zh-Hant" | "zh-HK" | "zh-TW" => "ZHT",
            "ja" | "ja-JP" => "JA",
            "ko" | "ko-KR" => "KO",
            "fr" | "fr-FR" | "fr-CA" => "FR",
            "de" | "de-DE" => "DE",
            "es" | "es-ES" | "es-MX" => "ES",
            "ru" | "ru-RU" => "RU",
            other => {
                if other.starts_with("zh-Han") {
                    "ZH"
                } else if let Some(idx) = other.find('-') {
                    &other[..idx]
                } else {
                    other
                }
            }
        };

        let combo_resp = egui::ComboBox::from_id_salt((group_id, preset_id, step_index, "ocr-step-lang"))
            .width(56.0)
            .selected_text(short_label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current_language.is_empty(), "Auto")
                    .on_hover_text("Use Windows OCR automatic language detection")
                    .clicked()
                {
                    step.ocr_lang = None;
                    *live_sync = true;
                }
                for (code, label, hint) in crate::ocr::OCR_SUPPORTED_LANGUAGE_CATALOG {
                    let is_selected = crate::ocr::language_tag_matches(
                        &[current_language.clone()],
                        code,
                    );
                    let has_ocr = crate::ocr::language_tag_matches(&available_languages, code);

                    let display = if has_ocr {
                        label.to_string()
                    } else {
                        format!("{} [not installed]", label)
                    };

                    let response = ui.selectable_label(is_selected, &display);
                    let hover_message = if has_ocr {
                        hint.to_string()
                    } else {
                        format!(
                            "{} - Windows OCR for this language is not installed on this PC. Click to install it now.",
                            hint
                        )
                    };

                    if response.on_hover_text(hover_message).clicked() {
                        step.ocr_lang = Some(code.to_string());
                        if !has_ocr {
                            *pending_ocr_language_settings =
                                Some((code.to_string(), label.to_string()));
                        }
                        *live_sync = true;
                    }
                }
            });

        combo_resp.response.on_hover_text(format!(
            "{}: {}",
            Self::tr_lang(language, "Language", "Ngôn ngữ"),
            language_label
        ));

        let target_id = ui.id().with((step_index, "ocr-target-text"));
        let target_resp = Self::render_variable_text_edit(
            ui,
            &mut step.ocr_target_text,
            target_id,
            120.0,
            240.0,
            18.0,
            18.0,
            &Self::tr_lang(language, "Target Text", "Van ban can tim"),
            false,
        );

        Self::apply_vietnamese_input_if_changed(
            &target_resp,
            vietnamese_input_enabled,
            vietnamese_input_mode,
            &mut step.ocr_target_text,
        );
        *live_sync |= target_resp.changed();
    }
}
