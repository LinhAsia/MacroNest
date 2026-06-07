use crate::model::*;
use crate::overlay::UiCommand;
use crate::ui::{CrosshairApp, OcrLanguageOperationKind, UpdateStatus};
use anyhow::{Result, bail};
use eframe::egui::{
    self, Button, Color32, Frame, Margin, Order, RichText, Shadow, Stroke, TextEdit, WidgetText,
    vec2,
};
use std::fs;
use std::path::PathBuf;
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
                                ui.horizontal(|ui| {
                                    ui.label("API Key");
                                    let key_editor =
                                        TextEdit::singleline(&mut self.state.groq_settings.api_key)
                                            .hint_text("gsk_...");
                                    let response = ui.add_sized(
                                        [280.0, 24.0],
                                        if self.state.groq_settings.show_api_key {
                                            key_editor
                                        } else {
                                            key_editor.password(true)
                                        },
                                    );
                                    Self::apply_vietnamese_input_if_changed(
                                        &response,
                                        self.state.vietnamese_input_enabled,
                                        self.state.vietnamese_input_mode,
                                        &mut self.state.groq_settings.api_key,
                                    );
                                    groq_changed |= response.changed();
                                    if ui
                                        .button(if self.state.groq_settings.show_api_key {
                                            Self::tr_lang(language, "Hide", "")
                                        } else {
                                            Self::tr_lang(language, "Show", "")
                                        })
                                        .clicked()
                                    {
                                        self.state.groq_settings.show_api_key =
                                            !self.state.groq_settings.show_api_key;
                                        groq_changed = true;
                                    }
                                });
                                ui.horizontal(|ui| {
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
                                });
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
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
                                    if Self::settings_action_button(ui, "Get API key").clicked() {
                                        let _ = crate::platform::open_url_in_browser(
                                            "https://console.groq.com/keys",
                                        );
                                    }
                                });
                            }
                        });
                    });
                    if groq_changed {
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
                    self.render_ocr_language_settings(ui);
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
                    ui.horizontal(|ui| {
                        ui.label(Self::tr_lang(language, "Mouse Click Delay:", ""));
                        let slider = egui::Slider::new(&mut self.state.macro_mouse_click_delay_ms, 0..=500)
                            .suffix(" ms");
                        let res = ui.add(slider);
                        if res.changed() {
                            delay_changed = true;
                        }
                    });

                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label(Self::tr_lang(language, "Keyboard Press Delay:", ""));
                        let slider = egui::Slider::new(&mut self.state.macro_keyboard_key_press_delay_ms, 0..=500)
                            .suffix(" ms");
                        let res = ui.add(slider);
                        if res.changed() {
                            delay_changed = true;
                        }
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
        let opencv_progress = self
            .opencv_download_job
            .as_ref()
            .map(|_| self.opencv_download_progress.load(Ordering::SeqCst) as f32 / 1000.0);
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
                        "Vision Support (OpenCV)",
                        &opencv_path,
                        self.opencv_installed,
                        opencv_progress,
                        60 * 1024 * 1024,
                        Self::tr_lang(language, "Download OpenCV", ""),
                        Self::tr_lang(language, "Vision features require OpenCV.", ""),
                        Self::tr_lang(language, "OpenCV DLL deleted.", ""),
                        Self::start_opencv_download,
                        Self::delete_opencv_tool,
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
            ui.label(RichText::new("Interception Driver").strong().size(13.0));
            ui.add_space(6.0);

            if downloading_progress.is_some() {
                if let Some(progress) = downloading_progress {
                    ui.horizontal(|ui| {
                        ui.label(Self::tr_lang(language, "Downloading package...", ""));
                        ui.add(egui::ProgressBar::new(progress).show_percentage());
                    });
                }
                ui.label(
                    RichText::new(Self::tool_size_label(&self.paths.interception_zip, 389_119))
                        .small()
                        .weak(),
                );
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
                    ui.label(
                        RichText::new(Self::tr_lang(
                            language,
                            "Download the Interception package to enable driver setup.",
                            "",
                        ))
                        .weak(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(Self::tool_size_label(&self.paths.interception_zip, 389_119))
                            .small()
                            .weak(),
                    );
                    if Self::settings_action_button(ui, Self::tr_lang(language, "Download", ""))
                        .clicked()
                    {
                        self.start_interception_download();
                    }
                });
                return;
            }

            ui.horizontal(|ui| {
                ui.label(RichText::new(Self::tr_lang(language, "Package downloaded.", "")).weak());
                ui.add_space(8.0);
                ui.label(
                    RichText::new(Self::tool_size_label(&self.paths.interception_zip, 389_119))
                        .small()
                        .weak(),
                );
            });

            ui.add_space(4.0);

            if driver_installed {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(Self::tr_lang(
                            language,
                            "Driver installed. Restart your PC to take effect.",
                            "",
                        ))
                        .color(Color32::from_rgb(126, 224, 182)),
                    );
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if Self::settings_action_button(
                        ui,
                        Self::tr_lang(language, "Delete Driver", ""),
                    )
                    .clicked()
                    {
                        self.start_interception_driver_uninstall();
                    }
                    if Self::settings_action_button(ui, Self::tr_lang(language, "Restart PC", ""))
                        .clicked()
                    {
                        match crate::platform::restart_windows() {
                            Ok(()) => {
                                self.status =
                                    Self::tr_lang(language, "Restarting Windows...", "").to_owned();
                            }
                            Err(error) => {
                                self.status = format!("Failed to restart Windows: {error}");
                            }
                        }
                    }
                });
                return;
            }

            ui.horizontal(|ui| {
                if restart_required {
                    ui.label(
                        RichText::new(Self::tr_lang(
                            language,
                            "Driver installed. Restart your PC to finish setup.",
                            "",
                        ))
                        .color(Color32::from_rgb(248, 214, 102)),
                    );
                } else {
                    ui.label(
                        RichText::new(Self::tr_lang(language, "Ready to install the driver.", ""))
                            .weak(),
                    );
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if Self::settings_action_button(ui, Self::tr_lang(language, "Install Driver", ""))
                    .clicked()
                {
                    self.start_interception_driver_install();
                }
                if Self::settings_action_button(ui, Self::tr_lang(language, "Delete", "")).clicked()
                {
                    self.delete_interception_package();
                }
            });

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
            }
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
        download_button_text: &str,
        description_text: &str,
        delete_status_text: &str,
        download_action: fn(&mut Self),
        delete_action: fn(&mut Self),
    ) {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).strong().size(13.0));
            ui.add_space(6.0);

            if installed {
                ui.horizontal(|ui| {
                    ui.label(Self::tr_lang(language, "Status: Installed", ""));
                    ui.label(
                        RichText::new(Self::tool_size_label(path, expected_size_bytes))
                            .small()
                            .weak(),
                    );
                    if Self::settings_action_button(ui, Self::tr_lang(language, "Delete", ""))
                        .clicked()
                    {
                        delete_action(self);
                        self.status = delete_status_text.to_owned();
                    }
                });
            } else if let Some(progress) = downloading_progress {
                ui.horizontal(|ui| {
                    ui.label(Self::tr_lang(language, "Downloading...", ""));
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                });
                ui.label(
                    RichText::new(Self::tool_size_label(path, expected_size_bytes))
                        .small()
                        .weak(),
                );
                ui.ctx().request_repaint();
            } else {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(description_text).weak());
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(Self::tool_size_label(path, expected_size_bytes))
                            .small()
                            .weak(),
                    );
                    if Self::settings_action_button(
                        ui,
                        RichText::new(download_button_text).strong(),
                    )
                    .clicked()
                    {
                        download_action(self);
                    }
                });
            }
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
                            Self::tr_lang(language, "Retry", "Thử lại"),
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
        let visuals = ui.visuals();
        let fill = if active {
            visuals.widgets.active.bg_fill
        } else {
            visuals.widgets.inactive.bg_fill
        };
        let stroke_color = if active {
            visuals.widgets.active.bg_stroke.color
        } else {
            visuals.widgets.inactive.bg_stroke.color
        };
        ui.add_sized(
            [ui.available_width(), 30.0],
            Button::new(label)
                .fill(fill)
                .stroke(Stroke::new(1.0, stroke_color))
                .min_size(vec2(0.0, 30.0)),
        )
    }

    fn settings_action_button(ui: &mut egui::Ui, label: impl Into<WidgetText>) -> egui::Response {
        let visuals = ui.visuals();
        ui.add(
            Button::new(label)
                .fill(visuals.widgets.inactive.bg_fill)
                .stroke(Stroke::new(1.0, visuals.widgets.inactive.bg_stroke.color))
                .min_size(vec2(104.0, 28.0)),
        )
    }

    pub(crate) fn render_custom_ai_modal(&mut self, ctx: &egui::Context) {
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
                                            egui::RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Example: Open Excel, write text to cell A1, then save...",
                                                "Ví dụ: Mở Excel, ghi nội dung vào ô A1, sau đó lưu lại...",
                                            ))
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
        if self.command_ai_dialog.is_none() {
            if self.command_ai_job.is_none() {
                self.command_ai_step_target = None;
                self.state
                    .command_presets
                    .retain(|preset| preset.id != 999999);
            }
        }
        if self.command_ai_dialog.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
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
            let url = "https://github.com/Baolinh0305/MacroNest/releases/download/tools/opencv_world4100.dll";
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

    pub(crate) fn start_interception_download(&mut self) {
        if self.interception_download_job.is_some() {
            return;
        }

        let paths = self.paths.clone();
        let progress = self.interception_download_progress.clone();
        progress.store(0, Ordering::SeqCst);

        let job = std::thread::spawn(move || -> Result<()> {
            let url =
                "https://github.com/Baolinh0305/MacroNest/releases/download/tools/Interception.zip";
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
            let cmd = std::env::var("ComSpec")
                .unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_owned());
            let cmd_args = format!(
                "/K cd /d \"{}\" && install-interception.exe /install",
                installer_dir.display()
            );
            crate::platform::launch_process_as_admin(Path::new(&cmd), Some(&cmd_args))?;
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
            let cmd = std::env::var("ComSpec")
                .unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_owned());
            let cmd_args = format!(
                "/K cd /d \"{}\" && install-interception.exe /uninstall",
                installer_dir.display()
            );
            crate::platform::launch_process_as_admin(Path::new(&cmd), Some(&cmd_args))?;
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
                    .get("https://api.github.com/repos/Baolinh0305/MacroNest/releases/latest")
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

    fn ocr_dism_path() -> PathBuf {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        Path::new(&system_root).join("System32").join("dism.exe")
    }

    pub(crate) fn install_ocr_language_capability(
        &mut self,
        lang_code: &str,
        display_name: &str,
    ) {
        let Some(capability_name) = crate::ocr::ocr_capability_name(lang_code) else {
            return;
        };

        crate::ocr::clear_available_ocr_languages_cache();
        self.ocr_lang_pack_open = true;
        self.ocr_lang_settings_focus = Some(lang_code.to_owned());
        self.ocr_lang_operation = Some((
            lang_code.to_owned(),
            OcrLanguageOperationKind::Install,
            Instant::now(),
        ));

        let args = format!(
            "/Online /Add-Capability /CapabilityName:{} /NoRestart",
            capability_name
        );
        match crate::platform::launch_hidden_process_as_admin(&Self::ocr_dism_path(), Some(&args)) {
            Ok(()) => {
                self.status = format!(
                    "Started Windows OCR install for {}. Click Refresh after the install finishes.",
                    display_name
                );
            }
            Err(error) => {
                self.status = format!("Failed to start OCR install for {}: {}", display_name, error);
            }
        }
    }

    fn uninstall_ocr_language_capability(&mut self, lang_code: &str, display_name: &str) {
        let Some(capability_name) = crate::ocr::ocr_capability_name(lang_code) else {
            return;
        };

        crate::ocr::clear_available_ocr_languages_cache();
        self.ocr_lang_pack_open = true;
        self.ocr_lang_settings_focus = Some(lang_code.to_owned());
        self.ocr_lang_operation = Some((
            lang_code.to_owned(),
            OcrLanguageOperationKind::Remove,
            Instant::now(),
        ));

        let args = format!(
            "/Online /Remove-Capability /CapabilityName:{} /NoRestart",
            capability_name
        );
        match crate::platform::launch_hidden_process_as_admin(&Self::ocr_dism_path(), Some(&args)) {
            Ok(()) => {
                self.status = format!(
                    "Started Windows OCR removal for {}. Click Refresh after the uninstall finishes.",
                    display_name
                );
            }
            Err(error) => {
                self.status = format!("Failed to start OCR removal for {}: {}", display_name, error);
            }
        }
    }

    fn refresh_ocr_language_status(&mut self) {
        crate::ocr::clear_available_ocr_languages_cache();
        self.ocr_lang_operation = None;
        self.status = "Refreshed Windows OCR language status.".to_owned();
    }

    pub(crate) fn render_ocr_language_settings(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        let focused_lang = self.ocr_lang_settings_focus.clone();
        let active_operation = self.ocr_lang_operation.clone();

        Self::settings_card_frame(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical(|ui| {
                let header_text = RichText::new(Self::tr_lang(language, "OCR Language Packs", "Goi Ngon ngu OCR"))
                    .strong()
                    .size(14.0);
                if Self::settings_section_button(ui, header_text, self.ocr_lang_pack_open).clicked() {
                    self.ocr_lang_pack_open = !self.ocr_lang_pack_open;
                }

                if !self.ocr_lang_pack_open {
                    return;
                }

                let avail_langs = crate::ocr::available_ocr_languages();

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if Self::settings_action_button(ui, Self::tr_lang(language, "Refresh", "Lam moi"))
                        .clicked()
                    {
                        self.refresh_ocr_language_status();
                    }
                });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(Self::tr_lang(
                        language,
                        "This list only shows languages that Windows OCR supports. OCR ready means the OCR capability is installed on this PC.",
                        "Danh sach nay chi hien nhung ngon ngu ma Windows OCR ho tro. OCR ready nghia la OCR capability da duoc cai tren may nay.",
                    ))
                    .small()
                    .weak(),
                );
                ui.add_space(8.0);

                if let Some(lang_code) = focused_lang.as_deref() {
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            Self::tr_lang(
                                language,
                                "Selected from OCR dropdown:",
                                "Duoc chon tu dropdown OCR:",
                            ),
                            lang_code
                        ))
                        .small()
                        .color(Color32::from_rgb(126, 224, 182)),
                    );
                    ui.add_space(6.0);
                }

                egui::Grid::new("ocr-lang-pack-grid")
                    .num_columns(3)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        let mut clear_focus_after_scroll = false;
                        for (lang_code, display_name, _) in crate::ocr::OCR_SUPPORTED_LANGUAGE_CATALOG {
                            let has_ocr = crate::ocr::language_tag_matches(&avail_langs, lang_code);
                            let is_focused = focused_lang.as_deref() == Some(*lang_code);
                            let operation_for_lang = active_operation.as_ref().and_then(
                                |(op_lang, kind, started_at)| {
                                    if op_lang == lang_code {
                                        Some((*kind, *started_at))
                                    } else {
                                        None
                                    }
                                },
                            );

                            if has_ocr {
                                ui.label(RichText::new("OK").color(Color32::from_rgb(126, 224, 182)));
                            } else {
                                ui.label(RichText::new("x").color(Color32::from_rgb(220, 100, 100)));
                            }

                            let name_response = ui.label(
                                RichText::new(*display_name).color(if is_focused {
                                    Color32::from_rgb(255, 216, 96)
                                } else {
                                    ui.visuals().text_color()
                                }),
                            );
                            if is_focused {
                                ui.scroll_to_rect(name_response.rect, Some(egui::Align::Center));
                                clear_focus_after_scroll = true;
                            }

                            if has_ocr {
                                if matches!(
                                    operation_for_lang,
                                    Some((OcrLanguageOperationKind::Install, _))
                                ) {
                                    self.ocr_lang_operation = None;
                                }
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(Self::tr_lang(language, "OCR ready", "OCR san sang"))
                                            .small()
                                            .color(Color32::from_rgb(126, 224, 182)),
                                    );
                                    if Self::settings_action_button(
                                        ui,
                                        Self::tr_lang(language, "Delete", "Xoa"),
                                    )
                                    .on_hover_text(Self::tr_lang(
                                        language,
                                        "Remove the Windows OCR capability for this language.",
                                        "Go OCR capability cua Windows cho ngon ngu nay.",
                                    ))
                                    .clicked()
                                    {
                                        self.uninstall_ocr_language_capability(
                                            lang_code,
                                            display_name,
                                        );
                                    }
                                });
                            } else {
                                if matches!(
                                    operation_for_lang,
                                    Some((OcrLanguageOperationKind::Remove, _))
                                ) {
                                    self.ocr_lang_operation = None;
                                }
                                if let Some((operation_kind, started_at)) = operation_for_lang {
                                    let elapsed = started_at.elapsed().as_secs_f32();
                                    let pulse = ((elapsed * 0.45).sin() + 1.0) * 0.5;
                                    let progress = 0.12 + pulse * 0.76;
                                    let label = match operation_kind {
                                        OcrLanguageOperationKind::Install => {
                                            Self::tr_lang(language, "Installing", "Dang cai")
                                        }
                                        OcrLanguageOperationKind::Remove => {
                                            Self::tr_lang(language, "Removing", "Dang xoa")
                                        }
                                    };
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(label)
                                                .small()
                                                .color(Color32::from_rgb(255, 216, 96)),
                                        );
                                        ui.add_sized(
                                            [140.0, 16.0],
                                            egui::ProgressBar::new(progress).show_percentage(),
                                        );
                                    });
                                    ui.ctx().request_repaint_after(Duration::from_millis(80));
                                } else if Self::settings_action_button(
                                    ui,
                                    Self::tr_lang(language, "Install", "Cai dat"),
                                )
                                .on_hover_text(Self::tr_lang(
                                    language,
                                    "Install the Windows OCR capability for this language directly.",
                                    "Cai Windows OCR capability cho ngon ngu nay truc tiep.",
                                ))
                                .clicked()
                                {
                                    self.install_ocr_language_capability(lang_code, display_name);
                                }
                            }

                            ui.end_row();
                        }
                        if clear_focus_after_scroll {
                            self.ocr_lang_settings_focus = None;
                        }
                    });
            });
        });
    }

}
