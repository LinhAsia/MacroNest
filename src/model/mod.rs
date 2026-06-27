mod audio_model;
mod geometry_model;
mod macro_model;
mod overlay_model;
mod settings_model;
mod vision_model;
mod window_model;

pub use audio_model::*;
pub use geometry_model::*;
pub use macro_model::*;
pub use overlay_model::*;
pub use settings_model::*;
pub use vision_model::*;
pub use window_model::*;

pub const DEFAULT_CROSSHAIR_X_OFFSET: i32 = 960;
pub const DEFAULT_CROSSHAIR_Y_OFFSET: i32 = 540;

fn default_x_offset() -> i32 {
    DEFAULT_CROSSHAIR_X_OFFSET
}

fn default_y_offset() -> i32 {
    DEFAULT_CROSSHAIR_Y_OFFSET
}

fn default_crosshair_length() -> f32 {
    10.0
}

fn default_custom_pixels_grid_size() -> u8 {
    15
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

fn default_hud_border_color() -> RgbaColor {
    RgbaColor {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    }
}

fn default_hud_border_thickness() -> f32 {
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

fn default_binary_threshold() -> u8 {
    128
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

fn default_focus_highlight_color() -> RgbaColor {
    RgbaColor {
        r: 126,
        g: 224,
        b: 182,
        a: 235,
    }
}

fn default_protractor_scale() -> f32 {
    1.0
}

fn default_protractor_needle1_angle() -> f32 {
    0.0
}

fn default_protractor_needle2_angle() -> f32 {
    90.0
}

fn default_protractor_center_x() -> i32 {
    500
}

fn default_protractor_center_y() -> i32 {
    500
}

fn default_protractor_thickness() -> f32 {
    2.0
}

fn default_quick_key_display_x() -> i32 {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN};
        GetSystemMetrics(SM_CXSCREEN).max(1) / 2
    }
    #[cfg(not(windows))]
    960
}

fn default_quick_key_display_y() -> i32 {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CYSCREEN};
        GetSystemMetrics(SM_CYSCREEN).max(1) / 2
    }
    #[cfg(not(windows))]
    540
}

fn default_quick_key_display_size() -> f32 {
    36.0
}

fn default_screen_draw_color() -> RgbaColor {
    RgbaColor {
        r: 0,
        g: 255,
        b: 170,
        a: 255,
    }
}

fn default_screen_draw_brush_size() -> f32 {
    10.0
}

fn default_screen_draw_smoothing_amount() -> f32 {
    0.45
}

fn default_key_sound_volume() -> f32 {
    1.0
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

fn default_geometry_stroke_color_expr() -> String {
    "#00FFAA".to_owned()
}

fn default_geometry_fill_color_expr() -> String {
    "#00FFAA".to_owned()
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
    0
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

fn default_ocr_language_code() -> String {
    crate::ocr::OCR_DEFAULT_CODE.to_owned()
}

fn default_macro_step_ocr_language() -> String {
    crate::ocr::OCR_DEFAULT_CODE.to_owned()
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

fn default_binary_target_color() -> RgbaColor {
    RgbaColor {
        r: 255,
        g: 255,
        b: 0,
        a: 255,
    }
}

fn default_binary_target_color_option() -> Option<RgbaColor> {
    Some(default_binary_target_color())
}
