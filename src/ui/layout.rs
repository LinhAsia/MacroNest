use eframe::egui::{self, vec2};

use crate::window_list::{self, WindowInfo};

use super::CrosshairApp;

impl CrosshairApp {
    pub(crate) fn modal_safe_rect(ctx: &egui::Context) -> egui::Rect {
        ctx.content_rect().shrink(18.0)
    }

    pub(crate) fn centered_modal_placement(
        ctx: &egui::Context,
        desired_size: egui::Vec2,
        min_size: egui::Vec2,
    ) -> (egui::Vec2, egui::Pos2) {
        let safe_rect = Self::modal_safe_rect(ctx);
        let panel_size = vec2(
            desired_size
                .x
                .min(safe_rect.width())
                .max(min_size.x.min(safe_rect.width())),
            desired_size
                .y
                .min(safe_rect.height())
                .max(min_size.y.min(safe_rect.height())),
        );
        let center = safe_rect.center();
        let panel_pos = egui::Pos2::new(
            (center.x - panel_size.x * 0.5)
                .round()
                .clamp(safe_rect.left(), safe_rect.right() - panel_size.x),
            (center.y - panel_size.y * 0.5)
                .round()
                .clamp(safe_rect.top(), safe_rect.bottom() - panel_size.y),
        );
        (panel_size, panel_pos)
    }

    pub(crate) fn truncate_window_title(title: &str, max_chars: usize) -> String {
        let chars: Vec<char> = title.chars().collect();
        if chars.len() > max_chars {
            let mut truncated: String = chars[..max_chars].iter().collect();
            truncated.push_str("...");
            truncated
        } else {
            title.to_owned()
        }
    }

    pub(crate) fn simplify_window_title(title: &str) -> String {
        window_list::simplify_window_title(title)
    }

    pub(crate) fn quick_action_window_display(
        selector: &str,
        open_windows: &[WindowInfo],
    ) -> String {
        let simplified = open_windows
            .iter()
            .find(|candidate| candidate.selector == selector)
            .map(|candidate| Self::simplify_window_title(&candidate.title))
            .unwrap_or_else(|| Self::simplify_window_title(selector));
        let duplicate_count = open_windows
            .iter()
            .filter(|candidate| Self::simplify_window_title(&candidate.title) == simplified)
            .count();
        if duplicate_count > 1 {
            Self::selector_base_title(selector).to_owned()
        } else {
            simplified
        }
    }
}
