use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const WHITE: Self = Self {
        r: 0,
        g: 255,
        b: 170,
        a: 255,
    };

    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    pub fn with_alpha(self, alpha: f32) -> Self {
        let mut next = self;
        next.a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        next
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrosshairStyle {
    pub enabled: bool,
    pub x_offset: i32,
    pub y_offset: i32,
    #[serde(default = "default_crosshair_length")]
    pub horizontal_length: f32,
    #[serde(default = "default_crosshair_length")]
    pub vertical_length: f32,
    pub arm_length: f32,
    pub thickness: f32,
    pub gap: f32,
    pub outline_enabled: bool,
    pub outline_thickness: f32,
    pub outline_color: RgbaColor,
    pub center_dot: bool,
    pub center_dot_size: f32,
    pub opacity: f32,
    pub color: RgbaColor,
    pub custom_asset: Option<String>,
    pub custom_scale: f32,
}

impl Default for CrosshairStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            x_offset: 0,
            y_offset: 0,
            horizontal_length: 10.0,
            vertical_length: 10.0,
            arm_length: 10.0,
            thickness: 3.0,
            gap: 5.0,
            outline_enabled: true,
            outline_thickness: 2.0,
            outline_color: RgbaColor::BLACK,
            center_dot: false,
            center_dot_size: 4.0,
            opacity: 0.95,
            color: RgbaColor::WHITE,
            custom_asset: None,
            custom_scale: 96.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileRecord {
    pub name: String,
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub collapsed: bool,
    pub style: CrosshairStyle,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
}

impl Default for ProfileRecord {
    fn default() -> Self {
        Self {
            name: "Default".to_owned(),
            enabled: true,
            collapsed: true,
            style: CrosshairStyle::default(),
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
        }
    }
}

fn default_crosshair_length() -> f32 {
    10.0
}

fn default_true() -> bool {
    true
}

fn default_timer_progress_border_color() -> RgbaColor {
    RgbaColor {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    }
}

fn default_timer_progress_border_thickness() -> f32 {
    1.0
}

fn default_timer_progress_smoothness_fps() -> u32 {
    30
}

fn default_false() -> bool {
    false
}

fn default_if_operator() -> String {
    "==".to_string()
}

fn default_condition_join_operator() -> String {
    "AND".to_string()
}

fn default_if_color_tolerance() -> u8 {
    10
}

fn default_image_search_confidence_threshold() -> f32 {
    0.99
}

fn default_image_search_color_tolerance() -> u8 {
    18
}

fn default_image_search_color_scan_rate_hz() -> u32 {
    24
}

fn default_image_search_offset_px() -> i32 {
    0
}

fn default_image_search_move_passes() -> u8 {
    3
}

fn default_image_search_move_delay_ms() -> u64 {
    10
}

fn default_image_search_distance_near_speed() -> f32 {
    0.75
}

fn default_image_search_distance_far_speed() -> f32 {
    5.0
}

fn default_geometry_stroke_color() -> RgbaColor {
    RgbaColor {
        r: 0,
        g: 255,
        b: 170,
        a: 255,
    }
}

fn default_geometry_fill_color() -> RgbaColor {
    RgbaColor {
        r: 0,
        g: 255,
        b: 170,
        a: 255,
    }
}

fn default_geometry_thickness() -> f32 {
    2.0
}

fn default_geometry_opacity() -> f32 {
    1.0
}

fn default_geometry_font_size() -> f32 {
    18.0
}

fn default_geometry_point_radius() -> f32 {
    6.0
}

fn default_geometry_arrow_head_size() -> f32 {
    16.0
}

fn default_macro_mouse_click_delay_ms() -> u32 {
    16
}

fn default_macro_keyboard_key_press_delay_ms() -> u32 {
    0
}

fn default_ocr_width() -> i32 {
    320
}

fn default_ocr_height() -> i32 {
    180
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
    #[serde(default = "default_true", alias = "stretch_enabled")]
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
            remove_title_bar: true,
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
                WindowLayoutCell { row: 0, col: 0, ..Default::default() },
                WindowLayoutCell { row: 0, col: 1, ..Default::default() },
                WindowLayoutCell { row: 1, col: 0, ..Default::default() },
                WindowLayoutCell { row: 1, col: 1, ..Default::default() },
            ],
            focus_on_apply: true,
            hotkey: None,
            trigger_keys: String::new(),
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum MacroAction {
    #[default]
    KeyPress,
    KeyDown,
    KeyUp,
    Wait,
    TypeText,
    ApplyWindowPreset,
    FocusWindowPreset,
    TriggerMacroPreset,
    #[serde(alias = "TriggerCustomPreset")]
    TriggerCommandPreset,
    EnableCrosshairProfile,
    DisableCrosshair,
    EnablePinPreset,
    DisablePin,
    PlayMousePathPreset,
    ApplyMouseSensitivityPreset,
    EnableZoomPreset,
    DisableZoom,
    PlaySoundPreset,
    PlayVideoPreset,
    #[serde(alias = "StartImageSearch")]
    StartVisionSearch,
    #[serde(alias = "ScanImageOnce", alias = "ScanVisionOnce")]
    ScanVisionOnce,
    #[serde(alias = "StartPitchDetect", alias = "StartSpatialAudioDetect")]
    StartAudioSensePreset,
    #[serde(alias = "StopAudioSensePreset")]
    StopAudioSense,

    #[serde(alias = "StopImageSearchWait")]
    StopVisionWait,
    #[serde(alias = "StopImageSearch")]
    StopVision,
    LoopStart,
    LoopEnd,
    StopIfTriggerPressedAgain,
    StopIfKeyPressed,
    #[serde(alias = "ShowToolbox")]
    ShowHud,
    #[serde(alias = "HideToolbox")]
    HideHud,
    LockKeys,
    UnlockKeys,
    LockMouse,
    UnlockMouse,
    EnableMacroPreset,
    DisableMacroPreset,
    MouseLeftClick,
    MouseLeftDown,
    MouseLeftUp,
    MouseRightClick,
    MouseRightDown,
    MouseRightUp,
    MouseMiddleClick,
    MouseMiddleDown,
    MouseMiddleUp,
    MouseX1Click,
    MouseX1Down,
    MouseX1Up,
    MouseX2Click,
    MouseX2Down,
    MouseX2Up,
    MouseWheelUp,
    MouseWheelDown,
    MouseMoveAbsolute,
    MouseMoveRelative,
    TriggerVisionTiming,
    StartVisionTiming,
    StopVisionTiming,
    IfStart,
    Else,
    IfEnd,
    SetVariable,
    StartTimerPreset,
    PauseTimerPreset,
    StopTimerPreset,
    EnableStep,
    DisableStep,
    OcrSearch,
    DrawGeometry,
    ShowGeometryPreset,
    HideGeometryPreset,
    JumpToStep,
    DrawSvgImage,
    #[serde(other)]
    Legacy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum AudioSensePresetKind {
    #[default]
    #[serde(alias = "Spatial", alias = "Surround")]
    Pitch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum AudioSenseSource {
    #[default]
    System,
    Microphone,
}

fn default_audio_sense_updates_per_second() -> u32 {
    60
}

fn default_audio_sense_duration_ms() -> u64 {
    1500
}

fn default_audio_sense_output_note_var() -> String {
    String::new()
}

fn default_audio_sense_output_level_var() -> String {
    String::new()
}

fn default_audio_sense_min_confidence() -> u32 {
    560
}

fn default_audio_sense_min_level() -> u32 {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudioSenseMonitorSettings {
    pub source: AudioSenseSource,
    pub input_device_name: Option<String>,
    #[serde(default = "default_audio_sense_updates_per_second")]
    pub updates_per_second: u32,
    #[serde(default, alias = "listen_forever")]
    pub permanent: bool,
    #[serde(default = "default_audio_sense_duration_ms")]
    pub duration_ms: u64,
}

impl Default for AudioSenseMonitorSettings {
    fn default() -> Self {
        Self {
            source: AudioSenseSource::System,
            input_device_name: None,
            updates_per_second: default_audio_sense_updates_per_second(),
            permanent: false,
            duration_ms: default_audio_sense_duration_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PitchAudioSenseSettings {
    pub monitor: AudioSenseMonitorSettings,
    pub show_sharps: bool,
    #[serde(default = "default_audio_sense_output_note_var")]
    pub output_note_var: String,
    #[serde(default = "default_audio_sense_output_level_var")]
    pub output_level_var: String,
    #[serde(default = "default_audio_sense_min_confidence")]
    pub min_confidence: u32,
    #[serde(default = "default_audio_sense_min_level")]
    pub min_level: u32,
}

impl Default for PitchAudioSenseSettings {
    fn default() -> Self {
        Self {
            monitor: AudioSenseMonitorSettings::default(),
            show_sharps: true,
            output_note_var: default_audio_sense_output_note_var(),
            output_level_var: default_audio_sense_output_level_var(),
            min_confidence: default_audio_sense_min_confidence(),
            min_level: default_audio_sense_min_level(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudioSensePreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub kind: AudioSensePresetKind,
    pub pitch: PitchAudioSenseSettings,
}

impl AudioSensePreset {
    pub fn new_pitch(id: u32) -> Self {
        Self {
            id,
            name: format!("Pitch Detect {id}"),
            enabled: true,
            collapsed: true,
            kind: AudioSensePresetKind::Pitch,
            pitch: PitchAudioSenseSettings::default(),
        }
    }
}

impl Default for AudioSensePreset {
    fn default() -> Self {
        Self::new_pitch(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudioSenseSpec {
    pub kind: AudioSensePresetKind,
    pub pitch: PitchAudioSenseSettings,
}

impl Default for AudioSenseSpec {
    fn default() -> Self {
        Self {
            kind: AudioSensePresetKind::Pitch,
            pitch: PitchAudioSenseSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum GeometryShapeKind {
    #[default]
    Point,
    Line,
    Circle,
    Rectangle,
    Label,
    Ellipse,
    Arrow,
    Polyline,
    Polygon,
    Arc,
    Svg,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeometrySpec {
    pub shape: GeometryShapeKind,
    pub x1_expr: String,
    pub y1_expr: String,
    pub x2_expr: String,
    pub y2_expr: String,
    pub x3_expr: String,
    pub y3_expr: String,
    pub x4_expr: String,
    pub y4_expr: String,
    pub width_expr: String,
    pub height_expr: String,
    pub radius_expr: String,
    pub radius_x_expr: String,
    pub radius_y_expr: String,
    pub start_angle_expr: String,
    pub end_angle_expr: String,
    pub rotation_expr: String,
    pub arrow_head_size_expr: String,
    pub font_size_expr: String,
    pub thickness_expr: String,
    pub opacity_expr: String,
    pub fill_opacity_expr: String,
    pub points_expr: String,
    pub text: String,
    pub stroke_color_expr: String,
    pub fill_color_expr: String,
    #[serde(default = "default_geometry_stroke_color")]
    pub stroke_color: RgbaColor,
    #[serde(default = "default_geometry_fill_color")]
    pub fill_color: RgbaColor,
    pub filled: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_geometry_thickness")]
    pub thickness: f32,
    #[serde(default = "default_geometry_opacity")]
    pub opacity: f32,
    #[serde(default = "default_geometry_opacity")]
    pub fill_opacity: f32,
    #[serde(default = "default_geometry_font_size")]
    pub font_size: f32,
    #[serde(default = "default_geometry_point_radius")]
    pub point_radius: f32,
    #[serde(default = "default_geometry_arrow_head_size")]
    pub arrow_head_size: f32,
}

impl Default for GeometrySpec {
    fn default() -> Self {
        Self {
            shape: GeometryShapeKind::Point,
            x1_expr: "960".to_owned(),
            y1_expr: "540".to_owned(),
            x2_expr: "1120".to_owned(),
            y2_expr: "540".to_owned(),
            x3_expr: String::new(),
            y3_expr: String::new(),
            x4_expr: String::new(),
            y4_expr: String::new(),
            width_expr: "180".to_owned(),
            height_expr: "120".to_owned(),
            radius_expr: "60".to_owned(),
            radius_x_expr: "90".to_owned(),
            radius_y_expr: "60".to_owned(),
            start_angle_expr: "0".to_owned(),
            end_angle_expr: "180".to_owned(),
            rotation_expr: "0".to_owned(),
            arrow_head_size_expr: "16".to_owned(),
            font_size_expr: "18".to_owned(),
            thickness_expr: "2".to_owned(),
            opacity_expr: "1".to_owned(),
            fill_opacity_expr: "0.3".to_owned(),
            points_expr: "960,540;1120,540;1120,660".to_owned(),
            text: "Label".to_owned(),
            stroke_color_expr: String::new(),
            fill_color_expr: String::new(),
            stroke_color: default_geometry_stroke_color(),
            fill_color: default_geometry_fill_color(),
            filled: false,
            visible: true,
            thickness: default_geometry_thickness(),
            opacity: default_geometry_opacity(),
            fill_opacity: 0.3,
            font_size: default_geometry_font_size(),
            point_radius: default_geometry_point_radius(),
            arrow_head_size: default_geometry_arrow_head_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeometryObject {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub spec: GeometrySpec,
}

impl GeometryObject {
    pub fn new(id: u32, shape: GeometryShapeKind) -> Self {
        let mut spec = GeometrySpec::default();
        spec.shape = shape;
        if shape == GeometryShapeKind::Svg {
            spec.text = String::new();
            spec.opacity_expr = "100".to_owned();
        }
        Self {
            id,
            name: format!("{shape:?} {id}"),
            enabled: true,
            spec,
        }
    }
}

impl Default for GeometryObject {
    fn default() -> Self {
        Self::new(1, GeometryShapeKind::Point)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeometryPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub objects: Vec<GeometryObject>,
}

impl GeometryPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Geometry {id}"),
            enabled: true,
            collapsed: true,
            objects: vec![GeometryObject::new(1, GeometryShapeKind::Point)],
        }
    }
}

impl Default for GeometryPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

fn default_svg_opacity() -> f32 {
    1.0
}

fn default_svg_x_expr() -> String {
    "960".to_owned()
}

fn default_svg_y_expr() -> String {
    "540".to_owned()
}

fn default_svg_width_expr() -> String {
    "200".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SvgImageSpec {
    /// Path to the SVG/PNG/JPEG/BMP/WEBP file (absolute or relative to assets folder).
    pub path: String,
    /// X position of the image top-left corner (screen pixels). Supports math expressions.
    #[serde(default = "default_svg_x_expr")]
    pub x_expr: String,
    /// Y position of the image top-left corner (screen pixels). Supports math expressions.
    #[serde(default = "default_svg_y_expr")]
    pub y_expr: String,
    /// Target width in pixels (0 = derive from height to maintain aspect ratio). Supports expressions.
    #[serde(default = "default_svg_width_expr")]
    pub width_expr: String,
    /// Target height in pixels (0 = derive from width to maintain aspect ratio). Supports expressions.
    pub height_expr: String,
    /// Overall opacity [0.0, 1.0]. Supports math expressions.
    pub opacity_expr: String,
    /// Rotation in degrees around the image center. Supports math expressions.
    pub rotation_expr: String,
    /// Whether the image should be drawn at all.
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Cached resolved opacity value (used as fallback when expression is empty).
    #[serde(default = "default_svg_opacity")]
    pub opacity: f32,
}

impl Default for SvgImageSpec {
    fn default() -> Self {
        Self {
            path: String::new(),
            x_expr: default_svg_x_expr(),
            y_expr: default_svg_y_expr(),
            width_expr: default_svg_width_expr(),
            height_expr: String::new(),
            opacity_expr: String::new(),
            rotation_expr: String::new(),
            visible: true,
            opacity: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum SetVariableSource {
    #[default]
    Expression,
    TimeHour,
    TimeMinute,
    TimeSecond,
    TimeMillisecond,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum IfConditionType {
    #[default]
    Variable,
    PixelColor,
    VisionMatch,
    KeyHeld,
    MouseHeld,
    MousePosition,
    PresetRunning,
    OcrMatch,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ExtraCondition {
    #[serde(default = "default_condition_join_operator")]
    pub join_operator: String,
    #[serde(default)]
    pub condition_type: IfConditionType,
    pub variable_name: String,
    #[serde(default = "default_if_operator")]
    pub operator: String,
    pub compare_value: i32,
    pub expression: String,
    #[serde(default)]
    pub if_contain_case_sensitive: bool,
    #[serde(default)]
    pub if_contain_isolated: bool,

    // OCR Match
    #[serde(default)]
    pub ocr_preset_id: Option<u32>,
    #[serde(default)]
    pub ocr_target_text: String,

    // Pixel Color
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub target_color: String,
    #[serde(default = "default_if_color_tolerance")]
    pub color_tolerance: u8,

    // Vision Match
    #[serde(default)]
    pub vision_preset_id: Option<u32>,

    // Key Held / Key Pressed
    #[serde(default)]
    pub key_held_name: String,

    // Mouse Held
    #[serde(default)]
    pub mouse_button: String,

    // Mouse Position
    #[serde(default)]
    pub mouse_axis: String,

    // Preset Running
    #[serde(default)]
    pub running_preset_id: Option<u32>,
    #[serde(skip)]
    pub running_preset_group_id: Option<u32>,
}

impl Default for ExtraCondition {
    fn default() -> Self {
        Self {
            join_operator: "AND".to_string(),
            condition_type: IfConditionType::Variable,
            variable_name: String::new(),
            operator: "==".to_string(),
            compare_value: 0,
            expression: String::new(),
            if_contain_case_sensitive: false,
            if_contain_isolated: false,
            ocr_preset_id: None,
            ocr_target_text: String::new(),
            x: 0,
            y: 0,
            target_color: String::new(),
            color_tolerance: 5,
            vision_preset_id: None,
            key_held_name: String::new(),
            mouse_button: "Left".to_string(),
            mouse_axis: "X".to_string(),
            running_preset_id: None,
            running_preset_group_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MacroStep {
    pub key: String,
    pub action: MacroAction,
    pub delay_ms: u64,
    #[serde(default)]
    pub delay_expr: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub x_expr: String,
    #[serde(default)]
    pub y_expr: String,
    pub text_override: String,
    #[serde(default, alias = "custom_preset_command")]
    pub command_preset_command: String,
    #[serde(default = "default_false", alias = "custom_preset_use_powershell")]
    pub command_preset_use_powershell: bool,
    pub timed_override: bool,
    pub duration_override_ms: u64,
    #[serde(default)]
    pub duration_expr: String,
    pub smooth_mouse_path: bool,
    #[serde(default)]
    pub mouse_speed_expr: String,
    pub mouse_speed_percent: u32,
    #[serde(default = "default_true", alias = "image_search_move_cursor_on_match")]
    pub vision_move_cursor_on_match: bool,
    #[serde(default, alias = "image_search_wait_until_found")]
    pub vision_wait_until_found: bool,
    #[serde(default, alias = "image_search_trigger_macro_enabled")]
    pub vision_trigger_macro_enabled: bool,
    #[serde(default, alias = "image_search_trigger_macro_preset_id")]
    pub vision_trigger_macro_preset_id: Option<u32>,
    #[serde(default)]
    pub if_condition_type: IfConditionType,
    #[serde(default)]
    pub if_target_color: String,
    #[serde(default = "default_if_color_tolerance")]
    pub if_color_tolerance: u8,
    #[serde(default)]
    pub if_vision_preset_id: Option<u32>,
    #[serde(default)]
    pub if_ocr_preset_id: Option<u32>,
    #[serde(default)]
    pub if_key_held_name: String,
    #[serde(default)]
    pub if_mouse_button: String,
    #[serde(default)]
    pub if_mouse_axis: String,
    #[serde(default)]
    pub if_running_preset_id: Option<u32>,
    #[serde(skip)]
    pub if_running_preset_group_id: Option<u32>,
    #[serde(default)]
    pub timer_preset_id: Option<u32>,
    #[serde(default)]
    pub timer_on_complete_macro_preset_id: Option<u32>,
    #[serde(default = "default_true")]
    pub lock_mouse_left: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_right: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_middle: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_scroll: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_x1: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_x2: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_move: bool,
    #[serde(default = "default_false")]
    pub toggle_enabled_on_run: bool,
    #[serde(default)]
    pub if_variable_name: String,
    #[serde(default = "default_if_operator")]
    pub if_operator: String,
    #[serde(default)]
    pub manual_mouse_sensitivity: bool,
    #[serde(default)]
    pub break_loop_by_variable: bool,
    #[serde(default)]
    pub break_loop_mode: String,
    #[serde(default)]
    pub if_compare_value: i32,
    #[serde(default)]
    pub if_compare_by_expression: bool,
    #[serde(default)]
    pub extra_conditions: Vec<ExtraCondition>,
    #[serde(default)]
    pub wait_time_unit: String,
    #[serde(default = "default_true")]
    pub unlock_on_exit: bool,
    #[serde(default)]
    pub set_variable_source: SetVariableSource,
    #[serde(default = "default_false")]
    pub wait_for_completion: bool,
    #[serde(default = "default_ocr_width")]
    pub ocr_width: i32,
    #[serde(default = "default_ocr_height")]
    pub ocr_height: i32,
    #[serde(default)]
    pub ocr_target_text: String,
    #[serde(default)]
    pub ocr_success_var: String,
    #[serde(default)]
    pub ocr_pos_var_x: String,
    #[serde(default)]
    pub ocr_pos_var_y: String,
    #[serde(default)]
    pub ocr_numeric_var: String,
    #[serde(default)]
    pub ocr_lang: Option<String>,
    #[serde(default)]
    pub ocr_text_var: String,
    #[serde(default)]
    pub vision_pos_var_x: String,
    #[serde(default)]
    pub vision_pos_var_y: String,
    #[serde(default)]
    pub vision_found_var: String,
    #[serde(default)]
    pub audio_sense_preset_id: Option<u32>,
    #[serde(default)]
    pub audio_sense_spec: AudioSenseSpec,
    #[serde(default = "default_true")]
    pub audio_sense_collapsed: bool,
    #[serde(default)]
    pub audio_sense_stop_all: bool,
    #[serde(default)]
    pub geometry_preset_id: Option<u32>,
    #[serde(default)]
    pub geometry_spec: GeometrySpec,
    #[serde(default = "default_true")]
    pub geometry_collapsed: bool,
    #[serde(default)]
    pub svg_image_spec: SvgImageSpec,
    #[serde(default = "default_true")]
    pub svg_image_collapsed: bool,
    pub if_contain_case_sensitive: bool,
    #[serde(default)]
    pub if_contain_isolated: bool,
    /// Which macro group to target for TriggerMacroPreset action
    #[serde(default)]
    pub trigger_macro_group_id: Option<u32>,
}

impl Default for MacroStep {
    fn default() -> Self {
        Self {
            key: String::new(),
            action: MacroAction::KeyPress,
            delay_ms: 0,
            delay_expr: String::new(),
            enabled: true,
            x: 0,
            y: 0,
            x_expr: String::new(),
            y_expr: String::new(),
            text_override: String::new(),
            command_preset_command: String::new(),
            command_preset_use_powershell: false,
            timed_override: false,
            duration_override_ms: 1500,
            duration_expr: String::new(),
            smooth_mouse_path: false,
            mouse_speed_expr: String::new(),
            mouse_speed_percent: 100,
            vision_move_cursor_on_match: true,
            vision_wait_until_found: false,
            vision_trigger_macro_enabled: false,
            vision_trigger_macro_preset_id: None,
            if_condition_type: IfConditionType::default(),
            if_target_color: String::new(),
            if_color_tolerance: 10,
            if_vision_preset_id: None,
            if_ocr_preset_id: None,
            if_key_held_name: String::new(),
            if_mouse_button: "MouseLeft".to_string(),
            if_mouse_axis: "X".to_string(),
            if_running_preset_id: None,
            if_running_preset_group_id: None,
            timer_preset_id: None,
            timer_on_complete_macro_preset_id: None,
            lock_mouse_left: true,
            lock_mouse_right: true,
            lock_mouse_middle: true,
            lock_mouse_scroll: true,
            lock_mouse_x1: true,
            lock_mouse_x2: true,
            lock_mouse_move: true,
            toggle_enabled_on_run: false,
            if_variable_name: String::new(),
            if_operator: "==".to_string(),
            manual_mouse_sensitivity: false,
            break_loop_by_variable: false,
            break_loop_mode: String::new(),
            if_compare_value: 0,
            if_compare_by_expression: false,
            extra_conditions: Vec::new(),
            wait_time_unit: String::new(),
            unlock_on_exit: true,
            set_variable_source: SetVariableSource::Expression,
            wait_for_completion: false,
            ocr_width: 320,
            ocr_height: 180,
            ocr_target_text: String::new(),
            ocr_success_var: String::new(),
            ocr_pos_var_x: String::new(),
            ocr_pos_var_y: String::new(),
            ocr_numeric_var: String::new(),
            ocr_lang: None,
            ocr_text_var: String::new(),
            vision_pos_var_x: String::new(),
            vision_pos_var_y: String::new(),
            vision_found_var: String::new(),
            audio_sense_preset_id: None,
            audio_sense_spec: AudioSenseSpec::default(),
            audio_sense_collapsed: true,
            audio_sense_stop_all: false,
            geometry_preset_id: None,
            geometry_spec: GeometrySpec::default(),
            geometry_collapsed: true,
            svg_image_spec: SvgImageSpec::default(),
            svg_image_collapsed: true,
            if_contain_case_sensitive: false,
            if_contain_isolated: false,
            trigger_macro_group_id: None,
        }
    }
}

impl MacroStep {
    pub fn get_break_loop_mode(&self) -> &str {
        if self.break_loop_mode.is_empty() {
            if self.break_loop_by_variable {
                "VarCompare"
            } else if !self.key.trim().is_empty() {
                "StopKey"
            } else {
                "Immediate"
            }
        } else {
            &self.break_loop_mode
        }
    }

    pub fn get_delay_ms(&self) -> u64 {
        if !self.delay_expr.trim().is_empty() {
            let interpolated = crate::overlay::interpolate_variables(&self.delay_expr);
            let base_val = crate::overlay::evaluate_math_expression(&interpolated);
            let multiplier = match self.wait_time_unit.as_str() {
                "s" => 1000,
                "m" => 60000,
                "h" => 3600000,
                _ => 1, // ms or empty
            };
            (base_val.max(0) as u64) * multiplier
        } else {
            self.delay_ms
        }
    }

    pub fn get_duration_ms(&self) -> u64 {
        match self.action {
            MacroAction::EnableCrosshairProfile
            | MacroAction::EnablePinPreset
            | MacroAction::ShowHud
            | MacroAction::DrawGeometry
            | MacroAction::DrawSvgImage => {
                if !self.duration_expr.trim().is_empty() {
                    let trimmed = self.duration_expr.trim();
                    let interpolated = crate::overlay::interpolate_variables(trimmed);
                    let val = crate::overlay::evaluate_math_expression(&interpolated);
                    val.max(0) as u64
                } else {
                    0
                }
            }
            _ => {
                if !self.duration_expr.trim().is_empty() {
                    let trimmed = self.duration_expr.trim();
                    let interpolated = crate::overlay::interpolate_variables(trimmed);
                    let val = crate::overlay::evaluate_math_expression(&interpolated);
                    val.max(0) as u64
                } else {
                    self.duration_override_ms
                }
            }
        }
    }

    pub fn get_x(&self) -> i32 {
        Self::resolve_i32_expression(&self.x_expr).unwrap_or(self.x)
    }

    pub fn get_y(&self) -> i32 {
        Self::resolve_i32_expression(&self.y_expr).unwrap_or(self.y)
    }

    pub fn get_mouse_speed_multiplier(&self) -> f32 {
        Self::resolve_mouse_speed_multiplier(&self.mouse_speed_expr)
            .unwrap_or_else(|| self.legacy_mouse_speed_multiplier())
    }

    pub fn format_mouse_speed_multiplier(multiplier: f32) -> String {
        let clamped = multiplier.clamp(0.1, 100.0);
        let mut number = format!("{clamped:.2}");
        while number.contains('.') && number.ends_with('0') {
            number.pop();
        }
        if number.ends_with('.') {
            number.pop();
        }
        format!("x{number}")
    }

    pub fn resolve_mouse_speed_multiplier(expr: &str) -> Option<f32> {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return None;
        }

        let interpolated = crate::overlay::interpolate_variables(trimmed);
        let normalized = interpolated
            .trim()
            .trim_start_matches('x')
            .trim_start_matches('X')
            .trim();

        if normalized.is_empty() {
            return None;
        }

        if let Ok(parsed) = normalized.parse::<f32>() {
            if parsed.is_finite() && parsed > 0.0 {
                return Some(parsed.clamp(0.1, 100.0));
            }
        }

        let evaluated = crate::overlay::evaluate_math_expression(normalized);
        if evaluated > 0 {
            Some((evaluated as f32).clamp(0.1, 100.0))
        } else {
            None
        }
    }

    fn resolve_i32_expression(expr: &str) -> Option<i32> {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return None;
        }

        let interpolated = crate::overlay::interpolate_variables(trimmed);
        Some(crate::overlay::evaluate_math_expression(&interpolated))
    }

    pub fn is_infinite_loop(&self) -> bool {
        self.action == MacroAction::LoopStart
            && matches!(
                self.key.trim().to_ascii_lowercase().as_str(),
                "infinite" | "inf" | "forever" | "-1"
            )
    }

    fn legacy_mouse_speed_multiplier(&self) -> f32 {
        self.mouse_speed_percent.max(10) as f32 / 100.0
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MacroTriggerMode {
    #[default]
    Press,
    Hold,
    Release,
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
    CommandPresetHotkey(u32),
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
        }
    }
}

impl Default for PinPreset {
    fn default() -> Self {
        Self::new(1)
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterWindowPresetState {
    pub id: u32,
    pub enabled: bool,
    pub animate_enabled: bool,
    pub restore_titlebar_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterWindowFocusPresetState {
    pub id: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterZoomPresetState {
    pub id: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterMacroPresetState {
    pub id: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterMacroGroupState {
    pub id: u32,
    pub enabled: bool,
    pub presets: Vec<MasterMacroPresetState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MasterPreset {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub macros_master_enabled: bool,
    pub window_expand_controls_enabled: bool,
    pub window_presets: Vec<MasterWindowPresetState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub window_focus_presets: Vec<MasterWindowFocusPresetState>,
    pub zoom_presets: Vec<MasterZoomPresetState>,
    pub macro_groups: Vec<MasterMacroGroupState>,
}

impl MasterPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Mode {id}"),
            collapsed: true,
            macros_master_enabled: true,
            window_expand_controls_enabled: false,
            window_presets: Vec::new(),
            window_focus_presets: Vec::new(),
            zoom_presets: Vec::new(),
            macro_groups: Vec::new(),
        }
    }
}

impl Default for MasterPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MacroPreset {
    pub id: u32,
    pub enabled: bool,
    pub collapsed: bool,
    pub trigger_mode: MacroTriggerMode,
    pub pass_through_press: bool,
    pub pass_through_hold: bool,
    pub stop_on_retrigger_immediate: bool,
    pub release_requires_all_inputs_released: bool,
    pub release_wait_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trigger_keys: String,
    pub hotkey: Option<HotkeyBinding>,
    pub hold_stop_step_enabled: bool,
    pub hold_stop_step: MacroStep,
    pub steps: Vec<MacroStep>,
    pub record_hotkey: Option<HotkeyBinding>,
    #[serde(skip)]
    pub acknowledged_infinite_loop: bool,
}

impl MacroPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            enabled: true,
            collapsed: true,
            trigger_mode: MacroTriggerMode::Press,
            pass_through_press: false,
            pass_through_hold: false,
            stop_on_retrigger_immediate: false,
            release_requires_all_inputs_released: false,
            release_wait_key: String::new(),
            trigger_keys: String::new(),
            hotkey: None,
            hold_stop_step_enabled: false,
            hold_stop_step: MacroStep::default(),
            steps: vec![MacroStep::default()],
            record_hotkey: None,
            acknowledged_infinite_loop: false,
        }
    }
}

impl Default for MacroPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MacroFolder {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
}

impl MacroFolder {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Folder {id}"),
            enabled: true,
            collapsed: false,
        }
    }
}

impl Default for MacroFolder {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MacroGroup {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    #[serde(default)]
    pub favorite: bool,
    pub folder_id: Option<u32>,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
    #[serde(default = "default_true")]
    pub match_duplicate_window_titles: bool,
    pub presets: Vec<MacroPreset>,
}

impl MacroGroup {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: "Macro Group".to_owned(),
            enabled: true,
            collapsed: false,
            favorite: false,
            folder_id: None,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            match_duplicate_window_titles: true,
            presets: vec![MacroPreset::new(1)],
        }
    }
}

impl Default for MacroGroup {
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
    pub show_api_key: bool,
    pub model: String,
    pub enabled: bool,
    pub details_open: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArduinoTransport {
    #[default]
    Serial,
    Hid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct VisionSettings {
    pub enabled: bool,
    pub trigger_hotkey: Option<HotkeyBinding>,
    pub click_after_move: bool,
    pub use_interception: bool,
    pub use_arduino_mouse: bool,
    pub arduino_transport: ArduinoTransport,
    pub arduino_com_port: String,
    pub arduino_vid: String,
    pub arduino_pid: String,
    pub use_arduino_spoof: bool,
}

impl Default for VisionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger_hotkey: None,
            click_after_move: false,
            use_interception: false,
            use_arduino_mouse: false,
            arduino_transport: ArduinoTransport::Serial,
            arduino_com_port: String::new(),
            arduino_vid: "0x2341".to_string(),
            arduino_pid: "0x8036".to_string(),
            use_arduino_spoof: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VisionPreset {
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
    pub click_after_move: bool,
    #[serde(default = "default_image_search_offset_px")]
    pub move_offset_x: i32,
    #[serde(default = "default_image_search_offset_px")]
    pub move_offset_y: i32,
    #[serde(default = "default_image_search_move_passes")]
    pub non_interception_move_passes: u8,
    #[serde(default = "default_image_search_move_delay_ms")]
    pub non_interception_move_delay_ms: u64,
    #[serde(default)]
    pub image_search_smooth_move: bool,
    #[serde(default = "default_image_search_distance_near_speed")]
    pub image_search_distance_near_speed: f32,
    #[serde(default = "default_image_search_distance_far_speed")]
    pub image_search_distance_far_speed: f32,
    #[serde(default = "default_image_search_confidence_threshold")]
    pub confidence_threshold: f32,
    #[serde(default)]
    pub use_color_matching: bool,
    #[serde(default)]
    pub repeat_until_triggered_again: bool,
    pub target_color: Option<RgbaColor>,
    #[serde(default)]
    pub target_colors: Vec<RgbaColor>,
    #[serde(default)]
    pub search_region_is_circle: bool,
    #[serde(default)]
    pub show_search_region_overlay: bool,
    #[serde(default)]
    pub color_priority_from_anchor: bool,
    pub color_priority_anchor_screen_x: Option<i32>,
    pub color_priority_anchor_screen_y: Option<i32>,
    #[serde(skip)]
    pub image_search_move_advanced_open: bool,
    #[serde(skip)]
    pub image_search_advanced_open: bool,
    #[serde(default = "default_image_search_color_tolerance")]
    pub color_tolerance: u8,
    #[serde(default = "default_image_search_color_scan_rate_hz")]
    pub color_scan_rate_hz: u32,
    #[serde(default)]
    pub dual_color_scan_midpoint: bool,
    #[serde(default)]
    pub require_connected_target_colors: bool,
    #[serde(default)]
    pub is_pixel_counter: bool,
    #[serde(default)]
    pub pixel_counter_variable_name: String,
    #[serde(default)]
    pub search_region_is_single_pixel: bool,
    pub last_capture_screen_x: Option<i32>,
    pub last_capture_screen_y: Option<i32>,
    pub search_region_screen_x: Option<i32>,
    pub search_region_screen_y: Option<i32>,
    pub search_region_width: Option<i32>,
    pub search_region_height: Option<i32>,
}

impl VisionPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Image Search {id}"),
            enabled: true,
            collapsed: true,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            match_duplicate_window_titles: true,
            hotkey: None,
            trigger_keys: String::new(),
            click_after_move: false,
            move_offset_x: default_image_search_offset_px(),
            move_offset_y: default_image_search_offset_px(),
            non_interception_move_passes: default_image_search_move_passes(),
            non_interception_move_delay_ms: default_image_search_move_delay_ms(),
            image_search_smooth_move: false,
            image_search_distance_near_speed: default_image_search_distance_near_speed(),
            image_search_distance_far_speed: default_image_search_distance_far_speed(),
            confidence_threshold: default_image_search_confidence_threshold(),
            use_color_matching: false,
            repeat_until_triggered_again: false,
            target_color: None,
            target_colors: Vec::new(),
            search_region_is_circle: false,
            show_search_region_overlay: false,
            color_priority_from_anchor: false,
            color_priority_anchor_screen_x: None,
            color_priority_anchor_screen_y: None,
            image_search_move_advanced_open: false,
            image_search_advanced_open: false,
            color_tolerance: default_image_search_color_tolerance(),
            color_scan_rate_hz: default_image_search_color_scan_rate_hz(),
            dual_color_scan_midpoint: false,
            require_connected_target_colors: false,
            is_pixel_counter: false,
            pixel_counter_variable_name: String::new(),
            search_region_is_single_pixel: false,
            last_capture_screen_x: None,
            last_capture_screen_y: None,
            search_region_screen_x: None,
            search_region_screen_y: None,
            search_region_width: None,
            search_region_height: None,
        }
    }
}

impl Default for VisionPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            show_api_key: false,
            system_instruction: "Convert the user's request into a JSON array of MacroNest macro steps. Each step must have at least: key, action, delay_ms. Use only supported MacroAction names. To build a toggle/bật tắt macro (alternating between two states on each trigger press), generate a 6-step pattern: Group 1 (State A): Step 1 (Action A, enabled: true), Step 3 (DisableStep for steps 1,3,4, enabled: true), Step 4 (EnableStep for steps 2,5,6, enabled: true). Group 2 (State B): Step 2 (Action B, enabled: false), Step 5 (EnableStep for steps 1,3,4, enabled: false), Step 6 (DisableStep for steps 2,5,6, enabled: false). Prefer KeyPress for taps, KeyDown/KeyUp for holds, TypeText for literal text, and MouseMoveAbsolute for exact coordinates. Return JSON only.".to_owned(),
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
            model: "openai/gpt-oss-120b".to_owned(),
            enabled: false,
            details_open: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioClipSettings {
    pub enabled: bool,
    pub file_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub volume: f32,
    pub speed: f32,
}

impl Default for AudioClipSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            file_path: String::new(),
            start_ms: 0,
            end_ms: 0,
            volume: 1.0,
            speed: 1.0,
        }
    }
}

fn default_video_resolution() -> String {
    "Auto".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VideoClipSettings {
    pub enabled: bool,
    pub file_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub chroma_key_enabled: bool,
    pub chroma_key_color: RgbaColor,
    pub chroma_key_tolerance: u8,
    #[serde(default = "default_video_resolution")]
    pub resolution: String,
}

impl Default for VideoClipSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            file_path: String::new(),
            start_ms: 0,
            end_ms: 0,
            chroma_key_enabled: true,
            chroma_key_color: RgbaColor {
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            },
            chroma_key_tolerance: 36,
            resolution: "Auto".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioSettings {
    pub startup: AudioClipSettings,
    pub exit: AudioClipSettings,
    pub library: Vec<SoundLibraryItem>,
    pub next_library_item_id: u32,
    pub presets: Vec<SoundPreset>,
    pub next_preset_id: u32,
    pub video_presets: Vec<VideoPreset>,
    pub next_video_preset_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SoundLibraryItem {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub clip: AudioClipSettings,
}

impl SoundLibraryItem {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Library Sound {id}"),
            collapsed: true,
            clip: AudioClipSettings::default(),
        }
    }
}

impl Default for SoundLibraryItem {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SoundPreset {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub clip: AudioClipSettings,
    pub sequence_library_ids: Vec<u32>,
}

impl SoundPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Sound {id}"),
            collapsed: true,
            clip: AudioClipSettings {
                enabled: true,
                ..AudioClipSettings::default()
            },
            sequence_library_ids: Vec::new(),
        }
    }
}

impl Default for SoundPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VideoPreset {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub clip: VideoClipSettings,
}

impl VideoPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Video {id}"),
            collapsed: true,
            clip: VideoClipSettings {
                enabled: true,
                ..VideoClipSettings::default()
            },
        }
    }
}

impl Default for VideoPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OcrPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub preview_enabled: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub lang: Option<String>,
    pub target_text: String,
    pub success_var: String,
    pub pos_var_x: String,
    pub pos_var_y: String,
    pub numeric_var: String,
}

impl OcrPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("OCR {id}"),
            enabled: true,
            collapsed: true,
            preview_enabled: false,
            x: 0,
            y: 0,
            width: 320,
            height: 180,
            lang: None,
            target_text: String::new(),
            success_var: String::new(),
            pos_var_x: String::new(),
            pos_var_y: String::new(),
            numeric_var: String::new(),
        }
    }
}

impl Default for OcrPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            startup: AudioClipSettings::default(),
            exit: AudioClipSettings::default(),
            library: Vec::new(),
            next_library_item_id: 1,
            presets: Vec::new(),
            next_preset_id: 1,
            video_presets: Vec::new(),
            next_video_preset_id: 1,
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
    #[serde(default)]
    pub next_geometry_object_id: u32,
    pub macros_master_enabled: bool,
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
    #[serde(default)]
    pub ocr_test_lang: Option<String>,
    #[serde(skip)]
    pub ocr_test_running: bool,
    #[serde(skip)]
    pub ocr_test_error: Option<String>,
    #[serde(skip)]
    pub ocr_test_result: Option<crate::ocr::OcrResult>,
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
            next_geometry_object_id: 1,
            macros_master_enabled: true,
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
            macro_mouse_click_delay_ms: 16,
            macro_keyboard_key_press_delay_ms: 0,
            global_constants: Vec::new(),
            ocr_presets: Vec::new(),
            next_ocr_preset_id: 1,
            ocr_test_x: 0,
            ocr_test_y: 0,
            ocr_test_width: 320,
            ocr_test_height: 180,
            ocr_test_lang: None,
            ocr_test_running: false,
            ocr_test_error: None,
            ocr_test_result: None,
        }
    }
}
