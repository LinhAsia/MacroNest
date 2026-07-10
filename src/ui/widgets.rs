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
        let res = Self::preset_frame(ui, enabled)
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
            });

        if res.response.hovered() {
            let hover_fill = if dark_mode {
                Color32::from_rgba_unmultiplied(255, 255, 255, 14)
            } else {
                Color32::from_rgba_unmultiplied(24, 64, 104, 18)
            };
            let hover_stroke = if dark_mode {
                Color32::from_rgba_unmultiplied(155, 220, 255, 92)
            } else {
                Color32::from_rgba_unmultiplied(42, 106, 166, 110)
            };
            ui.painter().rect_filled(
                res.response.rect,
                egui::CornerRadius::same(6),
                hover_fill,
            );
            ui.painter().rect_stroke(
                res.response.rect,
                egui::CornerRadius::same(6),
                egui::Stroke::new(1.0, hover_stroke),
                egui::StrokeKind::Middle,
            );
        }

        res.inner
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
        let res = egui::Frame::group(ui.style())
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
            });

        if res.response.hovered() {
            let hover_fill = if dark_mode {
                Color32::from_rgba_unmultiplied(255, 255, 255, 14)
            } else {
                Color32::from_rgba_unmultiplied(24, 64, 104, 18)
            };
            let hover_stroke = if dark_mode {
                Color32::from_rgba_unmultiplied(155, 220, 255, 92)
            } else {
                Color32::from_rgba_unmultiplied(42, 106, 166, 110)
            };
            ui.painter().rect_filled(
                res.response.rect,
                egui::CornerRadius::same(6),
                hover_fill,
            );
            ui.painter().rect_stroke(
                res.response.rect,
                egui::CornerRadius::same(6),
                egui::Stroke::new(1.0, hover_stroke),
                egui::StrokeKind::Middle,
            );
        }

        res.inner
    }

    pub(crate) fn hover_if(response: egui::Response, enabled: bool, text: &str) -> egui::Response {
        if enabled && !text.is_empty() {
            response.on_hover_text(text)
        } else {
            response
        }
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

    pub(crate) fn render_premium_color_picker(
        ui: &mut egui::Ui,
        color: &mut crate::model::RgbaColor,
        alpha_mode: egui::color_picker::Alpha,
    ) -> bool {
        let mut color32 = egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
        let mut hsva = egui::epaint::Hsva::from(color32);
        let mut changed = false;

        ui.vertical(|ui| {
            // 1. SV Grid (Saturation & Value)
            let mut s = hsva.s;
            let mut v = hsva.v;
            if Self::premium_color_slider_2d(ui, hsva.h, &mut s, &mut v) {
                hsva.s = s;
                hsva.v = v;
                changed = true;
            }

            ui.add_space(8.0);

            // 2. Hue Slider (Rainbow)
            let mut h = hsva.h;
            if Self::premium_hue_slider(ui, &mut h) {
                hsva.h = h;
                changed = true;
            }

            // 3. Alpha Slider (Opacity)
            match alpha_mode {
                egui::color_picker::Alpha::BlendOrAdditive
                | egui::color_picker::Alpha::OnlyBlend => {
                    ui.add_space(8.0);
                    let mut a = hsva.a;
                    if Self::premium_alpha_slider(ui, &mut a, hsva.h, hsva.s, hsva.v) {
                        hsva.a = a;
                        changed = true;
                    }
                }
                egui::color_picker::Alpha::Opaque => {}
            }

            if changed {
                color32 = egui::Color32::from(hsva);
                color.r = color32.r();
                color.g = color32.g();
                color.b = color32.b();
                color.a = color32.a();
            }

            // 4. Hex input at the bottom
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("#")
                        .strong()
                        .color(ui.visuals().weak_text_color()),
                );
                changed |= Self::render_rgba_hex_input(
                    ui,
                    ui.id().with("premium-color-hex"),
                    color,
                    alpha_mode,
                    120.0,
                );
            });
        });

        changed
    }

    pub(crate) fn render_rgba_hex_input(
        ui: &mut egui::Ui,
        id: egui::Id,
        color: &mut crate::model::RgbaColor,
        alpha_mode: egui::color_picker::Alpha,
        desired_width: f32,
    ) -> bool {
        let fallback = Self::rgba_hex_string(*color, alpha_mode);
        let mut draft = ui
            .ctx()
            .data(|data| data.get_temp::<String>(id))
            .unwrap_or_else(|| fallback.clone());

        let response = ui.add(
            egui::TextEdit::singleline(&mut draft)
                .font(egui::TextStyle::Monospace.resolve(ui.style()))
                .desired_width(desired_width),
        );

        let max_len = match alpha_mode {
            egui::color_picker::Alpha::Opaque => 6,
            _ => 8,
        };
        let mut cleaned: String = draft
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        cleaned.truncate(max_len);
        if cleaned != draft {
            draft = cleaned.clone();
        }

        let changed = if response.changed() {
            Self::apply_rgba_hex_string(color, alpha_mode, &cleaned)
        } else {
            false
        };

        let stored_value = if response.has_focus() {
            draft
        } else {
            Self::rgba_hex_string(*color, alpha_mode)
        };
        ui.ctx().data_mut(|data| data.insert_temp(id, stored_value));

        changed
    }

    fn rgba_hex_string(
        color: crate::model::RgbaColor,
        alpha_mode: egui::color_picker::Alpha,
    ) -> String {
        match alpha_mode {
            egui::color_picker::Alpha::Opaque => {
                format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b)
            }
            _ => {
                format!(
                    "{:02X}{:02X}{:02X}{:02X}",
                    color.r, color.g, color.b, color.a
                )
            }
        }
    }

    fn apply_rgba_hex_string(
        color: &mut crate::model::RgbaColor,
        alpha_mode: egui::color_picker::Alpha,
        cleaned: &str,
    ) -> bool {
        let before = (color.r, color.g, color.b, color.a);

        if cleaned.len() == 6 {
            if let Ok(r) = u8::from_str_radix(&cleaned[0..2], 16)
                && let Ok(g) = u8::from_str_radix(&cleaned[2..4], 16)
                && let Ok(b) = u8::from_str_radix(&cleaned[4..6], 16)
            {
                color.r = r;
                color.g = g;
                color.b = b;
                if matches!(alpha_mode, egui::color_picker::Alpha::Opaque) {
                    color.a = 255;
                }
            }
        } else if cleaned.len() == 8
            && !matches!(alpha_mode, egui::color_picker::Alpha::Opaque)
            && let Ok(r) = u8::from_str_radix(&cleaned[0..2], 16)
            && let Ok(g) = u8::from_str_radix(&cleaned[2..4], 16)
            && let Ok(b) = u8::from_str_radix(&cleaned[4..6], 16)
            && let Ok(a) = u8::from_str_radix(&cleaned[6..8], 16)
        {
            color.r = r;
            color.g = g;
            color.b = b;
            color.a = a;
        }

        before != (color.r, color.g, color.b, color.a)
    }

    fn premium_color_slider_2d(ui: &mut egui::Ui, h: f32, s: &mut f32, v: &mut f32) -> bool {
        let mut changed = false;
        let desired_size = egui::vec2(ui.available_width().max(200.0), 160.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

        if let Some(mpos) = response.interact_pointer_pos() {
            *s = ((mpos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            *v = (1.0 - (mpos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            changed = true;
        }

        if ui.is_rect_visible(rect) {
            let mut mesh = egui::epaint::Mesh::default();
            let steps = 12;
            for xi in 0..=steps {
                for yi in 0..=steps {
                    let st = xi as f32 / steps as f32;
                    let vt = 1.0 - (yi as f32 / steps as f32);
                    let color = egui::Color32::from(egui::epaint::Hsva::new(h, st, vt, 1.0));

                    let x = rect.left() + st * rect.width();
                    let y = rect.top() + (1.0 - vt) * rect.height();
                    mesh.colored_vertex(egui::pos2(x, y), color);

                    if xi < steps && yi < steps {
                        let row_len = steps + 1;
                        let tl = yi * row_len + xi;
                        mesh.add_triangle(tl, tl + 1, tl + row_len);
                        mesh.add_triangle(tl + 1, tl + row_len, tl + row_len + 1);
                    }
                }
            }
            ui.painter().add(egui::epaint::Shape::mesh(mesh));

            // Draw border
            ui.painter().rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                egui::StrokeKind::Inside,
            );

            // Draw selector dot
            let dot_x = rect.left() + (*s) * rect.width();
            let dot_y = rect.top() + (1.0 - *v) * rect.height();
            let picked_color = egui::Color32::from(egui::epaint::Hsva::new(h, *s, *v, 1.0));
            let contrast = if picked_color.intensity() < 128.0 {
                egui::Color32::WHITE
            } else {
                egui::Color32::BLACK
            };

            ui.painter().circle(
                egui::pos2(dot_x, dot_y),
                6.0,
                picked_color,
                egui::Stroke::new(2.0, contrast),
            );
        }

        changed
    }

    fn premium_hue_slider(ui: &mut egui::Ui, h: &mut f32) -> bool {
        let mut changed = false;
        let desired_size = egui::vec2(ui.available_width().max(200.0), 12.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

        if let Some(mpos) = response.interact_pointer_pos() {
            *h = ((mpos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            changed = true;
        }

        if ui.is_rect_visible(rect) {
            let mut mesh = egui::epaint::Mesh::default();
            let steps = 24;
            for i in 0..=steps {
                let ht = i as f32 / steps as f32;
                let color = egui::Color32::from(egui::epaint::Hsva::new(ht, 1.0, 1.0, 1.0));
                let x = rect.left() + ht * rect.width();
                mesh.colored_vertex(egui::pos2(x, rect.top()), color);
                mesh.colored_vertex(egui::pos2(x, rect.bottom()), color);

                if i < steps {
                    let idx = i * 2;
                    mesh.add_triangle(idx, idx + 1, idx + 2);
                    mesh.add_triangle(idx + 1, idx + 2, idx + 3);
                }
            }
            ui.painter().add(egui::epaint::Shape::mesh(mesh));

            // Draw border
            ui.painter().rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                egui::StrokeKind::Inside,
            );

            // Draw cursor indicator
            let cursor_x = rect.left() + (*h) * rect.width();
            ui.painter().line_segment(
                [
                    egui::pos2(cursor_x, rect.top() - 2.0),
                    egui::pos2(cursor_x, rect.bottom() + 2.0),
                ],
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
            ui.painter().line_segment(
                [
                    egui::pos2(cursor_x, rect.top() - 2.0),
                    egui::pos2(cursor_x, rect.bottom() + 2.0),
                ],
                egui::Stroke::new(1.0, egui::Color32::BLACK),
            );
        }

        changed
    }

    fn premium_alpha_slider(ui: &mut egui::Ui, a: &mut f32, h: f32, s: f32, v: f32) -> bool {
        let mut changed = false;
        let desired_size = egui::vec2(ui.available_width().max(200.0), 12.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

        if let Some(mpos) = response.interact_pointer_pos() {
            *a = ((mpos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            changed = true;
        }

        if ui.is_rect_visible(rect) {
            // Draw checkers background
            let cell_size = rect.height() / 2.0;
            let checkers_count = (rect.width() / cell_size).ceil() as i32;
            for i in 0..checkers_count {
                let x = rect.left() + i as f32 * cell_size;
                let color1 = egui::Color32::from_gray(60);
                let color2 = egui::Color32::from_gray(120);
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(x, rect.top()),
                        egui::vec2(cell_size, cell_size),
                    ),
                    0.0,
                    if i % 2 == 0 { color1 } else { color2 },
                );
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(x, rect.top() + cell_size),
                        egui::vec2(cell_size, cell_size),
                    ),
                    0.0,
                    if i % 2 == 0 { color2 } else { color1 },
                );
            }

            // Draw alpha gradient overlay
            let mut mesh = egui::epaint::Mesh::default();
            let steps = 10;
            for i in 0..=steps {
                let at = i as f32 / steps as f32;
                let color = egui::Color32::from(egui::epaint::Hsva::new(h, s, v, at));
                let x = rect.left() + at * rect.width();
                mesh.colored_vertex(egui::pos2(x, rect.top()), color);
                mesh.colored_vertex(egui::pos2(x, rect.bottom()), color);

                if i < steps {
                    let idx = i * 2;
                    mesh.add_triangle(idx, idx + 1, idx + 2);
                    mesh.add_triangle(idx + 1, idx + 2, idx + 3);
                }
            }
            ui.painter().add(egui::epaint::Shape::mesh(mesh));

            // Draw border
            ui.painter().rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                egui::StrokeKind::Inside,
            );

            // Draw cursor indicator
            let cursor_x = rect.left() + (*a) * rect.width();
            ui.painter().line_segment(
                [
                    egui::pos2(cursor_x, rect.top() - 2.0),
                    egui::pos2(cursor_x, rect.bottom() + 2.0),
                ],
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
            ui.painter().line_segment(
                [
                    egui::pos2(cursor_x, rect.top() - 2.0),
                    egui::pos2(cursor_x, rect.bottom() + 2.0),
                ],
                egui::Stroke::new(1.0, egui::Color32::BLACK),
            );
        }

        changed
    }
}
