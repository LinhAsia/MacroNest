use eframe::egui::{
    self, Button, Color32, Frame, Margin, Order, RichText, Sense, Shadow, Stroke, vec2,
};

use crate::model::{AppPanel, UiThemeMode};

use super::CrosshairApp;

impl CrosshairApp {
    pub(crate) fn render_panel_loading_shell(&self, ui: &mut egui::Ui, panel: AppPanel) {
        let title = self.panel_label(panel);
        let subtitle = self.tr("Preparing this panel...", "Preparing this panel...");
        let detail = self.tr(
            "The window is ready. Content will finish loading in the next moments.",
            "The window is ready. Content will finish loading in the next moments.",
        );
        ui.with_layout(
            egui::Layout::top_down_justified(egui::Align::Center),
            |ui| {
                ui.add_space((ui.available_height() * 0.18).max(24.0));
                ui.spinner();
                ui.add_space(14.0);
                ui.label(RichText::new(title).heading().strong());
                ui.add_space(6.0);
                ui.label(RichText::new(subtitle).strong());
                ui.add_space(4.0);
                ui.label(RichText::new(detail).small().weak());
            },
        );
    }

    pub(crate) fn render_blocking_confirmation_modal(
        &self,
        ctx: &egui::Context,
        modal_key: impl std::hash::Hash,
        title: &str,
        message: &str,
        confirm_label: &str,
        cancel_label: &str,
    ) -> Option<bool> {
        self.render_modal_backdrop(ctx, true);
        let (panel_size, panel_pos) =
            Self::centered_modal_placement(ctx, vec2(380.0, 160.0), vec2(320.0, 140.0));
        let mut outcome = None;
        egui::Area::new(egui::Id::new((modal_key, "blocking-confirmation-modal")))
            .order(Order::Foreground)
            .fixed_pos(panel_pos)
            .interactable(true)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(if self.state.ui_theme == UiThemeMode::Dark {
                        Color32::from_rgba_premultiplied(24, 26, 32, 250)
                    } else {
                        Color32::from_rgba_premultiplied(248, 248, 250, 250)
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
                    .inner_margin(Margin::same(20))
                    .show(ui, |ui| {
                        ui.set_min_size(panel_size);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).strong());
                            ui.add_space(10.0);
                            ui.label(message);
                            ui.add_space(18.0);
                            ui.horizontal(|ui| {
                                if ui
                                    .add_sized(
                                        [120.0, 26.0],
                                        Button::new(confirm_label)
                                            .fill(Color32::from_rgb(176, 72, 72)),
                                    )
                                    .clicked()
                                {
                                    outcome = Some(true);
                                }
                                if ui
                                    .add_sized([100.0, 26.0], Button::new(cancel_label))
                                    .clicked()
                                {
                                    outcome = Some(false);
                                }
                            });
                        });
                    });
            });
        if outcome.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            outcome = Some(false);
        }
        outcome
    }

    pub(crate) fn render_modal_backdrop(&self, ctx: &egui::Context, open: bool) {
        if !open {
            return;
        }

        let rect = ctx.content_rect();
        egui::Area::new(egui::Id::new("settings-modal-backdrop"))
            .order(Order::Middle)
            .fixed_pos(rect.min)
            .interactable(true)
            .show(ctx, |ui| {
                let (backdrop_rect, _) =
                    ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
                ui.painter().rect_filled(
                    backdrop_rect,
                    egui::CornerRadius::ZERO,
                    Color32::from_rgba_premultiplied(18, 18, 24, 150),
                );
            });
    }
}
