use std::sync::Arc;

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily};

use crate::model::{AppState, UiThemeMode};

pub(crate) const MATERIAL_ICONS_FONT: &str = "material_icons";
const UI_SANS_FONT: &str = "ui_sans";
const UI_SANS_SEMIBOLD_FONT: &str = "ui_sans_semibold";

fn text_has_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x2E80..=0x2FDF
                | 0x3040..=0x30FF
                | 0x31F0..=0x31FF
                | 0x3400..=0x4DBF
                | 0x4E00..=0x9FFF
                | 0xAC00..=0xD7AF
                | 0xF900..=0xFAFF
                | 0xFF66..=0xFF9F
        )
    })
}

pub fn app_state_needs_cjk_fallback(state: &AppState) -> bool {
    serde_json::to_string(state)
        .map(|json| text_has_cjk(&json))
        .unwrap_or(false)
}

#[cfg(windows)]
fn add_windows_cjk_fallback_fonts(fonts: &mut FontDefinitions) {
    for (font_key, path) in [
        ("cjk_yahei", "C:\\Windows\\Fonts\\msyh.ttc"),
        ("cjk_yugothic", "C:\\Windows\\Fonts\\YuGothM.ttc"),
        ("cjk_meiryo", "C:\\Windows\\Fonts\\meiryo.ttc"),
        ("cjk_msgothic", "C:\\Windows\\Fonts\\msgothic.ttc"),
        ("cjk_malgun", "C:\\Windows\\Fonts\\malgun.ttf"),
        ("cjk_simhei", "C:\\Windows\\Fonts\\simhei.ttf"),
    ] {
        if let Ok(font_bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                font_key.to_owned(),
                Arc::new(FontData::from_owned(font_bytes)),
            );
        }
    }
}

pub fn configure_fonts(ctx: &egui::Context, load_cjk_fallback: bool) {
    let mut fonts = FontDefinitions {
        font_data: Default::default(),
        families: Default::default(),
    };
    fonts.font_data.insert(
        UI_SANS_FONT.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/SegoeUI.ttf"
        ))),
    );
    fonts.font_data.insert(
        UI_SANS_SEMIBOLD_FONT.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/SegoeUI-Semibold.ttf"
        ))),
    );
    fonts.font_data.insert(
        MATERIAL_ICONS_FONT.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/MaterialIcons-Regular.ttf"
        ))),
    );
    #[cfg(windows)]
    if load_cjk_fallback {
        add_windows_cjk_fallback_fonts(&mut fonts);
    }
    let ui_family = FontFamily::Name(UI_SANS_FONT.into());
    fonts
        .families
        .entry(ui_family.clone())
        .or_default()
        .insert(0, UI_SANS_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, UI_SANS_SEMIBOLD_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push(UI_SANS_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push(UI_SANS_FONT.to_owned());
    let material_family = FontFamily::Name(MATERIAL_ICONS_FONT.into());
    fonts
        .families
        .entry(material_family.clone())
        .or_default()
        .insert(0, MATERIAL_ICONS_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push(MATERIAL_ICONS_FONT.to_owned());
    #[cfg(windows)]
    {
        let cjk_font_names = [
            "cjk_yahei",
            "cjk_yugothic",
            "cjk_meiryo",
            "cjk_msgothic",
            "cjk_malgun",
            "cjk_simhei",
        ];
        let loaded_cjk_fonts: Vec<String> = cjk_font_names
            .iter()
            .filter(|name| fonts.font_data.contains_key(**name))
            .map(|name| (*name).to_owned())
            .collect();
        if !loaded_cjk_fonts.is_empty() {
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .extend(loaded_cjk_fonts.iter().cloned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .extend(loaded_cjk_fonts);
        }
    }
    ctx.set_fonts(fonts);
    ctx.style_mut(|style| {
        style.interaction.show_tooltips_only_when_still = true;
        style.interaction.tooltip_delay = 0.08;
        style.interaction.tooltip_grace_time = 0.10;

        use egui::{FontId, TextStyle};
        let text_styles = &mut style.text_styles;
        text_styles.insert(
            TextStyle::Small,
            FontId::new(13.5, FontFamily::Proportional),
        );
        text_styles.insert(TextStyle::Body, FontId::new(16.0, FontFamily::Proportional));
        text_styles.insert(
            TextStyle::Button,
            FontId::new(15.5, FontFamily::Proportional),
        );
        text_styles.insert(
            TextStyle::Heading,
            FontId::new(21.0, FontFamily::Proportional),
        );
        text_styles.insert(
            TextStyle::Monospace,
            FontId::new(15.5, FontFamily::Monospace),
        );
    });
}

fn visuals_for_theme(theme: UiThemeMode) -> egui::Visuals {
    match theme {
        UiThemeMode::Dark => {
            let mut visuals = egui::Visuals::dark();
            visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(220, 228, 238);
            visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(228, 234, 242);
            visuals.widgets.hovered.fg_stroke.color = Color32::from_rgb(240, 246, 252);
            visuals.widgets.active.fg_stroke.color = Color32::from_rgb(248, 250, 252);
            visuals.widgets.open.fg_stroke.color = Color32::from_rgb(240, 246, 252);
            visuals
        }
        UiThemeMode::Light => {
            let mut visuals = egui::Visuals::light();
            visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(32, 40, 54);
            visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(28, 36, 48);
            visuals.widgets.hovered.fg_stroke.color = Color32::from_rgb(18, 26, 40);
            visuals.widgets.active.fg_stroke.color = Color32::from_rgb(16, 24, 38);
            visuals.widgets.open.fg_stroke.color = Color32::from_rgb(18, 26, 40);
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(238, 243, 248);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(248, 251, 254);
            visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(238, 244, 250);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(232, 241, 248);
            visuals.widgets.active.bg_fill = Color32::from_rgb(222, 235, 245);
            visuals.widgets.open.bg_fill = Color32::from_rgb(248, 251, 254);
            let control_stroke = egui::Stroke::new(1.0, Color32::from_rgb(178, 191, 207));
            visuals.widgets.noninteractive.bg_stroke = control_stroke;
            visuals.widgets.inactive.bg_stroke = control_stroke;
            visuals.widgets.hovered.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_rgb(132, 153, 176));
            visuals.widgets.active.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_rgb(96, 128, 160));
            visuals.widgets.open.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_rgb(132, 153, 176));
            visuals.extreme_bg_color = Color32::WHITE;
            visuals.faint_bg_color = Color32::from_rgb(229, 246, 236);
            visuals.weak_text_color = Some(Color32::from_rgb(90, 101, 116));
            visuals.hyperlink_color = Color32::from_rgb(26, 92, 164);
            visuals.panel_fill = Color32::from_rgb(248, 248, 248);
            visuals.window_fill = Color32::from_rgb(248, 248, 248);
            visuals
        }
    }
}

pub(crate) fn configure_theme(ctx: &egui::Context, theme: UiThemeMode) {
    ctx.set_visuals(visuals_for_theme(theme));
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(match theme {
        UiThemeMode::Dark => egui::SystemTheme::Dark,
        UiThemeMode::Light => egui::SystemTheme::Light,
    }));
}
