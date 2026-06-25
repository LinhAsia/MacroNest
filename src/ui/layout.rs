use std::cmp::Ordering;

use eframe::egui::{self, pos2, vec2};

use crate::window_list;

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

    pub(crate) fn clean_invisible_chars(s: &str) -> String {
        s.chars()
            .filter(|&c| c != '\u{200B}' && c != '\u{200C}' && c != '\u{200D}' && c != '\u{FEFF}')
            .collect()
    }

    pub(crate) fn simplify_window_title(title: &str) -> String {
        let title = if let Some(s) = title.strip_suffix(" [Lowest]") {
            s
        } else if let Some(s) = title.strip_suffix(" [Highest]") {
            s
        } else if let Some(s) = title.strip_suffix(" [Leftmost]") {
            s
        } else if let Some(s) = title.strip_suffix(" [Rightmost]") {
            s
        } else {
            title
        };
        let clean = Self::clean_invisible_chars(title);
        let base = Self::selector_base_title(&clean);

        if base.contains(" - Antigravity IDE - ") || base.ends_with(" - Antigravity IDE") {
            return "Antigravity IDE".to_owned();
        }

        const BROWSER_SUFFIXES: &[&str] = &[
            " - Microsoft Edge",
            " - Google Chrome",
            " - Brave",
            " - Firefox",
            " - Opera GX",
            " - Opera",
            " - Vivaldi",
            " - Chromium",
            " - Tor Browser",
            " - Arc",
            " - Visual Studio Code",
            " - VS Code",
            " - Discord",
            " - Slack",
            " - Spotify",
        ];

        for suffix in BROWSER_SUFFIXES {
            if base.ends_with(suffix) {
                return suffix.trim_start_matches(" - ").to_owned();
            }
        }

        if let Some((_, last)) = base.rsplit_once(" - ") {
            let trimmed = last.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }

        base.to_owned()
    }

    pub(crate) fn quick_action_window_display(selector: &str, open_windows: &[String]) -> String {
        let simplified = Self::simplify_window_title(selector);
        let duplicate_count = open_windows
            .iter()
            .filter(|candidate| Self::simplify_window_title(candidate) == simplified)
            .count();
        if duplicate_count > 1 {
            Self::selector_base_title(selector).to_owned()
        } else {
            simplified
        }
    }

    pub(crate) fn capture_info_window_placement(
        ctx: &egui::Context,
        pointer: Option<egui::Pos2>,
    ) -> (egui::Pos2, egui::Vec2) {
        let (left, top, width, height) = window_list::virtual_screen_bounds();
        let ppp = ctx.pixels_per_point().max(0.5);
        let size = vec2(240.0, 288.0);
        let margin = 18.0;
        let viewport_rect = egui::Rect::from_min_max(
            pos2(left as f32 / ppp, top as f32 / ppp),
            pos2(
                (left as f32 + width as f32) / ppp,
                (top as f32 + height as f32) / ppp,
            ),
        );
        let candidates = [
            egui::Rect::from_min_size(
                viewport_rect.right_top() - vec2(size.x + margin, -margin),
                size,
            ),
            egui::Rect::from_min_size(viewport_rect.left_top() + vec2(margin, margin), size),
            egui::Rect::from_min_size(
                viewport_rect.right_bottom() - vec2(size.x + margin, size.y + margin),
                size,
            ),
            egui::Rect::from_min_size(
                viewport_rect.left_bottom() + vec2(margin, -(size.y + margin)),
                size,
            ),
        ];
        let pos = if let Some(pointer) = pointer {
            let pointer_safe_zone = egui::Rect::from_center_size(pointer, vec2(320.0, 320.0));
            candidates
                .into_iter()
                .find(|candidate| !candidate.intersects(pointer_safe_zone))
                .unwrap_or_else(|| {
                    candidates
                        .into_iter()
                        .max_by(|a, b| {
                            let a_dist = a.center().distance_sq(pointer);
                            let b_dist = b.center().distance_sq(pointer);
                            a_dist.partial_cmp(&b_dist).unwrap_or(Ordering::Equal)
                        })
                        .unwrap_or(candidates[0])
                })
                .min
        } else {
            candidates[0].min
        };
        (pos, size)
    }

    pub(crate) fn refresh_capture_info_window(&mut self, ctx: &egui::Context) {
        let pointer = Self::current_screen_cursor_pos().map(|(x, y)| {
            let ppp = ctx.pixels_per_point().max(0.5);
            egui::pos2(x as f32 / ppp, y as f32 / ppp)
        });
        let (pos, size) = Self::capture_info_window_placement(ctx, pointer);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }

    pub(crate) fn show_capture_info_window(&mut self, ctx: &egui::Context) {
        self.refresh_capture_info_window(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}
