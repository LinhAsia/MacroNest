use eframe::egui::{self, Button, Color32, RichText};

use crate::model::{AppPanel, UiLanguage, UiThemeMode, VietnameseInputMode};

use super::CrosshairApp;

impl CrosshairApp {
    pub(crate) fn app_brand_title(&self) -> &'static str {
        "MacroNest"
    }

    pub(crate) fn versions_are_equal(v1: &str, v2: &str) -> bool {
        let mut parts1: Vec<u32> = v1
            .split('.')
            .map(|s| s.parse::<u32>().unwrap_or(0))
            .collect();
        let mut parts2: Vec<u32> = v2
            .split('.')
            .map(|s| s.parse::<u32>().unwrap_or(0))
            .collect();
        while parts1.last() == Some(&0) {
            parts1.pop();
        }
        while parts2.last() == Some(&0) {
            parts2.pop();
        }
        parts1 == parts2
    }

    pub(crate) fn app_version_label(&self) -> &'static str {
        option_env!("MACRONEST_BUILD_TAG").unwrap_or(env!("CARGO_PKG_VERSION"))
    }

    pub(crate) fn panel_label(&self, panel: AppPanel) -> &'static str {
        let english = match panel {
            AppPanel::Crosshair => "Crosshair",
            AppPanel::WindowPresets => "Window Control",
            AppPanel::Pin | AppPanel::Zoom => "Pin",
            AppPanel::Mouse => "Mouse",
            AppPanel::Vision => "Vision",
            AppPanel::AudioSense => "AudioSense",
            AppPanel::Macros | AppPanel::Modes => "Macro",
            AppPanel::Commands => "Commands",
            AppPanel::Sound => "Media",
            AppPanel::Media => "Editor",
            AppPanel::Hud => "HUD",
            AppPanel::Ocr => "OCR",
            AppPanel::Geometry => "Geometry",
        };
        if panel == AppPanel::Ocr {
            Self::tr_lang(self.state.ui_language, "OCR", "OCR")
        } else {
            Self::tr_lang(self.state.ui_language, english, english)
        }
    }

    pub(crate) fn language_button_text(&self) -> RichText {
        match self.state.ui_language {
            UiLanguage::English => RichText::new("EN").strong(),
            UiLanguage::Vietnamese => RichText::new("VI").strong(),
            UiLanguage::Icon => RichText::new("EN").strong(),
        }
    }

    pub(crate) fn theme_button_text(&self) -> RichText {
        match self.state.ui_theme {
            UiThemeMode::Dark => Self::material_icon_text(0xe51c, 18.0),
            UiThemeMode::Light => Self::material_icon_text(0xe518, 18.0),
        }
    }

    pub(crate) fn startup_loading_text(&self) -> &'static str {
        match self.state.ui_language {
            UiLanguage::English => "loading macro tools, overlays, and UI",
            UiLanguage::Vietnamese => self.tr(
                "loading macro tools, overlays, and UI",
                "loading macro tools, overlays, and UI",
            ),
            UiLanguage::Icon => "loading macro tools, overlays, and UI",
        }
    }

    pub(crate) fn titlebar_language_tooltip(&self) -> &'static str {
        self.tr("Switch language", "Switch language")
    }

    pub(crate) fn vietnamese_input_button_text(&self) -> RichText {
        if self.state.vietnamese_input_enabled {
            RichText::new("V")
                .strong()
                .color(Color32::from_rgb(235, 76, 80))
        } else {
            RichText::new("E")
                .strong()
                .color(Color32::from_rgb(76, 135, 235))
        }
    }

    pub(crate) fn titlebar_vietnamese_input_tooltip(&self) -> &'static str {
        if !self.state.vietnamese_input_enabled {
            self.tr("Vietnamese input: off", "Vietnamese input: off")
        } else {
            match self.state.vietnamese_input_mode {
                VietnameseInputMode::Telex => {
                    self.tr("Vietnamese input: Telex", "Vietnamese input: Telex")
                }
                VietnameseInputMode::Vni => {
                    self.tr("Vietnamese input: VNI", "Vietnamese input: VNI")
                }
                VietnameseInputMode::Off => {
                    self.tr("Vietnamese input: Telex", "Vietnamese input: Telex")
                }
            }
        }
    }

    pub(crate) fn titlebar_theme_tooltip(&self) -> &'static str {
        self.tr("Toggle dark / light theme", "Toggle dark / light theme")
    }

    pub(crate) fn titlebar_minimize_tooltip(&self) -> &'static str {
        self.tr("Minimize", "Minimize")
    }

    pub(crate) fn titlebar_maximize_tooltip(&self, maximized: bool) -> &'static str {
        if maximized {
            self.tr("Restore", "Restore")
        } else {
            self.tr("Maximize", "Maximize")
        }
    }

    pub(crate) fn titlebar_button(
        &self,
        text: RichText,
        active: bool,
        danger: bool,
    ) -> Button<'static> {
        let (fill, stroke) = match (self.state.ui_theme, active, danger) {
            (_, _, true) => (
                Color32::from_rgba_premultiplied(160, 48, 64, if active { 138 } else { 72 }),
                Color32::from_rgb(222, 106, 126),
            ),
            (UiThemeMode::Dark, true, false) => (
                Color32::from_rgba_premultiplied(74, 146, 118, 166),
                Color32::from_rgb(126, 224, 182),
            ),
            (UiThemeMode::Dark, false, false) => (
                Color32::from_rgba_premultiplied(54, 67, 88, 88),
                Color32::from_rgb(74, 92, 118),
            ),
            (UiThemeMode::Light, true, false) => (
                Color32::from_rgba_premultiplied(72, 156, 116, 120),
                Color32::from_rgb(34, 122, 88),
            ),
            (UiThemeMode::Light, false, false) => (
                Color32::from_rgba_premultiplied(220, 228, 238, 165),
                Color32::from_rgb(188, 198, 214),
            ),
        };
        Button::new(text)
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(8.0)
    }

    pub(crate) fn top_tab_button(
        &self,
        text: RichText,
        selected: bool,
        emphasized: bool,
    ) -> Button<'static> {
        let (fill, stroke) = match (self.state.ui_theme, selected, emphasized) {
            (UiThemeMode::Dark, true, _) => (
                Color32::from_rgba_premultiplied(58, 120, 96, 164),
                Color32::from_rgb(126, 224, 182),
            ),
            (UiThemeMode::Dark, false, true) => (
                Color32::from_rgba_premultiplied(42, 58, 46, 118),
                Color32::from_rgb(92, 180, 148),
            ),
            (UiThemeMode::Dark, false, false) => (
                Color32::from_rgba_premultiplied(34, 42, 56, 72),
                Color32::from_rgb(56, 68, 88),
            ),
            (UiThemeMode::Light, true, _) => (
                Color32::from_rgba_premultiplied(90, 180, 132, 98),
                Color32::from_rgb(34, 122, 88),
            ),
            (UiThemeMode::Light, false, true) => (
                Color32::from_rgba_premultiplied(214, 238, 226, 208),
                Color32::from_rgb(58, 146, 110),
            ),
            (UiThemeMode::Light, false, false) => (
                Color32::from_rgba_premultiplied(230, 236, 242, 165),
                Color32::from_rgb(202, 212, 224),
            ),
        };
        Button::new(text)
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(10.0)
    }
}
