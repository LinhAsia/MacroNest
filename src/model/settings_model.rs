use serde::{Deserialize, Serialize};

use crate::ocr::OcrResult;

use super::{
    AudioSensePreset, AudioSettings, CrosshairStyle, GeometryPreset, HotkeyBinding, MacroFolder,
    MacroGroup, MacroPreset, MasterPreset, MousePathPreset, MouseSensitivityPreset, OcrPreset,
    PinPreset, ProfileRecord, RgbaColor, VisionPreset, VisionSettings, WindowExpandControls,
    WindowFocusPreset, WindowLayout, WindowPreset, ZoomPreset, default_focus_highlight_color,
    default_hud_border_color, default_hud_border_thickness, default_key_sound_volume,
    default_macro_keyboard_key_press_delay_ms, default_macro_mouse_click_delay_ms,
    default_ocr_language_code, default_protractor_center_x, default_protractor_center_y,
    default_protractor_needle1_angle, default_protractor_needle2_angle, default_protractor_scale,
    default_protractor_thickness, default_quick_key_display_size, default_quick_key_display_x,
    default_quick_key_display_y, default_screen_draw_brush_size, default_screen_draw_color,
    default_screen_draw_freeze, default_screen_draw_smoothing_amount,
    default_timer_progress_border_color, default_timer_progress_border_thickness,
    default_timer_progress_smoothness_fps, default_true,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum QuickKeyDisplayMode {
    Normal,
    #[default]
    Mascot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum QuickScreenDrawTool {
    #[default]
    Brush,
    Line,
    Arrow,
    Rectangle,
    Ellipse,
    Circle,
    Polygon,
    Text,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum MascotStyle {
    #[default]
    #[serde(alias = "Custom")]
    Hachiware,
    ChiikawaClassic,
    #[serde(alias = "Gugugaga")]
    Chiikawa,
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
    Timer,
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
    #[serde(alias = "CyberMech")]
    Plain,
    Rainbow,
    FloralWood,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HudPreset {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub preview_enabled: bool,
    pub text: String,
    pub font_size: f32,
    pub background_opacity: f32,
    pub rounded_background: bool,
    #[serde(default)]
    pub border_enabled: bool,
    #[serde(default = "default_hud_border_color")]
    pub border_color: RgbaColor,
    #[serde(default = "default_hud_border_thickness")]
    pub border_thickness: f32,
    pub text_color: RgbaColor,
    pub background_color: RgbaColor,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl HudPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("HUD {id}"),
            collapsed: true,
            preview_enabled: false,
            text: "HUD text".to_owned(),
            font_size: 28.0,
            background_opacity: 0.72,
            rounded_background: true,
            border_enabled: false,
            border_color: default_hud_border_color(),
            border_thickness: default_hud_border_thickness(),
            text_color: RgbaColor {
                r: 244,
                g: 244,
                b: 244,
                a: 255,
            },
            background_color: RgbaColor {
                r: 34,
                g: 34,
                b: 34,
                a: 255,
            },
            x: 660,
            y: 36,
            width: 600,
            height: 80,
        }
    }
}

impl Default for HudPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TimerPreset {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub preview_enabled: bool,
    #[serde(default = "default_true")]
    pub show_overlay: bool,
    pub show_minutes: bool,
    pub show_seconds: bool,
    pub show_ms: bool,
    pub text_color: RgbaColor,
    pub background_color: RgbaColor,
    pub background_opacity: f32,
    pub rounded_background: bool,
    pub font_size: f32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_countdown: bool,
    pub duration_secs: u32,
    pub show_text: bool,
    pub show_progress_bar: bool,
    pub progress_color: RgbaColor,
    pub progress_height: u32,
    #[serde(default = "default_true")]
    pub progress_border_enabled: bool,
    #[serde(default = "default_timer_progress_border_color")]
    pub progress_border_color: RgbaColor,
    #[serde(default = "default_timer_progress_border_thickness")]
    pub progress_border_thickness: f32,
    #[serde(default = "default_timer_progress_smoothness_fps")]
    pub progress_smoothness_fps: u32,
}

impl TimerPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Timer {id}"),
            collapsed: true,
            preview_enabled: false,
            show_overlay: false,
            show_minutes: true,
            show_seconds: true,
            show_ms: true,
            text_color: RgbaColor {
                r: 244,
                g: 244,
                b: 244,
                a: 255,
            },
            background_color: RgbaColor {
                r: 34,
                g: 34,
                b: 34,
                a: 255,
            },
            background_opacity: 0.72,
            rounded_background: true,
            font_size: 28.0,
            x: 660,
            y: 136,
            width: 250,
            height: 60,
            is_countdown: false,
            duration_secs: 10,
            show_text: true,
            show_progress_bar: false,
            progress_color: RgbaColor {
                r: 0,
                g: 191,
                b: 255,
                a: 255,
            },
            progress_height: 10,
            progress_border_enabled: true,
            progress_border_color: RgbaColor {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            progress_border_thickness: 1.0,
            progress_smoothness_fps: 30,
        }
    }
}

impl Default for TimerPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CommandPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub hotkey: Option<HotkeyBinding>,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
    pub match_duplicate_window_titles: bool,
    pub use_powershell: bool,
    pub command: String,
    #[serde(skip)]
    pub run_output: Option<String>,
}

impl CommandPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Command {id}"),
            enabled: true,
            collapsed: true,
            hotkey: None,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            match_duplicate_window_titles: true,
            use_powershell: false,
            command: String::new(),
            run_output: None,
        }
    }
}

impl Default for CommandPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AiSettings {
    pub api_key: String,
    pub show_api_key: bool,
    pub system_instruction: String,
    pub prompt: String,
    pub model: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GroqSettings {
    pub api_key: String,
    #[serde(skip)]
    pub show_api_key: bool,
    pub enabled: bool,
    pub details_open: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            show_api_key: false,
            system_instruction: "Convert the user's request into a JSON array of MacroNest macro steps. Each step must have at least: key, action, delay_ms. Use only supported MacroAction names. To build a toggle macro (alternating between two states on each trigger press), generate a 6-step pattern: Group 1 (State A): Step 1 (Action A, enabled: true), Step 3 (DisableStep for steps 1,3,4, enabled: true), Step 4 (EnableStep for steps 2,5,6, enabled: true). Group 2 (State B): Step 2 (Action B, enabled: false), Step 5 (EnableStep for steps 1,3,4, enabled: false), Step 6 (DisableStep for steps 2,5,6, enabled: false). Prefer KeyPress for taps, KeyDown/KeyUp for holds, TypeText for literal text, and MouseMoveAbsolute for exact coordinates. Return JSON only.".to_owned(),
            prompt: String::new(),
            model: "gemini-2.5-flash".to_owned(),
            enabled: true,
        }
    }
}

impl Default for GroqSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            show_api_key: false,
            enabled: false,
            details_open: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub active_style: CrosshairStyle,
    pub profiles: Vec<ProfileRecord>,
    pub selected_profile: Option<String>,
    pub show_window: bool,
    pub active_panel: AppPanel,
    pub ui_language: UiLanguage,
    pub vietnamese_input_enabled: bool,
    pub vietnamese_input_mode: VietnameseInputMode,
    pub ui_theme: UiThemeMode,
    pub window_presets: Vec<WindowPreset>,
    pub next_preset_id: u32,
    #[serde(default)]
    pub window_layouts: Vec<WindowLayout>,
    #[serde(default)]
    pub next_window_layout_id: u32,
    pub window_expand_controls: WindowExpandControls,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub window_focus_presets: Vec<WindowFocusPreset>,
    pub next_window_focus_preset_id: u32,
    pub pin_presets: Vec<PinPreset>,
    pub next_pin_preset_id: u32,
    pub mouse_path_presets: Vec<MousePathPreset>,
    pub next_mouse_path_preset_id: u32,
    pub mouse_sensitivity_presets: Vec<MouseSensitivityPreset>,
    pub next_mouse_sensitivity_preset_id: u32,
    pub keyboard_arrow_mouse_enabled: bool,
    pub keyboard_arrow_mouse_step_px: u32,
    pub mouse_sensitivity_restore_on_exit: bool,
    pub mouse_sensitivity_restore_speed: u32,
    pub zoom_presets: Vec<ZoomPreset>,
    pub next_zoom_preset_id: u32,
    #[serde(alias = "toolbox_presets")]
    pub hud_presets: Vec<HudPreset>,
    #[serde(alias = "next_toolbox_preset_id")]
    pub next_hud_preset_id: u32,
    #[serde(alias = "custom_presets")]
    pub command_presets: Vec<CommandPreset>,
    #[serde(alias = "next_custom_preset_id")]
    pub next_command_preset_id: u32,
    pub master_presets: Vec<MasterPreset>,
    pub selected_master_preset_id: Option<u32>,
    pub next_master_preset_id: u32,
    pub macro_folders: Vec<MacroFolder>,
    pub next_macro_folder_id: u32,
    pub macro_groups: Vec<MacroGroup>,
    pub next_macro_group_id: u32,
    pub macro_presets: Vec<MacroPreset>,
    pub next_macro_preset_id: u32,
    #[serde(default)]
    pub geometry_presets: Vec<GeometryPreset>,
    #[serde(default)]
    pub next_geometry_preset_id: u32,
    pub macros_master_enabled: bool,
    #[serde(default)]
    pub windows_key_locked: bool,
    #[serde(default)]
    pub native_focus_highlight_enabled: bool,
    #[serde(default = "default_focus_highlight_color")]
    pub focus_highlight_color: RgbaColor,
    #[serde(default)]
    pub focus_highlight_decoration: FocusHighlightDecoration,
    #[serde(default, alias = "focus_highlight_rainbow", skip_serializing)]
    pub focus_highlight_rainbow_legacy: bool,
    #[serde(default)]
    pub protractor_enabled: bool,
    #[serde(default = "default_protractor_scale")]
    pub protractor_scale: f32,
    #[serde(default = "default_protractor_needle1_angle")]
    pub protractor_needle1_angle: f32,
    #[serde(default = "default_protractor_needle2_angle")]
    pub protractor_needle2_angle: f32,
    #[serde(default = "default_protractor_center_x")]
    pub protractor_center_x: i32,
    #[serde(default = "default_protractor_center_y")]
    pub protractor_center_y: i32,
    #[serde(default = "default_protractor_thickness")]
    pub protractor_thickness: f32,
    pub macros_master_hotkey: Option<HotkeyBinding>,
    #[serde(default = "default_true")]
    pub macro_infinite_loop_warning_enabled: bool,
    #[serde(alias = "image_search_presets")]
    pub vision_presets: Vec<VisionPreset>,
    #[serde(alias = "next_image_search_preset_id")]
    pub next_vision_preset_id: u32,
    #[serde(default)]
    pub audio_sense_presets: Vec<AudioSensePreset>,
    #[serde(default)]
    pub next_audio_sense_preset_id: u32,
    #[serde(default)]
    pub timer_presets: Vec<TimerPreset>,
    #[serde(default)]
    pub next_timer_preset_id: u32,
    pub ai_settings: AiSettings,
    pub groq_settings: GroqSettings,
    pub audio_settings: AudioSettings,
    #[serde(alias = "image_search_settings")]
    pub vision_settings: VisionSettings,
    #[serde(default = "default_macro_mouse_click_delay_ms")]
    pub macro_mouse_click_delay_ms: u32,
    #[serde(default = "default_macro_keyboard_key_press_delay_ms")]
    pub macro_keyboard_key_press_delay_ms: u32,
    #[serde(default)]
    pub global_constants: Vec<(String, i32)>,
    #[serde(default)]
    pub ocr_presets: Vec<OcrPreset>,
    #[serde(default = "default_ocr_language_code")]
    pub ocr_language: String,
    #[serde(default)]
    pub next_ocr_preset_id: u32,
    #[serde(default)]
    pub ocr_test_x: i32,
    #[serde(default)]
    pub ocr_test_y: i32,
    #[serde(default)]
    pub ocr_test_width: i32,
    #[serde(default)]
    pub ocr_test_height: i32,
    #[serde(skip)]
    pub ocr_test_running: bool,
    #[serde(skip)]
    pub ocr_test_error: Option<String>,
    #[serde(skip)]
    pub ocr_test_result: Option<OcrResult>,
    #[serde(default = "default_true")]
    pub quick_actions_copy_x: bool,
    #[serde(default = "default_true")]
    pub quick_actions_copy_y: bool,
    #[serde(default = "default_true")]
    pub quick_actions_copy_color: bool,
    #[serde(default = "default_true")]
    pub quick_actions_copy_ruler: bool,
    #[serde(default)]
    pub quick_key_display_enabled: bool,
    #[serde(default = "default_quick_key_display_x")]
    pub quick_key_display_x: i32,
    #[serde(default = "default_quick_key_display_y")]
    pub quick_key_display_y: i32,
    #[serde(default = "default_quick_key_display_size")]
    pub quick_key_display_size: f32,
    #[serde(default)]
    pub quick_key_display_mode: QuickKeyDisplayMode,
    #[serde(default)]
    pub quick_key_display_mascot_style: MascotStyle,
    #[serde(default)]
    pub quick_key_display_mascot_styles: Vec<MascotStyle>,
    #[serde(default)]
    pub quick_key_display_mascot_positions: Vec<(MascotStyle, i32, i32)>,
    #[serde(default)]
    pub quick_screen_draw_enabled: bool,
    #[serde(default)]
    pub quick_screen_draw_hotkey: Option<HotkeyBinding>,
    #[serde(default = "default_screen_draw_color")]
    pub quick_screen_draw_color: RgbaColor,
    #[serde(default = "default_screen_draw_brush_size")]
    pub quick_screen_draw_brush_size: f32,
    #[serde(default)]
    pub quick_screen_draw_smoothing: bool,
    #[serde(default = "default_screen_draw_smoothing_amount")]
    pub quick_screen_draw_smoothing_amount: f32,
    #[serde(default)]
    pub quick_screen_draw_fill: bool,
    #[serde(default = "default_screen_draw_freeze")]
    pub quick_screen_draw_freeze: bool,
    #[serde(default)]
    pub quick_screen_draw_tool: QuickScreenDrawTool,
    #[serde(default)]
    pub quick_screen_draw_text_border: bool,
    #[serde(default)]
    pub quick_key_sound_enabled: bool,
    #[serde(default)]
    pub quick_key_sound_style: u32,
    #[serde(default = "default_key_sound_volume")]
    pub quick_key_sound_volume: f32,
}

impl AppState {
    pub fn reset_session_preset_visibility(&mut self) -> bool {
        let mut changed = false;

        for profile in &mut self.profiles {
            if !profile.collapsed {
                profile.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.window_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }

        for layout in &mut self.window_layouts {
            if !layout.collapsed {
                layout.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.window_focus_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.pin_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }

        for preset in &mut self.mouse_path_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.mouse_sensitivity_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.zoom_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }

        for preset in &mut self.hud_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }

        for preset in &mut self.command_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.master_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        for folder in &mut self.macro_folders {
            if !folder.collapsed {
                folder.collapsed = true;
                changed = true;
            }
        }

        for group in &mut self.macro_groups {
            if !group.collapsed {
                group.collapsed = true;
                changed = true;
            }
            for preset in &mut group.presets {
                if !preset.collapsed {
                    preset.collapsed = true;
                    changed = true;
                }
            }
        }

        for preset in &mut self.macro_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.geometry_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.vision_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
            if preset.show_search_region_overlay {
                preset.show_search_region_overlay = false;
                changed = true;
            }
        }

        for preset in &mut self.audio_sense_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.timer_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }

        for preset in &mut self.ocr_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }

        for preset in &mut self.audio_settings.library {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        for preset in &mut self.audio_settings.presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }

        changed
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            active_style: CrosshairStyle {
                enabled: false,
                ..CrosshairStyle::default()
            },
            profiles: Vec::new(),
            selected_profile: None,
            show_window: true,
            active_panel: AppPanel::Macros,
            ui_language: UiLanguage::English,
            vietnamese_input_enabled: false,
            vietnamese_input_mode: VietnameseInputMode::Telex,
            ui_theme: UiThemeMode::Dark,
            window_presets: Vec::new(),
            next_preset_id: 1,
            window_layouts: Vec::new(),
            next_window_layout_id: 1,
            window_expand_controls: WindowExpandControls::default(),
            window_focus_presets: Vec::new(),
            next_window_focus_preset_id: 1,
            pin_presets: Vec::new(),
            next_pin_preset_id: 1,
            mouse_path_presets: Vec::new(),
            next_mouse_path_preset_id: 1,
            mouse_sensitivity_presets: Vec::new(),
            next_mouse_sensitivity_preset_id: 1,
            keyboard_arrow_mouse_enabled: false,
            keyboard_arrow_mouse_step_px: 4,
            mouse_sensitivity_restore_on_exit: false,
            mouse_sensitivity_restore_speed: 6,
            zoom_presets: Vec::new(),
            next_zoom_preset_id: 1,
            hud_presets: Vec::new(),
            next_hud_preset_id: 1,
            command_presets: Vec::new(),
            next_command_preset_id: 1,
            master_presets: Vec::new(),
            selected_master_preset_id: None,
            next_master_preset_id: 1,
            macro_folders: Vec::new(),
            next_macro_folder_id: 1,
            macro_groups: Vec::new(),
            next_macro_group_id: 1,
            macro_presets: Vec::new(),
            next_macro_preset_id: 1,
            geometry_presets: Vec::new(),
            next_geometry_preset_id: 1,
            macros_master_enabled: true,
            windows_key_locked: false,
            native_focus_highlight_enabled: false,
            focus_highlight_color: default_focus_highlight_color(),
            focus_highlight_decoration: FocusHighlightDecoration::Plain,
            focus_highlight_rainbow_legacy: false,
            protractor_enabled: false,
            protractor_scale: 1.0,
            protractor_needle1_angle: 0.0,
            protractor_needle2_angle: 90.0,
            protractor_center_x: 500,
            protractor_center_y: 500,
            protractor_thickness: 2.0,
            macros_master_hotkey: None,
            macro_infinite_loop_warning_enabled: true,
            vision_presets: vec![VisionPreset::default()],
            next_vision_preset_id: 2,
            audio_sense_presets: Vec::new(),
            next_audio_sense_preset_id: 1,
            timer_presets: Vec::new(),
            next_timer_preset_id: 1,
            ai_settings: AiSettings::default(),
            groq_settings: GroqSettings::default(),
            audio_settings: AudioSettings::default(),
            vision_settings: VisionSettings::default(),
            macro_mouse_click_delay_ms: 0,
            macro_keyboard_key_press_delay_ms: 0,
            global_constants: Vec::new(),
            ocr_presets: Vec::new(),
            ocr_language: default_ocr_language_code(),
            next_ocr_preset_id: 1,
            ocr_test_x: 0,
            ocr_test_y: 0,
            ocr_test_width: 320,
            ocr_test_height: 180,
            ocr_test_running: false,
            ocr_test_error: None,
            ocr_test_result: None,
            quick_actions_copy_x: true,
            quick_actions_copy_y: true,
            quick_actions_copy_color: true,
            quick_actions_copy_ruler: true,
            quick_key_display_enabled: false,
            quick_key_display_x: default_quick_key_display_x(),
            quick_key_display_y: default_quick_key_display_y(),
            quick_key_display_size: default_quick_key_display_size(),
            quick_key_display_mode: QuickKeyDisplayMode::Mascot,
            quick_key_display_mascot_style: MascotStyle::Hachiware,
            quick_key_display_mascot_styles: vec![MascotStyle::Hachiware],
            quick_key_display_mascot_positions: Vec::new(),
            quick_screen_draw_enabled: false,
            quick_screen_draw_hotkey: None,
            quick_screen_draw_color: default_screen_draw_color(),
            quick_screen_draw_brush_size: default_screen_draw_brush_size(),
            quick_screen_draw_smoothing: false,
            quick_screen_draw_smoothing_amount: default_screen_draw_smoothing_amount(),
            quick_screen_draw_fill: false,
            quick_screen_draw_freeze: default_screen_draw_freeze(),
            quick_screen_draw_tool: QuickScreenDrawTool::Brush,
            quick_screen_draw_text_border: false,
            quick_key_sound_enabled: false,
            quick_key_sound_style: 2,
            quick_key_sound_volume: default_key_sound_volume(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::AppState;

    #[test]
    fn app_state_deserializes_legacy_alias_fields() {
        let state: AppState = serde_json::from_value(json!({
            "toolbox_presets": [{ "id": 7, "name": "HUD 7" }],
            "next_toolbox_preset_id": 8,
            "custom_presets": [{ "id": 9, "name": "Command 9" }],
            "next_custom_preset_id": 10,
            "focus_highlight_rainbow": true
        }))
        .expect("legacy app state aliases should deserialize");

        assert_eq!(state.hud_presets.len(), 1);
        assert_eq!(state.hud_presets[0].id, 7);
        assert_eq!(state.next_hud_preset_id, 8);
        assert_eq!(state.command_presets.len(), 1);
        assert_eq!(state.command_presets[0].id, 9);
        assert_eq!(state.next_command_preset_id, 10);
        assert!(state.focus_highlight_rainbow_legacy);
    }

    #[test]
    fn app_state_maps_legacy_cyber_mech_focus_highlight_to_plain() {
        let state: AppState = serde_json::from_value(json!({
            "focus_highlight_decoration": "CyberMech"
        }))
        .expect("legacy CyberMech decoration should deserialize");

        assert_eq!(
            state.focus_highlight_decoration,
            super::FocusHighlightDecoration::Plain
        );
    }

    #[test]
    fn reset_session_preset_visibility_collapses_and_hides_session_previews() {
        let mut state = AppState::default();
        state.profiles.push(super::ProfileRecord {
            collapsed: false,
            ..Default::default()
        });
        state.window_presets.push(super::WindowPreset {
            collapsed: false,
            preview_enabled: true,
            ..Default::default()
        });
        state.window_layouts.push(super::WindowLayout {
            collapsed: false,
            ..Default::default()
        });
        state.window_focus_presets.push(super::WindowFocusPreset {
            collapsed: false,
            ..Default::default()
        });
        state.pin_presets.push(super::PinPreset {
            collapsed: false,
            preview_enabled: true,
            ..Default::default()
        });
        state.mouse_path_presets.push(super::MousePathPreset {
            collapsed: false,
            ..Default::default()
        });
        state.mouse_sensitivity_presets.push(super::MouseSensitivityPreset {
            collapsed: false,
            ..Default::default()
        });
        state.zoom_presets.push(super::ZoomPreset {
            collapsed: false,
            preview_enabled: true,
            ..Default::default()
        });
        state.hud_presets.push(super::HudPreset {
            collapsed: false,
            preview_enabled: true,
            ..Default::default()
        });
        state.command_presets.push(super::CommandPreset {
            collapsed: false,
            ..Default::default()
        });
        state.master_presets.push(super::MasterPreset {
            collapsed: false,
            ..Default::default()
        });
        state.macro_folders.push(super::MacroFolder {
            collapsed: false,
            ..Default::default()
        });
        state.macro_groups.push(super::MacroGroup {
            collapsed: false,
            presets: vec![super::MacroPreset {
                collapsed: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        state.macro_presets.push(super::MacroPreset {
            collapsed: false,
            ..Default::default()
        });
        state.geometry_presets.push(super::GeometryPreset {
            collapsed: false,
            ..Default::default()
        });
        state.vision_presets.push(super::VisionPreset {
            collapsed: false,
            show_search_region_overlay: true,
            ..Default::default()
        });
        state.audio_sense_presets.push(super::AudioSensePreset {
            collapsed: false,
            ..Default::default()
        });
        state.timer_presets.push(super::TimerPreset {
            collapsed: false,
            preview_enabled: true,
            ..Default::default()
        });
        state.ocr_presets.push(super::OcrPreset {
            collapsed: false,
            preview_enabled: true,
            ..Default::default()
        });
        state.audio_settings.library.push(super::SoundLibraryItem {
            collapsed: false,
            ..Default::default()
        });
        state.audio_settings.presets.push(super::SoundPreset {
            collapsed: false,
            ..Default::default()
        });

        assert!(state.reset_session_preset_visibility());

        assert!(state.profiles.iter().all(|preset| preset.collapsed));
        assert!(state.window_presets.iter().all(|preset| preset.collapsed && !preset.preview_enabled));
        assert!(state.window_layouts.iter().all(|preset| preset.collapsed));
        assert!(state.window_focus_presets.iter().all(|preset| preset.collapsed));
        assert!(state.pin_presets.iter().all(|preset| preset.collapsed && !preset.preview_enabled));
        assert!(state.mouse_path_presets.iter().all(|preset| preset.collapsed));
        assert!(state.mouse_sensitivity_presets.iter().all(|preset| preset.collapsed));
        assert!(state.zoom_presets.iter().all(|preset| preset.collapsed && !preset.preview_enabled));
        assert!(state.hud_presets.iter().all(|preset| preset.collapsed && !preset.preview_enabled));
        assert!(state.command_presets.iter().all(|preset| preset.collapsed));
        assert!(state.master_presets.iter().all(|preset| preset.collapsed));
        assert!(state.macro_folders.iter().all(|preset| preset.collapsed));
        assert!(state.macro_groups.iter().all(|preset| {
            preset.collapsed && preset.presets.iter().all(|macro_preset| macro_preset.collapsed)
        }));
        assert!(state.macro_presets.iter().all(|preset| preset.collapsed));
        assert!(state.geometry_presets.iter().all(|preset| preset.collapsed));
        assert!(state
            .vision_presets
            .iter()
            .all(|preset| preset.collapsed && !preset.show_search_region_overlay));
        assert!(state.audio_sense_presets.iter().all(|preset| preset.collapsed));
        assert!(state.timer_presets.iter().all(|preset| preset.collapsed && !preset.preview_enabled));
        assert!(state.ocr_presets.iter().all(|preset| preset.collapsed && !preset.preview_enabled));
        assert!(state.audio_settings.library.iter().all(|preset| preset.collapsed));
        assert!(state.audio_settings.presets.iter().all(|preset| preset.collapsed));
    }
}
