use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum QuickKeyDisplayMode {
    #[default]
    Normal,
    Mascot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MascotStyle {
    #[default]
    #[serde(alias = "Custom")]
    Hachiware,
    ChiikawaClassic,
    Gugugaga,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AppPanel {
    #[default]
    Crosshair,
    WindowPresets,
    Pin,
    Mouse,
    #[serde(alias = "ImageSearch")]
    Vision,
    AudioSense,
    Zoom,
    Modes,
    Macros,
    #[serde(alias = "Custom")]
    Commands,
    #[serde(alias = "Bindings")]
    Sound,
    Media,
    #[serde(alias = "Toolbox", alias = "Settings")]
    Hud,
    Ocr,
    Geometry,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum UiLanguage {
    #[default]
    English,
    Icon,
    Vietnamese,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum VietnameseInputMode {
    #[default]
    Telex,
    Vni,
    Off,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum UiThemeMode {
    Dark,
    #[default]
    Light,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum FocusHighlightDecoration {
    #[default]
    Plain,
    Rainbow,
    FloralWood,
    CyberMech,
}
