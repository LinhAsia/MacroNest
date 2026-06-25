use anyhow::{Result, bail};
use crate::model::*;
use crate::overlay::UiCommand;
use crate::ui::{CrosshairApp, UpdateStatus};
use eframe::egui::{
    self, Button, Color32, Frame, Margin, Order, RichText, Shadow, Stroke, TextEdit, WidgetText,
    vec2,
};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl CrosshairApp {
    pub(crate) fn render_settings_popup(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        egui::ScrollArea::vertical()
            .max_height(ui.available_height())
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    let mut groq_changed = false;
                    Self::settings_card_frame(ui).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.vertical(|ui| {
                            let api_header = Self::settings_section_button(
                                ui,
                                RichText::new("API (Groq / OpenAI)").strong().size(14.0),
                                self.state.groq_settings.details_open,
                            );
                            if api_header.clicked() {
                                self.state.groq_settings.details_open =
                                    !self.state.groq_settings.details_open;
                            }
                            if self.state.groq_settings.details_open {
                                ui.add_space(8.0);
                                let action_width = 104.0;
                                egui::Grid::new("api-settings-grid")
                                    .num_columns(3)
                                    .min_col_width(0.0)
                                    .spacing([12.0, 8.0])
                                    .show(ui, |ui| {
                                        ui.label("API Key");
                                        let key_editor = TextEdit::singleline(
                                            &mut self.state.groq_settings.api_key,
                                        )
                                        .hint_text("gsk_...");
                                        let response = ui.add_sized(
                                            [280.0, 24.0],
                                            if self.state.groq_settings.show_api_key {
                                                key_editor
                                            } else {
                                                key_editor.password(true)
                                            },
                                        );
                                        if self.focus_groq_api_key_pending {
                                            response.request_focus();
                                            self.focus_groq_api_key_pending = false;
                                        }
                                        Self::apply_vietnamese_input_if_changed(
                                            &response,
                                            self.state.vietnamese_input_enabled,
                                            self.state.vietnamese_input_mode,
                                            &mut self.state.groq_settings.api_key,
                                        );
                                        groq_changed |= response.changed();
                                        if Self::settings_action_button_fixed(
                                            ui,
                                            if self.state.groq_settings.show_api_key {
                                                Self::tr_lang(language, "Hide", "")
                                            } else {
                                                Self::tr_lang(language, "Show", "")
                                            },
                                            action_width,
                                        )
                                        .clicked()
                                        {
                                            self.state.groq_settings.show_api_key =
                                                !self.state.groq_settings.show_api_key;
                                            groq_changed = true;
                                        }
                                        ui.end_row();

                                        ui.label("Model");
                                        let selected_text = {
                                            let model = self.state.groq_settings.model.trim();
                                            Self::groq_model_catalog()
                                                .iter()
                                                .find(|(_, model_id)| {
                                                    model_id.eq_ignore_ascii_case(model)
                                                })
                                                .map(|(label, model_id)| {
                                                    format!("{label} ({model_id})")
                                                })
                                                .unwrap_or_else(|| model.to_owned())
                                        };
                                        egui::ComboBox::from_id_salt("groq-model-picker")
                                            .selected_text(selected_text)
                                            .width(280.0)
                                            .show_ui(ui, |ui| {
                                                for (label, model_id) in Self::groq_model_catalog() {
                                                    let selected = self
                                                        .state
                                                        .groq_settings
                                                        .model
                                                        .trim()
                                                        .eq(*model_id);
                                                    if ui
                                                        .selectable_label(
                                                            selected,
                                                            format!("{label} ({model_id})"),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.state.groq_settings.model =
                                                            (*model_id).to_owned();
                                                        groq_changed = true;
                                                        ui.close();
                                                    }
                                                }
                                            });
                                        ui.add_space(action_width);
                                        ui.end_row();

                                        ui.label("");
                                        let response = ui.add_sized(
                                            [280.0, 24.0],
                                            TextEdit::singleline(&mut self.state.groq_settings.model)
                                                .hint_text("openai/gpt-oss-120b"),
                                        );
                                        Self::apply_vietnamese_input_if_changed(
                                            &response,
                                            self.state.vietnamese_input_enabled,
                                            self.state.vietnamese_input_mode,
                                            &mut self.state.groq_settings.model,
                                        );
                                        groq_changed |= response.changed();
                                        if Self::settings_action_button_fixed(
                                            ui,
                                            "Get API key",
                                            action_width,
                                        )
                                        .clicked()
                                        {
                                            let _ = crate::platform::open_url_in_browser(
                                                "https://console.groq.com/keys",
                                            );
                                        }
                                        ui.end_row();
                                    });
                            }
                        });
                    });
                    if groq_changed {
                        self.sync_groq_settings();
                        self.persist();
                    }

                    ui.add_space(12.0);
                    Self::settings_card_frame(ui).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(Self::tr_lang(language, "Vietnamese input", ""))
                                    .strong()
                                    .size(14.0),
                            );
                            ui.add_space(8.0);
                            let mut vietnamese_input_changed = false;
                            ui.horizontal(|ui| {
                                vietnamese_input_changed |= ui
                                    .radio_value(
                                        &mut self.state.vietnamese_input_mode,
                                        VietnameseInputMode::Telex,
                                        "Telex",
                                    )
                                    .changed();
                                ui.add_space(12.0);
                                vietnamese_input_changed |= ui
                                    .radio_value(
                                        &mut self.state.vietnamese_input_mode,
                                        VietnameseInputMode::Vni,
                                        "VNI",
                                    )
                                    .changed();
                            });
                            if vietnamese_input_changed {
                                self.persist();
                            }
                        });
                    });
                    ui.add_space(12.0);
                    Self::settings_card_frame(ui).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(Self::tr_lang(language, "App data", ""))
                                    .strong()
                                    .size(14.0),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if Self::settings_action_button(
                                    ui,
                                    Self::tr_lang(language, "Open data folder", ""),
                                )
                                .clicked()
                                {
                                    self.open_app_data_folder();
                                }
                                ui.add_space(6.0);
                                let is_copied = self
                                    .copy_folder_feedback_until
                                    .map(|until| Instant::now() < until)
                                    .unwrap_or(false);

                                let btn_label = if is_copied {
                                    Self::tr_lang(language, "Copied!", "")
                                } else {
                                    Self::tr_lang(language, "Copy folder", "")
                                };

                                if is_copied {
                                    ui.ctx().request_repaint_after(Duration::from_millis(200));
                                }

                                if Self::settings_action_button(ui, btn_label).clicked() {
                                    if let Err(e) =
                                        crate::platform::copy_folder_to_clipboard(&self.paths.root)
                                    {
                                        self.status = format!("Failed to copy folder: {e}");
                                    } else {
                                        self.status = Self::tr_lang(
                                            language,
                                            "Folder copied to clipboard.",
                                            "",
                                        )
                                        .to_owned();
                                        self.copy_folder_feedback_until =
                                            Some(Instant::now() + Duration::from_secs(2));
                                    }
                                }
                            });
                        });
                    });
                    ui.add_space(12.0);
                    self.render_advanced_settings(ui);
                    ui.add_space(12.0);
                    self.render_downloaded_tools_settings(ui);
                    ui.add_space(12.0);
                    let ctx_clone = ui.ctx().clone();
                    self.render_update_settings(ui, &ctx_clone);
                    ui.add_space(8.0);
                });
            });
    }

    pub(crate) fn render_advanced_settings(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        Self::settings_card_frame(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical(|ui| {
                let header_text = RichText::new(Self::tr_lang(language, "Advanced", ""))
                    .strong()
                    .size(14.0);
                if Self::settings_section_button(ui, header_text, self.advanced_settings_open).clicked() {
                    self.advanced_settings_open = !self.advanced_settings_open;
                }

                if self.advanced_settings_open {
                    ui.add_space(8.0);
                    let explanation_en = "Note: Some games might not register inputs if the delays are set too low (e.g., 0ms). You can adjust these values if your macros do not work correctly in-game.";
                    let explanation_vi = "";
                    ui.label(
                        RichText::new(Self::tr_lang(language, explanation_en, explanation_vi))
                            .small()
                            .weak(),
                    );
                    ui.add_space(8.0);

                    let mut delay_changed = false;
                    let slider_track_fill = if ui.visuals().dark_mode {
                        Color32::from_rgb(64, 78, 98)
                    } else {
                        Color32::from_rgb(204, 214, 226)
                    };
                    let slider_track_stroke = if ui.visuals().dark_mode {
                        Color32::from_rgb(102, 122, 152)
                    } else {
                        Color32::from_rgb(148, 163, 184)
                    };
                    let slider_handle_fill = if ui.visuals().dark_mode {
                        Color32::from_rgb(117, 219, 166)
                    } else {
                        Color32::from_rgb(72, 168, 118)
                    };

                    egui::Grid::new("advanced-delay-grid")
                        .num_columns(3)
                        .min_col_width(0.0)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(Self::tr_lang(language, "Mouse Click Delay:", ""));
                            ui.scope(|ui| {
                                let visuals = ui.visuals_mut();
                                visuals.widgets.inactive.bg_fill = slider_track_fill;
                                visuals.widgets.inactive.bg_stroke =
                                    Stroke::new(1.0, slider_track_stroke);
                                visuals.widgets.hovered.bg_fill = slider_track_fill;
                                visuals.widgets.hovered.bg_stroke =
                                    Stroke::new(1.0, slider_track_stroke);
                                visuals.widgets.active.bg_fill = slider_track_fill;
                                visuals.widgets.active.bg_stroke =
                                    Stroke::new(1.0, slider_track_stroke);
                                visuals.selection.bg_fill = slider_handle_fill;
                                let res = ui.add_sized(
                                    [220.0, 22.0],
                                    egui::Slider::new(
                                        &mut self.state.macro_mouse_click_delay_ms,
                                        0..=500,
                                    )
                                    .show_value(false),
                                );
                                if res.changed() {
                                    delay_changed = true;
                                }
                            });
                            if ui
                                .add_sized(
                                    [58.0, 24.0],
                                    egui::DragValue::new(
                                        &mut self.state.macro_mouse_click_delay_ms,
                                    )
                                    .range(0..=500)
                                    .suffix(" ms"),
                                )
                                .changed()
                            {
                                delay_changed = true;
                            }
                            ui.end_row();

                            ui.label(Self::tr_lang(language, "Keyboard Press Delay:", ""));
                            ui.scope(|ui| {
                                let visuals = ui.visuals_mut();
                                visuals.widgets.inactive.bg_fill = slider_track_fill;
                                visuals.widgets.inactive.bg_stroke =
                                    Stroke::new(1.0, slider_track_stroke);
                                visuals.widgets.hovered.bg_fill = slider_track_fill;
                                visuals.widgets.hovered.bg_stroke =
                                    Stroke::new(1.0, slider_track_stroke);
                                visuals.widgets.active.bg_fill = slider_track_fill;
                                visuals.widgets.active.bg_stroke =
                                    Stroke::new(1.0, slider_track_stroke);
                                visuals.selection.bg_fill = slider_handle_fill;
                                let res = ui.add_sized(
                                    [220.0, 22.0],
                                    egui::Slider::new(
                                        &mut self.state.macro_keyboard_key_press_delay_ms,
                                        0..=500,
                                    )
                                    .show_value(false),
                                );
                                if res.changed() {
                                    delay_changed = true;
                                }
                            });
                            if ui
                                .add_sized(
                                    [58.0, 24.0],
                                    egui::DragValue::new(
                                        &mut self.state.macro_keyboard_key_press_delay_ms,
                                    )
                                    .range(0..=500)
                                    .suffix(" ms"),
                                )
                                .changed()
                            {
                                delay_changed = true;
                            }
                            ui.end_row();
                        });

                    if delay_changed {
                        self.sync_macro_delay_settings();
                        self.persist();
                    }
                }
            });
        });
    }

    pub(crate) fn render_downloaded_tools_settings(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        let opencv_path = self.paths.opencv_dll.clone();
        let arduino_path = self.paths.avrdude_exe.clone();
        let opencv_progress = self
            .opencv_download_job
            .as_ref()
            .map(|_| self.opencv_download_progress.load(Ordering::SeqCst) as f32 / 1000.0);
        let interception_progress = self
            .interception_download_job
            .as_ref()
            .map(|_| self.interception_download_progress.load(Ordering::SeqCst) as f32 / 1000.0);
        let arduino_progress = self
            .arduino_download_job
            .as_ref()
            .map(|_| self.arduino_download_progress.load(Ordering::SeqCst) as f32 / 1000.0);
        Self::settings_card_frame(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical(|ui| {
                if Self::settings_section_button(
                    ui,
                    RichText::new(Self::tr_lang(language, "Downloaded Tools", ""))
                        .strong()
                        .size(14.0),
                    self.downloaded_tools_open,
                )
                .clicked()
                {
                    self.downloaded_tools_open = !self.downloaded_tools_open;
                }

                if self.downloaded_tools_open {
                    ui.add_space(6.0);
                    self.render_downloaded_tool_entry(
                        ui,
                        language,
                        "OpenCV",
                        &opencv_path,
                        self.opencv_installed,
                        opencv_progress,
                        60 * 1024 * 1024,
                        Self::tr_lang(language, "OpenCV DLL deleted.", ""),
                        Self::start_opencv_download,
                        Self::delete_opencv_tool,
                    );
                    ui.add_space(10.0);
                    self.render_ocr_tool_header(ui, language);
                    if self.ocr_tools_open {
                        ui.add_space(6.0);
                        self.render_ocr_tool_entry(ui, language);
                    }
                    ui.add_space(10.0);
                    self.render_interception_driver_entry(ui, language, interception_progress);
                    ui.add_space(10.0);
                    self.render_downloaded_tool_entry(
                        ui,
                        language,
                        "Arduino Tools",
                        &arduino_path,
                        self.arduino_tools_downloaded,
                        arduino_progress,
                        1_000_000,
                        Self::tr_lang(language, "Arduino tools deleted.", ""),
                        Self::start_arduino_tools_download,
                        Self::delete_arduino_tools,
                    );
                }
            });
        });
    }

    pub(crate) fn render_interception_driver_entry(
        &mut self,
        ui: &mut egui::Ui,
        language: UiLanguage,
        downloading_progress: Option<f32>,
    ) {
        let package_ready = self.interception_package_downloaded;
        let driver_installed = self.interception_driver_installed;
        let restart_required = self.interception_driver_needs_restart;

        ui.vertical(|ui| {
            let action_width = 148.0;

            if downloading_progress.is_some() {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Interception Driver").strong().size(13.0));
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(Self::tool_size_label(&self.paths.interception_zip, 389_119))
                                .small()
                                .weak(),
                        );
                    });
                    let spacer = (ui.available_width() - action_width).max(0.0);
                    if spacer > 0.0 {
                        ui.add_space(spacer);
                    }
                    if let Some(progress) = downloading_progress {
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_width(action_width)
                                .show_percentage(),
                        );
                    }
                });
                ui.ctx().request_repaint();
                return;
            }

            if self.interception_install_job.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(Self::tr_lang(language, "Installing driver...", ""));
                });
                ui.ctx().request_repaint();
                return;
            }

            if self.interception_uninstall_job.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(Self::tr_lang(language, "Uninstalling driver...", ""));
                });
                ui.ctx().request_repaint();
                return;
            }

            if !package_ready {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Interception Driver").strong().size(13.0));
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(Self::tool_size_label(&self.paths.interception_zip, 389_119))
                                .small()
                                .weak(),
                        );
                    });
                    let spacer = (ui.available_width() - action_width).max(0.0);
                    if spacer > 0.0 {
                        ui.add_space(spacer);
                    }
                    if Self::settings_action_button_fixed(
                        ui,
                        Self::tr_lang(language, "Download", ""),
                        action_width,
                    )
                        .clicked()
                    {
                        self.start_interception_download();
                    }
                });
                return;
            }

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Interception Driver").strong().size(13.0));
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(Self::tool_size_label(&self.paths.interception_zip, 389_119))
                            .small()
                            .weak(),
                    );
                });
                let spacer = (ui.available_width() - action_width).max(0.0);
                if spacer > 0.0 {
                    ui.add_space(spacer);
                }
                if driver_installed {
                    if Self::settings_action_button_fixed(
                        ui,
                        Self::tr_lang(language, "Delete", ""),
                        action_width,
                    )
                        .clicked()
                    {
                        self.start_interception_driver_uninstall();
                    }
                } else if Self::settings_action_button_fixed(
                    ui,
                    Self::tr_lang(language, "Install", ""),
                    action_width,
                )
                    .clicked()
                {
                    self.start_interception_driver_install();
                }
            });

            ui.add_space(4.0);
            if restart_required {
                ui.label(
                    RichText::new(Self::tr_lang(
                        language,
                        "You must restart Windows before Interception will work in games.",
                        "",
                    ))
                    .small()
                    .color(Color32::from_rgb(248, 214, 102)),
                );
            } else if driver_installed {
                ui.label(
                    RichText::new(Self::tr_lang(
                        language,
                        "Installed. Restart your PC to take effect.",
                        "",
                    ))
                    .color(Color32::from_rgb(126, 224, 182)),
                );
            }
        });
    }

    fn render_ocr_tool_header(&mut self, ui: &mut egui::Ui, language: UiLanguage) {
        let installed_count = crate::ocr::count_installed_language_packs();
        let total_size = crate::ocr::total_installed_language_pack_size();
        let action_width = 148.0;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("OCR").strong().size(13.0));
                ui.add_space(2.0);
                let detail = if installed_count > 0 {
                    format!(
                        "{} packs ({})",
                        installed_count,
                        Self::format_byte_size(total_size)
                    )
                } else {
                    Self::tr_lang(language, "No packs installed", "").to_owned()
                };
                ui.label(RichText::new(detail).small().weak());
            });

            let spacer = (ui.available_width() - action_width).max(0.0);
            if spacer > 0.0 {
                ui.add_space(spacer);
            }

            ui.allocate_ui_with_layout(
                vec2(action_width, 52.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let button_label = if self.ocr_tools_open {
                        Self::tr_lang(language, "Close", "")
                    } else {
                        Self::tr_lang(language, "Open", "")
                    };
                    if Self::settings_action_button_fixed(ui, button_label, action_width).clicked()
                    {
                        self.ocr_tools_open = !self.ocr_tools_open;
                    }
                },
            );
        });
    }

    fn render_ocr_tool_entry(&mut self, ui: &mut egui::Ui, language: UiLanguage) {
        ui.vertical(|ui| {
            let is_downloading = self.ocr_download_job.is_some();
            let download_progress = if is_downloading {
                Some(self.ocr_download_progress.load(Ordering::SeqCst) as f32 / 1000.0)
            } else {
                None
            };
            let installed_count = crate::ocr::count_installed_language_packs();
            let installed_size = crate::ocr::total_installed_language_pack_size();
            let expected_size = crate::ocr::total_expected_language_pack_size();
            let all_installed = crate::ocr::are_all_language_packs_installed();

            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    vec2((ui.available_width() - 160.0).max(180.0), 32.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(Self::tr_lang(
                                    language,
                                    "All OCR language packs",
                                    "All OCR language packs",
                                ))
                                .strong(),
                            );
                            ui.add_space(2.0);
                            let detail = if installed_count > 0 {
                                format!(
                                    "{} packs installed ({}) / total {}",
                                    installed_count,
                                    Self::format_byte_size(installed_size),
                                    Self::format_byte_size(expected_size)
                                )
                            } else {
                                format!(
                                    "{}",
                                    Self::tr_lang(
                                        language,
                                        "Not installed",
                                        "Not installed",
                                    )
                                ) + &format!(" ({})", Self::format_byte_size(expected_size))
                            };
                            ui.label(RichText::new(detail).small().weak());
                        });
                    },
                );

                ui.allocate_ui_with_layout(
                    vec2(148.0, 60.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if let Some(progress) = download_progress {
                            ui.add(
                                egui::ProgressBar::new(progress)
                                    .desired_width(148.0)
                                    .show_percentage(),
                            );
                            ui.ctx().request_repaint();
                        } else if all_installed {
                            ui.label(
                                RichText::new(Self::tr_lang(language, "Installed", "Installed"))
                                    .weak(),
                            );
                        } else if Self::settings_action_button_fixed(
                            ui,
                            RichText::new(
                                if crate::ocr::is_ocr_assets_archive_cached() {
                                    Self::tr_lang(language, "Install", "Install")
                                } else {
                                    Self::tr_lang(language, "Download", "")
                                },
                            )
                            .strong(),
                            148.0,
                            )
                            .clicked()
                        {
                            self.start_ocr_download_for(crate::ocr::OCR_DEFAULT_CODE);
                        } else if Self::settings_action_button_fixed(
                            ui,
                            Self::tr_lang(language, "Delete", ""),
                            148.0,
                        )
                        .clicked()
                        {
                            self.delete_ocr_assets();
                            self.status = "OCR assets deleted.".to_owned();
                        }
                    },
                );
            });
        });
    }

    fn render_downloaded_tool_entry(
        &mut self,
        ui: &mut egui::Ui,
        language: UiLanguage,
        title: &str,
        path: &Path,
        installed: bool,
        downloading_progress: Option<f32>,
        expected_size_bytes: u64,
        delete_status_text: &str,
        download_action: fn(&mut Self),
        delete_action: fn(&mut Self),
    ) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).strong().size(13.0));
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(Self::tool_size_label(path, expected_size_bytes))
                            .small()
                            .weak(),
                    );
                });

                let action_width = 148.0;
                let spacer = (ui.available_width() - action_width).max(0.0);
                if spacer > 0.0 {
                    ui.add_space(spacer);
                }

                ui.allocate_ui_with_layout(
                    vec2(action_width, 52.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if installed {
                            if Self::settings_action_button_fixed(
                                ui,
                                Self::tr_lang(language, "Delete", ""),
                                action_width,
                            )
                            .clicked()
                            {
                                delete_action(self);
                                self.status = delete_status_text.to_owned();
                            }
                        } else if let Some(progress) = downloading_progress {
                            ui.add(
                                egui::ProgressBar::new(progress)
                                    .desired_width(action_width)
                                    .show_percentage(),
                            );
                            ui.ctx().request_repaint();
                        } else if Self::settings_action_button_fixed(
                            ui,
                            RichText::new(Self::tr_lang(language, "Download", "")).strong(),
                            action_width,
                        )
                        .clicked()
                        {
                            download_action(self);
                        }
                    },
                );
            });
        });
    }

    pub(crate) fn render_update_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let language = self.state.ui_language;
        Self::settings_card_frame(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(Self::tr_lang(language, "Update", ""))
                        .strong()
                        .size(14.0),
                );
                ui.add_space(8.0);
                match &self.update_status {
                    UpdateStatus::Idle => {
                        if Self::settings_action_button(
                            ui,
                            Self::tr_lang(language, "Check for update", ""),
                        )
                        .clicked()
                        {
                            self.check_for_update(ctx);
                        }
                    }
                    UpdateStatus::Checking => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(Self::tr_lang(language, "Checking for updates...", ""));
                        });
                    }
                    UpdateStatus::Available(version, body, url) => {
                        ui.label(
                            RichText::new(format!("New version available: v{}", version))
                                .color(Color32::GREEN),
                        );
                        if !body.is_empty() {
                            ui.label(RichText::new(body).small().weak());
                        }
                        if Self::settings_action_button(
                            ui,
                            Self::tr_lang(language, "Download and Update", ""),
                        )
                        .clicked()
                        {
                            self.start_download_update(ctx, url.clone());
                        }
                    }
                    UpdateStatus::Downloading => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(Self::tr_lang(language, "Downloading update...", ""));
                        });
                    }
                    UpdateStatus::ReadyToRestart(path) => {
                        ui.label(RichText::new("Update downloaded!").color(Color32::GREEN));
                        let path = path.clone();
                        if Self::settings_action_button(
                            ui,
                            RichText::new(Self::tr_lang(language, "Restart App", "")).strong(),
                        )
                        .clicked()
                        {
                            self.restart_and_apply_update(path);
                        }
                    }
                    UpdateStatus::UpToDate => {
                        ui.label(Self::tr_lang(language, "App is up to date.", ""));
                        ui.add_space(4.0);
                        if Self::settings_action_button(
                            ui,
                            Self::tr_lang(language, "Check again", ""),
                        )
                        .clicked()
                        {
                            self.check_for_update(ctx);
                        }
                    }
                    UpdateStatus::Error(e) => {
                        ui.label(RichText::new(format!("Error: {}", e)).color(Color32::RED));
                        ui.add_space(4.0);
                        if Self::settings_action_button(
                            ui,
                            Self::tr_lang(language, "Retry", "Retry"),
                        )
                        .clicked()
                        {
                            self.check_for_update(ctx);
                        }
                    }
                }
            });
        });
    }

    fn settings_section_button(
        ui: &mut egui::Ui,
        label: impl Into<WidgetText>,
        active: bool,
    ) -> egui::Response {
        let is_dark = ui.visuals().dark_mode;
        
        let (fill, stroke_color) = if is_dark {
            if active {
                (Color32::from_rgb(57, 72, 96), Color32::from_rgb(117, 219, 166))
            } else {
                (Color32::from_rgb(42, 52, 68), Color32::from_rgb(72, 88, 116))
            }
        } else {
            if active {
                (Color32::from_rgb(181, 192, 206), Color32::from_rgb(72, 168, 118))
            } else {
                (Color32::from_rgb(214, 223, 235), Color32::from_rgb(164, 178, 198))
            }
        };

        let text_color = if is_dark { Color32::WHITE } else { Color32::BLACK };
        ui.add_sized(
            [ui.available_width(), 32.0],
            Button::new(label.into().color(text_color))
                .fill(fill)
                .stroke(Stroke::new(1.0, stroke_color))
                .corner_radius(8.0),
        )
    }

    fn settings_action_button(ui: &mut egui::Ui, label: impl Into<WidgetText>) -> egui::Response {
        Self::settings_action_button_fixed(ui, label, 0.0)
    }

    fn settings_action_button_fixed(
        ui: &mut egui::Ui,
        label: impl Into<WidgetText>,
        fixed_width: f32,
    ) -> egui::Response {
        let is_dark = ui.visuals().dark_mode;
        
        let (fill, stroke_color) = if is_dark {
            (Color32::from_rgb(48, 58, 76), Color32::from_rgb(84, 100, 124))
        } else {
            (Color32::from_rgb(220, 228, 238), Color32::from_rgb(170, 182, 198))
        };

        let label_text = label.into();
        let text_style = egui::TextStyle::Button;
        
        let wrap_width = ui.available_width();
        let galley = label_text.into_galley(ui, None, wrap_width, text_style);
        
        let button_width = if fixed_width > 0.0 {
            fixed_width
        } else {
            (galley.size().x + 20.0).max(104.0)
        };
        let button_size = vec2(button_width, (galley.size().y + 10.0).max(28.0));
        
        let (rect, response) = ui.allocate_exact_size(button_size, egui::Sense::click());
        
        let hovered = response.hovered();
        let pressed = response.is_pointer_button_down_on();
        
        let final_fill = if pressed {
            fill.linear_multiply(0.8)
        } else if hovered {
            fill.linear_multiply(1.1)
        } else {
            fill
        };

        ui.painter().rect(
            rect,
            6.0,
            final_fill,
            Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );

        let text_pos = rect.center() - galley.size() / 2.0;
        let text_color = if is_dark { Color32::WHITE } else { Color32::BLACK };
        ui.painter().galley(text_pos, galley, text_color);

        Self::paint_show_hover_outline(ui, &response);
        response
    }

    fn format_byte_size(size: u64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;
        const GB: f64 = MB * 1024.0;
        let size_f = size as f64;
        if size_f >= GB {
            format!("{:.1} GB", size_f / GB)
        } else if size_f >= MB {
            format!("{:.1} MB", size_f / MB)
        } else if size_f >= KB {
            format!("{:.1} KB", size_f / KB)
        } else {
            format!("{size} B")
        }
    }

    pub(crate) fn cleanup_custom_ai_dialog_state(&mut self) {
        self.command_ai_step_target = None;
        self.state
            .command_presets
            .retain(|preset| preset.id != 999999);
    }

    pub(crate) fn render_custom_ai_modal(&mut self, ctx: &egui::Context) {
        let dialog_was_open = self.command_ai_dialog.is_some();
        let generating = self.command_ai_job.is_some();
        let Some(dialog_preset_id) = self
            .command_ai_dialog
            .as_ref()
            .map(|dialog| dialog.preset_id)
        else {
            return;
        };
        let Some(preset_name) = self
            .state
            .command_presets
            .iter()
            .find(|preset| preset.id == dialog_preset_id)
            .map(|preset| preset.name.clone())
        else {
            self.command_ai_dialog = None;
            self.status = "Custom preset was removed.".to_owned();
            return;
        };

        if self.capture_target.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.command_ai_dialog = None;
            return;
        }

        self.render_modal_backdrop(ctx, true);
        let (panel_size, panel_pos) =
            Self::centered_modal_placement(ctx, vec2(560.0, 220.0), vec2(480.0, 180.0));
        let mut close_request = false;
        let mut generate_request = false;
        let dark_theme = self.state.ui_theme == UiThemeMode::Dark;
        let vietnamese_input_mode = self.state.vietnamese_input_mode;
        {
            let Some(dialog) = self.command_ai_dialog.as_mut() else {
                return;
            };
            egui::Area::new(egui::Id::new("custom-ai-modal"))
                .order(Order::Foreground)
                .fixed_pos(panel_pos)
                .interactable(true)
                .show(ctx, |ui| {
                    ui.output_mut(|output| output.cursor_icon = egui::CursorIcon::Default);
                    Frame::new()
                        .fill(if dark_theme {
                            Color32::from_rgba_premultiplied(24, 26, 32, 248)
                        } else {
                            Color32::from_rgba_premultiplied(248, 248, 250, 248)
                        })
                        .stroke(Stroke::new(
                            1.0,
                            Color32::from_rgba_premultiplied(90, 94, 108, 180),
                        ))
                        .shadow(Shadow {
                            offset: [0, 14],
                            blur: 32,
                            spread: 0,
                            color: Color32::from_rgba_premultiplied(12, 12, 16, 72),
                        })
                        .corner_radius(24.0)
                        .inner_margin(Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_min_width(panel_size.x);
                            ui.set_max_width(panel_size.x);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("AI Custom").strong());
                                        ui.label(
                                            RichText::new(preset_name.clone())
                                                .small()
                                                .color(ui.visuals().weak_text_color()),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add_sized(
                                                    [34.0, 28.0],
                                                    Button::new(Self::material_icon_text(
                                                        0xe5cd, 18.0,
                                                    )),
                                                )
                                                .clicked()
                                            {
                                                close_request = true;
                                            }
                                        },
                                    );
                                });
                                let original_weak_color = ui.style().visuals.weak_text_color;
                                ui.style_mut().visuals.weak_text_color = if dark_theme {
                                    Some(Color32::from_gray(85))
                                } else {
                                    Some(Color32::from_gray(175))
                                };
                                let original_extreme_bg = ui.visuals().extreme_bg_color;
                                ui.visuals_mut().extreme_bg_color = if dark_theme {
                                    Color32::from_rgba_unmultiplied(12, 13, 16, 50)
                                } else {
                                    Color32::from_rgba_unmultiplied(240, 240, 242, 50)
                                };
                                let response = ui.add_sized(
                                    [ui.available_width(), 92.0],
                                    TextEdit::multiline(&mut dialog.prompt)
                                        .desired_rows(4)
                                        .text_color(if dark_theme {
                                            Color32::from_gray(210)
                                        } else {
                                            Color32::from_gray(60)
                                        })
                                        .hint_text(
                                            egui::RichText::new(Self::tr_lang(self.state.ui_language, "Example: Open Excel, write text to cell A1, then save...", "Example: Open Excel, write text to cell A1, then save..."))
                                            .color(if dark_theme {
                                                Color32::from_rgba_unmultiplied(120, 120, 120, 140)
                                            } else {
                                                Color32::from_rgba_unmultiplied(140, 140, 140, 180)
                                            })
                                            .italics(),
                                        ),
                                );
                                ui.style_mut().visuals.weak_text_color = original_weak_color;
                                ui.visuals_mut().extreme_bg_color = original_extreme_bg;
                                Self::apply_vietnamese_input_if_changed(
                                    &response,
                                    self.state.vietnamese_input_enabled,
                                    vietnamese_input_mode,
                                    &mut dialog.prompt,
                                );

                                let enter_generate = !generating
                                    && !dialog.prompt.trim().is_empty()
                                    && ctx.input(|input| input.key_pressed(egui::Key::Enter));
                                if generating {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label("Generating...");
                                    });
                                } else if let Some(feedback) = self.command_ai_feedback.as_ref() {
                                    ui.label(
                                        RichText::new(feedback)
                                            .small()
                                            .color(ui.visuals().strong_text_color()),
                                    );
                                }
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    let can_generate =
                                        !generating && !dialog.prompt.trim().is_empty();
                                    if ui
                                        .add_enabled(
                                            can_generate,
                                            Button::new("Generate").min_size(vec2(100.0, 28.0)),
                                        )
                                        .clicked()
                                    {
                                        generate_request = true;
                                    }
                                    if ui
                                        .add_enabled(
                                            true,
                                            Button::new("Close").min_size(vec2(100.0, 28.0)),
                                        )
                                        .clicked()
                                    {
                                        close_request = true;
                                    }
                                });
                                if enter_generate {
                                    generate_request = true;
                                }
                            });
                        });
                });
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }

        if generate_request {
            self.start_custom_ai_generation(ctx);
            if self.command_ai_job.is_some() {
                self.command_ai_dialog = None;
            }
        }
        if close_request {
            self.command_ai_dialog = None;
        }
        if dialog_was_open && self.command_ai_dialog.is_none() && self.command_ai_job.is_none() {
            self.cleanup_custom_ai_dialog_state();
        }
    }

    pub(crate) fn start_opencv_download(&mut self) {
        if self.opencv_download_job.is_some() {
            return;
        }

        let paths = self.paths.clone();
        let progress = self.opencv_download_progress.clone();
        progress.store(0, Ordering::SeqCst);

        let job = std::thread::spawn(move || -> Result<()> {
            let url = "https://github.com/NBaoLinh/MacroNest/releases/download/tools/opencv_world4100.dll";
            let mut response = reqwest::blocking::get(url)?.error_for_status()?;
            let total_size = response.content_length().unwrap_or(64 * 1024 * 1024);

            let mut file = fs::File::create(&paths.opencv_dll)?;
            let mut downloaded: u64 = 0;
            let mut buffer = [0u8; 16384];

            use std::io::{Read, Write};
            loop {
                let n = response.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buffer[..n])?;
                downloaded += n as u64;
                let p = (downloaded as f32 / total_size as f32 * 1000.0) as u32;
                progress.store(p, Ordering::SeqCst);
            }

            Ok(())
        });

        self.opencv_download_job = Some(job);
    }

    pub(crate) fn start_ocr_download_for(&mut self, _language_code: &str) {
        if self.ocr_download_job.is_some() {
            return;
        }

        let progress = self.ocr_download_progress.clone();
        progress.store(0, Ordering::SeqCst);

        let job = std::thread::spawn(move || -> Result<()> {
            crate::ocr::install_all_language_packs(|downloaded, total| {
                let ratio = if total == 0 {
                    0.0
                } else {
                    downloaded as f32 / total as f32
                };
                progress.store((ratio * 1000.0).round() as u32, Ordering::SeqCst);
            })
        });

        self.ocr_download_job = Some(job);
    }

    pub(crate) fn start_interception_download(&mut self) {
        if self.interception_download_job.is_some() {
            return;
        }

        let paths = self.paths.clone();
        let progress = self.interception_download_progress.clone();
        progress.store(0, Ordering::SeqCst);

        let job = std::thread::spawn(move || -> Result<()> {
            let url =
                "https://github.com/NBaoLinh/MacroNest/releases/download/tools/Interception.zip";
            let mut response = reqwest::blocking::get(url)?.error_for_status()?;
            let total_size = response.content_length().unwrap_or(389_119);

            let mut file = fs::File::create(&paths.interception_zip)?;
            let mut downloaded: u64 = 0;
            let mut buffer = [0u8; 16384];

            use std::io::{Read, Write};
            loop {
                let n = response.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buffer[..n])?;
                downloaded += n as u64;
                let p = (downloaded as f32 / total_size as f32 * 1000.0) as u32;
                progress.store(p, Ordering::SeqCst);
            }

            drop(file);

            let _ = fs::remove_dir_all(&paths.interception_package_dir);
            let extract_script = format!(
                "Expand-Archive -LiteralPath {} -DestinationPath {} -Force",
                Self::powershell_quote(&paths.interception_zip.to_string_lossy()),
                Self::powershell_quote(&paths.bin_dir.to_string_lossy()),
            );
            let extract_status = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &extract_script])
                .status()?;
            if !extract_status.success() {
                bail!("Failed to extract Interception.zip");
            }

            let extracted_dll = paths
                .interception_package_dir
                .join("library")
                .join("x64")
                .join("interception.dll");
            if !extracted_dll.exists() {
                bail!("Interception package did not contain the x64 interception.dll");
            }

            fs::copy(&extracted_dll, &paths.interception_dll)?;

            Ok(())
        });

        self.interception_download_job = Some(job);
    }

    fn delete_opencv_tool(&mut self) {
        let _ = fs::remove_file(&self.paths.opencv_dll);
        let _ = fs::remove_file(&self.paths.opencv_videoio_ffmpeg_dll);
        self.opencv_installed = false;
    }

    fn delete_ocr_assets(&mut self) {
        let _ = crate::ocr::delete_all_language_packs();
    }

    fn delete_interception_package(&mut self) {
        let _ = fs::remove_file(&self.paths.interception_zip);
        let _ = fs::remove_dir_all(&self.paths.interception_package_dir);
        self.interception_package_downloaded = false;
        self.interception_driver_needs_restart = false;
    }

    fn start_interception_driver_install(&mut self) {
        if self.interception_install_job.is_some() || self.interception_uninstall_job.is_some() {
            return;
        }
        if !self.paths.interception_installer_exe.exists() {
            self.status =
                "Interception installer was not found. Download the package first.".to_owned();
            return;
        }

        let installer_dir = self
            .paths
            .interception_installer_exe
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| self.paths.bin_dir.clone());
        let job = std::thread::spawn(move || -> Result<()> {
            let installer = installer_dir.join("install-interception.exe");
            crate::platform::launch_hidden_process_as_admin(&installer, Some("/install"))?;
            let deadline = Instant::now() + Duration::from_secs(60);
            while Instant::now() < deadline {
                if crate::platform::is_interception_driver_installed() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            bail!("Timed out waiting for the Interception driver to install");
        });

        self.interception_install_job = Some(job);
        self.status = "Launching Interception driver installer...".to_owned();
    }

    fn start_interception_driver_uninstall(&mut self) {
        if self.interception_install_job.is_some() || self.interception_uninstall_job.is_some() {
            return;
        }
        if !self.paths.interception_installer_exe.exists() {
            self.status =
                "Interception installer was not found. Download the package first.".to_owned();
            return;
        }

        let installer_dir = self
            .paths
            .interception_installer_exe
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| self.paths.bin_dir.clone());
        let job = std::thread::spawn(move || -> Result<()> {
            let installer = installer_dir.join("install-interception.exe");
            crate::platform::launch_hidden_process_as_admin(&installer, Some("/uninstall"))?;
            let deadline = Instant::now() + Duration::from_secs(60);
            while Instant::now() < deadline {
                if !crate::platform::is_interception_driver_installed() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            bail!("Timed out waiting for the Interception driver to uninstall");
        });

        self.interception_uninstall_job = Some(job);
        self.status = "Launching Interception driver uninstaller...".to_owned();
    }

    fn tool_size_label(path: &Path, expected_size_bytes: u64) -> String {
        match fs::metadata(path) {
            Ok(metadata) => format!("Size: {}", Self::format_file_size(metadata.len())),
            Err(_) => format!(
                "Expected size: ~{}",
                Self::format_file_size(expected_size_bytes)
            ),
        }
    }

    fn format_file_size(bytes: u64) -> String {
        const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
        let mut value = bytes as f64;
        let mut unit = 0usize;
        while value >= 1024.0 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }

        if unit == 0 {
            format!("{bytes} {}", UNITS[unit])
        } else {
            format!("{value:.1} {}", UNITS[unit])
        }
    }

    pub(crate) fn check_for_update(&mut self, ctx: &egui::Context) {
        if matches!(
            self.update_status,
            UpdateStatus::Checking | UpdateStatus::Downloading
        ) {
            return;
        }
        self.update_status = UpdateStatus::Checking;
        let ui_tx = self.ui_tx.clone();
        let ctx = ctx.clone();
        let current_version = self.app_version_label().to_owned();
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .user_agent("MacroNest")
                .build()
                .map_err(|e| e.to_string());
            let result = client.and_then(|c| {
                let resp = c
                    .get("https://api.github.com/repos/NBaoLinh/MacroNest/releases/latest")
                    .send()
                    .map_err(|e| e.to_string())?;

                if resp.status() == reqwest::StatusCode::NOT_FOUND {
                    return Err("No releases found on GitHub.".to_owned());
                }

                if !resp.status().is_success() {
                    return Err(format!("GitHub API error: {}", resp.status()));
                }

                let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
                let latest_version = json["tag_name"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches('v')
                    .to_owned();
                if latest_version.is_empty() {
                    return Err("Failed to parse version from GitHub".to_owned());
                }
                if Self::versions_are_equal(&latest_version, &current_version) {
                    let _ = ui_tx.send(UiCommand::UpdateUpToDate);
                    return Ok(());
                }
                let body = json["body"].as_str().unwrap_or("").to_owned();
                let download_url = json["assets"]
                    .as_array()
                    .and_then(|assets| {
                        assets.iter().find(|a| {
                            a["name"]
                                .as_str()
                                .map(|n| n.ends_with(".exe"))
                                .unwrap_or(false)
                        })
                    })
                    .and_then(|a| a["browser_download_url"].as_str())
                    .map(|s| s.to_owned());
                if let Some(url) = download_url {
                    let _ = ui_tx.send(UiCommand::UpdateAvailable(latest_version, body, url));
                } else {
                    let _ = ui_tx.send(UiCommand::UpdateError(
                        "No executable found in the latest release".to_owned(),
                    ));
                }
                Ok(())
            });
            if let Err(e) = result {
                let _ = ui_tx.send(UiCommand::UpdateError(e));
            }
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_download_update(&mut self, ctx: &egui::Context, download_url: String) {
        self.update_status = UpdateStatus::Downloading;
        let ui_tx = self.ui_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .user_agent("MacroNest")
                .build();
            let result = client.map_err(|e| e.to_string()).and_then(|c| {
                let mut resp = c.get(download_url).send().map_err(|e| e.to_string())?;
                let temp_dir = std::env::temp_dir();
                let temp_path = temp_dir.join("macronest_update.exe");
                let mut file = fs::File::create(&temp_path).map_err(|e| e.to_string())?;
                std::io::copy(&mut resp, &mut file).map_err(|e| e.to_string())?;
                let _ = ui_tx.send(UiCommand::UpdateDownloadFinished(
                    temp_path.to_string_lossy().to_string(),
                ));
                Ok(())
            });
            if let Err(e) = result {
                let _ = ui_tx.send(UiCommand::UpdateError(e));
            }
            ctx.request_repaint();
        });
    }

    pub(crate) fn restart_and_apply_update(&mut self, new_exe_path: String) {
        let current_exe = std::env::current_exe().unwrap_or_default();
        let old_exe = current_exe.with_extension("exe.old");
        let result: anyhow::Result<()> = (|| {
            if old_exe.exists() {
                let _ = fs::remove_file(&old_exe);
            }
            fs::rename(&current_exe, &old_exe)?;
            fs::copy(&new_exe_path, &current_exe)?;
            Command::new(&current_exe).spawn()?;
            std::process::exit(0);
        })();
        if let Err(e) = result {
            self.status = format!("Failed to apply update: {e}");
        }
    }

    pub(crate) fn open_app_data_folder(&mut self) {
        match crate::platform::open_folder_in_explorer(&self.paths.root) {
            Ok(()) => {
                self.status = format!("Opened data folder: {}.", self.paths.root.display());
            }
            Err(error) => {
                self.status = format!("Failed to open data folder: {error}");
            }
        }
    }
    pub(crate) fn open_ai_debug_folder(&mut self) {}
}
