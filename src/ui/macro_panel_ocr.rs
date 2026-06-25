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
        let outputs_label = Self::tr_lang(language, "Outputs", "Outputs").to_owned();

        egui::ComboBox::from_id_salt((group_id, preset_id, step_index, "ocr-outputs"))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(110.0)
            .selected_text(outputs_label)
            .show_ui(ui, |ui| {
                ui.set_min_width(260.0);
                let has_target_text = !step.ocr_target_text.trim().is_empty();

                egui::Grid::new("ocr_outputs_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        let target_label =
                            ui.label(Self::tr_lang(language, "Target Text:", "Target Text:"));
                        target_label.on_hover_text(Self::tr_lang(
                            language,
                            "Only mark success when OCR finds this text",
                            "Only mark success when OCR finds this text",
                        ));
                        let target_id = ui.id().with((group_id, preset_id, step_index, "ocr-target-text"));
                        let target_resp = Self::render_variable_text_edit(
                            ui,
                            &mut step.ocr_target_text,
                            target_id,
                            120.0,
                            240.0,
                            18.0,
                            18.0,
                            &Self::tr_lang(language, "Target Text", "Target Text"),
                            false,
                        );
                        Self::apply_vietnamese_input_if_changed(
                            &target_resp,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut step.ocr_target_text,
                        );
                        *live_sync |= target_resp.changed();
                        ui.end_row();

                        let found_label =
                            ui.label(Self::tr_lang(language, "Found Var:", "Found Var:"));
                        found_label.on_hover_text(Self::tr_lang(
                            language,
                            "Assigns 1 if the target text was found (or if OCR succeeded when no target is set), 0 otherwise",
                            "Assigns 1 if the target text was found (or if OCR succeeded when no target is set), 0 otherwise",
                        ));
                        let found_resp = ui
                            .add_enabled(
                                has_target_text,
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
                            "Assigns the absolute X coordinate of the center of found text",
                        ));
                        let pos_x_resp = ui
                            .add_enabled(
                                has_target_text,
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
                            "Assigns the absolute Y coordinate of the center of found text",
                        ));
                        let pos_y_resp = ui
                            .add_enabled(
                                has_target_text,
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
                            "Stores ALL recognized text into this variable, regardless of the Target Text filter",
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
        current_ocr_download_language_code: Option<&str>,
        is_ocr_download_running: bool,
        pending_ocr_language_download: &mut Option<String>,
    ) {
        let ctrl_height = ui.spacing().interact_size.y;

        let pick_btn = egui::Button::new(Self::material_icon_text(0xe55f, 16.0));
        if ui
            .add_sized([ctrl_height, ctrl_height], pick_btn)
            .on_hover_text(Self::tr_lang(
                language,
                "Pick area - Drag on screen to select the OCR scan region",
                "Pick area - Drag on screen to select the OCR scan region",
            ))
            .clicked()
        {
            *pending_ocr_step_capture = Some((group_id, preset_id, step_index));
        }

        egui::ComboBox::from_id_salt((group_id, preset_id, step_index, "ocr-language-step"))
            .selected_text(crate::ocr::compact_label_for_language_code(&step.ocr_language).to_owned())
            .width(92.0)
            .show_ui(ui, |ui| {
                for pack in crate::ocr::ocr_language_packs() {
                    let installed = crate::ocr::is_language_pack_installed(pack.code);
                    let is_downloading = current_ocr_download_language_code == Some(pack.code)
                        && is_ocr_download_running;
                    let row = ui
                        .horizontal(|ui| {
                            let label_response = ui.add_sized(
                                [118.0, 0.0],
                                egui::Button::selectable(
                                    step.ocr_language == pack.code,
                                    crate::ocr::display_label_for_language_code(pack.code),
                                ),
                            );
                            if is_downloading {
                                ui.add_sized([58.0, 18.0], egui::Spinner::new());
                            } else if !installed && ui.small_button("Dl").clicked() {
                                *pending_ocr_language_download = Some(pack.code.to_owned());
                            }
                            label_response
                        })
                        .inner;
                    if row.clicked() {
                        step.ocr_language = pack.code.to_owned();
                        step.ocr_language =
                            crate::ocr::normalize_language_code(&step.ocr_language);
                        *live_sync = true;
                    }
                }
            });
    }
}
