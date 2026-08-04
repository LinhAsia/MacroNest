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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EspOrientationSource {
    #[default]
    Angles,
    ForwardVector,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EspForwardLayout {
    #[default]
    Xyz,
    Xzy,
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
    pub orientation_source: EspOrientationSource,
    pub camera_forward_x: String,
    pub camera_forward_y: String,
    pub camera_forward_z: String,
    pub forward_layout: EspForwardLayout,
    pub swap_forward_horizontal: bool,
    pub invert_forward_x: bool,
    pub invert_forward_y: bool,
    pub invert_forward_z: bool,
    pub value_type: MemoryValueType,
    pub yaw_unit: EspAngleUnit,
    pub pitch_unit: EspAngleUnit,
    pub horizontal_plane: EspHorizontalPlane,
    pub invert_camera_yaw: bool,
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
            orientation_source: EspOrientationSource::Angles,
            camera_forward_x: String::new(),
            camera_forward_y: String::new(),
            camera_forward_z: String::new(),
            forward_layout: EspForwardLayout::Xyz,
            swap_forward_horizontal: false,
            invert_forward_x: false,
            invert_forward_y: false,
            invert_forward_z: false,
            value_type: MemoryValueType::F32,
            yaw_unit: EspAngleUnit::Degrees,
            pitch_unit: EspAngleUnit::Radians,
            horizontal_plane: EspHorizontalPlane::Xy,
            invert_camera_yaw: false,
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

#[derive(Debug, Clone, Copy)]
pub struct EspCalibrationSample {
    pub bearing_yaw: f32,
    pub bearing_pitch: f32,
    pub camera_yaw: f32,
    pub camera_pitch: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct EspCalibrationResult {
    pub invert_camera_yaw: bool,
    pub invert_yaw: bool,
    pub yaw_offset_degrees: f32,
    pub invert_pitch: bool,
    pub pitch_offset_degrees: f32,
    pub yaw_error_degrees: f32,
    pub pitch_error_degrees: f32,
}

fn esp_angle_to_radians(value: f32, unit: EspAngleUnit) -> f32 {
    match unit {
        EspAngleUnit::Degrees => value.to_radians(),
        EspAngleUnit::Radians => value,
    }
}

fn esp_angle_from_radians(value: f32, unit: EspAngleUnit) -> f32 {
    match unit {
        EspAngleUnit::Degrees => value.to_degrees(),
        EspAngleUnit::Radians => value,
    }
}

pub(crate) fn esp_orientation_from_forward(
    preset: &EspPreset,
    mut forward: [f32; 3],
) -> Option<(f32, f32)> {
    if preset.invert_forward_x {
        forward[0] = -forward[0];
    }
    if preset.invert_forward_y {
        forward[1] = -forward[1];
    }
    if preset.invert_forward_z {
        forward[2] = -forward[2];
    }
    let (mut forward_a, mut forward_b, vertical) = match preset.forward_layout {
        EspForwardLayout::Xyz => (forward[0], forward[1], forward[2]),
        EspForwardLayout::Xzy => (forward[0], forward[2], forward[1]),
    };
    if preset.swap_forward_horizontal {
        std::mem::swap(&mut forward_a, &mut forward_b);
    }
    let horizontal = forward_a.hypot(forward_b);
    let length = horizontal.hypot(vertical);
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    Some((
        esp_angle_from_radians(forward_b.atan2(forward_a), preset.yaw_unit),
        esp_angle_from_radians(vertical.atan2(horizontal), preset.pitch_unit),
    ))
}

fn wrap_angle(value: f32) -> f32 {
    (value + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

pub(crate) fn esp_calibration_sample(
    preset: &EspPreset,
    target: [f32; 3],
    camera: [f32; 3],
    yaw: f32,
    pitch: f32,
) -> Option<EspCalibrationSample> {
    let dx = target[0] - camera[0];
    let dy = target[1] - camera[1];
    let dz = target[2] - camera[2];
    let (forward_a, forward_b, vertical) = match preset.horizontal_plane {
        EspHorizontalPlane::Xy => (dx, dy, dz + preset.target_vertical_offset),
        EspHorizontalPlane::Xz => (dx, dz, dy + preset.target_vertical_offset),
    };
    let horizontal_distance = forward_a.hypot(forward_b);
    (horizontal_distance > f32::EPSILON).then_some(EspCalibrationSample {
        bearing_yaw: forward_b.atan2(forward_a),
        bearing_pitch: vertical.atan2(horizontal_distance),
        camera_yaw: esp_angle_to_radians(yaw, preset.yaw_unit),
        camera_pitch: esp_angle_to_radians(pitch, preset.pitch_unit),
    })
}

pub(crate) fn solve_esp_calibration(
    samples: &[EspCalibrationSample],
    current_invert_camera_yaw: bool,
    current_invert_yaw: bool,
    current_invert_pitch: bool,
) -> Option<EspCalibrationResult> {
    if samples.len() < 2 {
        return None;
    }
    let solve_wrapped = |sign: f32| {
        let (sin_sum, cos_sum) = samples.iter().fold((0.0, 0.0), |(sin, cos), sample| {
            let offset = wrap_angle(sample.bearing_yaw - sign * sample.camera_yaw);
            (sin + offset.sin(), cos + offset.cos())
        });
        let offset = sin_sum.atan2(cos_sum);
        let error = (samples
            .iter()
            .map(|sample| {
                wrap_angle(sample.bearing_yaw - sign * sample.camera_yaw - offset).powi(2)
            })
            .sum::<f32>()
            / samples.len() as f32)
            .sqrt();
        (offset, error)
    };
    let normal_yaw = solve_wrapped(1.0);
    let inverted_yaw = solve_wrapped(-1.0);
    let yaw_tie = (normal_yaw.1 - inverted_yaw.1).abs() < 0.001;
    let (invert_camera_yaw, (yaw_offset, yaw_error)) = if yaw_tie {
        if current_invert_camera_yaw {
            (true, inverted_yaw)
        } else {
            (false, normal_yaw)
        }
    } else if inverted_yaw.1 < normal_yaw.1 {
        (true, inverted_yaw)
    } else {
        (false, normal_yaw)
    };

    let solve_pitch = |sign: f32| {
        let offset = samples
            .iter()
            .map(|sample| sample.bearing_pitch - sign * sample.camera_pitch)
            .sum::<f32>()
            / samples.len() as f32;
        let error = (samples
            .iter()
            .map(|sample| (sample.bearing_pitch - sign * sample.camera_pitch - offset).powi(2))
            .sum::<f32>()
            / samples.len() as f32)
            .sqrt();
        (offset, error)
    };
    let (pitch_offset, pitch_error) = solve_pitch(1.0);

    Some(EspCalibrationResult {
        invert_camera_yaw,
        // Centered calibration samples determine angular zero, not which side of the screen is
        // positive. Preserve the explicit screen-axis choices instead of guessing them.
        invert_yaw: current_invert_yaw,
        yaw_offset_degrees: wrap_angle(yaw_offset).to_degrees(),
        invert_pitch: current_invert_pitch,
        pitch_offset_degrees: pitch_offset.to_degrees(),
        yaw_error_degrees: yaw_error.to_degrees(),
        pitch_error_degrees: pitch_error.to_degrees(),
    })
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
    let mut yaw = esp_angle_to_radians(yaw, preset.yaw_unit);
    let mut pitch = esp_angle_to_radians(pitch, preset.pitch_unit);
    if preset.invert_camera_yaw {
        yaw = -yaw;
    }
    yaw += preset.yaw_offset_degrees.to_radians();
    pitch += preset.pitch_offset_degrees.to_radians();
    let mut yaw_delta = forward_b.atan2(forward_a) - yaw;
    yaw_delta = wrap_angle(yaw_delta);
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

    #[test]
    fn inverted_yaw_flips_screen_side_without_moving_the_center() {
        let mut preset = EspPreset::default();
        let normal_side = project_esp_normalized(
            &preset,
            [10.0, 5.0, 0.0],
            [0.0, 0.0, 0.0],
            0.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        preset.invert_yaw = true;
        let inverted_side = project_esp_normalized(
            &preset,
            [10.0, 5.0, 0.0],
            [0.0, 0.0, 0.0],
            0.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        assert!((normal_side.0 + inverted_side.0).abs() < 0.001);

        let projected = project_esp_normalized(
            &preset,
            [10.0, 10.0, 0.0],
            [0.0, 0.0, 0.0],
            45.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        assert!(projected.0.abs() < 0.001 && projected.1.abs() < 0.001);
    }

    #[test]
    fn inverted_camera_yaw_only_changes_rotation_direction() {
        let mut preset = EspPreset::default();
        preset.invert_camera_yaw = true;
        let projected = project_esp_normalized(
            &preset,
            [10.0, 10.0, 0.0],
            [0.0, 0.0, 0.0],
            -45.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        assert!(projected.0.abs() < 0.001);
    }

    #[test]
    fn four_direction_calibration_recovers_offset_and_preserves_screen_axis() {
        let samples = [0.0_f32, 90.0, 180.0, -90.0].map(|bearing| {
            let bearing = bearing.to_radians();
            EspCalibrationSample {
                bearing_yaw: bearing,
                bearing_pitch: 0.0,
                camera_yaw: bearing - 30.0_f32.to_radians(),
                camera_pitch: 0.0,
            }
        });
        let result = solve_esp_calibration(&samples, false, true, false).unwrap();
        assert!(!result.invert_camera_yaw);
        assert!(result.invert_yaw);
        assert!((result.yaw_offset_degrees - 30.0).abs() < 0.01);
        assert!(result.yaw_error_degrees < 0.01);
    }

    #[test]
    fn forward_vector_reconstructs_yaw_and_pitch() {
        let mut preset = EspPreset::default();
        preset.yaw_unit = EspAngleUnit::Radians;
        preset.pitch_unit = EspAngleUnit::Radians;
        let (yaw, pitch) = esp_orientation_from_forward(&preset, [1.0, 1.0, 1.0]).unwrap();
        assert!((yaw - std::f32::consts::FRAC_PI_4).abs() < 0.001);
        assert!((pitch - (1.0_f32 / 2.0_f32.sqrt()).atan()).abs() < 0.001);

        preset.forward_layout = EspForwardLayout::Xzy;
        preset.swap_forward_horizontal = true;
        preset.invert_forward_x = true;
        let (yaw, _) = esp_orientation_from_forward(&preset, [1.0, 0.0, 0.0]).unwrap();
        assert!((yaw + std::f32::consts::FRAC_PI_2).abs() < 0.001);
    }
}
