use serde::{Deserialize, Serialize};

use super::overlay_model::RgbaColor;
use super::window_model::HotkeyBinding;
use super::{
    default_image_search_color_scan_rate_hz, default_image_search_color_tolerance,
    default_image_search_confidence_threshold, default_image_search_distance_far_speed,
    default_image_search_distance_near_speed, default_image_search_move_delay_ms,
    default_image_search_move_passes, default_image_search_offset_px, default_true,
};

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
    pub color_scan_average_centroid: bool,
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
            color_scan_average_centroid: false,
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
