use crate::model::{AudioSensePreset, AudioSenseSource, PitchAudioSenseSettings, UiLanguage};
use crate::ui::CrosshairApp;
use eframe::egui::{self, Color32, ComboBox, DragValue, Sense, TextEdit, vec2};

impl CrosshairApp {
    pub(crate) fn render_audiosense_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        let ctx = ui.ctx().clone();
        ui.add_space(2.0);

        ui.horizontal(|ui| {
            if ui
                .button(Self::tr_lang(language, "+ Pitch preset", "+ Preset cao do"))
                .clicked()
            {
                let id = self
                    .state
                    .audio_sense_presets
                    .iter()
                    .map(|preset| preset.id)
                    .max()
                    .unwrap_or(0)
                    + 1;
                self.state.next_audio_sense_preset_id = id + 1;
                self.state
                    .audio_sense_presets
                    .push(AudioSensePreset::new_pitch(id));
                self.sync_audio_sense_presets();
                self.persist();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button_label = if self.audio_sense_test_active {
                    Self::tr_lang(language, "Stop test", "Dung test")
                } else {
                    Self::tr_lang(language, "Test sound", "Test sound")
                };
                if ui.button(button_label).clicked() {
                    if self.audio_sense_test_active {
                        self.pitch_monitor.stop();
                        self.audio_sense_test_active = false;
                    } else {
                        let config = PitchAudioSenseSettings {
                            monitor: Self::audio_sense_test_monitor_settings(
                                &self.audio_sense_test_settings,
                            ),
                            output_note_var: String::new(),
                            output_confidence_var: String::new(),
                            output_level_var: String::new(),
                            ..PitchAudioSenseSettings::default()
                        };
                        if self.pitch_monitor.start(config).is_ok() {
                            self.audio_sense_test_active = true;
                        }
                    }
                }

                if self.audio_sense_test_settings.source == AudioSenseSource::Microphone {
                    let mut device_changed = false;
                    ComboBox::from_id_salt("audiosense-test-device")
                        .width(210.0)
                        .selected_text(
                            self.audio_sense_test_settings
                                .input_device_name
                                .clone()
                                .unwrap_or_else(|| {
                                    Self::tr_lang(
                                        language,
                                        "Default microphone",
                                        "Micro mac dinh",
                                    )
                                    .to_owned()
                                }),
                        )
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    self.audio_sense_test_settings.input_device_name.is_none(),
                                    Self::tr_lang(
                                        language,
                                        "Default microphone",
                                        "Micro mac dinh",
                                    ),
                                )
                                .clicked()
                            {
                                self.audio_sense_test_settings.input_device_name = None;
                                device_changed = true;
                            }
                            for device in &self.audio_sense_devices {
                                if ui
                                    .selectable_label(
                                        self.audio_sense_test_settings.input_device_name.as_deref()
                                            == Some(device.as_str()),
                                        device,
                                    )
                                    .clicked()
                                {
                                    self.audio_sense_test_settings.input_device_name =
                                        Some(device.clone());
                                    device_changed = true;
                                }
                            }
                        });
                    if device_changed && self.audio_sense_test_active {
                        self.restart_audio_sense_test();
                    }
                }

                let mut source_changed = false;
                ComboBox::from_id_salt("audiosense-test-source")
                    .width(130.0)
                    .selected_text(match self.audio_sense_test_settings.source {
                        AudioSenseSource::System => Self::tr_lang(language, "System", "He thong"),
                        AudioSenseSource::Microphone => {
                            Self::tr_lang(language, "Microphone", "Micro")
                        }
                    })
                    .show_ui(ui, |ui| {
                        source_changed |= ui
                            .selectable_value(
                                &mut self.audio_sense_test_settings.source,
                                AudioSenseSource::System,
                                Self::tr_lang(language, "System", "He thong"),
                            )
                            .changed();
                        source_changed |= ui
                            .selectable_value(
                                &mut self.audio_sense_test_settings.source,
                                AudioSenseSource::Microphone,
                                Self::tr_lang(language, "Microphone", "Micro"),
                            )
                            .changed();
                    });
                if source_changed && self.audio_sense_test_active {
                    self.restart_audio_sense_test();
                }
            });
        });

        ui.add_space(8.0);

        let test_snapshot = self.pitch_monitor.snapshot();
        if self.audio_sense_test_active || !test_snapshot.waveform.is_empty() {
            if self.audio_sense_test_active {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
            Self::show_preset_card(ui, self.audio_sense_test_active, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(Self::tr_lang(
                            language,
                            "Live input preview",
                            "Xem truoc am thanh",
                        ))
                        .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!(
                            "{} {:.2}",
                            Self::tr_lang(language, "Level", "Muc"),
                            test_snapshot.level
                        ));
                        ui.separator();
                        ui.label(format!(
                            "{} {}",
                            Self::tr_lang(language, "Note", "Note"),
                            test_snapshot.note
                        ));
                    });
                });
                ui.add_space(4.0);
                Self::render_audio_sense_live_waveform(ui, &test_snapshot.waveform);
                if let Some(error) = test_snapshot.error.as_ref() {
                    ui.add_space(4.0);
                    ui.colored_label(Color32::from_rgb(255, 120, 120), error);
                }
            });
            ui.add_space(8.0);
        }

        let mut remove_id = None;
        let mut changed = false;
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(Self::tr_lang(language, "Detect Pitch", "Phat hien cao do"))
                .strong()
                .size(14.0),
        );
        ui.add_space(4.0);

        let matching_indices = self
            .state
            .audio_sense_presets
            .iter()
            .enumerate()
            .map(|(index, _)| index)
            .collect::<Vec<_>>();

        for (position, preset_index) in matching_indices.iter().copied().enumerate() {
                let preset = &mut self.state.audio_sense_presets[preset_index];
                ui.add_space(6.0);
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
                        changed |= response.changed();

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if Self::sound_style_remove_button(ui).clicked() {
                                remove_id = Some(preset.id);
                            }
                            if Self::sound_style_toggle_button(
                                ui,
                                if preset.collapsed {
                                    Self::tr_lang(language, "Show", "Hien")
                                } else {
                                    Self::tr_lang(language, "Hide", "An")
                                },
                            )
                            .clicked()
                            {
                                preset.collapsed = !preset.collapsed;
                                changed = true;
                            }
                        });
                    });

                    if !preset.collapsed {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(Self::tr_lang(language, "Source", "Nguon"));
                            ComboBox::from_id_salt(("audiosense-source", preset.id))
                                .width(120.0)
                                .selected_text(match preset.pitch.monitor.source {
                                    AudioSenseSource::System => {
                                        Self::tr_lang(language, "System", "He thong")
                                    }
                                    AudioSenseSource::Microphone => {
                                        Self::tr_lang(language, "Microphone", "Micro")
                                    }
                                })
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut preset.pitch.monitor.source,
                                            AudioSenseSource::System,
                                            Self::tr_lang(language, "System", "He thong"),
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut preset.pitch.monitor.source,
                                            AudioSenseSource::Microphone,
                                            Self::tr_lang(language, "Microphone", "Micro"),
                                        )
                                        .changed();
                                });

                            if preset.pitch.monitor.source == AudioSenseSource::Microphone {
                                ComboBox::from_id_salt(("audiosense-device", preset.id))
                                    .width(210.0)
                                    .selected_text(
                                        preset
                                            .pitch
                                            .monitor
                                            .input_device_name
                                            .clone()
                                            .unwrap_or_else(|| {
                                                Self::tr_lang(
                                                    language,
                                                    "Default microphone",
                                                    "Micro mac dinh",
                                                )
                                                .to_owned()
                                            }),
                                    )
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(
                                                preset.pitch.monitor.input_device_name.is_none(),
                                                Self::tr_lang(
                                                    language,
                                                    "Default microphone",
                                                    "Micro mac dinh",
                                                ),
                                            )
                                            .clicked()
                                        {
                                            preset.pitch.monitor.input_device_name = None;
                                            changed = true;
                                        }
                                        for device in &self.audio_sense_devices {
                                            if ui
                                                .selectable_label(
                                                    preset
                                                        .pitch
                                                        .monitor
                                                        .input_device_name
                                                        .as_deref()
                                                        == Some(device.as_str()),
                                                    device,
                                                )
                                                .clicked()
                                            {
                                                preset.pitch.monitor.input_device_name =
                                                    Some(device.clone());
                                                changed = true;
                                            }
                                        }
                                    });
                            }
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Hz");
                            changed |= ui
                                .add(
                                    DragValue::new(&mut preset.pitch.monitor.updates_per_second)
                                        .range(1..=60)
                                        .speed(0.2),
                                )
                                .changed();
                            changed |= ui
                                .checkbox(
                                    &mut preset.pitch.monitor.listen_forever,
                                    Self::tr_lang(language, "Listen forever", "Nghe vinh vien"),
                                )
                                .changed();
                            if !preset.pitch.monitor.listen_forever {
                                ui.label("ms");
                                changed |= ui
                                    .add(
                                        DragValue::new(&mut preset.pitch.monitor.duration_ms)
                                            .range(100..=60_000)
                                            .speed(10.0),
                                    )
                                    .changed();
                            }
                        });

                        ui.add_space(4.0);
                        changed |= render_pitch_settings_ui(
                            ui,
                            language,
                            &mut preset.pitch,
                            &self.pitch_monitor.snapshot(),
                        );
                    }
                });
                ui.add_space(4.0);
                if position + 1 < matching_indices.len() {
                    ui.separator();
                }
        }

        if let Some(remove_id) = remove_id {
            self.state.audio_sense_presets.retain(|preset| preset.id != remove_id);
            if self.active_pitch_preview_preset_id == Some(remove_id) {
                self.pitch_monitor.stop();
                self.active_pitch_preview_preset_id = None;
            }
            changed = true;
        }

        if changed {
            self.sync_audio_sense_presets();
            self.persist();
        }
    }

    fn restart_audio_sense_test(&mut self) {
        self.pitch_monitor.stop();
        let config = PitchAudioSenseSettings {
            monitor: Self::audio_sense_test_monitor_settings(&self.audio_sense_test_settings),
            output_note_var: String::new(),
            output_confidence_var: String::new(),
            output_level_var: String::new(),
            ..PitchAudioSenseSettings::default()
        };
        self.audio_sense_test_active = self.pitch_monitor.start(config).is_ok();
    }

    fn audio_sense_test_monitor_settings(
        settings: &crate::model::AudioSenseMonitorSettings,
    ) -> crate::model::AudioSenseMonitorSettings {
        let mut monitor = settings.clone();
        monitor.listen_forever = true;
        monitor.updates_per_second = monitor.updates_per_second.max(60);
        monitor
    }

    fn render_audio_sense_live_waveform(ui: &mut egui::Ui, waveform: &[f32]) {
        const WAVEFORM_SLOTS: usize = 160;
        let desired_size = vec2(ui.available_width().max(220.0), 72.0);
        let (rect, _) = ui.allocate_exact_size(desired_size, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8.0, ui.visuals().extreme_bg_color);

        let bar_width = rect.width() / WAVEFORM_SLOTS as f32;
        let zero_fill = WAVEFORM_SLOTS.saturating_sub(waveform.len());
        for index in 0..WAVEFORM_SLOTS {
            let level = if index < zero_fill {
                0.0
            } else {
                waveform
                    .get(index - zero_fill)
                    .copied()
                    .unwrap_or(0.0)
            };
            let amplitude = level.clamp(0.0, 1.0);
            let center_x = rect.left() + (index as f32 + 0.5) * bar_width;
            let half_height = amplitude * rect.height() * 0.42;
            if half_height > 0.0 {
                let wave_rect = egui::Rect::from_min_max(
                    egui::pos2(
                        center_x - (bar_width * 0.35).max(1.0),
                        rect.center().y - half_height,
                    ),
                    egui::pos2(
                        center_x + (bar_width * 0.35).max(1.0),
                        rect.center().y + half_height,
                    ),
                );
                painter.rect_filled(wave_rect, 1.0, Color32::from_rgb(96, 172, 224));
            }
        }

        if waveform.is_empty() {
            painter.line_segment(
                [
                    egui::pos2(rect.left(), rect.center().y),
                    egui::pos2(rect.right(), rect.center().y),
                ],
                egui::Stroke::new(2.0, Color32::from_gray(120)),
            );
        }
    }
}

fn render_pitch_settings_ui(
    ui: &mut egui::Ui,
    language: UiLanguage,
    settings: &mut PitchAudioSenseSettings,
    snapshot: &crate::audiosense::PitchSnapshot,
) -> bool {
    let mut changed = false;
    changed |= ui
        .checkbox(
            &mut settings.show_sharps,
            CrosshairApp::tr_lang(language, "Sharps", "Dau thang"),
        )
        .changed();

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(format!(
            "{}: {}",
            CrosshairApp::tr_lang(language, "Current note", "Cao do hien tai"),
            snapshot.note
        ));
        ui.label(format!("conf {:.2}", snapshot.confidence));
        ui.label(format!("level {:.2}", snapshot.level));
    });
    changed
}

