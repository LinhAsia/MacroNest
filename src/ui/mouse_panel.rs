use crate::hotkey;
use crate::model::*;
use crate::overlay::OverlayCommand;
use crate::ui::{CrosshairApp, MouseCaptureKind, MouseMoveAbsoluteCaptureTarget};
use crate::window_list;
use eframe::egui::{
    self, Button, Color32, DragValue, Frame, Margin, RichText, Sense, Slider, TextBuffer, TextEdit,
    vec2,
};
use std::time::Duration;

#[cfg(windows)]
use crate::ui::{GetCursorPos, POINT};

impl CrosshairApp {
    pub(crate) fn render_mouse_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        let language = self.state.ui_language;

        let mut remove_mouse_sensitivity_id = None;
        let mut next_mouse_sensitivity_capture_target = None;
        let mut cancel_active_capture_sensitivity = false;
        let mut mouse_sensitivity_live_sync = false;

        ui.horizontal(|ui| {
            if ui
                .button(self.tr("+ Add sensitivity preset", "+ Thêm preset độ nhạy"))
                .clicked()
            {
                self.add_mouse_sensitivity_preset();
                self.persist_mouse_sensitivity_presets();
            }

            if ui
                .button(self.tr("+ Add path preset", "+ Thêm preset đường chuột"))
                .clicked()
            {
                self.add_mouse_path_preset();
                self.persist_mouse_path_presets();
            }

            if let Some(active_id) = self.active_mouse_record_preset_id {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(match language {
                        UiLanguage::Vietnamese => format!("Đang ghi preset #{active_id}"),
                        _ => format!("Recording preset #{active_id}"),
                    })
                    .strong()
                    .color(Color32::from_rgb(255, 96, 96)),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(current) = Self::current_mouse_speed() {
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            Self::tr_lang(language, "Current sensitivity:", "Độ nhạy hiện tại:"),
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
                ui.label(Self::tr_lang(language, "Speed", "Tốc độ"));

                mouse_sensitivity_live_sync |= ui
                    .checkbox(&mut self.state.mouse_sensitivity_restore_on_exit, "")
                    .changed();
                ui.label(
                    RichText::new(Self::tr_lang(
                        language,
                        "Restore sensitivity on exit",
                        "Khôi phục độ nhạy khi tắt app",
                    ))
                    .strong(),
                );
            });
        });

        ui.add_space(8.0);

        ui.label(RichText::new(Self::tr_lang(language, "Sensitivity", "Độ nhạy")).strong());

        for index in 0..self.state.mouse_sensitivity_presets.len() {
            let active_capture_target = self.capture_target.clone();
            let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
            ui.add_space(6.0);
            let preset = &mut self.state.mouse_sensitivity_presets[index];
            preset.target_window_title = None;
            preset.extra_target_window_titles.clear();
            preset.enabled = preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
            Self::show_preset_card(ui, preset.enabled, |ui| {
                ui.horizontal(|ui| {
                    let mut disabled_by_button = false;
                    let name_width = Self::preset_header_name_width(ui);
                    let response =
                        ui.add_sized([name_width, 21.0], TextEdit::singleline(&mut preset.name));
                    Self::apply_vietnamese_input_if_changed(
                        &response,
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
                        &mut preset.name,
                    );
                    mouse_sensitivity_live_sync |= response.changed();

                    let capture_target = CaptureRequest::MouseSensitivityPresetHotkey(preset.id);
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
                        Self::tr_lang(language, "Apply", "Áp dụng"),
                    )
                    .clicked()
                    {
                        let _ = self
                            .overlay_tx
                            .send(OverlayCommand::ApplyMouseSensitivityPreset(preset.id));
                    }
                    if Self::sound_style_toggle_button(
                        ui,
                        Self::tr_lang(language, "Restore", "Khôi phục"),
                    )
                    .clicked()
                    {
                        let _ = self
                            .overlay_tx
                            .send(OverlayCommand::RestoreMouseSensitivity);
                    }
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
                                cancel_active_capture_sensitivity = true;
                            } else {
                                next_mouse_sensitivity_capture_target = Some((
                                    capture_target,
                                    match language {
                                        UiLanguage::Vietnamese => {
                                            format!("Đang bật phím tắt cho {}.", preset.name)
                                        }
                                        _ => format!("Capturing hotkey for {}.", preset.name),
                                    },
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
                                Self::tr_lang(language, "Show", "Hiện")
                            } else {
                                Self::tr_lang(language, "Hide", "Hide")
                            },
                        )
                        .clicked()
                        {
                            preset.collapsed = !preset.collapsed;
                            mouse_sensitivity_live_sync = true;
                        }
                    });
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
                        ui.label(Self::tr_lang(language, "Speed", "Tốc độ chuột"));
                        mouse_sensitivity_live_sync |= ui
                            .add(Slider::new(&mut preset.speed, 1..=20).show_value(true))
                            .changed();
                        ui.end_row();
                    });
            });
        }

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

        let mut remove_id = None;
        let mut next_capture_target = None;
        let mut live_sync = false;
        let mut cancel_active_capture = false;

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(Self::tr_lang(language, "Mouse Path", "Đường dẫn chuột")).strong(),
            );
        });

        for index in 0..self.state.mouse_path_presets.len() {
            let active_capture_target = self.capture_target.clone();
            let pending_combo_keys = self.capture_hotkey_combo_keys.clone();
            ui.add_space(6.0);
            let preset = &mut self.state.mouse_path_presets[index];
            Self::show_preset_card(ui, false, |ui| {
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
                egui::Grid::new((preset.id, "mouse-path-grid"))
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(Self::tr_lang(language, "Record Hotkey", "Record Hotkey"));
                        ui.horizontal_wrapped(|ui| {
                            let capture_target = CaptureRequest::MousePathRecordHotkey(preset.id);
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
                                    match language {
                                        UiLanguage::Vietnamese => {
                                            format!("Đang bật phím tắt ghi cho {}.", preset.name)
                                        }
                                        _ => {
                                            format!("Capturing record hotkey for {}.", preset.name)
                                        }
                                    },
                                ));
                            }
                            if cancel_capture {
                                cancel_active_capture = true;
                            }
                        });
                        ui.end_row();

                        if self.active_mouse_record_preset_id == Some(preset.id) {
                            ui.label("");
                            ui.label(
                                RichText::new(Self::tr_lang(
                                    language,
                                    "Recording via hotkey...",
                                    "Đang ghi bằng phím tắt...",
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
                                        "Di chuyển tương đối",
                                    ),
                                )
                                .changed();
                        });
                        ui.end_row();
                    });
                ui.add_space(6.0);
                Self::render_mouse_path_preview(ui, language, &preset.events, 240.0);
            });
        }

        if let Some(rem_id) = remove_id {
            if self.mouse_path_step_preview_preset_id == Some(rem_id) {
                self.mouse_path_step_preview_preset_id = None;
                let _ = self.overlay_tx.send(OverlayCommand::PreviewMousePath(None));
                crate::overlay::wake_command_queue();
            }
            self.state
                .mouse_path_presets
                .retain(|preset| preset.id != rem_id);
            live_sync = true;
        }
        if let Some((target, status)) = next_capture_target {
            self.begin_capture(target, status);
        }
        if cancel_active_capture {
            self.cancel_capture();
        }
        if live_sync {
            self.persist_mouse_path_presets();
        }

        // Poll background jobs
        if let Some(ref job) = self.arduino_download_job {
            if job.is_finished() {
                let job = self.arduino_download_job.take();
                if let Some(j) = job {
                    match j.join() {
                        Ok(Ok(())) => {
                            self.arduino_tools_downloaded = true;
                            self.status = self.tr("Arduino tools downloaded successfully!", "Tải công cụ Arduino thành công!").to_owned();
                        }
                        Ok(Err(e)) => {
                            self.status = format!("Download failed: {e}");
                        }
                        Err(_) => {
                            self.status = "Download thread panicked".to_owned();
                        }
                    }
                }
            }
        }

        if self.arduino_flash_running {
            let mut res_guard = self.arduino_flash_result.lock();
            if let Some(res) = res_guard.take() {
                self.arduino_flash_running = false;
                match res {
                    Ok(()) => {
                        self.arduino_flash_status = self.tr("Flash Success!", "Nạp thành công!").to_owned();
                    }
                    Err(e) => {
                        self.arduino_flash_status = format!("Error: {e}");
                    }
                }
            } else {
                ui.ctx().request_repaint();
            }
        }

        let label_arduino = self.tr("Use Arduino Leonardo Mouse Emulation", "Sử dụng giả lập chuột Arduino Leonardo");
        let refresh_txt = self.tr("🔄 Refresh Ports", "🔄 Tải lại danh sách cổng");
        let select_port_txt = self.tr("Select Port", "Chọn cổng");
        let com_port_lbl = self.tr("COM Port:", "Cổng COM:");
        let guide_txt = self.tr("Setup Guide & Arduino Firmware", "Hướng dẫn cài đặt & Firmware Arduino");
        let setup_guide_1 = self.tr(
            "To prevent detection by anti-cheats, you should spoof the Vendor ID (VID) and Product ID (PID) of your Arduino Leonardo board at the firmware level:",
            "Để tránh bị các ứng dụng/game phát hiện, bạn nên giả lập (spoof) Vendor ID (VID) và Product ID (PID) của bo mạch Arduino Leonardo ở cấp độ firmware:"
        );
        let setup_guide_2 = self.tr(
            "1. Open boards.txt in the Arduino hardware packages folder:\n   C:\\Users\\<YourUser>\\AppData\\Local\\Arduino15\\packages\\arduino\\hardware\\avr\\<version>\\boards.txt",
            "1. Mở tệp boards.txt trong thư mục cài đặt phần cứng Arduino:\n   C:\\Users\\<TênUser>\\AppData\\Local\\Arduino15\\packages\\arduino\\hardware\\avr\\<version>\\boards.txt"
        );
        let setup_guide_3 = self.tr(
            "2. Search for the 'leonardo.build.vid' and 'leonardo.build.pid' variables, and change them to values of a standard commercial mouse (e.g., Logitech VID: 0x046D, PID: 0xC077).",
            "2. Tìm các biến 'leonardo.build.vid' và 'leonardo.build.pid', rồi thay đổi giá trị của chúng thành mã của một chuột thương mại tiêu chuẩn (ví dụ, Logitech VID: 0x046D, PID: 0xC077)."
        );
        let setup_guide_4 = self.tr(
            "3. Change string descriptors 'leonardo.build.usb_product' and 'leonardo.build.usb_manufacturer' to match standard mouse identifiers.",
            "3. Đổi tên nhận diện chuỗi 'leonardo.build.usb_product' và 'leonardo.build.usb_manufacturer' sang chuột thông thường."
        );
        let upload_sketch_lbl = self.tr(
            "Upload the following sketch to your Arduino Leonardo via Arduino IDE:",
            "Nạp chương trình (sketch) sau vào Arduino Leonardo bằng Arduino IDE:"
        );
        let copy_code_lbl = self.tr("📋 Copy Code", "📋 Sao chép mã");
        let copy_feedback_lbl = self.tr("Sketch code copied to clipboard!", "Đã sao chép mã nguồn sketch vào clipboard!");
        let arduino_panel_title = self.tr("Arduino Leonardo Emulation", "Giả lập phần cứng Arduino Leonardo");

        if self.arduino_available_ports.is_empty() {
            if let Ok(ports) = serialport::available_ports() {
                self.arduino_available_ports = ports.into_iter().map(|p| p.port_name).collect();
            }
        }

        let is_connected = !self.state.vision_settings.arduino_com_port.is_empty() 
            && self.arduino_available_ports.contains(&self.state.vision_settings.arduino_com_port);

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(arduino_panel_title).strong());
            ui.add_space(8.0);
            if is_connected {
                ui.label(RichText::new(self.tr("✅ Connected", "✅ Đang kết nối")).color(Color32::from_rgb(126, 224, 182)));
            } else {
                ui.label(RichText::new(self.tr("❌ Disconnected", "❌ Chưa kết nối")).color(Color32::from_rgb(255, 96, 96)));
            }
        });

        let mut arduino_changed = false;
        Self::show_preset_card(ui, self.state.vision_settings.use_arduino_mouse, |ui| {
            ui.horizontal(|ui| {
                let mut checkbox_res = ui.add_enabled(
                    is_connected,
                    egui::Checkbox::new(
                        &mut self.state.vision_settings.use_arduino_mouse,
                        label_arduino,
                    )
                );
                if !is_connected {
                    checkbox_res = checkbox_res.on_hover_text(self.tr(
                        "Please plug in the Arduino board and select the correct COM port to enable emulation.",
                        "Vui lòng cắm mạch Arduino và chọn đúng cổng COM để kích hoạt giả lập."
                    ));
                }
                if checkbox_res.changed() {
                    arduino_changed = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(refresh_txt).clicked() {
                        if let Ok(ports) = serialport::available_ports() {
                            self.arduino_available_ports = ports.into_iter().map(|p| p.port_name).collect();
                        }
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

            ui.add_space(8.0);

            // Flashing / Download section
            if !self.arduino_tools_downloaded {
                if self.arduino_download_job.is_some() {
                    let progress = self.arduino_download_progress.load(std::sync::atomic::Ordering::SeqCst) as f32 / 1000.0;
                    ui.horizontal(|ui| {
                        ui.label(self.tr("Downloading tools...", "Đang tải công cụ..."));
                        ui.add(egui::ProgressBar::new(progress).show_percentage());
                    });
                    ui.ctx().request_repaint();
                } else {
                    let download_btn_lbl = self.tr("📥 Download Flashing Tools & Firmware", "📥 Tải công cụ nạp & Firmware");
                    if ui.button(download_btn_lbl).clicked() {
                        self.start_arduino_tools_download();
                    }
                }
            } else {
                ui.horizontal(|ui| {
                    let flash_btn_lbl = self.tr("⚡ Auto-Flash Firmware", "⚡ Tự động nạp Firmware");
                    let flash_btn = ui.add_enabled(
                        !self.arduino_flash_running && !self.state.vision_settings.arduino_com_port.is_empty(),
                        egui::Button::new(flash_btn_lbl)
                    );
                    if flash_btn.clicked() {
                        self.start_arduino_flash();
                    }

                    ui.add_space(8.0);
                    let delete_btn_lbl = self.tr("🗑️ Delete Tools", "🗑️ Xóa công cụ nạp");
                    if ui.button(delete_btn_lbl).clicked() {
                        let _ = std::fs::remove_file(&self.paths.avrdude_exe);
                        let _ = std::fs::remove_file(&self.paths.avrdude_conf);
                        let _ = std::fs::remove_file(&self.paths.arduino_firmware_hex);
                        self.arduino_tools_downloaded = false;
                        self.arduino_flash_status.clear();
                    }
                    
                    if !self.arduino_flash_status.is_empty() {
                        ui.add_space(14.0);
                        ui.label(RichText::new(&self.arduino_flash_status).strong());
                    }
                });
            }

            ui.add_space(6.0);
            
            ui.collapsing(guide_txt, |ui| {
                ui.label(setup_guide_1);
                
                ui.add_space(4.0);
                ui.label(RichText::new(setup_guide_2).small().weak());
                ui.label(RichText::new(setup_guide_3).small().weak());
                ui.label(RichText::new(setup_guide_4).small().weak());

                ui.add_space(6.0);
                ui.label(RichText::new(upload_sketch_lbl).strong());
                
                let sketch_code = r#"#include <Mouse.h>

void setup() {
  Serial.begin(115200);
  while (!Serial) { ; }
  Mouse.begin();
}

void loop() {
  if (Serial.available() >= 6) {
    if (Serial.read() == 0xAA) {
      int cmd = Serial.read();
      if (cmd == 1) { // Move Relative
        int dx_high = Serial.read();
        int dx_low = Serial.read();
        int dy_high = Serial.read();
        int dy_low = Serial.read();
        int16_t dx = (int16_t)((dx_high << 8) | dx_low);
        int16_t dy = (int16_t)((dy_high << 8) | dy_low);
        
        while (dx != 0 || dy != 0) {
          int moveX = constrain(dx, -127, 127);
          int moveY = constrain(dy, -127, 127);
          Mouse.move(moveX, moveY, 0);
          dx -= moveX;
          dy -= moveY;
        }
      }
      else if (cmd == 2) { // Button Press/Release
        int btn = Serial.read();
        int state = Serial.read();
        Serial.read(); Serial.read(); // Unused
        char mouseBtn = MOUSE_LEFT;
        if (btn == 2) mouseBtn = MOUSE_RIGHT;
        else if (btn == 3) mouseBtn = MOUSE_MIDDLE;
        
        if (state == 1) Mouse.press(mouseBtn);
        else Mouse.release(mouseBtn);
      }
      else if (cmd == 3) { // Wheel Scroll
        int val = Serial.read();
        if (val & 0x80) val |= 0xFFFFFF00;
        Serial.read(); Serial.read(); Serial.read(); // Unused
        Mouse.move(0, 0, val);
      }
    }
  }
}"#;

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(copy_code_lbl).clicked() {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            let _ = clipboard.set_text(sketch_code.to_string());
                            self.status = copy_feedback_lbl.to_owned();
                        }
                    }
                });
                
                ui.add_space(4.0);
                let mut code_box = sketch_code;
                ui.add(
                    egui::TextEdit::multiline(&mut code_box)
                        .font(egui::FontId::monospace(12.0))
                        .desired_rows(15)
                        .lock_focus(true)
                        .interactive(false)
                );
            });
        });

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
                    "Ghi một đường chuột để xem trước tại đây",
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

    pub(crate) fn render_mouse_move_absolute_capture_overlay(
        &mut self,
        ctx: &egui::Context,
    ) -> bool {
        if self.mouse_move_absolute_capture_target.is_none() {
            return false;
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) || Self::is_vk_down(0x1B) {
            self.cancel_mouse_move_absolute_capture(ctx);
            return true;
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(120));
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .inner_margin(Margin::same(8)),
            )
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                let pointer = self.precise_image_search_capture_pointer(ctx);
                if pointer.is_some() {
                    if let Some((x, y)) = Self::current_screen_cursor_pos() {
                        let sampled_color = self.update_image_search_cursor_preview(ctx, x, y, 17);
                        self.render_image_search_cursor_preview_panel(
                            painter,
                            rect,
                            pointer,
                            sampled_color,
                            Some((x, y)),
                        );
                    }
                }
                self.refresh_capture_info_window(ctx);
            });
        true
    }

    pub(crate) fn add_mouse_path_preset(&mut self) -> u32 {
        self.add_mouse_path_preset_from(None)
    }

    pub(crate) fn add_mouse_path_preset_from(&mut self, source_preset_id: Option<u32>) -> u32 {
        let mut id = 1;
        while self.state.mouse_path_presets.iter().any(|p| p.id == id) {
            id += 1;
        }
        let source_preset = source_preset_id.and_then(|preset_id| {
            self.state
                .mouse_path_presets
                .iter()
                .find(|preset| preset.id == preset_id)
                .cloned()
        });
        self.state.next_mouse_path_preset_id = (self
            .state
            .mouse_path_presets
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            + 1)
        .max(id + 1);
        let mut new_preset = MousePathPreset::new(id);
        new_preset.collapsed = false;
        if let Some(source_preset) = source_preset {
            new_preset.replay_relative_motion = source_preset.replay_relative_motion;
            new_preset.events = source_preset.events;
        }
        self.state.mouse_path_presets.push(new_preset);
        self.sync_mouse_path_presets();
        self.status = format!("Added mouse path preset {id}.");
        id
    }

    pub(crate) fn sync_mouse_path_presets(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateMousePathPresets(
                self.state.mouse_path_presets.clone(),
            ));
    }

    pub(crate) fn add_mouse_sensitivity_preset(&mut self) {
        let mut id = 1;
        while self
            .state
            .mouse_sensitivity_presets
            .iter()
            .any(|p| p.id == id)
        {
            id += 1;
        }
        self.state.next_mouse_sensitivity_preset_id = (self
            .state
            .mouse_sensitivity_presets
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            + 1)
        .max(id + 1);
        self.state
            .mouse_sensitivity_presets
            .push(MouseSensitivityPreset::new(id));
        self.sync_mouse_sensitivity_presets();
        self.status = format!("Added mouse sensitivity preset {id}.");
    }

    pub(crate) fn sync_mouse_sensitivity_presets(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateMouseSensitivityPresets(
                self.state.mouse_sensitivity_presets.clone(),
            ));
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
        self.sync_mouse_path_presets();
        self.persist();
    }

    pub(crate) fn persist_mouse_sensitivity_presets(&mut self) {
        self.sync_mouse_sensitivity_presets();
        self.persist();
    }

    pub(crate) fn begin_mouse_path_draw_capture(
        &mut self,
        ctx: &egui::Context,
        preset_id: u32,
    ) {
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
                "Khong tim thay mouse path da chon.",
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
        self.center_window_next_frame = false;
        self.enforce_square_window_frames = 0;
        self.status = Self::tr_lang(
            self.state.ui_language,
            "Hide app. Hold left mouse to draw the path, then release to save. Press Esc to cancel.",
            "An app. Giu chuot trai de ve duong, tha ra de luu. Nhan Esc de huy.",
        )
        .to_owned();

        let _ = self.overlay_tx.send(OverlayCommand::BeginMousePathDrawCapture {
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
        let _ = self.overlay_tx.send(OverlayCommand::CancelMousePathDrawCapture);
        crate::overlay::wake_command_queue();
        self.restore_mouse_path_draw_capture_window(ctx);
        self.status = Self::tr_lang(
            self.state.ui_language,
            "Mouse path draw cancelled.",
            "Da huy ve mouse path.",
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
        if self.mouse_move_absolute_capture_target.is_some() {
            return;
        }
        let uses_blocked_click = Self::mouse_move_absolute_capture_uses_blocked_click(target);
        self.mouse_move_absolute_capture_target = Some(target);
        self.mouse_move_absolute_capture_wait_for_mouse_release = !uses_blocked_click;
        let viewport = ctx.input(|input| input.viewport().clone());
        self.mouse_move_absolute_restore_inner_size = viewport
            .inner_rect
            .map(|rect| rect.size())
            .or(Some(Self::desired_window_size()));
        self.mouse_move_absolute_restore_outer_pos = viewport.outer_rect.map(|rect| rect.min);
        self.center_window_next_frame = false;
        self.enforce_square_window_frames = 0;
        self.status = Self::tr_lang(
            self.state.ui_language,
            "Click anywhere on screen to capture X/Y. Press Esc to cancel.",
            "Bấm vào bất kỳ vị trí nào trên màn hình để lấy X/Y. Nhấn Esc để hủy.",
        )
        .to_owned();
        if uses_blocked_click {
            self.set_image_search_capture_mouse_blocked(true, false);
            let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(false));
            crate::overlay::wake_command_queue();
        }
        self.show_capture_info_window(ctx);
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    pub(crate) fn cancel_mouse_move_absolute_capture(&mut self, ctx: &egui::Context) {
        let Some(target) = self.mouse_move_absolute_capture_target else {
            return;
        };
        if Self::mouse_move_absolute_capture_uses_blocked_click(target) {
            self.set_image_search_capture_mouse_blocked(false, false);
        }
        self.mouse_move_absolute_capture_target = None;
        self.mouse_move_absolute_capture_wait_for_mouse_release = false;
        self.restore_mouse_move_absolute_capture_window(ctx);
        self.mouse_move_absolute_capture_raise_window = true;
        self.status = Self::tr_lang(
            self.state.ui_language,
            "Mouse position capture cancelled.",
            "Đã hủy bắt tọa độ chuột.",
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
                    self.mouse_move_absolute_capture_wait_for_mouse_release = false;
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
                self.mouse_move_absolute_capture_wait_for_mouse_release = false;
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
                            Some(&mut preset.hold_stop_step)
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
            self.status = match self.state.ui_language {
                crate::model::UiLanguage::Vietnamese => {
                    format!("Đã lấy tọa độ {}, {}.", screen_x, screen_y)
                }
                _ => format!("Captured position {}, {}.", screen_x, screen_y),
            };
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
                    self.mouse_move_absolute_capture_wait_for_mouse_release = false;
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
                self.mouse_move_absolute_capture_wait_for_mouse_release = false;
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
                            Some(&mut preset.hold_stop_step)
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
            self.status = match self.state.ui_language {
                crate::model::UiLanguage::Vietnamese => {
                    format!("Đã lấy tọa độ {}, {}.", screen_x, screen_y)
                }
                _ => format!("Captured position {}, {}.", screen_x, screen_y),
            };
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
                        Some(&mut preset.hold_stop_step)
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
            let is_secondary = matches!(target.capture_kind, MouseCaptureKind::GeometrySecondaryPos);
            let step = step_result;
            if let Some(step) = step {
                if let Some(point_idx) = target.extra_cond_index {
                    // Polyline/polygon point
                    let mut points: Vec<(String, String)> = step.geometry_spec.points_expr
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
            self.mouse_move_absolute_capture_wait_for_mouse_release = false;
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
            let pair_index = if matches!(target.capture_kind, MouseCaptureKind::GeometrySecondaryPos)
            {
                1
            } else {
                0
            };
            let object_result = self
                .state
                .geometry_presets
                .iter_mut()
                .find(|preset| preset.id == target.preset_id)
                .and_then(|preset| preset.objects.iter_mut().find(|object| object.id == object_id));

            let Some(object) = object_result else {
                self.cancel_mouse_move_absolute_capture(ctx);
                self.status = Self::tr_lang(
                    self.state.ui_language,
                    "Geometry target was not found.",
                    "Khong tim thay hinh hoc de lay toa do.",
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
            self.mouse_move_absolute_capture_wait_for_mouse_release = false;
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
            self.sync_geometry_presets();
            self.persist();
            return;
        }

        let Some(step) = step_result else {
            self.cancel_mouse_move_absolute_capture(ctx);
            self.status = Self::tr_lang(
                self.state.ui_language,
                "Mouse position capture target was not found.",
                "Không tìm thấy step để bắt tọa độ chuột.",
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
        self.mouse_move_absolute_capture_wait_for_mouse_release = false;
        self.restore_mouse_move_absolute_capture_window(ctx);
        self.mouse_move_absolute_capture_raise_window = true;
        self.status = match self.state.ui_language {
            UiLanguage::Vietnamese => {
                format!("Đã lấy tọa độ chuột {}, {}.", screen_x, screen_y)
            }
            _ => format!("Captured mouse position {}, {}.", screen_x, screen_y),
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
        // Chụp màn hình vùng 1x1 tại tọa độ (screen_x, screen_y) trước khi khôi phục cửa sổ và nhả chặn chuột.
        let capture = window_list::capture_virtual_screen_region(screen_x, screen_y, 1, 1);

        if used_blocked_click {
            self.set_image_search_capture_mouse_blocked(false, false);
        }
        self.mouse_move_absolute_capture_target = None;
        self.mouse_move_absolute_capture_wait_for_mouse_release = false;
        self.restore_mouse_move_absolute_capture_window(ctx);

        capture.and_then(|frame| {
            (frame.rgba.len() >= 4).then(|| RgbaColor {
                r: frame.rgba[0],
                g: frame.rgba[1],
                b: frame.rgba[2],
                a: 255,
            })
        })
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
        self.restore_mouse_move_absolute_viewport(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(true));
        crate::overlay::wake_command_queue();
    }

    pub(crate) fn poll_mouse_move_absolute_capture(&mut self, ctx: &egui::Context) {
        let Some(target) = self.mouse_move_absolute_capture_target else {
            return;
        };
        ctx.request_repaint_after(Duration::from_millis(16));
        if Self::is_vk_down(0x1B) {
            self.cancel_mouse_move_absolute_capture(ctx);
            return;
        }
        if Self::mouse_move_absolute_capture_uses_blocked_click(target) {
            return;
        }
        if self.mouse_move_absolute_capture_wait_for_mouse_release {
            if Self::is_vk_down(0x01) {
                return;
            }
            self.mouse_move_absolute_capture_wait_for_mouse_release = false;
            ctx.request_repaint();
            return;
        }
        if !Self::is_vk_down(0x01) {
            return;
        }
        let mut point = POINT::default();
        if unsafe { GetCursorPos(&mut point) }.is_ok() {
            self.finish_mouse_move_absolute_capture(ctx, target, point.x, point.y, None);
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
                "https://github.com/Baolinh0305/MacroNest/releases/download/tools/arduino_tools.zip";
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
                let p = (downloaded as f32 / total_size as f32 * 1000.0) as u32;
                progress.store(p, std::sync::atomic::Ordering::SeqCst);
            }

            drop(file);

            // Extract using PowerShell
            let extract_script = format!(
                "Expand-Archive -LiteralPath {} -DestinationPath {} -Force",
                Self::powershell_quote(&paths.arduino_tools_zip.to_string_lossy()),
                Self::powershell_quote(&paths.bin_dir.to_string_lossy()),
            );
            let extract_status = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &extract_script])
                .status()?;
            if !extract_status.success() {
                anyhow::bail!("Failed to extract arduino_tools.zip");
            }

            let _ = std::fs::remove_file(&paths.arduino_tools_zip);

            Ok(())
        });

        self.arduino_download_job = Some(job);
    }

    pub(crate) fn start_arduino_flash(&mut self) {
        if self.arduino_flash_running {
            return;
        }

        let port = self.state.vision_settings.arduino_com_port.clone();
        if port.is_empty() {
            self.arduino_flash_status = self.tr("Error: Select a COM Port first", "Lỗi: Hãy chọn cổng COM trước").to_owned();
            return;
        }

        self.arduino_flash_running = true;
        self.arduino_flash_status = self.tr("Resetting board (1200 baud touch)...", "Đang khôi phục mạch (chạm 1200 baud)...").to_owned();

        let paths = self.paths.clone();
        let flash_result = self.arduino_flash_result.clone();
        *flash_result.lock() = None;

        let ui_lang = self.state.ui_language;

        std::thread::spawn(move || {
            let run_flash = || -> anyhow::Result<()> {
                // 1. Scan ports before touch
                let ports_before = serialport::available_ports().unwrap_or_default();
                let before_names: std::collections::HashSet<String> = ports_before
                    .into_iter()
                    .map(|p| p.port_name)
                    .collect();

                // 2. Perform 1200 baud touch to reset into bootloader mode
                {
                    let _ = serialport::new(&port, 1200)
                        .timeout(std::time::Duration::from_millis(500))
                        .open();
                }

                // 3. Wait for bootloader COM port to re-appear
                std::thread::sleep(std::time::Duration::from_millis(1500));

                let mut bootloader_port = port.clone();
                let start_time = std::time::Instant::now();
                let mut found = false;

                while start_time.elapsed().as_secs() < 6 {
                    let current_ports = serialport::available_ports().unwrap_or_default();
                    for p in &current_ports {
                        if !before_names.contains(&p.port_name) {
                            bootloader_port = p.port_name.clone();
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }

                // 4. Execute avrdude to flash
                if !paths.avrdude_exe.exists() {
                    anyhow::bail!("avrdude.exe not found");
                }
                
                let output = std::process::Command::new(&paths.avrdude_exe)
                    .args([
                        "-C", &paths.avrdude_conf.to_string_lossy(),
                        "-v",
                        "-p", "atmega32u4",
                        "-c", "avr109",
                        "-P", &bootloader_port,
                        "-b", "57600",
                        "-D",
                        "-U", &format!("flash:w:{}:i", paths.arduino_firmware_hex.to_string_lossy()),
                    ])
                    .output()?;

                if !output.status.success() {
                    let err_msg = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!("avrdude flash failed: {}", err_msg);
                }

                Ok(())
            };

            let res = run_flash();
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
