use serde::{Deserialize, Serialize};

use super::{
    default_crosshair_length, default_custom_pixels_grid_size, default_true, default_x_offset,
    default_y_offset,
};

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
    #[serde(default = "default_x_offset")]
    pub x_offset: i32,
    #[serde(default = "default_y_offset")]
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
    #[serde(default)]
    pub custom_pixels: Option<String>,
    #[serde(default = "default_custom_pixels_grid_size")]
    pub custom_pixels_grid_size: u8,
}

impl Default for CrosshairStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            x_offset: default_x_offset(),
            y_offset: default_y_offset(),
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
            custom_pixels: None,
            custom_pixels_grid_size: default_custom_pixels_grid_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
