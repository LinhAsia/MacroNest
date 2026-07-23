use crate::hotkey;
use crate::model::*;
use crate::overlay::{OverlayCommand, UiCommand};
use crate::ui::{CrosshairApp, MouseCaptureKind, MouseMoveAbsoluteCaptureTarget};
use crate::window_list;
use eframe::egui::{
    self, Button, Color32, DragValue, RichText, Sense, Slider, TextBuffer, TextEdit, vec2,
};
use std::time::Duration;

#[cfg(windows)]
use crate::ui::{GetCursorPos, POINT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseInputBackendMode {
    Normal,
    Arduino,
    Interception,
}
#[derive(Clone, Default)]
struct MousePathTimelineOutcome {
    changed: bool,
    preview_selection: Option<Vec<MousePathEvent>>,
    preview_from_ms: Option<u64>,
    sync_preview: bool,
    selected_merge_source: u32,
    trim_range: Option<(u64, u64)>,
    split_at_ms: Option<u64>,
    merge_source_id: Option<u32>,
}

impl CrosshairApp {
    pub(crate) fn sync_mouse_path_preview(
        &mut self,
        preset_id: Option<u32>,
        events: Option<Vec<MousePathEvent>>,
        preview_from_ms: Option<u64>,
    ) {
        self.mouse_path_step_preview_preset_id = preset_id;
        let preview =
            preset_id.map(|active_id| (active_id, events.unwrap_or_default(), preview_from_ms));
        let _ = self
            .overlay_tx
            .send(OverlayCommand::PreviewMousePath(preview));
        crate::overlay::wake_command_queue();
    }

    pub(crate) fn clear_mouse_path_preview(&mut self) {
        if self.mouse_path_step_preview_preset_id.take().is_some() {
            let _ = self.overlay_tx.send(OverlayCommand::PreviewMousePath(None));
            crate::overlay::wake_command_queue();
        }
    }

    fn selected_mouse_input_backend_mode(&self) -> MouseInputBackendMode {
        if self.state.vision_settings.use_interception {
            MouseInputBackendMode::Interception
        } else if self.state.vision_settings.use_arduino_mouse {
            MouseInputBackendMode::Arduino
        } else {
            MouseInputBackendMode::Normal
        }
    }

    fn set_mouse_input_backend_mode(&mut self, mode: MouseInputBackendMode) -> bool {
        let use_arduino_mouse = matches!(mode, MouseInputBackendMode::Arduino);
        let use_interception = matches!(mode, MouseInputBackendMode::Interception);
        let changed = self.state.vision_settings.use_arduino_mouse != use_arduino_mouse
            || self.state.vision_settings.use_interception != use_interception;
        self.state.vision_settings.use_arduino_mouse = use_arduino_mouse;
        self.state.vision_settings.use_interception = use_interception;
        self.arduino_restore_emulation_after_flash = false;
        changed
    }

    fn render_mouse_input_mode_card_header(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        active: bool,
        open: &mut bool,
    ) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).strong());
            if active {
                ui.label(
                    RichText::new(self.tr("Active", "Đang hoạt động"))
                        .small()
                        .color(Color32::from_rgb(126, 224, 182)),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if Self::sound_style_toggle_button(
                    ui,
                    if *open {
                        Self::tr_lang(self.state.ui_language, "Hide", "Ẩn")
                    } else {
                        Self::tr_lang(self.state.ui_language, "Show", "Hiện")
                    },
                )
                .clicked()
                {
                    *open = !*open;
                }
            });
        });
    }

    pub(crate) fn render_mouse_panel(&mut self, ui: &mut egui::Ui) {
        self.poll_mouse_tool_jobs();
        ui.add_space(2.0);
        let language = self.state.ui_language;

        // --- Declarations ---

        ui.add_space(2.0);
        let language = self.state.ui_language;

        let mut remove_mouse_sensitivity_id = None;
        let mut next_mouse_sensitivity_capture_target = None;
        let mut cancel_active_capture_sensitivity = false;
        let mut mouse_sensitivity_live_sync = false;

        let mut remove_id = None;
        let mut next_capture_target = None;
        let mut live_sync = false;
        let mut cancel_active_capture = false;
        let mut draw_preset_id = None;
        let mut mouse_path_timeline_zoom = self.trim_timeline_zoom;
        let mouse_path_options: Vec<(u32, String)> = self
            .state
            .mouse_path_presets
            .iter()
            .map(|preset| (preset.id, preset.name.clone()))
            .collect();
        let mut preview_mouse_path_selection: Option<(u32, Vec<MousePathEvent>, Option<u64>)> =
            None;
        let mut trim_mouse_path_request: Option<(u32, u64, u64)> = None;
        let mut split_mouse_path_request: Option<(u32, u64)> = None;
        let mut merge_mouse_path_request: Option<(u32, u32)> = None;

        // --- Poll Background Jobs & Setup Backend Variables ---
        if self.arduino_flash_running {
            let flash_progress = self.arduino_flash_progress.lock().clone();
            if let Some(progress) = flash_progress {
                self.arduino_flash_status = progress;
            }
            let flash_result = {
                let mut res_guard = self.arduino_flash_result.lock();
                res_guard.take()
            };
            if let Some(res) = flash_result {
                self.arduino_flash_running = false;
                *self.arduino_flash_progress.lock() = None;
                self.refresh_arduino_ports();
                if self.arduino_restore_emulation_after_flash {
                    self.state.vision_settings.use_arduino_mouse = true;
                    self.arduino_restore_emulation_after_flash = false;
                    self.sync_vision_settings();
                }
                match res {
                    Ok(()) => {
                        if self.interception_driver_installed {
                            self.refresh_interception_for_arduino();
                            self.arduino_flash_status = self.tr(
                                "Flash complete. Restart Windows once to finish Arduino setup.",
                                "Nạp firmware xong. Hãy khởi động lại Windows một lần để hoàn tất thiết lập Arduino.",
                            ).to_owned();
                        } else {
                            self.arduino_flash_status = self
                                .tr("Flash Success!", "Nạp firmware thành công!")
                                .to_owned();
                        }
                    }
                    Err(e) => {
                        self.arduino_flash_status = format!("Error: {e}");
                    }
                }
            } else {
                ui.ctx().request_repaint();
            }
        }

        let refresh_txt = self.tr("Refresh Ports", "Làm mới cổng");
        let select_port_txt = self.tr("Select Port", "Chọn cổng");
        let com_port_lbl = self.tr("COM Port:", "Cổng COM:");

        let selected_port_exists = !self.state.vision_settings.arduino_com_port.is_empty()
            && self
                .arduino_available_ports
                .contains(&self.state.vision_settings.arduino_com_port);
        let (arduino_port_open, arduino_open_port, overlay_flash_in_progress) =
            crate::overlay::arduino_connection_snapshot();
        let selected_port = self.state.vision_settings.arduino_com_port.clone();
        let selected_mode = self.selected_mouse_input_backend_mode();
        let is_connected = selected_mode == MouseInputBackendMode::Arduino
            && arduino_port_open
            && arduino_open_port == selected_port;
        let should_recover_port = selected_mode == MouseInputBackendMode::Arduino
            && !self.arduino_flash_running
            && !is_connected
            && self
                .arduino_ports_last_refresh
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(5));
        if should_recover_port {
            self.refresh_arduino_ports();
        }
        if is_connected && self.arduino_flash_status.starts_with("Error:") {
            self.arduino_flash_status.clear();
        }
        let selected_port_text = if selected_port.is_empty() {
            "none".to_owned()
        } else {
            selected_port.clone()
        };
        let app_port_text = if arduino_open_port.is_empty() {
            "none".to_owned()
        } else {
            arduino_open_port.clone()
        };

        // --- Mouse Input Backend (Sticky at the top) ---
        ui.add_space(10.0);
        ui.label(RichText::new(self.tr("Mouse Input Backend", "Mouse Input Backend")).strong());

        let mut next_mode = selected_mode;
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut next_mode,
                MouseInputBackendMode::Normal,
                self.tr("Normal", "Bình thường"),
            );
            ui.selectable_value(
                &mut next_mode,
                MouseInputBackendMode::Arduino,
                self.tr("Arduino (Not Stable)", "Arduino (Chưa ổn định)"),
            );
            ui.selectable_value(
                &mut next_mode,
                MouseInputBackendMode::Interception,
                self.tr("Interception", "Interception"),
            );
        });

        let mut arduino_changed = false;
        if next_mode != selected_mode {
            if next_mode == MouseInputBackendMode::Interception && !self.interception_installed {
                self.status = self
                    .tr(
                        "Please download and install the Interception Driver wrapper first!",
                        "Hay tai va cai wrapper Interception Driver truoc!",
                    )
                    .to_owned();
                next_mode = selected_mode;
            } else {
                arduino_changed |= self.set_mouse_input_backend_mode(next_mode);
            }
        }

        ui.add_space(6.0);

        let normal_title = self.tr("Normal Windows Input", "Đầu vào chuột Windows thông thường");
        let normal_summary = self.tr(
            "Uses the standard Windows mouse path with no extra driver or hardware.",
            "Uses the standard Windows mouse path with no extra driver or hardware.",
        );
        let normal_hint = self.tr(
            "This mode follows the default SendInput and SetCursorPos path.",
            "This mode follows the default SendInput and SetCursorPos path.",
        );
        let mut normal_open = self.mouse_input_normal_open;
        Self::show_preset_card(ui, next_mode == MouseInputBackendMode::Normal, |ui| {
            self.render_mouse_input_mode_card_header(
                ui,
                normal_title,
                next_mode == MouseInputBackendMode::Normal,
                &mut normal_open,
            );
            if !normal_open {
                return;
            }
            ui.add_space(6.0);
            ui.label(RichText::new(normal_summary).small());
            ui.label(RichText::new(normal_hint).small().weak());
        });
        self.mouse_input_normal_open = normal_open;

        ui.add_space(6.0);

        let interception_progress = self.interception_download_job.as_ref().map(|_| {
            self.interception_download_progress
                .load(std::sync::atomic::Ordering::SeqCst) as f32
                / 1000.0
        });
        let interception_title = self.tr("Interception Driver", "Interception Driver");
        let interception_summary = self.tr(
            "Uses the Interception driver path for mouse movement and clicks in games.",
            "Uses the Interception driver path for mouse movement and clicks in games.",
        );
        let mut interception_open = self.mouse_input_interception_open;
        Self::show_preset_card(ui, next_mode == MouseInputBackendMode::Interception, |ui| {
            self.render_mouse_input_mode_card_header(
                ui,
                interception_title,
                next_mode == MouseInputBackendMode::Interception,
                &mut interception_open,
            );
            if !interception_open {
                return;
            }
            ui.add_space(6.0);
            let interception_status_color = if self.interception_status.contains("Active") {
                Color32::from_rgb(126, 224, 182)
            } else if self.interception_status.contains("Fallback") {
                Color32::from_rgb(248, 214, 102)
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(RichText::new(interception_summary).small());
            ui.label(
                RichText::new(&self.interception_status)
                    .small()
                    .color(interception_status_color),
            );
            ui.add_space(6.0);
            self.render_interception_driver_entry(ui, language, interception_progress);
        });
        self.mouse_input_interception_open = interception_open;

        ui.add_space(6.0);

        let arduino_panel_title = format!(
            "{} ({})",
            self.tr("Arduino ATmega32U4 Mouse", "Chuột Arduino ATmega32U4"),
            self.tr(
                "Not Stable / Under Development",
                "Chưa ổn định / Đang phát triển"
            )
        );
        let mut arduino_open = self.mouse_input_arduino_open;
        Self::show_preset_card(ui, next_mode == MouseInputBackendMode::Arduino, |ui| {
            self.render_mouse_input_mode_card_header(
                ui,
                &arduino_panel_title,
                false,
                &mut arduino_open,
            );
            if !arduino_open {
                return;
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if self.arduino_flash_running || overlay_flash_in_progress {
                    ui.label(
                        RichText::new("Flashing - port released")
                            .color(Color32::from_rgb(255, 206, 96)),
                    );
                } else if selected_port_exists
                    && next_mode == MouseInputBackendMode::Arduino
                    && !is_connected
                {
                    ui.label(
                        RichText::new(self.tr("Connecting...", "Đang kết nối..."))
                            .color(Color32::from_rgb(255, 206, 96)),
                    );
                } else if is_connected {
                    ui.label(
                        RichText::new(self.tr("Connected", "Đã kết nối"))
                            .color(Color32::from_rgb(126, 224, 182)),
                    );
                } else {
                    ui.label(
                        RichText::new(self.tr("Disconnected", "Đã ngắt kết nối"))
                            .color(Color32::from_rgb(255, 96, 96)),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(refresh_txt).clicked() {
                        self.refresh_arduino_ports();
                    }

                    let current_port = &mut self.state.vision_settings.arduino_com_port;
                    egui::ComboBox::from_id_salt("arduino_com_port_combo")
                        .width(120.0)
                        .selected_text(if current_port.is_empty() {
                            select_port_txt
                        } else {
                            current_port.as_str()
                        })
                        .show_ui(ui, |ui| {
                            for port in &self.arduino_available_ports {
                                let res = ui.selectable_value(current_port, port.clone(), port);
                                if res.changed() {
                                    arduino_changed = true;
                                }
                            }
                        });
                    ui.label(com_port_lbl);
                });
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let mut spoof_enabled = self.state.vision_settings.arduino_spoof_type > 0;
                let spoof_cb = ui.checkbox(
                    &mut spoof_enabled,
                    self.tr("Spoof USB Device", "Giả mạo thiết bị USB"),
                );
                if spoof_cb.changed() {
                    if spoof_enabled {
                        self.state.vision_settings.arduino_spoof_type = 1;
                    } else {
                        self.state.vision_settings.arduino_spoof_type = 0;
                    }
                    self.sync_vision_settings();
                    self.persist();
                }

                if spoof_enabled {
                    ui.add_space(8.0);
                    let mut current_type = self.state.vision_settings.arduino_spoof_type;
                    let resp = egui::ComboBox::from_id_salt("arduino_spoof_type_combo")
                        .width(160.0)
                        .selected_text(match current_type {
                            1 => "Logitech G Pro Wireless",
                            2 => "Razer DeathAdder V2",
                            3 => "SteelSeries Sensei",
                            _ => "Default Arduino",
                        })
                        .show_ui(ui, |ui| {
                            let mut changed = false;
                            changed |= ui
                                .selectable_value(&mut current_type, 0, "Default Arduino")
                                .changed();
                            changed |= ui
                                .selectable_value(&mut current_type, 1, "Logitech G Pro Wireless")
                                .changed();
                            changed |= ui
                                .selectable_value(&mut current_type, 2, "Razer DeathAdder V2")
                                .changed();
                            changed |= ui
                                .selectable_value(&mut current_type, 3, "SteelSeries Sensei")
                                .changed();
                            changed
                        });
                    if resp.inner.unwrap_or(false) {
                        self.state.vision_settings.arduino_spoof_type = current_type;
                        self.sync_vision_settings();
                        self.persist();
                    }
                }
            });

            if self.state.vision_settings.arduino_spoof_type > 0 {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(self.tr(
                        "⚠️ Note: You must click 'Auto-Flash Firmware' below to apply the spoofing to your Arduino.",
                        "⚠️ Lưu ý: Bạn cần nhấn 'Tự động nạp firmware' phía dưới để áp dụng spoof vào Arduino."
                    ))
                    .small()
                    .color(Color32::from_rgb(220, 180, 80)),
                );
            }

            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "{}: Serial COM | {}: {selected_port_text} | {}: {app_port_text}",
                    self.tr("Connection", "Kết nối"),
                    self.tr("Flash COM", "COM nạp firmware"),
                    self.tr("Active endpoint", "Cổng đang hoạt động"),
                ))
                .small()
                .weak(),
            );

            ui.add_space(4.0);
            let note_lbl = self.tr("Make sure you clicked 'Auto-Flash Firmware' at least once to program the connected board.", "Hãy nhấn 'Tự động nạp firmware' ít nhất một lần để lập trình bo mạch đang kết nối.");
            ui.label(
                RichText::new(note_lbl)
                    .small()
                    .weak()
                    .color(Color32::from_rgb(220, 180, 80)),
            );

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let test_button = ui.add_enabled(
                    is_connected && !self.arduino_flash_running,
                    egui::Button::new(self.tr("Test Arduino", "Kiểm tra Arduino")),
                );
                if test_button.clicked() {
                    self.arduino_flash_status = match crate::overlay::test_arduino_mouse_direct() {
                        Ok(()) => self
                            .tr(
                                "Arduino test passed: cursor moved right.",
                                "Kiểm tra Arduino thành công: con trỏ đã di chuyển sang phải.",
                            )
                            .to_owned(),
                        Err(error) => format!(
                            "{}: {error}",
                            self.tr("Arduino test failed", "Kiểm tra Arduino thất bại")
                        ),
                    };
                }
                ui.label(
                    RichText::new(self.tr(
                        "Direct hardware test - no Windows fallback",
                        "Kiểm tra phần cứng trực tiếp - không fallback sang Windows",
                    ))
                    .small()
                    .weak(),
                );
            });

            ui.add_space(8.0);

            if !self.arduino_tools_downloaded {
                if self.arduino_download_job.is_some() {
                    let progress = self
                        .arduino_download_progress
                        .load(std::sync::atomic::Ordering::SeqCst)
                        as f32
                        / 1000.0;
                    ui.horizontal(|ui| {
                        ui.label(self.tr("Downloading tools...", "Downloading tools..."));
                        ui.add(egui::ProgressBar::new(progress).show_percentage());
                    });
                    ui.ctx().request_repaint();
                } else {
                    let download_btn_lbl = self.tr(
                        "Download Flashing Tools & Firmware",
                        "Download Flashing Tools & Firmware",
                    );
                    if ui.button(download_btn_lbl).clicked() {
                        self.start_arduino_tools_download();
                    }
                }
            } else {
                ui.horizontal(|ui| {
                    let flash_btn_lbl = self.tr("Auto-Flash Firmware", "Tự động nạp firmware");
                    let flash_btn = ui.add_enabled(
                        !self.arduino_flash_running
                            && !self.state.vision_settings.arduino_com_port.is_empty(),
                        egui::Button::new(flash_btn_lbl),
                    );
                    if flash_btn.clicked() {
                        self.start_arduino_flash();
                    }

                    ui.add_space(8.0);
                    let delete_btn_lbl = self.tr("Delete Tools", "Xóa công cụ");
                    if ui.button(delete_btn_lbl).clicked() {
                        self.delete_arduino_tools();
                    }

                    if !self.arduino_flash_status.is_empty() {
                        ui.add_space(14.0);
                        ui.label(RichText::new(&self.arduino_flash_status).strong());
                    }
                });
            }

            ui.add_space(6.0);
        });
        self.mouse_input_arduino_open = arduino_open;

        // --- Separator ---
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // --- Scrollable Presets Section ---
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Sensitivity Presets
                ui.horizontal(|ui| {
                    if ui
                        .button(self.tr("+ Add sensitivity preset", "+ Add sensitivity preset"))
                        .clicked()
                    {
                        self.add_mouse_sensitivity_preset();
                        self.persist_mouse_sensitivity_presets();
                    }

                    if ui
                        .button(self.tr("+ Add path preset", "+ Add path preset"))
                        .clicked()
                    {
                        self.add_mouse_path_preset();
                        self.persist_mouse_path_presets();
                    }

                    if let Some(active_id) = self.active_mouse_record_preset_id {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(
                                crate::lang::translate(language, "Recording preset #{}")
                                    .unwrap_or("Recording preset #{}")
                                    .replace("{}", &active_id.to_string()),
                            )
                            .strong()
                            .color(Color32::from_rgb(255, 96, 96)),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(current) = Self::current_mouse_speed() {
                            ui.label(
                                RichText::new(format!(
                                    "{} {}",
                                    Self::tr_lang(
                                        language,
                                        "Current sensitivity:",
                                        "Current sensitivity:"
                                    ),
                                    current
                                ))
                                .strong()
                                .color(Color32::from_rgb(96, 172, 224)),
                            );
                            ui.add_space(14.0);
                        }

                        mouse_sensitivity_live_sync |= ui
                            .add(
                                DragValue::new(&mut self.state.mouse_sensitivity_restore_speed)
                                    .range(1..=20),
                            )
                            .changed();
                        ui.label(Self::tr_lang(language, "Speed", "Speed"));

                        mouse_sensitivity_live_sync |= ui
                            .checkbox(&mut self.state.mouse_sensitivity_restore_on_exit, "")
                            .changed();
                        ui.label(
                            RichText::new(Self::tr_lang(
                                language,
                                "Restore sensitivity on exit",
                                "Restore sensitivity on exit",
                            ))
                            .strong(),
                        );
                    });
                });

                ui.add_space(16.0);
                ui.label(
                    RichText::new(Self::tr_lang(language, "Sensitivity", "Sensitivity"))
                        .strong()
                        .size(14.0),
                );
                ui.add_space(4.0);

                for index in 0..self.state.mouse_sensitivity_presets.len() {
                    let active_capture_target = self.capture_target.clone();
                    let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
                    let preset = &mut self.state.mouse_sensitivity_presets[index];
                    preset.target_window_title = None;
                    preset.extra_target_window_titles.clear();
                    preset.enabled =
                        preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
                    Self::show_preset_card(ui, preset.enabled, |ui| {
                        ui.horizontal(|ui| {
                            let mut disabled_by_button = false;
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
                            mouse_sensitivity_live_sync |= response.changed();

                            let capture_target =
                                CaptureRequest::MouseSensitivityPresetHotkey(preset.id);
                            mouse_sensitivity_live_sync |= Self::render_preset_trigger_chips(
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

                            if Self::sound_style_toggle_button(
                                ui,
                                Self::tr_lang(language, "Apply", "Apply"),
                            )
                            .clicked()
                            {
                                let _ = self
                                    .overlay_tx
                                    .send(OverlayCommand::ApplyMouseSensitivityPreset(preset.id));
                            }
                            if Self::sound_style_toggle_button(
                                ui,
                                Self::tr_lang(language, "Restore", "Restore"),
                            )
                            .clicked()
                            {
                                let _ = self
                                    .overlay_tx
                                    .send(OverlayCommand::RestoreMouseSensitivity);
                            }
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
                                            cancel_active_capture_sensitivity = true;
                                        } else {
                                            next_mouse_sensitivity_capture_target = Some((
                                                capture_target,
                                                crate::lang::translate(
                                                    language,
                                                    "Capturing hotkey for {}.",
                                                )
                                                .unwrap_or("Capturing hotkey for {}.")
                                                .replace("{}", &preset.name),
                                            ));
                                        }
                                    }
                                    if btn_response.secondary_clicked() {
                                        preset.hotkey = None;
                                        preset.trigger_keys.clear();
                                        preset.enabled = false;
                                        disabled_by_button = true;
                                        mouse_sensitivity_live_sync = true;
                                    }

                                    if Self::sound_style_remove_button(ui).clicked() {
                                        remove_mouse_sensitivity_id = Some(preset.id);
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
                                        mouse_sensitivity_live_sync = true;
                                    }
                                },
                            );
                            if disabled_by_button {
                                let _ = self
                                    .overlay_tx
                                    .send(OverlayCommand::RestoreMouseSensitivity);
                            }
                        });
                        if preset.collapsed {
                            return;
                        }
                        egui::Grid::new((preset.id, "mouse-sensitivity-grid"))
                            .num_columns(2)
                            .spacing([14.0, 8.0])
                            .show(ui, |ui| {
                                ui.label(Self::tr_lang(language, "Speed", "Speed"));
                                mouse_sensitivity_live_sync |= ui
                                    .add(Slider::new(&mut preset.speed, 1..=20).show_value(true))
                                    .changed();
                                ui.end_row();
                            });
                    });
                }

                // Mouse Path Presets
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(Self::tr_lang(language, "Mouse Path", "Mouse Path")).strong(),
                    );
                });

                for index in 0..self.state.mouse_path_presets.len() {
                    let active_capture_target = self.capture_target.clone();
                    let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
                    let preset = &mut self.state.mouse_path_presets[index];
                    if self.mouse_path_timeline_initialized.insert(preset.id) {
                        Self::reset_mouse_path_timeline_state(ui.ctx(), preset.id, &preset.events);
                    }
                    Self::show_preset_card(ui, false, |ui| {
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
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
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
                                },
                            );
                        });
                        if preset.collapsed {
                            return;
                        }
                        egui::Grid::new((preset.id, "mouse-path-grid"))
                            .num_columns(2)
                            .spacing([14.0, 8.0])
                            .show(ui, |ui| {
                                ui.label(Self::tr_lang(language, "Record Hotkey", "Record Hotkey"));
                                ui.horizontal_wrapped(|ui| {
                                    let capture_target =
                                        CaptureRequest::MousePathRecordHotkey(preset.id);
                                    let (begin_capture, cancel_capture) =
                                        Self::render_hotkey_capture_control(
                                            ui,
                                            language,
                                            &mut preset.record_hotkey,
                                            &capture_target,
                                            active_capture_target.as_ref(),
                                            pending_combo_keys.as_ref(),
                                            &mut live_sync,
                                        );
                                    if begin_capture {
                                        next_capture_target = Some((
                                            capture_target,
                                            crate::lang::translate(
                                                language,
                                                "Capturing record hotkey for {}.",
                                            )
                                            .unwrap_or("Capturing record hotkey for {}.")
                                            .replace("{}", &preset.name),
                                        ));
                                    }
                                    if cancel_capture {
                                        cancel_active_capture = true;
                                    }
                                });
                                ui.end_row();

                                ui.label(Self::tr_lang(language, "Draw Path", "Vẽ Path"));
                                ui.horizontal(|ui| {
                                    let draw_text = Self::tr_lang(
                                        language,
                                        "Draw on screen",
                                        "Vẽ trên màn hình",
                                    );
                                    let draw_btn = Button::new(RichText::new(draw_text).strong());
                                    if ui
                                        .add(draw_btn)
                                        .on_hover_text(Self::tr_lang(
                                            language,
                                            "Hide app and draw path with mouse",
                                            "Ẩn ứng dụng và vẽ đường di chuột bằng chuột",
                                        ))
                                        .clicked()
                                    {
                                        draw_preset_id = Some(preset.id);
                                    }
                                });
                                ui.end_row();

                                if self.active_mouse_record_preset_id == Some(preset.id) {
                                    ui.label("");
                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            language,
                                            "Recording via hotkey...",
                                            "Recording via hotkey...",
                                        ))
                                        .color(Color32::from_rgb(255, 96, 96))
                                        .strong(),
                                    );
                                    ui.end_row();
                                }

                                ui.label("");
                                ui.horizontal_wrapped(|ui| {
                                    live_sync |= ui
                                        .checkbox(
                                            &mut preset.replay_relative_motion,
                                            Self::tr_lang(
                                                language,
                                                "Relative motion",
                                                "Relative motion",
                                            ),
                                        )
                                        .changed();
                                });
                                ui.end_row();
                            });
                        ui.add_space(6.0);
                        let preview_events =
                            Self::preview_mouse_path_events(ui.ctx(), preset.id, &preset.events);
                        Self::render_mouse_path_preview(ui, language, &preview_events, 240.0);
                        ui.add_space(8.0);
                        let preset_hovered = ui.rect_contains_pointer(ui.min_rect());
                        let timeline_outcome = Self::render_mouse_path_timeline_editor(
                            ui,
                            language,
                            preset.id,
                            &preset.events,
                            &mouse_path_options,
                            &mut mouse_path_timeline_zoom,
                            preset_hovered,
                            self.mouse_path_merge_selection
                                .get(&preset.id)
                                .copied()
                                .unwrap_or(0),
                        );
                        if timeline_outcome.selected_merge_source == 0 {
                            self.mouse_path_merge_selection.remove(&preset.id);
                        } else {
                            self.mouse_path_merge_selection
                                .insert(preset.id, timeline_outcome.selected_merge_source);
                        }
                        if let Some(events) = timeline_outcome.preview_selection {
                            preview_mouse_path_selection =
                                Some((preset.id, events, timeline_outcome.preview_from_ms));
                        }
                        if timeline_outcome.sync_preview
                            && self.mouse_path_step_preview_preset_id == Some(preset.id)
                        {
                            let preview_events = Self::preview_mouse_path_events(
                                ui.ctx(),
                                preset.id,
                                &preset.events,
                            );
                            let preview_from_ms = Self::mouse_path_preview_from_ms(
                                ui.ctx(),
                                preset.id,
                                &preset.events,
                            );
                            preview_mouse_path_selection =
                                Some((preset.id, preview_events, Some(preview_from_ms)));
                        }
                        if let Some((start_ms, end_ms)) = timeline_outcome.trim_range {
                            trim_mouse_path_request = Some((preset.id, start_ms, end_ms));
                        }
                        if let Some(split_at_ms) = timeline_outcome.split_at_ms {
                            split_mouse_path_request = Some((preset.id, split_at_ms));
                        }
                        if let Some(source_id) = timeline_outcome.merge_source_id {
                            merge_mouse_path_request = Some((preset.id, source_id));
                        }
                    });
                }
            });

        // --- Post UI Side-Effects ---
        if let Some(remove_mouse_sensitivity_id) = remove_mouse_sensitivity_id {
            self.state
                .mouse_sensitivity_presets
                .retain(|preset| preset.id != remove_mouse_sensitivity_id);
            mouse_sensitivity_live_sync = true;
        }
        if let Some((target, status)) = next_mouse_sensitivity_capture_target {
            self.begin_capture(target, status);
        }
        if mouse_sensitivity_live_sync {
            self.persist_mouse_sensitivity_presets();
            self.sync_mouse_sensitivity_settings();
            self.persist();
        }
        if cancel_active_capture_sensitivity {
            self.cancel_capture();
        }

        self.trim_timeline_zoom = mouse_path_timeline_zoom;

        if let Some((preset_id, events, preview_from_ms)) = preview_mouse_path_selection {
            let has_move = events
                .iter()
                .any(|event| matches!(event.kind, MousePathEventKind::Move));
            self.sync_mouse_path_preview(
                has_move.then_some(preset_id),
                has_move.then_some(events),
                preview_from_ms,
            );
        }
        if let Some((preset_id, start_ms, end_ms)) = trim_mouse_path_request {
            let mut pending_trim_preview: Option<(
                Option<u32>,
                Option<Vec<MousePathEvent>>,
                Option<u64>,
            )> = None;
            if let Some(preset) = self
                .state
                .mouse_path_presets
                .iter_mut()
                .find(|preset| preset.id == preset_id)
            {
                preset.events = Self::slice_mouse_path_events(&preset.events, start_ms, end_ms);
                Self::reset_mouse_path_timeline_state(ui.ctx(), preset_id, &preset.events);
                if self.mouse_path_step_preview_preset_id == Some(preset_id) {
                    let has_move = preset
                        .events
                        .iter()
                        .any(|event| matches!(event.kind, MousePathEventKind::Move));
                    if has_move {
                        pending_trim_preview =
                            Some((Some(preset_id), Some(preset.events.clone()), Some(0)));
                    } else {
                        pending_trim_preview = Some((None, None, None));
                    }
                }
                live_sync = true;
            }
            if let Some((preview_preset_id, preview_events, preview_from_ms)) = pending_trim_preview
            {
                if preview_preset_id.is_some() {
                    self.sync_mouse_path_preview(
                        preview_preset_id,
                        preview_events,
                        preview_from_ms,
                    );
                } else {
                    self.clear_mouse_path_preview();
                }
            }
        }
        if let Some((preset_id, split_at_ms)) = split_mouse_path_request {
            if self.split_mouse_path_preset(preset_id, split_at_ms) {
                self.mouse_path_merge_selection.remove(&preset_id);
                if let Some(preset) = self
                    .state
                    .mouse_path_presets
                    .iter()
                    .find(|preset| preset.id == preset_id)
                {
                    Self::reset_mouse_path_timeline_state(ui.ctx(), preset_id, &preset.events);
                }
                live_sync = true;
            }
        }
        if let Some((target_id, source_id)) = merge_mouse_path_request {
            if self.merge_mouse_path_presets(target_id, source_id) {
                self.mouse_path_merge_selection.remove(&target_id);
                self.mouse_path_merge_selection.remove(&source_id);
                live_sync = true;
            }
        }

        if let Some(rem_id) = remove_id {
            self.mouse_path_timeline_initialized.remove(&rem_id);
            self.mouse_path_merge_selection.remove(&rem_id);
            Self::clear_mouse_path_timeline_state(ui.ctx(), rem_id);
            if self.mouse_path_step_preview_preset_id == Some(rem_id) {
                self.clear_mouse_path_preview();
            }
            if self.mouse_path_draw_capture_preset_id == Some(rem_id) {
                self.mouse_path_draw_capture_preset_id = None;
                self.restore_mouse_path_draw_capture_window(ui.ctx());
            }
            if self.active_mouse_record_preset_id == Some(rem_id) {
                self.active_mouse_record_preset_id = None;
            }
            self.state
                .mouse_path_presets
                .retain(|preset| preset.id != rem_id);
            if self.clear_mouse_path_step_references(rem_id) {
                self.persist_macro_presets();
            }
            live_sync = true;
        }
        if let Some((target, status)) = next_capture_target {
            self.begin_capture(target, status);
        }
        if cancel_active_capture {
            self.cancel_capture();
        }
        if let Some(preset_id) = draw_preset_id {
            self.begin_mouse_path_draw_capture(ui.ctx(), preset_id);
        }
        if live_sync {
            self.persist_mouse_path_presets();
        }

        if arduino_changed {
            self.sync_vision_settings();
            self.persist();
        }
    }
    pub(crate) fn render_mouse_path_preview(
        ui: &mut egui::Ui,
        language: UiLanguage,
        events: &[MousePathEvent],
        _desired_height: f32,
    ) {
        let screen_size = Self::screen_size();
        let aspect_ratio = if screen_size.y > 0.0 {
            screen_size.x / screen_size.y
        } else {
            16.0 / 9.0
        };
        let width = ui.available_width();
        let height = width / aspect_ratio;
        let max_height = 480.0;
        let (desired_width, desired_height) = if height > max_height {
            (max_height * aspect_ratio, max_height)
        } else {
            (width, height)
        };
        let (canvas_rect, _) = ui.allocate_exact_size(vec2(width, desired_height), Sense::hover());
        let draw_rect =
            egui::Rect::from_center_size(canvas_rect.center(), vec2(desired_width, desired_height))
                .shrink(8.0);
        ui.painter().rect_filled(
            draw_rect,
            8.0,
            Color32::from_rgba_premultiplied(18, 24, 22, 220),
        );
        ui.painter().rect_stroke(
            draw_rect,
            8.0,
            egui::Stroke::new(1.0, Color32::from_rgb(104, 148, 124)),
            egui::StrokeKind::Outside,
        );

        let moves = events
            .iter()
            .filter(|event| matches!(event.kind, MousePathEventKind::Move))
            .collect::<Vec<_>>();
        if moves.len() < 2 {
            ui.painter().text(
                draw_rect.center(),
                egui::Align2::CENTER_CENTER,
                Self::tr_lang(
                    language,
                    "Record a mouse path to preview it here",
                    "Record a mouse path to preview it here",
                ),
                egui::FontId::proportional(16.0),
                Color32::from_rgb(210, 210, 210),
            );
            return;
        }

        let scale_x = draw_rect.width() / screen_size.x.max(1.0);
        let scale_y = draw_rect.height() / screen_size.y.max(1.0);
        let to_pos = |event: &MousePathEvent| {
            egui::pos2(
                draw_rect.left() + event.x as f32 * scale_x,
                draw_rect.top() + event.y as f32 * scale_y,
            )
        };
        let mut last = None;
        for event in moves {
            let current = to_pos(event);
            if let Some(prev) = last {
                ui.painter().line_segment(
                    [prev, current],
                    egui::Stroke::new(2.0, Color32::from_rgb(255, 92, 92)),
                );
            }
            last = Some(current);
        }
    }

    fn mouse_path_total_duration_ms(events: &[MousePathEvent]) -> u64 {
        events
            .iter()
            .fold(0u64, |total, event| total.saturating_add(event.delay_ms))
    }

    fn preview_mouse_path_events(
        ctx: &egui::Context,
        preset_id: u32,
        events: &[MousePathEvent],
    ) -> Vec<MousePathEvent> {
        let total_ms = Self::mouse_path_total_duration_ms(events).max(1);
        let trim_start_id = egui::Id::new((preset_id, "mouse-path-trim-start"));
        let trim_end_id = egui::Id::new((preset_id, "mouse-path-trim-end"));
        let trim_start_ms = ctx
            .data(|data| data.get_temp::<u64>(trim_start_id))
            .unwrap_or(0)
            .min(total_ms);
        let trim_end_ms = ctx
            .data(|data| data.get_temp::<u64>(trim_end_id))
            .unwrap_or(total_ms)
            .clamp(trim_start_ms, total_ms);
        Self::slice_mouse_path_events(events, trim_start_ms, trim_end_ms)
    }

    fn mouse_path_preview_from_ms(
        ctx: &egui::Context,
        preset_id: u32,
        events: &[MousePathEvent],
    ) -> u64 {
        let total_ms = Self::mouse_path_total_duration_ms(events).max(1);
        let trim_start_id = egui::Id::new((preset_id, "mouse-path-trim-start"));
        let trim_end_id = egui::Id::new((preset_id, "mouse-path-trim-end"));
        let playhead_id = egui::Id::new((preset_id, "mouse-path-playhead"));
        let trim_start_ms = ctx
            .data(|data| data.get_temp::<u64>(trim_start_id))
            .unwrap_or(0)
            .min(total_ms);
        let trim_end_ms = ctx
            .data(|data| data.get_temp::<u64>(trim_end_id))
            .unwrap_or(total_ms)
            .clamp(trim_start_ms, total_ms);
        let playhead_ms = ctx
            .data(|data| data.get_temp::<u64>(playhead_id))
            .unwrap_or(trim_start_ms)
            .clamp(trim_start_ms, trim_end_ms);
        playhead_ms.saturating_sub(trim_start_ms)
    }

    pub(crate) fn reset_mouse_path_timeline_state(
        ctx: &egui::Context,
        preset_id: u32,
        events: &[MousePathEvent],
    ) {
        let total_ms = Self::mouse_path_total_duration_ms(events).max(1);
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new((preset_id, "mouse-path-trim-start")), 0u64);
            data.insert_temp(egui::Id::new((preset_id, "mouse-path-trim-end")), total_ms);
            data.insert_temp(egui::Id::new((preset_id, "mouse-path-playhead")), 0u64);
            data.insert_temp(egui::Id::new((preset_id, "mouse-path-scroll")), 0.0f32);
            data.remove::<bool>(egui::Id::new((
                preset_id,
                "mouse-path-trim-hotkey-adjusting",
            )));
        });
    }

    fn clear_mouse_path_timeline_state(ctx: &egui::Context, preset_id: u32) {
        ctx.data_mut(|data| {
            data.remove::<u64>(egui::Id::new((preset_id, "mouse-path-trim-start")));
            data.remove::<u64>(egui::Id::new((preset_id, "mouse-path-trim-end")));
            data.remove::<u64>(egui::Id::new((preset_id, "mouse-path-playhead")));
            data.remove::<f32>(egui::Id::new((preset_id, "mouse-path-scroll")));
            data.remove::<u32>(egui::Id::new((preset_id, "mouse-path-merge-source")));
            data.remove::<bool>(egui::Id::new((
                preset_id,
                "mouse-path-trim-hotkey-adjusting",
            )));
        });
    }

    fn interpolate_mouse_path_event(
        start: &MousePathEvent,
        end: &MousePathEvent,
        start_ms: u64,
        end_ms: u64,
        target_ms: u64,
    ) -> MousePathEvent {
        let span_ms = end_ms.saturating_sub(start_ms).max(1);
        let offset_ms = target_ms.saturating_sub(start_ms).min(span_ms);
        let ratio = offset_ms as f32 / span_ms as f32;
        MousePathEvent {
            kind: MousePathEventKind::Move,
            x: start.x + ((end.x - start.x) as f32 * ratio).round() as i32,
            y: start.y + ((end.y - start.y) as f32 * ratio).round() as i32,
            delay_ms: 0,
        }
    }

    fn slice_mouse_path_events(
        events: &[MousePathEvent],
        start_ms: u64,
        end_ms: u64,
    ) -> Vec<MousePathEvent> {
        if events.is_empty() || start_ms >= end_ms {
            return Vec::new();
        }
        let mut elapsed_ms = 0u64;
        let mut previous_event: Option<(&MousePathEvent, u64)> = None;
        let mut previous_kept_at = None;
        let mut sliced = Vec::new();
        for event in events {
            let current_ms = elapsed_ms.saturating_add(event.delay_ms);
            if let Some((prev_event, prev_ms)) = previous_event
                && matches!(prev_event.kind, MousePathEventKind::Move)
                && matches!(event.kind, MousePathEventKind::Move)
                && prev_ms < current_ms
            {
                if start_ms > prev_ms && start_ms < current_ms {
                    let mut boundary = Self::interpolate_mouse_path_event(
                        prev_event, event, prev_ms, current_ms, start_ms,
                    );
                    boundary.delay_ms = previous_kept_at
                        .map(|kept_ms| start_ms.saturating_sub(kept_ms))
                        .unwrap_or(0);
                    previous_kept_at = Some(start_ms);
                    sliced.push(boundary);
                }
                if end_ms > prev_ms && end_ms < current_ms {
                    let mut boundary = Self::interpolate_mouse_path_event(
                        prev_event, event, prev_ms, current_ms, end_ms,
                    );
                    boundary.delay_ms = previous_kept_at
                        .map(|kept_ms| end_ms.saturating_sub(kept_ms))
                        .unwrap_or(0);
                    sliced.push(boundary);
                    break;
                }
            }
            elapsed_ms = elapsed_ms.saturating_add(event.delay_ms);
            if elapsed_ms < start_ms || elapsed_ms > end_ms {
                previous_event = Some((event, elapsed_ms));
                continue;
            }
            let mut next_event = event.clone();
            next_event.delay_ms = previous_kept_at
                .map(|prev| elapsed_ms.saturating_sub(prev))
                .unwrap_or(0);
            previous_kept_at = Some(elapsed_ms);
            sliced.push(next_event);
            previous_event = Some((event, elapsed_ms));
        }
        sliced
    }

    fn render_mouse_path_timeline_editor(
        ui: &mut egui::Ui,
        language: UiLanguage,
        preset_id: u32,
        events: &[MousePathEvent],
        preset_options: &[(u32, String)],
        timeline_zoom: &mut f32,
        preset_hovered: bool,
        initial_merge_source: u32,
    ) -> MousePathTimelineOutcome {
        let mut outcome = MousePathTimelineOutcome::default();
        if events.is_empty() {
            return outcome;
        }

        let total_ms = Self::mouse_path_total_duration_ms(events).max(1);
        let total_ms_f32 = total_ms as f32;
        *timeline_zoom = (*timeline_zoom).clamp(1.0, 8.0);

        let trim_start_id = egui::Id::new((preset_id, "mouse-path-trim-start"));
        let trim_end_id = egui::Id::new((preset_id, "mouse-path-trim-end"));
        let playhead_id = egui::Id::new((preset_id, "mouse-path-playhead"));
        let zoom_scroll_offset_id = egui::Id::new((preset_id, "mouse-path-scroll"));
        let trim_hotkey_adjusting_id =
            egui::Id::new((preset_id, "mouse-path-trim-hotkey-adjusting"));

        let mut trim_start_ms = ui
            .ctx()
            .data(|data| data.get_temp::<u64>(trim_start_id))
            .unwrap_or(0)
            .min(total_ms);
        let mut trim_end_ms = ui
            .ctx()
            .data(|data| data.get_temp::<u64>(trim_end_id))
            .unwrap_or(total_ms)
            .clamp(trim_start_ms, total_ms);
        let mut playhead_ms = ui
            .ctx()
            .data(|data| data.get_temp::<u64>(playhead_id))
            .unwrap_or(trim_start_ms)
            .clamp(trim_start_ms, trim_end_ms);
        let mut selected_merge_source = initial_merge_source;

        ui.horizontal(|ui| {
            ui.label(Self::material_icon_text(0xe14e, 14.0));
            ui.label(
                RichText::new(Self::tr_lang(language, "Timeline", "Timeline"))
                    .size(13.0)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("{:.1}x", *timeline_zoom))
                    .size(12.0)
                    .color(ui.visuals().weak_text_color()),
            );
        });

        let viewport_width = (ui.available_width() - 24.0).max(296.0);
        let timeline_size = vec2((viewport_width * *timeline_zoom).max(viewport_width), 88.0);
        let stored_scroll_offset = ui
            .ctx()
            .data(|data| data.get_temp::<f32>(zoom_scroll_offset_id))
            .unwrap_or(0.0);
        let mut next_scroll_offset = stored_scroll_offset;
        let mut hovered_timeline = false;
        let mut pointer_time_ms = None;

        egui::ScrollArea::horizontal()
            .id_salt((preset_id, "mouse-path-timeline-scroll"))
            .max_height(timeline_size.y + 8.0)
            .show_viewport(ui, |ui, viewport| {
                let (rect, response) =
                    ui.allocate_exact_size(timeline_size, Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 18.0, Color32::from_rgb(28, 31, 34));
                painter.rect_stroke(
                    rect,
                    18.0,
                    egui::Stroke::new(1.0, Color32::from_rgb(88, 104, 118)),
                    egui::StrokeKind::Outside,
                );

                let mut elapsed_ms = 0u64;
                let mut last_move_pos = None;
                for event in events {
                    elapsed_ms = elapsed_ms.saturating_add(event.delay_ms);
                    let t = elapsed_ms as f32 / total_ms_f32;
                    let x = rect.left() + rect.width() * t.clamp(0.0, 1.0);
                    let y = match event.kind {
                        MousePathEventKind::Move => rect.center().y,
                        MousePathEventKind::LeftDown | MousePathEventKind::LeftUp => {
                            rect.top() + 18.0
                        }
                        MousePathEventKind::RightDown | MousePathEventKind::RightUp => {
                            rect.top() + 34.0
                        }
                        MousePathEventKind::MiddleDown | MousePathEventKind::MiddleUp => {
                            rect.top() + 50.0
                        }
                        MousePathEventKind::WheelUp | MousePathEventKind::WheelDown => {
                            rect.bottom() - 18.0
                        }
                    };
                    let color = match event.kind {
                        MousePathEventKind::Move => Color32::from_rgb(88, 194, 255),
                        MousePathEventKind::LeftDown | MousePathEventKind::LeftUp => {
                            Color32::from_rgb(255, 208, 92)
                        }
                        MousePathEventKind::RightDown | MousePathEventKind::RightUp => {
                            Color32::from_rgb(255, 124, 124)
                        }
                        MousePathEventKind::MiddleDown | MousePathEventKind::MiddleUp => {
                            Color32::from_rgb(162, 132, 255)
                        }
                        MousePathEventKind::WheelUp | MousePathEventKind::WheelDown => {
                            Color32::from_rgb(126, 224, 182)
                        }
                    };
                    let pos = egui::pos2(x, y);
                    if matches!(event.kind, MousePathEventKind::Move) {
                        if let Some(prev) = last_move_pos {
                            painter.line_segment([prev, pos], egui::Stroke::new(2.0, color));
                        }
                        last_move_pos = Some(pos);
                    } else {
                        painter.circle_filled(pos, 4.0, color);
                    }
                }

                let start_x = rect.left()
                    + rect.width() * (trim_start_ms as f32 / total_ms_f32).clamp(0.0, 1.0);
                let end_x = rect.left()
                    + rect.width() * (trim_end_ms as f32 / total_ms_f32).clamp(0.0, 1.0);
                let playhead_x = rect.left()
                    + rect.width() * (playhead_ms as f32 / total_ms_f32).clamp(0.0, 1.0);
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(start_x, rect.top()),
                        egui::pos2(end_x.max(start_x + 2.0), rect.bottom()),
                    ),
                    18.0,
                    Color32::from_rgba_premultiplied(72, 198, 120, 48),
                );
                painter.line_segment(
                    [
                        egui::pos2(start_x, rect.top()),
                        egui::pos2(start_x, rect.bottom()),
                    ],
                    egui::Stroke::new(2.0, Color32::from_rgb(255, 232, 96)),
                );
                painter.line_segment(
                    [
                        egui::pos2(end_x, rect.top()),
                        egui::pos2(end_x, rect.bottom()),
                    ],
                    egui::Stroke::new(2.0, Color32::from_rgb(255, 232, 96)),
                );
                painter.line_segment(
                    [
                        egui::pos2(playhead_x, rect.top() + 6.0),
                        egui::pos2(playhead_x, rect.bottom() - 6.0),
                    ],
                    egui::Stroke::new(2.0, Color32::WHITE),
                );

                let hovered_pointer_pos = response.hover_pos();
                hovered_timeline = response.hovered() || hovered_pointer_pos.is_some();
                if let Some(pointer) = hovered_pointer_pos {
                    let ratio = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    pointer_time_ms = Some((ratio * total_ms_f32).round() as u64);
                }
                if (response.clicked() || response.dragged())
                    && let Some(pointer) = response.interact_pointer_pos()
                {
                    let ratio = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    playhead_ms = (ratio * total_ms_f32).round() as u64;
                    playhead_ms = playhead_ms.clamp(trim_start_ms, trim_end_ms);
                    outcome.changed = true;
                    outcome.sync_preview = true;
                }

                if hovered_timeline
                    && ui.input(|input| input.modifiers.ctrl && input.raw_scroll_delta.y != 0.0)
                {
                    let delta = ui.input(|input| input.raw_scroll_delta.y);
                    let factor = if delta > 0.0 { 1.1 } else { 1.0 / 1.1 };
                    *timeline_zoom = (*timeline_zoom * factor).clamp(1.0, 8.0);
                    outcome.changed = true;
                }

                let move_left = hovered_timeline && ui.input(|input| input.key_down(egui::Key::Q));
                let move_right = hovered_timeline && ui.input(|input| input.key_down(egui::Key::W));
                if let Some(pointer_time_ms) = pointer_time_ms {
                    if move_left {
                        trim_start_ms = pointer_time_ms.min(trim_end_ms.saturating_sub(1));
                        outcome.changed = true;
                        outcome.sync_preview = true;
                        ui.ctx().request_repaint();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(trim_hotkey_adjusting_id, true));
                    }
                    if move_right {
                        trim_end_ms = pointer_time_ms.max(trim_start_ms.saturating_add(1));
                        outcome.changed = true;
                        outcome.sync_preview = true;
                        ui.ctx().request_repaint();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(trim_hotkey_adjusting_id, true));
                    }
                }
                if !move_left
                    && !move_right
                    && ui
                        .ctx()
                        .data(|data| data.get_temp::<bool>(trim_hotkey_adjusting_id))
                        .unwrap_or(false)
                {
                    ui.ctx()
                        .data_mut(|data| data.remove::<bool>(trim_hotkey_adjusting_id));
                }
                if hovered_timeline && ui.input(|input| input.key_pressed(egui::Key::A)) {
                    playhead_ms = playhead_ms.saturating_sub((total_ms / 100).max(1));
                    playhead_ms = playhead_ms.clamp(trim_start_ms, trim_end_ms);
                    outcome.changed = true;
                    outcome.sync_preview = true;
                }
                if hovered_timeline && ui.input(|input| input.key_pressed(egui::Key::D)) {
                    playhead_ms = playhead_ms.saturating_add((total_ms / 100).max(1));
                    playhead_ms = playhead_ms.clamp(trim_start_ms, trim_end_ms);
                    outcome.changed = true;
                    outcome.sync_preview = true;
                }
                let preview_hotkeys_active = preset_hovered || hovered_timeline;
                if preview_hotkeys_active && ui.input(|input| input.key_pressed(egui::Key::Space)) {
                    outcome.preview_selection = Some(Self::slice_mouse_path_events(
                        events,
                        trim_start_ms,
                        trim_end_ms,
                    ));
                    outcome.preview_from_ms = Some(playhead_ms.saturating_sub(trim_start_ms));
                }
                if preview_hotkeys_active && ui.input(|input| input.key_pressed(egui::Key::S)) {
                    outcome.preview_selection = Some(Self::slice_mouse_path_events(
                        events,
                        trim_start_ms,
                        trim_end_ms,
                    ));
                    outcome.preview_from_ms = Some(0);
                }

                next_scroll_offset = viewport.left().max(0.0);
            });

        ui.ctx()
            .data_mut(|data| data.insert_temp(zoom_scroll_offset_id, next_scroll_offset));
        ui.ctx()
            .data_mut(|data| data.insert_temp(trim_start_id, trim_start_ms));
        ui.ctx()
            .data_mut(|data| data.insert_temp(trim_end_id, trim_end_ms));
        ui.ctx()
            .data_mut(|data| data.insert_temp(playhead_id, playhead_ms));

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{} {}",
                Self::tr_lang(language, "Start", "Start"),
                Self::format_ms(trim_start_ms)
            ));
            ui.separator();
            ui.label(format!(
                "{} {}",
                Self::tr_lang(language, "End", "End"),
                Self::format_ms(trim_end_ms)
            ));
            ui.separator();
            ui.label(format!(
                "{} {}",
                Self::tr_lang(language, "Playhead", "Playhead"),
                Self::format_ms(playhead_ms)
            ));
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(Self::tr_lang(
                    language,
                    "Preview selection",
                    "Preview selection",
                ))
                .clicked()
            {
                outcome.preview_selection = Some(Self::slice_mouse_path_events(
                    events,
                    trim_start_ms,
                    trim_end_ms,
                ));
                outcome.preview_from_ms = Some(playhead_ms);
            }
            if ui
                .button(Self::tr_lang(
                    language,
                    "Trim to selection",
                    "Trim to selection",
                ))
                .clicked()
            {
                outcome.trim_range = Some((trim_start_ms, trim_end_ms));
            }
            if ui
                .button(Self::tr_lang(
                    language,
                    "Split at playhead",
                    "Split at playhead",
                ))
                .clicked()
            {
                outcome.split_at_ms = Some(playhead_ms);
            }
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(Self::tr_lang(language, "Merge from", "Merge from"));
            let selected_label = preset_options
                .iter()
                .find(|(id, _)| *id == selected_merge_source)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| {
                    Self::tr_lang(language, "Select preset", "Select preset").to_owned()
                });
            egui::ComboBox::from_id_salt((preset_id, "mouse-path-merge-select"))
                .width(170.0)
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    for (other_id, other_name) in preset_options {
                        if *other_id == preset_id {
                            continue;
                        }
                        if ui
                            .selectable_value(
                                &mut selected_merge_source,
                                *other_id,
                                other_name.clone(),
                            )
                            .changed()
                        {
                            ui.ctx().request_repaint();
                        }
                    }
                });
            if ui
                .add_enabled(
                    selected_merge_source != 0 && selected_merge_source != preset_id,
                    Button::new(Self::tr_lang(
                        language,
                        "Merge into this",
                        "Merge into this",
                    )),
                )
                .clicked()
            {
                outcome.merge_source_id = Some(selected_merge_source);
            }
        });

        outcome.selected_merge_source = selected_merge_source;

        outcome
    }

    pub(crate) fn add_mouse_path_preset(&mut self) -> u32 {
        self.add_mouse_path_preset_from(None)
    }

    pub(crate) fn add_mouse_path_preset_with_events(
        &mut self,
        name: String,
        events: Vec<MousePathEvent>,
        replay_relative_motion: bool,
    ) -> u32 {
        let id = Self::allocate_next_id(
            &self.state.mouse_path_presets,
            &mut self.state.next_mouse_path_preset_id,
            |preset| preset.id,
        );
        let mut new_preset = MousePathPreset::new(id);
        new_preset.name = name;
        new_preset.collapsed = true;
        new_preset.replay_relative_motion = replay_relative_motion;
        new_preset.events = events;
        self.state.mouse_path_presets.push(new_preset);
        self.sync_mouse_path_presets();
        id
    }

    pub(crate) fn add_mouse_path_preset_from(&mut self, source_preset_id: Option<u32>) -> u32 {
        let id = Self::allocate_next_id(
            &self.state.mouse_path_presets,
            &mut self.state.next_mouse_path_preset_id,
            |preset| preset.id,
        );
        let source_preset = source_preset_id.and_then(|preset_id| {
            self.state
                .mouse_path_presets
                .iter()
                .find(|preset| preset.id == preset_id)
                .cloned()
        });
        let mut new_preset = MousePathPreset::new(id);
        new_preset.collapsed = true;
        if let Some(source_preset) = source_preset {
            new_preset.replay_relative_motion = source_preset.replay_relative_motion;
            new_preset.events = source_preset.events;
            new_preset.name = format!("{} Copy", source_preset.name);
        } else {
            let mut suffix = 1;
            while self
                .state
                .mouse_path_presets
                .iter()
                .any(|p| p.name == format!("Mouse Path {}", suffix))
            {
                suffix += 1;
            }
            new_preset.name = format!("Mouse Path {}", suffix);
        }
        self.state.mouse_path_presets.push(new_preset);
        self.sync_mouse_path_presets();
        self.status = format!("Added mouse path preset {id}.");
        id
    }

    pub(crate) fn clear_mouse_path_step_references(&mut self, removed_preset_id: u32) -> bool {
        let removed_key = removed_preset_id.to_string();
        let mut changed = false;
        for group in &mut self.state.macro_groups {
            for preset in &mut group.presets {
                for step in &mut preset.steps {
                    if step.action == MacroAction::PlayMousePathPreset
                        && step.key.trim() == removed_key
                    {
                        step.key.clear();
                        changed = true;
                    }
                }
                if preset.hold_stop_step.action == MacroAction::PlayMousePathPreset
                    && preset.hold_stop_step.key.trim() == removed_key
                {
                    preset.hold_stop_step.key.clear();
                    changed = true;
                }
            }
        }
        changed
    }

    pub(crate) fn remap_mouse_path_step_references(
        &mut self,
        old_preset_id: u32,
        new_preset_id: u32,
    ) -> bool {
        let old_key = old_preset_id.to_string();
        let new_key = new_preset_id.to_string();
        let mut changed = false;
        for group in &mut self.state.macro_groups {
            for preset in &mut group.presets {
                for step in &mut preset.steps {
                    if step.action == MacroAction::PlayMousePathPreset && step.key.trim() == old_key
                    {
                        step.key = new_key.clone();
                        changed = true;
                    }
                }
                if preset.hold_stop_step.action == MacroAction::PlayMousePathPreset
                    && preset.hold_stop_step.key.trim() == old_key
                {
                    preset.hold_stop_step.key = new_key.clone();
                    changed = true;
                }
            }
        }
        changed
    }

    pub(crate) fn split_mouse_path_preset(&mut self, preset_id: u32, split_at_ms: u64) -> bool {
        let Some(index) = self
            .state
            .mouse_path_presets
            .iter()
            .position(|preset| preset.id == preset_id)
        else {
            return false;
        };
        let preset = self.state.mouse_path_presets[index].clone();
        let total_ms = Self::mouse_path_total_duration_ms(&preset.events);
        if split_at_ms == 0 || split_at_ms >= total_ms {
            return false;
        }
        let left_events = Self::slice_mouse_path_events(&preset.events, 0, split_at_ms);
        let right_events = Self::slice_mouse_path_events(&preset.events, split_at_ms, total_ms);
        if left_events.is_empty() || right_events.is_empty() {
            return false;
        }
        self.state.mouse_path_presets[index].events = left_events;
        let new_name = format!("{} Part 2", preset.name);
        self.add_mouse_path_preset_with_events(
            new_name,
            right_events,
            preset.replay_relative_motion,
        );
        true
    }

    pub(crate) fn merge_mouse_path_presets(&mut self, target_id: u32, source_id: u32) -> bool {
        if target_id == source_id {
            return false;
        }
        let Some(target_index) = self
            .state
            .mouse_path_presets
            .iter()
            .position(|preset| preset.id == target_id)
        else {
            return false;
        };
        let Some(source_index) = self
            .state
            .mouse_path_presets
            .iter()
            .position(|preset| preset.id == source_id)
        else {
            return false;
        };
        let source_events = self.state.mouse_path_presets[source_index].events.clone();
        if source_events.is_empty() {
            return false;
        }
        self.state.mouse_path_presets[target_index]
            .events
            .extend(source_events);
        self.state.mouse_path_presets.remove(source_index);
        let refs_changed = self.remap_mouse_path_step_references(source_id, target_id);
        if self.mouse_path_step_preview_preset_id == Some(source_id) {
            self.mouse_path_step_preview_preset_id = Some(target_id);
        }
        if refs_changed {
            self.persist_macro_presets();
        }
        true
    }

    pub(crate) fn sync_mouse_path_presets(&mut self) {
        let presets = self.state.mouse_path_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            presets,
            &mut self.last_synced_mouse_path_presets,
            OverlayCommand::UpdateMousePathPresets,
        );
    }

    pub(crate) fn add_mouse_sensitivity_preset(&mut self) {
        let id = Self::allocate_next_id(
            &self.state.mouse_sensitivity_presets,
            &mut self.state.next_mouse_sensitivity_preset_id,
            |preset| preset.id,
        );
        let mut new_preset = MouseSensitivityPreset::new(id);
        let mut suffix = 1;
        while self
            .state
            .mouse_sensitivity_presets
            .iter()
            .any(|p| p.name == format!("Mouse Sensitivity {}", suffix))
        {
            suffix += 1;
        }
        new_preset.name = format!("Mouse Sensitivity {}", suffix);
        self.state.mouse_sensitivity_presets.push(new_preset);
        self.sync_mouse_sensitivity_presets();
        self.status = format!("Added mouse sensitivity preset {id}.");
    }

    pub(crate) fn sync_mouse_sensitivity_presets(&mut self) {
        let presets = self.state.mouse_sensitivity_presets.clone();
        Self::sync_overlay_state_if_changed(
            &self.overlay_tx,
            presets,
            &mut self.last_synced_mouse_sensitivity_presets,
            OverlayCommand::UpdateMouseSensitivityPresets,
        );
    }

    pub(crate) fn sync_mouse_sensitivity_settings(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateMouseSensitivitySettings {
                restore_on_exit: self.state.mouse_sensitivity_restore_on_exit,
                restore_speed: self.state.mouse_sensitivity_restore_speed,
            });
    }

    pub(crate) fn sync_mouse_driver_settings(&self) {}

    pub(crate) fn sync_keyboard_arrow_mouse_settings(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateKeyboardArrowMouseSettings {
                enabled: self.state.keyboard_arrow_mouse_enabled,
                step_px: self.state.keyboard_arrow_mouse_step_px,
            });
    }

    pub(crate) fn persist_mouse_path_presets(&mut self) {
        self.persist_after_sync(Self::sync_mouse_path_presets);
    }

    pub(crate) fn persist_mouse_sensitivity_presets(&mut self) {
        self.persist_after_sync(Self::sync_mouse_sensitivity_presets);
    }

    pub(crate) fn begin_mouse_path_draw_capture(&mut self, ctx: &egui::Context, preset_id: u32) {
        if self.mouse_path_draw_capture_preset_id.is_some()
            || self.active_mouse_record_preset_id.is_some()
        {
            return;
        }

        let Some(preset_name) = self
            .state
            .mouse_path_presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .map(|preset| preset.name.clone())
        else {
            self.status = Self::tr_lang(
                self.state.ui_language,
                "Selected mouse path preset was not found.",
                "Selected mouse path preset was not found.",
            )
            .to_owned();
            return;
        };

        let viewport = ctx.input(|input| input.viewport().clone());
        self.mouse_path_draw_capture_restore_inner_size = viewport
            .inner_rect
            .map(|rect| rect.size())
            .or(Some(Self::desired_window_size()));
        self.mouse_path_draw_capture_restore_outer_pos = viewport.outer_rect.map(|rect| rect.min);
        self.mouse_path_draw_capture_preset_id = Some(preset_id);
        self.enforce_square_window_frames = 0;
        self.status = Self::tr_lang(self.state.ui_language, "Hide app. Hold left mouse to draw the path, then release to save. Press Esc to cancel.", "Hide app. Hold left mouse to draw the path, then release to save. Press Esc to cancel.")
        .to_owned();

        let _ = self
            .overlay_tx
            .send(OverlayCommand::BeginMousePathDrawCapture {
                preset_id,
                preset_name,
            });
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(false));
        crate::overlay::wake_command_queue();

        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    pub(crate) fn cancel_mouse_path_draw_capture(&mut self, ctx: &egui::Context) {
        if self.mouse_path_draw_capture_preset_id.is_none() {
            return;
        }

        self.mouse_path_draw_capture_preset_id = None;
        self.active_mouse_record_preset_id = None;
        let _ = self
            .overlay_tx
            .send(OverlayCommand::CancelMousePathDrawCapture);
        crate::overlay::wake_command_queue();
        self.restore_mouse_path_draw_capture_window(ctx);
        self.status = Self::tr_lang(
            self.state.ui_language,
            "Mouse path draw cancelled.",
            "Mouse path draw cancelled.",
        )
        .to_owned();
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    pub(crate) fn restore_mouse_path_draw_capture_window(&mut self, ctx: &egui::Context) {
        if let Some(size) = self.mouse_path_draw_capture_restore_inner_size.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
        if let Some(pos) = self.mouse_path_draw_capture_restore_outer_pos.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(true));
        crate::overlay::wake_command_queue();
    }

    pub(crate) fn begin_mouse_move_absolute_capture(
        &mut self,
        ctx: &egui::Context,
        target: MouseMoveAbsoluteCaptureTarget,
    ) {
        if self.mouse_move_absolute_capture_target.is_some() || self.native_capture_in_progress {
            return;
        }

        self.mouse_move_absolute_capture_target = Some(target);
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
        let vietnamese = self.state.ui_language == crate::model::UiLanguage::Vietnamese;

        std::thread::spawn(move || {
            // Sleep to let OS process window hide
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Capture virtual screen bounds
            let (left, top, width, height) = crate::window_list::virtual_screen_bounds();
            let (result, capture_frame) = if let Some(capture) =
                crate::window_list::capture_virtual_screen_region(left, top, width, height)
            {
                let mode = crate::overlay::native_capture::NativeCaptureMode::PointClick {
                    vietnamese,
                    dim_background: true,
                };
                let res = crate::overlay::native_capture::run_capture_overlay(
                    capture.clone(),
                    left,
                    top,
                    width,
                    height,
                    mode,
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

            let _ = ui_tx.send(UiCommand::NativeMouseMoveAbsoluteCaptureFinished {
                target,
                result,
                capture_frame,
            });
            egui_ctx.request_repaint();
        });
    }

    pub(crate) fn cancel_mouse_move_absolute_capture(&mut self, ctx: &egui::Context) {
        let Some(target) = self.mouse_move_absolute_capture_target else {
            return;
        };
        if Self::mouse_move_absolute_capture_uses_blocked_click(target) {
            self.set_image_search_capture_mouse_blocked(false, false);
        }
        self.mouse_move_absolute_capture_target = None;
        self.restore_mouse_move_absolute_capture_window(ctx);
        self.mouse_move_absolute_capture_raise_window = true;
        self.status = Self::tr_lang(
            self.state.ui_language,
            "Mouse position capture cancelled.",
            "Mouse position capture cancelled.",
        )
        .to_owned();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    pub(crate) fn finish_mouse_move_absolute_capture(
        &mut self,
        ctx: &egui::Context,
        target: MouseMoveAbsoluteCaptureTarget,
        screen_x: i32,
        screen_y: i32,
        color: Option<RgbaColor>,
    ) {
        let uses_blocked_click = Self::mouse_move_absolute_capture_uses_blocked_click(target);
        let is_pixel_color = matches!(
            target.capture_kind,
            MouseCaptureKind::IfStartPixelColor | MouseCaptureKind::ExtraCondPixelColor
        );
        if uses_blocked_click && (!is_pixel_color || color.is_some()) {
            self.set_image_search_capture_mouse_blocked(false, false);
        }

        // --- Handle ExtraCondition captures ---
        if matches!(
            target.capture_kind,
            MouseCaptureKind::ExtraCondMousePos | MouseCaptureKind::ExtraCondPixelColor
        ) {
            let color = if target.capture_kind == MouseCaptureKind::ExtraCondPixelColor {
                if let Some(color) = color {
                    self.mouse_move_absolute_capture_target = None;
                    self.restore_mouse_move_absolute_capture_window(ctx);
                    Some(color)
                } else {
                    self.sample_mouse_move_absolute_capture_color(
                        ctx,
                        screen_x,
                        screen_y,
                        uses_blocked_click,
                    )
                }
            } else {
                self.mouse_move_absolute_capture_target = None;
                self.restore_mouse_move_absolute_capture_window(ctx);
                None
            };

            let extra_idx = target.extra_cond_index.unwrap_or(0);
            let step_result = if let Some(group_id) = target.group_id {
                self.state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .and_then(|group| {
                        group
                            .presets
                            .iter_mut()
                            .find(|preset| preset.id == target.preset_id)
                    })
                    .and_then(|preset| {
                        if target.is_hold_stop {
                            Some(&mut *preset.hold_stop_step)
                        } else {
                            preset.steps.get_mut(target.step_index)
                        }
                    })
            } else {
                None
            };
            if let Some(step) = step_result {
                if let Some(cond) = step.extra_conditions.get_mut(extra_idx) {
                    match target.capture_kind {
                        MouseCaptureKind::ExtraCondMousePos => {
                            cond.expression = screen_x.to_string();
                        }
                        MouseCaptureKind::ExtraCondPixelColor => {
                            cond.x = screen_x;
                            cond.y = screen_y;
                            if let Some(c) = color {
                                cond.target_color = format!("{},{},{}", c.r, c.g, c.b);
                                cond.color_tolerance = 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
            self.mouse_move_absolute_capture_raise_window = true;
            self.status =
                crate::lang::translate(self.state.ui_language, "Captured position {}, {}.")
                    .unwrap_or("Captured position {}, {}.")
                    .replacen("{}", &screen_x.to_string(), 1)
                    .replacen("{}", &screen_y.to_string(), 1);
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
            self.persist();
            if target.group_id.is_some() {
                self.sync_macro_presets();
            }
            return;
        }

        // --- Handle IfStart captures ---
        if matches!(
            target.capture_kind,
            MouseCaptureKind::IfStartMousePos | MouseCaptureKind::IfStartPixelColor
        ) {
            let color = if target.capture_kind == MouseCaptureKind::IfStartPixelColor {
                if let Some(color) = color {
                    self.mouse_move_absolute_capture_target = None;
                    self.restore_mouse_move_absolute_capture_window(ctx);
                    Some(color)
                } else {
                    self.sample_mouse_move_absolute_capture_color(
                        ctx,
                        screen_x,
                        screen_y,
                        uses_blocked_click,
                    )
                }
            } else {
                self.mouse_move_absolute_capture_target = None;
                self.restore_mouse_move_absolute_capture_window(ctx);
                None
            };

            let step_result = if let Some(group_id) = target.group_id {
                self.state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .and_then(|group| {
                        group
                            .presets
                            .iter_mut()
                            .find(|preset| preset.id == target.preset_id)
                    })
                    .and_then(|preset| {
                        if target.is_hold_stop {
                            Some(&mut *preset.hold_stop_step)
                        } else {
                            preset.steps.get_mut(target.step_index)
                        }
                    })
            } else {
                None
            };
            if let Some(step) = step_result {
                match target.capture_kind {
                    MouseCaptureKind::IfStartMousePos => {
                        step.key = screen_x.to_string();
                    }
                    MouseCaptureKind::IfStartPixelColor => {
                        step.x = screen_x;
                        step.y = screen_y;
                        if let Some(c) = color {
                            step.if_target_color = format!("{},{},{}", c.r, c.g, c.b);
                            step.if_color_tolerance = 1;
                        }
                    }
                    _ => {}
                }
            }
            self.mouse_move_absolute_capture_raise_window = true;
            self.status =
                crate::lang::translate(self.state.ui_language, "Captured position {}, {}.")
                    .unwrap_or("Captured position {}, {}.")
                    .replacen("{}", &screen_x.to_string(), 1)
                    .replacen("{}", &screen_y.to_string(), 1);
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
            self.persist();
            if target.group_id.is_some() {
                self.sync_macro_presets();
            }
            return;
        }

        // --- Original: MoveMouseAbsolute ---
        let step_result = if let Some(group_id) = target.group_id {
            self.state
                .macro_groups
                .iter_mut()
                .find(|group| group.id == group_id)
                .and_then(|group| {
                    group
                        .presets
                        .iter_mut()
                        .find(|preset| preset.id == target.preset_id)
                })
                .and_then(|preset| {
                    if target.is_hold_stop {
                        Some(&mut *preset.hold_stop_step)
                    } else {
                        preset.steps.get_mut(target.step_index)
                    }
                })
        } else {
            None
        };

        if target.group_id.is_some()
            && matches!(
                target.capture_kind,
                MouseCaptureKind::GeometryPrimaryPos | MouseCaptureKind::GeometrySecondaryPos
            )
        {
            let is_secondary =
                matches!(target.capture_kind, MouseCaptureKind::GeometrySecondaryPos);
            let step = step_result;
            if let Some(step) = step {
                if let Some(point_idx) = target.extra_cond_index {
                    // Polyline/polygon point
                    let mut points: Vec<(String, String)> = step
                        .geometry_spec
                        .points_expr
                        .split(';')
                        .filter(|s| !s.is_empty())
                        .map(|pair| {
                            if let Some((x, y)) = pair.split_once(',') {
                                (x.trim().to_owned(), y.trim().to_owned())
                            } else {
                                (pair.trim().to_owned(), String::new())
                            }
                        })
                        .collect();
                    if point_idx < points.len() {
                        points[point_idx] = (screen_x.to_string(), screen_y.to_string());
                        step.geometry_spec.points_expr = points
                            .iter()
                            .map(|(x, y)| format!("{},{}", x, y))
                            .collect::<Vec<_>>()
                            .join(";");
                    }
                } else if is_secondary {
                    step.geometry_spec.x2_expr = screen_x.to_string();
                    step.geometry_spec.y2_expr = screen_y.to_string();
                } else {
                    step.geometry_spec.x1_expr = screen_x.to_string();
                    step.geometry_spec.y1_expr = screen_y.to_string();
                }
            }
            self.mouse_move_absolute_capture_target = None;
            self.restore_mouse_move_absolute_capture_window(ctx);
            self.mouse_move_absolute_capture_raise_window = true;
            self.status = match self.state.ui_language {
                UiLanguage::Vietnamese => {
                    format!("Da lay toa do geometry {}, {}.", screen_x, screen_y)
                }
                _ => format!("Captured geometry position {}, {}.", screen_x, screen_y),
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
            ctx.request_repaint_after(Duration::from_millis(33));
            self.persist();
            if target.group_id.is_some() {
                self.sync_macro_presets();
            }
            return;
        }

        if target.group_id.is_none()
            && matches!(
                target.capture_kind,
                MouseCaptureKind::GeometryPrimaryPos | MouseCaptureKind::GeometrySecondaryPos
            )
        {
            let object_id = target.step_index as u32;
            let pair_index =
                if matches!(target.capture_kind, MouseCaptureKind::GeometrySecondaryPos) {
                    1
                } else {
                    0
                };
            let object_result = self
                .state
                .geometry_presets
                .iter_mut()
                .find(|preset| preset.id == target.preset_id)
                .and_then(|preset| {
                    preset
                        .objects
                        .iter_mut()
                        .find(|object| object.id == object_id)
                });

            let Some(object) = object_result else {
                self.cancel_mouse_move_absolute_capture(ctx);
                self.status = Self::tr_lang(
                    self.state.ui_language,
                    "Geometry target was not found.",
                    "Geometry target was not found.",
                )
                .to_owned();
                return;
            };

            if let Some(point_idx) = target.extra_cond_index {
                let mut points = object
                    .spec
                    .points_expr
                    .split(';')
                    .map(|pair| {
                        if let Some((x, y)) = pair.split_once(',') {
                            (x.trim().to_owned(), y.trim().to_owned())
                        } else {
                            (pair.trim().to_owned(), String::new())
                        }
                    })
                    .collect::<Vec<_>>();
                if point_idx < points.len() {
                    points[point_idx] = (screen_x.to_string(), screen_y.to_string());
                    object.spec.points_expr = points
                        .iter()
                        .map(|(x, y)| format!("{},{}", x, y))
                        .collect::<Vec<_>>()
                        .join(";");
                }
            } else {
                match pair_index {
                    0 => {
                        object.spec.x1_expr = screen_x.to_string();
                        object.spec.y1_expr = screen_y.to_string();
                    }
                    _ => {
                        object.spec.x2_expr = screen_x.to_string();
                        object.spec.y2_expr = screen_y.to_string();
                    }
                }
            }

            self.mouse_move_absolute_capture_target = None;
            self.restore_mouse_move_absolute_capture_window(ctx);
            self.mouse_move_absolute_capture_raise_window = true;
            self.status = match self.state.ui_language {
                UiLanguage::Vietnamese => {
                    format!("Da lay toa do geometry {}, {}.", screen_x, screen_y)
                }
                _ => format!("Captured geometry position {}, {}.", screen_x, screen_y),
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
            ctx.request_repaint_after(Duration::from_millis(33));
            self.persist_geometry_presets();
            return;
        }

        let Some(step) = step_result else {
            self.cancel_mouse_move_absolute_capture(ctx);
            self.status = Self::tr_lang(
                self.state.ui_language,
                "Mouse position capture target was not found.",
                "Mouse position capture target was not found.",
            )
            .to_owned();
            return;
        };

        step.x = screen_x;
        step.y = screen_y;
        step.x_expr = screen_x.to_string();
        step.y_expr = screen_y.to_string();
        step.action = MacroAction::MouseMoveAbsolute;
        self.mouse_move_absolute_capture_target = None;
        self.restore_mouse_move_absolute_capture_window(ctx);
        self.mouse_move_absolute_capture_raise_window = true;
        self.status =
            crate::lang::translate(self.state.ui_language, "Captured mouse position {}, {}.")
                .unwrap_or("Captured mouse position {}, {}.")
                .replacen("{}", &screen_x.to_string(), 1)
                .replacen("{}", &screen_y.to_string(), 1);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
        ctx.request_repaint_after(Duration::from_millis(33));
        self.persist();
        if target.group_id.is_some() {
            self.sync_macro_presets();
        }
    }

    pub(crate) fn mouse_move_absolute_capture_uses_blocked_click(
        target: MouseMoveAbsoluteCaptureTarget,
    ) -> bool {
        matches!(
            target.capture_kind,
            MouseCaptureKind::IfStartPixelColor | MouseCaptureKind::ExtraCondPixelColor
        )
    }

    fn sample_mouse_move_absolute_capture_color(
        &mut self,
        ctx: &egui::Context,
        screen_x: i32,
        screen_y: i32,
        used_blocked_click: bool,
    ) -> Option<RgbaColor> {
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
            capture.and_then(|frame| {
                (frame.rgba.len() >= 4).then(|| RgbaColor {
                    r: frame.rgba[0],
                    g: frame.rgba[1],
                    b: frame.rgba[2],
                    a: 255,
                })
            })
        };

        if used_blocked_click {
            self.set_image_search_capture_mouse_blocked(false, false);
        }
        self.mouse_move_absolute_capture_target = None;
        self.restore_mouse_move_absolute_capture_window(ctx);

        color
    }

    pub(crate) fn restore_mouse_move_absolute_viewport(&mut self, ctx: &egui::Context) {
        if let Some(size) = self.mouse_move_absolute_restore_inner_size.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
        if let Some(pos) = self.mouse_move_absolute_restore_outer_pos.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
    }

    pub(crate) fn restore_mouse_move_absolute_capture_window(&mut self, ctx: &egui::Context) {
        self.captured_freeze_texture = None;
        self.captured_freeze_frame = None;
        self.restore_mouse_move_absolute_viewport(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(true));
        crate::overlay::wake_command_queue();
    }

    fn refresh_arduino_ports(&mut self) {
        self.arduino_ports_last_refresh = Some(std::time::Instant::now());
        let Ok(ports) = serialport::available_ports() else {
            self.arduino_available_ports.clear();
            return;
        };
        let preferred_port =
            preferred_arduino_port(&ports, self.state.vision_settings.arduino_spoof_type);
        self.arduino_available_ports = ports.into_iter().map(|p| p.port_name).collect();
        self.arduino_available_ports.sort();

        if let Some(runtime_port) = preferred_port.as_ref()
            && self.state.vision_settings.arduino_com_port != *runtime_port
        {
            self.state.vision_settings.arduino_com_port = runtime_port.clone();
            self.sync_vision_settings();
            self.persist();
            return;
        }

        let current_port = self.state.vision_settings.arduino_com_port.clone();
        if current_port.is_empty() {
            if let Some(port) = preferred_port.or_else(|| {
                (self.arduino_available_ports.len() == 1)
                    .then(|| self.arduino_available_ports[0].clone())
            }) {
                self.state.vision_settings.arduino_com_port = port;
                self.sync_vision_settings();
            }
            return;
        }

        if !self.arduino_available_ports.contains(&current_port) {
            if let Some(port) = preferred_port.or_else(|| {
                (self.arduino_available_ports.len() == 1)
                    .then(|| self.arduino_available_ports[0].clone())
            }) {
                self.state.vision_settings.arduino_com_port = port;
                self.sync_vision_settings();
            }
        }
    }

    pub(crate) fn start_arduino_tools_download(&mut self) {
        if self.arduino_download_job.is_some() {
            return;
        }

        let paths = self.paths.clone();
        let progress = self.arduino_download_progress.clone();
        progress.store(0, std::sync::atomic::Ordering::SeqCst);

        let job = std::thread::spawn(move || -> anyhow::Result<()> {
            let url =
                "https://github.com/LinhAsia/MacroNest/releases/download/tools/arduino_tools.zip";
            let mut response = reqwest::blocking::get(url)?.error_for_status()?;
            let total_size = response.content_length().unwrap_or(1000000);

            // Ensure bin directory exists
            std::fs::create_dir_all(&paths.bin_dir)?;

            let mut file = std::fs::File::create(&paths.arduino_tools_zip)?;
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
                let p = ((downloaded as f32 / total_size as f32) * 999.0).round() as u32;
                progress.store(p, std::sync::atomic::Ordering::SeqCst);
            }

            drop(file);

            Self::extract_zip_archive(&paths.arduino_tools_zip, &paths.bin_dir)?;

            let _ = std::fs::remove_file(&paths.arduino_tools_zip);
            progress.store(1000, std::sync::atomic::Ordering::SeqCst);

            Ok(())
        });

        self.arduino_download_job = Some(job);
    }

    pub(crate) fn delete_arduino_tools(&mut self) {
        let _ = std::fs::remove_file(&self.paths.avrdude_exe);
        let _ = std::fs::remove_file(&self.paths.avrdude_conf);
        let _ = std::fs::remove_file(&self.paths.arduino_firmware_hex);
        self.arduino_tools_downloaded = false;
        self.arduino_flash_status.clear();
    }

    pub(crate) fn start_arduino_flash(&mut self) {
        if self.arduino_flash_running {
            return;
        }

        if let Err(error) = self.paths.ensure_arduino_runtime_files() {
            self.arduino_flash_status = format!("Error: {error}");
            return;
        }

        let port = self.state.vision_settings.arduino_com_port.clone();
        if port.is_empty() {
            self.arduino_flash_status = self
                .tr(
                    "Error: Select a COM Port first",
                    "Error: Select a COM Port first",
                )
                .to_owned();
            return;
        }

        self.arduino_restore_emulation_after_flash = self.state.vision_settings.use_arduino_mouse;
        if self.state.vision_settings.use_arduino_mouse {
            self.state.vision_settings.use_arduino_mouse = false;
            self.sync_vision_settings();
        }
        self.arduino_flash_running = true;
        self.arduino_flash_status = format!("Preparing flash: releasing {port}...");

        let paths = self.paths.clone();
        let flash_result = self.arduino_flash_result.clone();
        *flash_result.lock() = None;
        let flash_progress = self.arduino_flash_progress.clone();
        *flash_progress.lock() = Some(format!("Preparing flash: releasing {port}..."));
        let spoof_type = self.state.vision_settings.arduino_spoof_type;

        std::thread::spawn(move || {
            let run_flash = || -> anyhow::Result<()> {
                let set_progress = |message: String| {
                    *flash_progress.lock() = Some(message);
                };

                // Directly and synchronously close the Arduino serial port and set flash flag.
                // This is reliable because it directly acquires the mutex — no async channel delay.
                set_progress(format!("Releasing {port} for flashing..."));
                crate::overlay::close_arduino_port_for_flash();
                std::thread::sleep(std::time::Duration::from_millis(1500));

                // 1. Scan ports before touch
                let ports_before = serialport::available_ports().unwrap_or_default();
                let before_names: std::collections::HashSet<String> =
                    ports_before.into_iter().map(|p| p.port_name).collect();

                // 2. Perform 1200 baud touch to reset into bootloader mode
                set_progress(format!("Resetting {port} into bootloader (1200 baud)..."));
                touch_arduino_bootloader_port(
                    &port,
                    std::time::Duration::from_secs(8),
                )
                .map_err(|error| {
                    anyhow::anyhow!(
                        "Could not open {port} at 1200 baud to enter bootloader after releasing the app connection: {error}"
                    )
                })?;

                // 3. Wait for bootloader COM port to re-appear
                std::thread::sleep(std::time::Duration::from_millis(400));
                set_progress("Waiting for Arduino bootloader COM port...".to_owned());

                let mut bootloader_port = detect_bootloader_port(
                    &port,
                    &before_names,
                    std::time::Duration::from_secs(15),
                )?;
                set_progress(format!(
                    "Bootloader detected on {bootloader_port}; starting avrdude..."
                ));
                std::thread::sleep(std::time::Duration::from_millis(120));

                // 4. Use the bundled serial firmware.
                set_progress("Preparing firmware image...".to_owned());
                let base_firmware_hex = paths.arduino_firmware_hex.clone();

                if !base_firmware_hex.exists() {
                    anyhow::bail!(
                        "Firmware image not found for the selected runtime: {}",
                        base_firmware_hex.display()
                    );
                }

                let hex_to_flash = if spoof_type > 0 {
                    set_progress("Applying USB spoofing to firmware...".to_owned());
                    let temp_hex_path = base_firmware_hex.with_extension("spoof.hex");
                    let hex_content = std::fs::read_to_string(&base_firmware_hex)?;
                    let modified_hex = patch_arduino_firmware_hex(&hex_content, spoof_type)?;
                    std::fs::write(&temp_hex_path, modified_hex)?;
                    temp_hex_path
                } else {
                    base_firmware_hex
                };

                // 5. Execute avrdude to flash
                if !paths.avrdude_exe.exists() {
                    anyhow::bail!("avrdude.exe not found");
                }

                let mut last_error: Option<String> = None;
                let mut flashed = false;

                for attempt in 1..=3 {
                    set_progress(format!(
                        "Flashing firmware on {bootloader_port} (attempt {attempt}/3)..."
                    ));
                    let output = run_avrdude_flash(&paths, &bootloader_port, &hex_to_flash)?;

                    if output.status.success() {
                        flashed = true;
                        break;
                    }

                    let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                    last_error = Some(err_msg.clone());

                    if attempt == 3 || !is_retryable_avrdude_error(&err_msg) {
                        anyhow::bail!("avrdude flash failed: {}", err_msg);
                    }

                    set_progress(format!(
                        "Flash attempt {attempt} failed; resetting into bootloader and retrying..."
                    ));
                    std::thread::sleep(std::time::Duration::from_millis(700));

                    wait_for_serial_port_present(&port, std::time::Duration::from_secs(10))?;
                    let ports_before_retry = serialport::available_ports().unwrap_or_default();
                    let before_names_retry: std::collections::HashSet<String> = ports_before_retry
                        .into_iter()
                        .map(|p| p.port_name)
                        .collect();
                    touch_arduino_bootloader_port(&port, std::time::Duration::from_secs(8))?;
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    bootloader_port = detect_bootloader_port(
                        &port,
                        &before_names_retry,
                        std::time::Duration::from_secs(15),
                    )?;
                }

                if !flashed {
                    anyhow::bail!(
                        "avrdude flash failed: {}",
                        last_error.unwrap_or_else(|| "unknown avrdude error".to_owned())
                    );
                }

                set_progress("Flash complete. Waiting for the firmware COM port...".to_owned());
                let reconnect_deadline =
                    std::time::Instant::now() + std::time::Duration::from_secs(15);
                let (expected_vid, expected_pid) = get_arduino_vid_pid(spoof_type);
                loop {
                    let ports = serialport::available_ports().unwrap_or_default();
                    let bootloader_gone = !ports
                        .iter()
                        .any(|candidate| candidate.port_name == bootloader_port);
                    let new_firmware_present = ports.iter().any(|candidate| {
                        matches!(
                            &candidate.port_type,
                            serialport::SerialPortType::UsbPort(info)
                                if info.vid == expected_vid && info.pid == expected_pid
                        )
                    });
                    if bootloader_gone && new_firmware_present {
                        break;
                    }
                    if std::time::Instant::now() >= reconnect_deadline {
                        anyhow::bail!(
                            "Flash verification failed: Arduino did not restart with PID {:04X}",
                            expected_pid
                        );
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                set_progress("Flash complete. Reconnecting Arduino emulation...".to_owned());
                Ok(())
            };

            let res = run_flash();
            // Re-enable the background Arduino connection manager
            crate::overlay::finish_arduino_flash();
            if spoof_type > 0 {
                let temp_hex_path = paths.arduino_firmware_hex.with_extension("spoof.hex");
                if temp_hex_path.exists() {
                    let _ = std::fs::remove_file(&temp_hex_path);
                }
            }
            match res {
                Ok(_) => {
                    *flash_result.lock() = Some(Ok(()));
                }
                Err(e) => {
                    *flash_result.lock() = Some(Err(e.to_string()));
                }
            }
        });
    }
}

fn preferred_arduino_port(ports: &[serialport::SerialPortInfo], spoof_type: u32) -> Option<String> {
    let mut scored_ports = ports
        .iter()
        .filter_map(|port| {
            let score = arduino_port_score(port, spoof_type);
            (score > 0).then(|| (score, port.port_name.clone()))
        })
        .collect::<Vec<_>>();
    scored_ports.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let best = scored_ports.first()?;
    if scored_ports.get(1).is_some_and(|second| second.0 == best.0) {
        return None;
    }
    Some(best.1.clone())
}

fn arduino_port_score(port: &serialport::SerialPortInfo, spoof_type: u32) -> u32 {
    let serialport::SerialPortType::UsbPort(info) = &port.port_type else {
        return 0;
    };

    let (spoof_vid, spoof_pid) = get_arduino_vid_pid(spoof_type);

    let mut score = 0;
    if info.vid == spoof_vid && info.pid == spoof_pid {
        score += 250;
    } else if info.vid == 0x2341 && matches!(info.pid, 0x8036 | 0x8037) {
        score += 200;
    } else if info.vid == 0x2341 && matches!(info.pid, 0x0036 | 0x0037) {
        score += 20;
    } else if info.vid == 0x2341 {
        score += 60;
    }

    let text = format!(
        "{} {} {}",
        info.manufacturer.as_deref().unwrap_or_default(),
        info.product.as_deref().unwrap_or_default(),
        info.serial_number.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();
    for needle in ["arduino", "leonardo", "atmega32u4"] {
        if text.contains(needle) {
            score += 50;
        }
    }

    score
}

fn wait_for_serial_port_openable(
    port: &str,
    baud_rate: u32,
    timeout: std::time::Duration,
) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    let mut last_error = None;
    while start.elapsed() < timeout {
        match serialport::new(port, baud_rate)
            .timeout(std::time::Duration::from_millis(250))
            .open()
        {
            Ok(_) => return Ok(()),
            Err(error) => {
                last_error = Some(error.to_string());
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
    }

    anyhow::bail!(
        "{}",
        last_error.unwrap_or_else(|| "serial port was not available".to_owned())
    )
}
fn wait_for_serial_port_present(port: &str, timeout: std::time::Duration) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let ports = serialport::available_ports().unwrap_or_default();
        if ports.iter().any(|info| info.port_name == port) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    anyhow::bail!("serial port {port} did not reappear in time")
}

fn detect_bootloader_port(
    runtime_port: &str,
    before_names: &std::collections::HashSet<String>,
    timeout: std::time::Duration,
) -> anyhow::Result<String> {
    let start_time = std::time::Instant::now();
    let mut original_port_disappeared = false;

    while start_time.elapsed() < timeout {
        let current_ports = serialport::available_ports().unwrap_or_default();
        let current_names: std::collections::HashSet<String> =
            current_ports.iter().map(|p| p.port_name.clone()).collect();

        for name in &current_names {
            if !before_names.contains(name) {
                return Ok(name.clone());
            }
        }

        if !current_names.contains(runtime_port) {
            original_port_disappeared = true;
        }

        if original_port_disappeared && current_names.contains(runtime_port) {
            return Ok(runtime_port.to_owned());
        }

        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    anyhow::bail!(
        "Arduino bootloader port did not appear after resetting {runtime_port}. Try pressing reset twice quickly, then flash again."
    )
}

fn run_avrdude_flash(
    paths: &crate::storage::AppPaths,
    bootloader_port: &str,
    hex_to_flash: &std::path::Path,
) -> anyhow::Result<std::process::Output> {
    let mut cmd = std::process::Command::new(&paths.avrdude_exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let flash_arg = format!("flash:w:{}:i", hex_to_flash.to_string_lossy());
    let args = [
        "-C",
        &paths.avrdude_conf.to_string_lossy(),
        "-v",
        "-p",
        "atmega32u4",
        "-c",
        "avr109",
        "-P",
        bootloader_port,
        "-b",
        "57600",
        "-D",
        "-U",
        &flash_arg,
    ];

    Ok(cmd.args(args).output()?)
}

fn is_retryable_avrdude_error(stderr: &str) -> bool {
    let text = stderr.to_ascii_lowercase();
    text.contains("access is denied")
        || text.contains("unable to read")
        || text.contains("read signature")
        || text.contains("butterfly_send")
        || text.contains("i/o operation has been aborted")
        || text.contains("the system cannot find the file specified")
}

fn touch_arduino_bootloader_port(port: &str, timeout: std::time::Duration) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    let mut last_error = None;
    while start.elapsed() < timeout {
        match serialport::new(port, 1200)
            .timeout(std::time::Duration::from_millis(500))
            .open()
        {
            Ok(mut serial) => {
                let _ = serial.write_data_terminal_ready(true);
                std::thread::sleep(std::time::Duration::from_millis(100));
                let _ = serial.write_data_terminal_ready(false);
                std::thread::sleep(std::time::Duration::from_millis(300));
                return Ok(());
            }
            Err(error) => {
                last_error = Some(error.to_string());
                let bootloader_present = serialport::available_ports()
                    .unwrap_or_default()
                    .iter()
                    .any(|candidate| {
                        matches!(
                            &candidate.port_type,
                            serialport::SerialPortType::UsbPort(info)
                                if info.vid == 0x2341
                                    && matches!(info.pid, 0x0036 | 0x0037)
                        )
                    });
                if bootloader_present {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }

    anyhow::bail!(
        "{}",
        last_error.unwrap_or_else(|| "serial port was not available".to_owned())
    )
}

pub fn get_arduino_vid_pid(spoof_type: u32) -> (u16, u16) {
    match spoof_type {
        1 => (0x046D, 0xC08B), // Logitech G Pro Wireless
        2 => (0x1532, 0x0029), // Razer DeathAdder V2
        3 => (0x1038, 0x1360), // SteelSeries Sensei
        _ => (0x2341, 0x8037), // Default Arduino (using Micro's PID 8037)
    }
}

fn patch_arduino_firmware_hex(hex_content: &str, spoof_type: u32) -> anyhow::Result<String> {
    let (vid, pid) = get_arduino_vid_pid(spoof_type);
    let mut modified_lines = Vec::new();
    let mut found = false;

    for line in hex_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(':') && trimmed.len() >= 11 {
            let byte_count = usize::from_str_radix(&trimmed[1..3], 16)?;
            let record_type = &trimmed[7..9];
            if record_type == "00" && trimmed.len() == 1 + 2 + 4 + 2 + 2 * byte_count + 2 {
                let data_hex = &trimmed[9..9 + 2 * byte_count];
                let mut data_bytes = Vec::new();
                for i in 0..byte_count {
                    let b = u8::from_str_radix(&data_hex[2 * i..2 * i + 2], 16)?;
                    data_bytes.push(b);
                }

                for offset in 0..=byte_count.saturating_sub(14) {
                    if data_bytes[offset] == 0x12 && data_bytes[offset + 1] == 0x01 {
                        let current_vid =
                            (data_bytes[offset + 9] as u16) << 8 | (data_bytes[offset + 8] as u16);
                        let current_pid = (data_bytes[offset + 11] as u16) << 8
                            | (data_bytes[offset + 10] as u16);
                        if current_vid == 0x2341 && current_pid == 0x8037 {
                            data_bytes[offset + 8] = (vid & 0xFF) as u8;
                            data_bytes[offset + 9] = ((vid >> 8) & 0xFF) as u8;
                            data_bytes[offset + 10] = (pid & 0xFF) as u8;
                            data_bytes[offset + 11] = ((pid >> 8) & 0xFF) as u8;
                            found = true;
                        }
                    }
                }

                if found {
                    let addr_str = &trimmed[3..7];
                    let mut line_bytes = Vec::new();
                    line_bytes.push(byte_count as u8);
                    line_bytes.push(u8::from_str_radix(&addr_str[0..2], 16)?);
                    line_bytes.push(u8::from_str_radix(&addr_str[2..4], 16)?);
                    line_bytes.push(0u8);
                    line_bytes.extend(&data_bytes);

                    let sum: u32 = line_bytes.iter().map(|&b| b as u32).sum();
                    let checksum = ((0x100 - (sum & 0xFF)) & 0xFF) as u8;

                    let mut new_line = format!(":{:02X}{}{:02X}", byte_count, addr_str, 0);
                    for b in data_bytes {
                        new_line.push_str(&format!("{:02X}", b));
                    }
                    new_line.push_str(&format!("{:02X}", checksum));
                    modified_lines.push(new_line);
                    found = false;
                    continue;
                }
            }
        }
        modified_lines.push(trimmed.to_owned());
    }

    Ok(modified_lines.join("\n"))
}
