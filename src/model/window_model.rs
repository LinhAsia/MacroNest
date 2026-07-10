use serde::{Deserialize, Serialize};

use super::overlay_model::RgbaColor;
use super::{
    MacroStep, default_binary_target_color_option, default_binary_threshold, default_true,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HotkeyBinding {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: String,
    #[serde(default)]
    pub combo_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WindowPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub width: i32,
    pub height: i32,
    pub anchor: WindowAnchor,
    pub x: i32,
    pub y: i32,
    pub hotkey: Option<HotkeyBinding>,
    #[serde(default)]
    pub trigger_keys: String,
    #[serde(default, alias = "stretch_enabled")]
    pub remove_title_bar: bool,
    pub animate_enabled: bool,
    pub animate_duration_ms: u64,
    pub animate_hotkey: Option<HotkeyBinding>,
    pub restore_titlebar_enabled: bool,
    pub titlebar_hotkey: Option<HotkeyBinding>,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
    #[serde(default = "default_true")]
    pub match_duplicate_window_titles: bool,
    #[serde(default)]
    pub preview_enabled: bool,
}

impl WindowPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Window Resize {id}"),
            enabled: true,
            collapsed: true,
            width: 1920,
            height: 1080,
            anchor: WindowAnchor::Manual,
            x: 0,
            y: 0,
            hotkey: None,
            trigger_keys: String::new(),
            remove_title_bar: false,
            animate_enabled: false,
            animate_duration_ms: 260,
            animate_hotkey: None,
            restore_titlebar_enabled: false,
            titlebar_hotkey: None,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            match_duplicate_window_titles: true,
            preview_enabled: false,
        }
    }
}

impl Default for WindowPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WindowFocusPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
    #[serde(default = "default_true")]
    pub match_duplicate_window_titles: bool,
    pub hotkey: Option<HotkeyBinding>,
    #[serde(default)]
    pub trigger_keys: String,
}

impl WindowFocusPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Focus {id}"),
            enabled: true,
            collapsed: true,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            match_duplicate_window_titles: true,
            hotkey: None,
            trigger_keys: String::new(),
        }
    }
}

impl Default for WindowFocusPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WindowLayoutCell {
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
    pub match_duplicate_window_titles: bool,
}

impl Default for WindowLayoutCell {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            match_duplicate_window_titles: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowLayout {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub rows: usize,
    pub cols: usize,
    pub row_ratios: Vec<f32>,
    pub col_ratios: Vec<f32>,
    pub cells: Vec<WindowLayoutCell>,
    pub focus_on_apply: bool,
    pub hotkey: Option<HotkeyBinding>,
    pub trigger_keys: String,
    pub block_taskbar: bool,
    pub remove_title_bar: bool,
    pub animate_enabled: bool,
    pub animate_duration_ms: u64,
}

impl WindowLayout {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Layout {id}"),
            enabled: true,
            collapsed: true,
            rows: 2,
            cols: 2,
            row_ratios: vec![0.5, 0.5],
            col_ratios: vec![0.5, 0.5],
            cells: vec![
                WindowLayoutCell {
                    row: 0,
                    col: 0,
                    ..Default::default()
                },
                WindowLayoutCell {
                    row: 0,
                    col: 1,
                    ..Default::default()
                },
                WindowLayoutCell {
                    row: 1,
                    col: 0,
                    ..Default::default()
                },
                WindowLayoutCell {
                    row: 1,
                    col: 1,
                    ..Default::default()
                },
            ],
            focus_on_apply: true,
            hotkey: None,
            trigger_keys: String::new(),
            block_taskbar: false,
            remove_title_bar: false,
            animate_enabled: false,
            animate_duration_ms: 260,
        }
    }
}

impl Default for WindowLayout {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum WindowAnchor {
    #[default]
    Manual,
    Center,
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CaptureRequest {
    WindowPresetHotkey(u32),
    WindowFocusPresetHotkey(u32),
    WindowLayoutHotkey(u32),
    WindowPresetAnimateHotkey(u32),
    WindowPresetTitlebarHotkey(u32),
    WindowExpandHotkey(WindowExpandDirection),
    PinPresetHotkey(u32),
    MousePathRecordHotkey(u32),
    MouseSensitivityPresetHotkey(u32),
    ZoomPresetHotkey(u32),
    VisionPresetHotkey(u32),
    MacrosMasterHotkey,
    MacroPresetHotkey(u32, u32),
    MacroPresetRecordHotkey(u32, u32),
    MacroPresetReleaseWaitKey(u32, u32),
    MacroPresetHoldStopInput(u32, u32),
    MacroPresetPressStopInput(u32, u32),
    CommandPresetHotkey(u32),
    QuickScreenDrawHotkey,
    MacroStepInput {
        group_id: u32,
        preset_id: u32,
        step_index: usize,
        extra_cond_index: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CapturedInput {
    Binding(HotkeyBinding),
    Step(MacroStep),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MousePathEventKind {
    #[default]
    Move,
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
    MiddleDown,
    MiddleUp,
    WheelUp,
    WheelDown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MousePathEvent {
    pub kind: MousePathEventKind,
    pub x: i32,
    pub y: i32,
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct MousePathPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub record_hotkey: Option<HotkeyBinding>,
    #[serde(default)]
    pub replay_relative_motion: bool,
    pub events: Vec<MousePathEvent>,
}

impl MousePathPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Mouse Path {id}"),
            enabled: true,
            collapsed: true,
            record_hotkey: None,
            replay_relative_motion: false,
            events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MouseSensitivityPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
    #[serde(default = "default_true")]
    pub match_duplicate_window_titles: bool,
    pub speed: u32,
    #[serde(default)]
    pub restore_on_exit: bool,
    #[serde(default = "default_mouse_sensitivity_restore_speed")]
    pub restore_speed: u32,
    pub hotkey: Option<HotkeyBinding>,
    #[serde(default)]
    pub trigger_keys: String,
}

fn default_mouse_sensitivity_restore_speed() -> u32 {
    6
}

impl MouseSensitivityPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Mouse Sensitivity {id}"),
            enabled: true,
            collapsed: true,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            match_duplicate_window_titles: true,
            speed: 15,
            restore_on_exit: false,
            restore_speed: default_mouse_sensitivity_restore_speed(),
            hotkey: None,
            trigger_keys: String::new(),
        }
    }
}

impl Default for MouseSensitivityPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WindowExpandDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WindowExpandControls {
    pub enabled: bool,
    pub amount_px: i32,
    pub up: Option<HotkeyBinding>,
    pub down: Option<HotkeyBinding>,
    pub left: Option<HotkeyBinding>,
    pub right: Option<HotkeyBinding>,
}

impl Default for WindowExpandControls {
    fn default() -> Self {
        Self {
            enabled: false,
            amount_px: 48,
            up: Some(HotkeyBinding {
                ctrl: false,
                alt: false,
                shift: false,
                win: false,
                key: "Up".to_owned(),
                combo_keys: Vec::new(),
            }),
            down: Some(HotkeyBinding {
                ctrl: false,
                alt: false,
                shift: false,
                win: false,
                key: "Down".to_owned(),
                combo_keys: Vec::new(),
            }),
            left: Some(HotkeyBinding {
                ctrl: false,
                alt: false,
                shift: false,
                win: false,
                key: "Left".to_owned(),
                combo_keys: Vec::new(),
            }),
            right: Some(HotkeyBinding {
                ctrl: false,
                alt: false,
                shift: false,
                win: false,
                key: "Right".to_owned(),
                combo_keys: Vec::new(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ZoomPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub preview_enabled: bool,
    pub source_x: i32,
    pub source_y: i32,
    pub source_width: i32,
    pub source_height: i32,
    pub target_x: i32,
    pub target_y: i32,
    pub target_width: i32,
    pub target_height: i32,
    pub fps: u32,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
    pub hotkey: Option<HotkeyBinding>,
}

impl ZoomPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Zoom {id}"),
            enabled: true,
            collapsed: true,
            preview_enabled: false,
            source_x: 0,
            source_y: 0,
            source_width: 320,
            source_height: 180,
            target_x: 100,
            target_y: 100,
            target_width: 640,
            target_height: 360,
            fps: 30,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            hotkey: None,
        }
    }
}

impl Default for ZoomPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PinBinaryMode {
    #[default]
    Grayscale,
    ColorSimilarity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PinOverlayStyle {
    #[default]
    Rectangle,
    Circle,
    HorizontalBar,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PinPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub preview_enabled: bool,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
    #[serde(default = "default_true")]
    pub match_duplicate_window_titles: bool,
    pub hotkey: Option<HotkeyBinding>,
    #[serde(default)]
    pub trigger_keys: String,
    #[serde(default = "default_true")]
    pub use_custom_bounds: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub overlay_style: PinOverlayStyle,
    #[serde(default = "default_true")]
    pub use_source_crop: bool,
    pub source_crop_initialized: bool,
    pub source_crop_fit_version: u8,
    pub source_x: i32,
    pub source_y: i32,
    pub source_width: i32,
    pub source_height: i32,
    #[serde(default)]
    pub binary_filter: bool,
    #[serde(default)]
    pub binary_transparent_black: bool,
    #[serde(default)]
    pub binary_transparent_white: bool,
    #[serde(default = "default_binary_threshold")]
    pub binary_threshold: u8,
    #[serde(default)]
    pub binary_mode: PinBinaryMode,
    #[serde(default = "default_binary_target_color_option")]
    pub binary_target_color: Option<RgbaColor>,
    #[serde(default)]
    pub binary_target_colors: Vec<RgbaColor>,
}

impl PinPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Pin {id}"),
            enabled: true,
            collapsed: true,
            preview_enabled: false,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            match_duplicate_window_titles: true,
            hotkey: None,
            trigger_keys: String::new(),
            use_custom_bounds: true,
            x: 100,
            y: 100,
            width: 640,
            height: 360,
            overlay_style: PinOverlayStyle::Rectangle,
            use_source_crop: true,
            source_crop_initialized: false,
            source_crop_fit_version: 0,
            source_x: 0,
            source_y: 0,
            source_width: 320,
            source_height: 180,
            binary_filter: false,
            binary_transparent_black: false,
            binary_transparent_white: false,
            binary_threshold: 128,
            binary_mode: PinBinaryMode::Grayscale,
            binary_target_color: default_binary_target_color_option(),
            binary_target_colors: Vec::new(),
        }
    }

    pub fn binary_target_colors(&self) -> Vec<RgbaColor> {
        if !self.binary_target_colors.is_empty() {
            return self.binary_target_colors.clone();
        }
        self.binary_target_color.into_iter().collect()
    }

    pub fn add_binary_target_color(&mut self, color: RgbaColor) {
        if self.binary_target_colors.is_empty()
            && let Some(existing) = self.binary_target_color
        {
            self.binary_target_colors.push(existing);
        }
        self.binary_target_colors.push(color);
        self.binary_target_color = self.binary_target_colors.first().copied();
    }

    pub fn remove_binary_target_color_at(&mut self, index: usize) -> bool {
        if self.binary_target_colors.is_empty() {
            if self.binary_target_color.is_some() && index == 0 {
                self.binary_target_color = None;
                return true;
            }
            return false;
        }

        if index >= self.binary_target_colors.len() {
            return false;
        }

        self.binary_target_colors = self
            .binary_target_colors
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(i, color)| (i != index).then_some(color))
            .collect();
        self.binary_target_color = self.binary_target_colors.first().copied();
        true
    }
}

impl Default for PinPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::MouseSensitivityPreset;

    #[test]
    fn mouse_sensitivity_default_keeps_restore_defaults() {
        let preset = MouseSensitivityPreset::default();
        assert_eq!(preset.restore_speed, 6);
        assert!(preset.match_duplicate_window_titles);
        assert_eq!(preset.speed, 15);
    }
}
