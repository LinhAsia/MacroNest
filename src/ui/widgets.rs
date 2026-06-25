use eframe::egui::{self, Button, Color32, RichText, StrokeKind};

use super::{CrosshairApp, MacroStepInlineFeedback};

impl CrosshairApp {
    pub(crate) fn preset_frame(ui: &egui::Ui, enabled: bool) -> egui::Frame {
        let dark_mode = ui.visuals().dark_mode;
        let fill = if enabled {
            if dark_mode {
                Color32::from_rgba_premultiplied(32, 92, 52, 120)
            } else {
                Color32::from_rgb(198, 232, 210)
            }
        } else if dark_mode {
            ui.visuals().faint_bg_color
        } else {
            Color32::from_rgb(250, 251, 252)
        };
        let stroke_color = if enabled {
            if dark_mode {
                Color32::from_rgb(108, 224, 148)
            } else {
                Color32::from_rgb(112, 204, 142)
            }
        } else if dark_mode {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        } else {
            Color32::from_rgb(214, 222, 230)
        };
        egui::Frame::group(ui.style())
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke_color))
    }

    pub(crate) fn folder_frame(ui: &egui::Ui, active: bool, hovered: bool) -> egui::Frame {
        let (fill, stroke_color) = if active {
            let border = if hovered {
                Color32::from_rgb(255, 170, 75)
            } else {
                Color32::from_rgb(220, 130, 45)
            };
            (Color32::from_rgba_premultiplied(100, 60, 20, 100), border)
        } else {
            let border = if hovered {
                Color32::from_rgb(190, 135, 75)
            } else {
                Color32::from_rgb(140, 90, 45)
            };
            (Color32::from_rgba_premultiplied(45, 30, 15, 60), border)
        };
        egui::Frame::group(ui.style())
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke_color))
    }

    pub(crate) fn show_folder_card<R>(
        ui: &mut egui::Ui,
        active: bool,
        hovered: bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> (R, egui::Response) {
        let dark_mode = ui.visuals().dark_mode;
        let res = Self::folder_frame(ui, active, hovered).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let previous = ui.visuals().override_text_color;
            if dark_mode {
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(255, 240, 220));
            }
            let output = add_contents(ui);
            ui.visuals_mut().override_text_color = previous;
            output
        });
        (res.inner, res.response)
    }

    pub(crate) fn preset_body_text_color(dark_mode: bool, enabled: bool) -> Color32 {
        match (dark_mode, enabled) {
            (true, true) => Color32::from_rgb(248, 250, 252),
            (true, false) => Color32::from_rgb(214, 222, 232),
            (false, true) => Color32::from_rgb(250, 250, 250),
            (false, false) => Color32::from_rgb(32, 32, 32),
        }
    }

    pub(crate) fn preset_header_name_width(_ui: &egui::Ui) -> f32 {
        160.0
    }

    pub(crate) fn show_preset_card<R>(
        ui: &mut egui::Ui,
        enabled: bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let dark_mode = ui.visuals().dark_mode;
        Self::preset_frame(ui, enabled)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let previous = ui.visuals().override_text_color;
                if dark_mode {
                    ui.visuals_mut().override_text_color =
                        Some(Self::preset_body_text_color(dark_mode, enabled));
                }
                let output = add_contents(ui);
                ui.visuals_mut().override_text_color = previous;
                output
            })
            .inner
    }

    pub(crate) fn show_macro_preset_card<R>(
        ui: &mut egui::Ui,
        group_enabled: bool,
        preset_enabled: bool,
        window_focus_trigger: bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let dark_mode = ui.visuals().dark_mode;
        let (fill, stroke_color) = if group_enabled {
            if preset_enabled {
                if window_focus_trigger {
                    if dark_mode {
                        (
                            Color32::from_rgba_premultiplied(32, 76, 106, 132),
                            Color32::from_rgb(116, 204, 255),
                        )
                    } else {
                        (
                            Color32::from_rgb(218, 236, 248),
                            Color32::from_rgb(76, 146, 204),
                        )
                    }
                } else if dark_mode {
                    (
                        Color32::from_rgba_premultiplied(32, 92, 52, 120),
                        Color32::from_rgb(108, 224, 148),
                    )
                } else {
                    (
                        Color32::from_rgb(198, 232, 210),
                        Color32::from_rgb(112, 204, 142),
                    )
                }
            } else {
                (
                    if dark_mode {
                        ui.visuals().faint_bg_color
                    } else {
                        Color32::from_rgb(250, 251, 252)
                    },
                    if dark_mode {
                        ui.visuals().widgets.noninteractive.bg_stroke.color
                    } else {
                        Color32::from_rgb(214, 222, 230)
                    },
                )
            }
        } else if preset_enabled {
            if window_focus_trigger {
                if dark_mode {
                    (
                        Color32::from_rgba_premultiplied(22, 54, 78, 72),
                        Color32::from_rgb(78, 132, 176),
                    )
                } else {
                    (
                        Color32::from_rgb(232, 240, 247),
                        Color32::from_rgb(118, 152, 184),
                    )
                }
            } else if dark_mode {
                (
                    Color32::from_rgba_premultiplied(25, 65, 40, 60),
                    Color32::from_rgb(60, 120, 85),
                )
            } else {
                (
                    Color32::from_rgb(236, 242, 238),
                    Color32::from_rgb(180, 198, 188),
                )
            }
        } else {
            (
                if dark_mode {
                    ui.visuals().faint_bg_color
                } else {
                    Color32::from_rgb(250, 251, 252)
                },
                if dark_mode {
                    ui.visuals().widgets.noninteractive.bg_stroke.color
                } else {
                    Color32::from_rgb(214, 222, 230)
                },
            )
        };
        egui::Frame::group(ui.style())
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke_color))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let previous = ui.visuals().override_text_color;
                if dark_mode {
                    ui.visuals_mut().override_text_color =
                        Some(Self::preset_body_text_color(dark_mode, preset_enabled));
                }
                let output = add_contents(ui);
                ui.visuals_mut().override_text_color = previous;
                output
            })
            .inner
    }

    pub(crate) fn hover_if(response: egui::Response, enabled: bool, text: &str) -> egui::Response {
        if enabled && !text.is_empty() {
            response.on_hover_text(text)
        } else {
            response
        }
    }

    pub(crate) fn add_with_show_hover(
        ui: &mut egui::Ui,
        widget: impl egui::Widget,
    ) -> egui::Response {
        let response = ui.add(widget);
        Self::paint_show_hover_outline(ui, &response);
        response
    }

    pub(crate) fn add_with_show_hover_radius(
        ui: &mut egui::Ui,
        radius: u8,
        widget: impl egui::Widget,
    ) -> egui::Response {
        let response = ui.add(widget);
        Self::paint_show_hover_outline_radius(ui, &response, radius);
        response
    }

    pub(crate) fn add_sized_with_show_hover(
        ui: &mut egui::Ui,
        size: impl Into<egui::Vec2>,
        widget: impl egui::Widget,
    ) -> egui::Response {
        let response = ui.add_sized(size, widget);
        Self::paint_show_hover_outline(ui, &response);
        response
    }

    pub(crate) fn add_sized_with_show_hover_radius(
        ui: &mut egui::Ui,
        size: impl Into<egui::Vec2>,
        radius: u8,
        widget: impl egui::Widget,
    ) -> egui::Response {
        let response = ui.add_sized(size, widget);
        Self::paint_show_hover_outline_radius(ui, &response, radius);
        response
    }

    pub(crate) fn render_macro_step_inline_feedback(
        ui: &mut egui::Ui,
        feedback: Option<&MacroStepInlineFeedback>,
    ) -> bool {
        let Some(feedback) = feedback else {
            return false;
        };
        if feedback.message.trim().is_empty() {
            return false;
        }

        let mut open_settings_clicked = false;
        let message_response = ui.add_sized(
            [170.0, 18.0],
            egui::Label::new(
                RichText::new(feedback.message.clone())
                    .size(11.0)
                    .color(Color32::from_rgb(255, 170, 170)),
            )
            .wrap_mode(egui::TextWrapMode::Truncate),
        );
        if message_response.hovered() {
            let _ = message_response.on_hover_text(feedback.message.clone());
        }
        if feedback.open_groq_settings
            && Self::add_sized_with_show_hover(
                ui,
                [58.0, 18.0],
                egui::Button::new(RichText::new("Settings").size(11.0)),
            )
            .clicked()
        {
            open_settings_clicked = true;
        }

        open_settings_clicked
    }

    pub(crate) fn paint_show_hover_outline(ui: &mut egui::Ui, response: &egui::Response) {
        if response.hovered() {
            let hovered = ui.visuals().widgets.hovered;
            ui.painter().rect_stroke(
                response.rect,
                hovered.corner_radius,
                hovered.bg_stroke,
                StrokeKind::Inside,
            );
        }
    }

    pub(crate) fn paint_show_hover_outline_radius(
        ui: &mut egui::Ui,
        response: &egui::Response,
        radius: u8,
    ) {
        if response.hovered() {
            let hovered = ui.visuals().widgets.hovered;
            ui.painter().rect_stroke(
                response.rect,
                egui::CornerRadius::same(radius),
                hovered.bg_stroke,
                StrokeKind::Inside,
            );
        }
    }

    pub(crate) fn sized_button(ui: &mut egui::Ui, width: f32, label: &str) -> egui::Response {
        Self::with_emphasized_button_hover(ui, |ui| ui.add_sized([width, 24.0], Button::new(label)))
    }

    pub(crate) fn sound_style_toggle_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
        Self::with_emphasized_button_hover(ui, |ui| ui.add_sized([84.0, 24.0], Button::new(label)))
    }

    pub(crate) fn sound_style_remove_button(ui: &mut egui::Ui) -> egui::Response {
        Self::with_emphasized_button_hover(ui, |ui| {
            ui.add_sized(
                [36.0, 24.0],
                Button::new(Self::material_icon_text(0xe872, 18.0)),
            )
        })
    }

    pub(crate) fn sound_style_icon_button(ui: &mut egui::Ui, icon: RichText) -> egui::Response {
        Self::with_emphasized_button_hover(ui, |ui| ui.add_sized([36.0, 24.0], Button::new(icon)))
    }

    pub(crate) fn enabled_icon_button(ui: &mut egui::Ui, enabled: bool) -> egui::Response {
        let icon = if enabled { 0xe5ca } else { 0xe835 };
        let fill = if enabled {
            Color32::from_rgba_premultiplied(72, 156, 116, 120)
        } else {
            ui.visuals().faint_bg_color
        };
        let stroke = if enabled {
            Color32::from_rgb(126, 224, 182)
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        Self::with_emphasized_button_hover(ui, |ui| {
            ui.add_sized(
                [36.0, 24.0],
                Button::new(Self::material_icon_text(icon, 18.0))
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0, stroke)),
            )
        })
    }

    pub(crate) fn with_emphasized_button_hover(
        ui: &mut egui::Ui,
        add_contents: impl FnOnce(&mut egui::Ui) -> egui::Response,
    ) -> egui::Response {
        let response = add_contents(ui);
        if response.hovered() {
            Self::paint_show_hover_outline(ui, &response);
        }
        response
    }

    pub(crate) fn with_emphasized_button_hover_radius(
        ui: &mut egui::Ui,
        radius: u8,
        add_contents: impl FnOnce(&mut egui::Ui) -> egui::Response,
    ) -> egui::Response {
        let response = add_contents(ui);
        Self::paint_show_hover_outline_radius(ui, &response, radius);
        response
    }

    pub(crate) fn settings_card_frame(ui: &egui::Ui) -> egui::Frame {
        let is_dark = ui.visuals().dark_mode;
        let (fill, stroke) = if is_dark {
            (
                Color32::from_rgba_premultiplied(54, 67, 88, 50),
                Color32::from_rgba_premultiplied(96, 118, 148, 120),
            )
        } else {
            (
                Color32::from_rgba_premultiplied(214, 223, 235, 80),
                Color32::from_rgba_premultiplied(170, 182, 198, 120),
            )
        };
        egui::Frame::group(ui.style())
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(14.0)
            .inner_margin(egui::Margin::same(16))
    }
}
