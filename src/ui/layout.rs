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
        if let Some((idx, _)) = title.char_indices().nth(max_chars) {
            let mut result = String::with_capacity(idx + 3);
            result.push_str(&title[..idx]);
            result.push_str("...");
            result
        } else {
            title.to_owned()
        }
    }

    pub(crate) fn simplify_window_title(title: &str) -> std::borrow::Cow<'_, str> {
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
        if open_windows.len() > 1
            && open_windows
                .iter()
                .filter(|candidate| Self::simplify_window_title(&candidate.title) == simplified)
                .nth(1)
                .is_some()
        {
            Self::selector_base_title(selector).to_owned()
        } else {
            simplified.into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CrosshairApp;

    #[test]
    fn truncate_window_title_leaves_short_title_untouched() {
        assert_eq!(CrosshairApp::truncate_window_title("Notepad", 10), "Notepad");
    }

    #[test]
    fn truncate_window_title_truncates_long_ascii() {
        assert_eq!(
            CrosshairApp::truncate_window_title("VeryLongWindowTitleName", 8),
            "VeryLong..."
        );
    }

    #[test]
    fn truncate_window_title_handles_unicode_correctly() {
        assert_eq!(
            CrosshairApp::truncate_window_title("Trình quản lý tác vụ", 5),
            "Trình..."
        );
    }

    #[test]
    fn quick_action_window_display_handles_unique_and_duplicates() {
        use crate::window_list::WindowInfo;

        let w1 = WindowInfo {
            selector: "calc.exe::Calculator".to_string(),
            title: "Calculator".to_string(),
            process_id: 100,
            process_path: "calc.exe".to_string(),
        };
        // Single window: returns simplified
        assert_eq!(
            CrosshairApp::quick_action_window_display(&w1.selector, &[w1.clone()]),
            "Calculator"
        );

        let w2 = WindowInfo {
            selector: "calc2.exe::Calculator".to_string(),
            title: "Calculator".to_string(),
            process_id: 200,
            process_path: "calc2.exe".to_string(),
        };
        // Multiple windows with same simplified title: returns selector base title
        assert_eq!(
            CrosshairApp::quick_action_window_display(&w1.selector, &[w1.clone(), w2]),
            "calc.exe::Calculator"
        );
    }
}
