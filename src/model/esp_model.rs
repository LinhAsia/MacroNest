use serde::{Deserialize, Serialize};

use super::{MemoryValueType, RgbaColor};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EspMarkerKind {
    #[default]
    Dot,
    Box,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EspAngleUnit {
    #[default]
    Degrees,
    Radians,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EspHorizontalPlane {
    #[default]
    Xy,
    Xz,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EspPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub target_window: String,
    pub target_x: String,
    pub target_y: String,
    pub target_z: String,
    pub camera_x: String,
    pub camera_y: String,
    pub camera_z: String,
    pub camera_yaw: String,
    pub camera_pitch: String,
    pub value_type: MemoryValueType,
    pub yaw_unit: EspAngleUnit,
    pub pitch_unit: EspAngleUnit,
    pub horizontal_plane: EspHorizontalPlane,
    pub invert_yaw: bool,
    pub invert_pitch: bool,
    pub yaw_offset_degrees: f32,
    pub pitch_offset_degrees: f32,
    pub target_vertical_offset: f32,
    pub screen_offset_x: f32,
    pub screen_offset_y: f32,
    pub horizontal_fov: f32,
    pub marker: EspMarkerKind,
    pub dot_radius: f32,
    pub box_width: f32,
    pub box_height: f32,
    pub thickness: f32,
    pub filled: bool,
    pub color: RgbaColor,
    pub show_tracer: bool,
    pub show_distance: bool,
    pub update_interval_ms: u32,
}

impl EspPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("ESP {id}"),
            enabled: false,
            collapsed: false,
            target_window: String::new(),
            target_x: String::new(),
            target_y: String::new(),
            target_z: String::new(),
            camera_x: String::new(),
            camera_y: String::new(),
            camera_z: String::new(),
            camera_yaw: String::new(),
            camera_pitch: String::new(),
            value_type: MemoryValueType::F32,
            yaw_unit: EspAngleUnit::Degrees,
            pitch_unit: EspAngleUnit::Radians,
            horizontal_plane: EspHorizontalPlane::Xy,
            invert_yaw: false,
            invert_pitch: false,
            yaw_offset_degrees: 0.0,
            pitch_offset_degrees: 0.0,
            target_vertical_offset: 0.0,
            screen_offset_x: 0.0,
            screen_offset_y: 0.0,
            horizontal_fov: 90.0,
            marker: EspMarkerKind::Dot,
            dot_radius: 7.0,
            box_width: 44.0,
            box_height: 88.0,
            thickness: 2.0,
            filled: false,
            color: RgbaColor {
                r: 0,
                g: 255,
                b: 170,
                a: 255,
            },
            show_tracer: false,
            show_distance: false,
            update_interval_ms: 33,
        }
    }
}

impl Default for EspPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

/// Projects a world position into normalized screen coordinates (-1..=1).
pub(crate) fn project_esp_normalized(
    preset: &EspPreset,
    target: [f32; 3],
    camera: [f32; 3],
    yaw: f32,
    pitch: f32,
    aspect: f32,
) -> Option<(f32, f32, f32)> {
    let angle = |value: f32, unit: EspAngleUnit| match unit {
        EspAngleUnit::Degrees => value.to_radians(),
        EspAngleUnit::Radians => value,
    };
    let dx = target[0] - camera[0];
    let dy = target[1] - camera[1];
    let dz = target[2] - camera[2];
    let (forward_a, forward_b, vertical) = match preset.horizontal_plane {
        EspHorizontalPlane::Xy => (dx, dy, dz + preset.target_vertical_offset),
        EspHorizontalPlane::Xz => (dx, dz, dy + preset.target_vertical_offset),
    };
    let horizontal_distance = forward_a.hypot(forward_b);
    let distance = horizontal_distance.hypot(vertical);
    if distance <= f32::EPSILON {
        return None;
    }
    let yaw = angle(yaw, preset.yaw_unit) + preset.yaw_offset_degrees.to_radians();
    let pitch = angle(pitch, preset.pitch_unit) + preset.pitch_offset_degrees.to_radians();
    let mut yaw_delta = forward_b.atan2(forward_a) - yaw;
    yaw_delta =
        (yaw_delta + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    let mut pitch_delta = vertical.atan2(horizontal_distance) - pitch;
    if preset.invert_yaw {
        yaw_delta = -yaw_delta;
    }
    if preset.invert_pitch {
        pitch_delta = -pitch_delta;
    }
    let half_fov_x = (preset.horizontal_fov.clamp(1.0, 179.0).to_radians() * 0.5).max(0.001);
    let half_fov_y = (half_fov_x.tan() / aspect.max(0.01)).atan();
    let x = yaw_delta.tan() / half_fov_x.tan();
    let y = pitch_delta.tan() / half_fov_y.tan();
    (yaw_delta.abs() < half_fov_x && pitch_delta.abs() < half_fov_y).then_some((x, y, distance))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_target_projects_to_screen_center() {
        let preset = EspPreset::default();
        let projected = project_esp_normalized(
            &preset,
            [10.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            0.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        assert!(projected.0.abs() < 0.001 && projected.1.abs() < 0.001);
    }

    #[test]
    fn yaw_offset_calibrates_a_different_game_zero_direction() {
        let mut preset = EspPreset::default();
        preset.yaw_offset_degrees = 90.0;
        let projected = project_esp_normalized(
            &preset,
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 0.0],
            0.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        assert!(projected.0.abs() < 0.001 && projected.1.abs() < 0.001);
    }
}
