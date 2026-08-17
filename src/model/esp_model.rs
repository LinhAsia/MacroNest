use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use super::{MemoryValueType, RgbaColor};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EspMarkerKind {
    #[default]
    Dot,
    Box,
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EspMarkerSource {
    #[default]
    Geometry,
    Text,
    Svg,
    Image,
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EspAngleUnit {
    #[default]
    Degrees,
    Radians,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EspPitchInput {
    #[default]
    Angle,
    SineComponent,
    TangentComponent,
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
    #[serde(alias = "ForwardVector")]
    DirectionPairPitch,
}

fn default_entity_auto_hit_step() -> u32 {
    1
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
    pub entity_list_enabled: bool,
    pub entity_root: String,
    pub entity_x_offset: i64,
    pub entity_y_offset: i64,
    pub entity_z_offset: i64,
    pub entity_stride: u32,
    pub entity_root_step: Option<u32>,
    pub entity_root_step_multiplier: u32,
    pub entity_count: u32,
    pub entity_auto_code_module: String,
    pub entity_auto_code_offset: usize,
    pub entity_auto_capture_count: u32,
    pub entity_auto_hit_order: bool,
    #[serde(default = "default_entity_auto_hit_step")]
    pub entity_auto_hit_step: u32,
    #[serde(default)]
    pub entity_hit_order_drop_nearest: bool,
    pub entity_hit_order_addresses: Vec<usize>,
    pub entity_auto_hud_enabled: bool,
    pub entity_auto_hud_preset_id: Option<u32>,
    pub entity_aabb_center: bool,
    pub entity_aabb_pair_offset: i64,
    #[serde(default)]
    pub custom_entity_colors: HashMap<u32, RgbaColor>,
    pub camera_x: String,
    pub camera_y: String,
    pub camera_z: String,
    pub camera_yaw: String,
    pub camera_pitch: String,
    pub orientation_source: EspOrientationSource,
    #[serde(alias = "camera_forward_x")]
    pub camera_direction_a: String,
    #[serde(alias = "camera_forward_z")]
    pub camera_direction_b: String,
    pub direction_multiplier: f32,
    #[serde(alias = "swap_forward_horizontal")]
    pub swap_direction_pair: bool,
    #[serde(alias = "invert_forward_x")]
    pub invert_direction_a: bool,
    #[serde(alias = "invert_forward_z")]
    pub invert_direction_b: bool,
    pub value_type: MemoryValueType,
    pub yaw_unit: EspAngleUnit,
    pub pitch_unit: EspAngleUnit,
    pub pitch_input: EspPitchInput,
    pub pitch_multiplier: f32,
    pub horizontal_plane: EspHorizontalPlane,
    pub invert_camera_yaw: bool,
    pub invert_camera_pitch: bool,
    pub invert_vertical: bool,
    pub invert_yaw: bool,
    pub invert_pitch: bool,
    pub yaw_offset_degrees: f32,
    pub pitch_offset_degrees: f32,
    pub target_vertical_offset: f32,
    pub height_scale: f32,
    pub screen_offset_x: f32,
    pub screen_offset_y: f32,
    pub horizontal_fov: f32,
    pub vertical_projection_multiplier: f32,
    pub marker_source: EspMarkerSource,
    pub marker: EspMarkerKind,
    /// Image file path. Kept under its original key for saved-preset compatibility.
    pub marker_asset_path: String,
    /// Inline SVG or an SVG file path. Separate from image paths so changing marker type is lossless.
    pub marker_svg_source: String,
    pub marker_text: String,
    pub text_offset_x: f32,
    pub text_offset_y: f32,
    pub text_font_size: f32,
    pub text_opacity: f32,
    pub scale_with_distance: bool,
    pub distance_reference: f32,
    pub distance_scale_strength_percent: f32,
    pub marker_size_offset_percent: f32,
    pub marker_billboard_3d: bool,
    pub marker_offset_x: f32,
    pub marker_offset_y: f32,
    pub dot_radius: f32,
    pub box_width: f32,
    pub box_height: f32,
    pub svg_width: f32,
    pub svg_height: f32,
    pub image_width: f32,
    pub image_height: f32,
    pub thickness: f32,
    pub filled: bool,
    pub color: RgbaColor,
    pub show_tracer: bool,
    pub show_distance: bool,
    pub debug_mode: bool,
    pub target_audio_enabled: bool,
    pub target_audio_path: String,
    pub target_audio_loop: bool,
    pub target_audio_volume: f32,
    pub target_audio_full_volume_distance: f32,
    pub target_audio_max_distance: f32,
    pub update_interval_ms: u32,
    pub motion_smoothing_ms: u32,
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
            entity_list_enabled: false,
            entity_root: String::new(),
            entity_x_offset: 0,
            entity_y_offset: 4,
            entity_z_offset: 8,
            entity_stride: 0x48,
            entity_root_step: None,
            entity_root_step_multiplier: 1,
            entity_count: 32,
            entity_auto_code_module: String::new(),
            entity_auto_code_offset: 0,
            entity_auto_capture_count: 5,
            entity_auto_hit_order: false,
            entity_auto_hit_step: 1,
            entity_hit_order_drop_nearest: false,
            entity_hit_order_addresses: Vec::new(),
            entity_auto_hud_enabled: false,
            entity_auto_hud_preset_id: None,
            entity_aabb_center: false,
            entity_aabb_pair_offset: 0x0C,
            custom_entity_colors: HashMap::new(),
            camera_x: String::new(),
            camera_y: String::new(),
            camera_z: String::new(),
            camera_yaw: String::new(),
            camera_pitch: String::new(),
            orientation_source: EspOrientationSource::Angles,
            camera_direction_a: String::new(),
            camera_direction_b: String::new(),
            direction_multiplier: 1.0,
            swap_direction_pair: false,
            invert_direction_a: false,
            invert_direction_b: false,
            value_type: MemoryValueType::F32,
            yaw_unit: EspAngleUnit::Degrees,
            pitch_unit: EspAngleUnit::Radians,
            pitch_input: EspPitchInput::Angle,
            pitch_multiplier: 1.0,
            horizontal_plane: EspHorizontalPlane::Xz,
            invert_camera_yaw: false,
            invert_camera_pitch: false,
            invert_vertical: false,
            invert_yaw: false,
            invert_pitch: false,
            yaw_offset_degrees: 0.0,
            pitch_offset_degrees: 0.0,
            target_vertical_offset: 0.0,
            height_scale: 1.0,
            screen_offset_x: 0.0,
            screen_offset_y: 0.0,
            horizontal_fov: 90.0,
            vertical_projection_multiplier: 1.0,
            marker_source: EspMarkerSource::Geometry,
            marker: EspMarkerKind::Dot,
            marker_asset_path: String::new(),
            marker_svg_source: String::new(),
            marker_text: "Target".to_owned(),
            text_offset_x: 0.0,
            text_offset_y: 0.0,
            text_font_size: 18.0,
            text_opacity: 1.0,
            scale_with_distance: false,
            distance_reference: 100.0,
            distance_scale_strength_percent: 100.0,
            marker_size_offset_percent: 0.0,
            marker_billboard_3d: false,
            marker_offset_x: 0.0,
            marker_offset_y: 0.0,
            dot_radius: 7.0,
            box_width: 44.0,
            box_height: 88.0,
            svg_width: 44.0,
            svg_height: 88.0,
            image_width: 44.0,
            image_height: 88.0,
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
            debug_mode: false,
            target_audio_enabled: false,
            target_audio_path: String::new(),
            target_audio_loop: true,
            target_audio_volume: 1.0,
            target_audio_full_volume_distance: 5.0,
            target_audio_max_distance: 500.0,
            update_interval_ms: 33,
            motion_smoothing_ms: 40,
        }
    }

    pub fn migrate_marker_sources(&mut self) -> bool {
        if self.marker_source == EspMarkerSource::Svg
            && self.marker_svg_source.trim().is_empty()
            && !self.marker_asset_path.trim().is_empty()
        {
            self.marker_svg_source = std::mem::take(&mut self.marker_asset_path);
            return true;
        }
        false
    }
}

pub(crate) fn entity_field_address(
    root: usize,
    index: u32,
    stride: u32,
    offset: i64,
) -> Option<usize> {
    let entity = root.checked_add((index as usize).checked_mul(stride as usize)?)?;
    if offset >= 0 {
        entity.checked_add(offset as usize)
    } else {
        entity.checked_sub(offset.unsigned_abs() as usize)
    }
}

pub(crate) fn aabb_center_component(first: f32, second: f32) -> f32 {
    first + (second - first) * 0.5
}

pub(crate) fn shift_raw_entity_root(text: &str, stride: u32, slots: i32) -> Option<String> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let address = usize::from_str_radix(digits, 16).ok()?;
    let offset = i64::from(stride).checked_mul(i64::from(slots))?;
    let address = entity_field_address(address, 0, 1, offset)?;
    Some(format!("0x{address:X}"))
}

pub(crate) fn entity_root_from_instruction_hits(
    hits: &[usize],
    required: u32,
    stride: u32,
) -> Result<usize, &'static str> {
    let (candidate, matched) = entity_instruction_hit_progress(hits, required, stride);
    if matched >= required.max(1) as usize {
        candidate.ok_or("no complete group matches the configured Stride")
    } else {
        Err("no complete group matches the configured Stride")
    }
}

pub(crate) fn entity_instruction_hit_progress(
    hits: &[usize],
    required: u32,
    stride: u32,
) -> (Option<usize>, usize) {
    let required = required.max(1) as usize;
    let stride = stride.max(1) as usize;
    let min_stride_consensus = 3.min(required);
    let mut hits = hits.to_vec();
    hits.sort_unstable();
    hits.dedup();
    let mut best = 0;
    let mut best_start = None;
    for &start in &hits {
        let mut matched = 1;
        while matched < required {
            let Some(address) = stride
                .checked_mul(matched)
                .and_then(|offset| start.checked_add(offset))
            else {
                break;
            };
            if hits.binary_search(&address).is_err() {
                break;
            }
            matched += 1;
        }
        if matched > best {
            best = matched;
            best_start = Some(start);
        }
        if matched == required {
            return (Some(start), required);
        }
    }
    let candidate = if best >= min_stride_consensus {
        best_start
    } else {
        None
    };
    (candidate, best.min(required))
}

pub(crate) fn entity_hits_in_capture_order(
    hits: &[usize],
    required: u32,
    hit_step: u32,
) -> Option<Vec<usize>> {
    let (selected, _) = entity_hits_in_capture_order_progress(hits, required, hit_step);
    if selected.len() >= required.max(1) as usize {
        Some(selected)
    } else {
        None
    }
}

pub(crate) fn entity_hits_in_capture_order_progress(
    hits: &[usize],
    required: u32,
    hit_step: u32,
) -> (Vec<usize>, usize) {
    let required = required.max(1) as usize;
    let hit_step = hit_step.max(1) as usize;
    if hit_step == 1 {
        let mut raw_unique = Vec::with_capacity(hits.len());
        for &address in hits {
            if address != 0 && !raw_unique.contains(&address) {
                raw_unique.push(address);
            }
        }
        let count = raw_unique.len().min(required);
        let selected = raw_unique.into_iter().take(required).collect::<Vec<_>>();
        return (selected, count);
    }

    // When hit_step >= 2 (paired min/max hits per entity, e.g. AABB pair at delta 12 bytes):
    // Group addresses that are within 16 bytes of each other as the same entity,
    // and always record the minimum address (min AABB) as the entity base.
    const PAIR_DELTA_THRESHOLD: usize = 16;
    let mut entities: Vec<usize> = Vec::new();
    for &address in hits {
        if address == 0 {
            continue;
        }
        let mut found = false;
        for base in entities.iter_mut() {
            let diff = if address >= *base {
                address - *base
            } else {
                *base - address
            };
            if diff <= PAIR_DELTA_THRESHOLD {
                if address < *base {
                    *base = address;
                }
                found = true;
                break;
            }
        }
        if !found {
            entities.push(address);
        }
    }
    let count = entities.len().min(required);
    let selected = entities.into_iter().take(required).collect::<Vec<_>>();
    (selected, count)
}

#[cfg(test)]
mod entity_address_tests {
    use super::{
        aabb_center_component, entity_field_address, entity_hits_in_capture_order,
        entity_instruction_hit_progress, entity_root_from_instruction_hits, shift_raw_entity_root,
    };

    #[test]
    fn calculates_positive_and_negative_entity_fields() {
        assert_eq!(entity_field_address(0x1000, 2, 0x48, 8), Some(0x1098));
        assert_eq!(entity_field_address(0x1000, 1, 0x48, -8), Some(0x1040));
        assert_eq!(entity_field_address(4, 0, 1, -8), None);
    }

    #[test]
    fn calculates_aabb_center_component() {
        assert_eq!(aabb_center_component(184.0, 194.0), 189.0);
        assert_eq!(aabb_center_component(-8.0, 2.0), -3.0);
    }

    #[test]
    fn shifts_raw_entity_root_text_by_whole_strides() {
        assert_eq!(
            shift_raw_entity_root("0x1000", 0x18, 2).as_deref(),
            Some("0x1030")
        );
        assert_eq!(
            shift_raw_entity_root("1000", 0x18, -2).as_deref(),
            Some("0xFD0")
        );
        assert_eq!(shift_raw_entity_root("game.exe+1000", 0x18, 1), None);
        assert_eq!(
            shift_raw_entity_root("0x1F009EAF340", 0x370, 1).as_deref(),
            Some("0x1F009EAF6B0")
        );
    }

    #[test]
    fn instruction_hits_resolve_the_lowest_evenly_spaced_entity() {
        assert_eq!(
            entity_root_from_instruction_hits(
                &[0x5001, 0x1090, 0x1030, 0x7777, 0x1070, 0x1010, 0x1050],
                5,
                0x20,
            ),
            Ok(0x1010)
        );
        assert!(entity_root_from_instruction_hits(&[0x1010, 0x1030, 0x1080], 3, 0x20).is_err());
        assert_eq!(
            entity_instruction_hit_progress(&[0x9001, 0x1010, 0x1050, 0x1030], 5, 0x20),
            (Some(0x1010), 3)
        );
        assert_eq!(
            entity_instruction_hit_progress(&[0x9001, 0x1010, 0x1030], 5, 0x20),
            (None, 2)
        );
    }

    #[test]
    fn instruction_hits_keep_first_seen_order_without_stride_grouping() {
        assert_eq!(
            entity_hits_in_capture_order(&[0x3000, 0x1000, 0x3000, 0x2200], 3, 1),
            Some(vec![0x3000, 0x1000, 0x2200])
        );
        assert_eq!(
            entity_hits_in_capture_order(
                &[0x100C, 0x1000, 0x200C, 0x2000, 0x300C, 0x3000],
                3,
                2
            ),
            Some(vec![0x1000, 0x2000, 0x3000])
        );
        // Test IdentityV / NeoX 5 pairs out of order
        let hits = [
            0x29A9456B0F4, // E1 max
            0x29A9456B0E8, // E1 min
            0x29A9456B10C, // E2 max
            0x29A9456B100, // E2 min
            0x29A9456B124, // E3 max
            0x29A9456B118, // E3 min
            0x29A9456B130, // E4 min
            0x29A9456B13C, // E4 max
            0x29A9456B154, // E5 max
            0x29A9456B148, // E5 min
        ];
        assert_eq!(
            entity_hits_in_capture_order(&hits, 5, 2),
            Some(vec![
                0x29A9456B0E8,
                0x29A9456B100,
                0x29A9456B118,
                0x29A9456B130,
                0x29A9456B148
            ])
        );
    }
}

impl Default for EspPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

pub fn esp_marker_scale(preset: &EspPreset, distance: f32) -> f32 {
    let size_offset = 1.0 + preset.marker_size_offset_percent / 100.0;
    let perspective = if preset.scale_with_distance || preset.marker_billboard_3d {
        let ratio = preset.distance_reference.max(0.01) / distance.max(0.01);
        let strength = if preset.distance_scale_strength_percent.is_finite() {
            preset.distance_scale_strength_percent.clamp(0.0, 100.0) / 100.0
        } else {
            1.0
        };
        ratio.powf(strength)
    } else {
        1.0
    };
    // ponytail: keep malformed presets from producing invisible or enormous overlay surfaces.
    (perspective * size_offset).clamp(0.05, 20.0)
}

pub(crate) fn esp_spatial_audio_gain(preset: &EspPreset, distance: f32) -> f32 {
    let near = preset.target_audio_full_volume_distance.max(0.0);
    let far = preset.target_audio_max_distance.max(near + 0.01);
    let distance_gain = if distance <= near {
        1.0
    } else if distance >= far {
        0.0
    } else {
        let remaining = 1.0 - (distance - near) / (far - near);
        remaining * remaining
    };
    preset.target_audio_volume.clamp(0.0, 2.0) * distance_gain
}

pub(crate) fn esp_spatial_audio_pan(
    preset: &EspPreset,
    target: [f32; 3],
    camera: [f32; 3],
    yaw: f32,
) -> f32 {
    let delta = [
        target[0] - camera[0],
        target[1] - camera[1],
        target[2] - camera[2],
    ];
    let (forward_a, forward_b) = match preset.horizontal_plane {
        EspHorizontalPlane::Xy => (delta[0], delta[1]),
        EspHorizontalPlane::Xz => (delta[0], delta[2]),
    };
    if forward_a.hypot(forward_b) <= f32::EPSILON {
        return 0.0;
    }
    let mut camera_yaw = esp_angle_to_radians(yaw, preset.yaw_unit);
    if preset.invert_camera_yaw {
        camera_yaw = -camera_yaw;
    }
    camera_yaw += preset.yaw_offset_degrees.to_radians();
    let mut yaw_delta = wrap_angle(forward_b.atan2(forward_a) - camera_yaw);
    if preset.invert_yaw {
        yaw_delta = -yaw_delta;
    }
    // Sine preserves left/right even when the target is outside the visible FOV or behind.
    yaw_delta.sin().clamp(-1.0, 1.0)
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

fn esp_pitch_to_radians(preset: &EspPreset, pitch: f32) -> f32 {
    let multiplier = if preset.pitch_multiplier.is_finite() {
        preset.pitch_multiplier
    } else {
        1.0
    };
    let pitch = match preset.pitch_input {
        EspPitchInput::Angle => esp_angle_to_radians(pitch, preset.pitch_unit),
        EspPitchInput::SineComponent => pitch.clamp(-1.0, 1.0).asin(),
        EspPitchInput::TangentComponent => pitch.atan(),
    };
    pitch * multiplier
}

pub(crate) fn esp_orientation_from_direction_pair(
    preset: &EspPreset,
    mut direction: [f32; 2],
    pitch: f32,
) -> Option<(f32, f32)> {
    if preset.invert_direction_a {
        direction[0] = -direction[0];
    }
    if preset.invert_direction_b {
        direction[1] = -direction[1];
    }
    if preset.swap_direction_pair {
        direction.swap(0, 1);
    }
    let mult = if preset.direction_multiplier.is_finite() && preset.direction_multiplier != 0.0 {
        preset.direction_multiplier
    } else {
        1.0
    };
    // Only the B/A ratio affects atan2; multiplying both components cancels out.
    direction[1] *= mult;
    let length = direction[0].hypot(direction[1]);
    if !length.is_finite() || length <= f32::EPSILON || !pitch.is_finite() {
        return None;
    }
    Some((
        esp_angle_from_radians(direction[1].atan2(direction[0]), preset.yaw_unit),
        pitch,
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
    let h_mult = if preset.height_scale.is_finite() && preset.height_scale != 0.0 {
        preset.height_scale
    } else {
        1.0
    };
    let (forward_a, forward_b, mut vertical) = match preset.horizontal_plane {
        EspHorizontalPlane::Xy => (dx, dy, (dz + preset.target_vertical_offset) * h_mult),
        EspHorizontalPlane::Xz => (dx, dz, (dy + preset.target_vertical_offset) * h_mult),
    };
    if preset.invert_vertical {
        vertical = -vertical;
    }
    let horizontal_distance = forward_a.hypot(forward_b);
    (horizontal_distance > f32::EPSILON).then_some(EspCalibrationSample {
        bearing_yaw: forward_b.atan2(forward_a),
        bearing_pitch: vertical.atan2(horizontal_distance),
        camera_yaw: esp_angle_to_radians(yaw, preset.yaw_unit),
        camera_pitch: esp_pitch_to_radians(preset, pitch),
    })
}

pub(crate) fn solve_esp_calibration(
    samples: &[EspCalibrationSample],
    current_invert_camera_yaw: bool,
    current_invert_camera_pitch: bool,
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
        let sign = if current_invert_camera_pitch {
            -sign
        } else {
            sign
        };
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EspProjection {
    pub normalized_x: f32,
    pub normalized_y: f32,
    pub distance: f32,
    pub in_front: bool,
    pub on_screen: bool,
}

/// Projects a world position and keeps visibility metadata for non-rendering consumers.
pub(crate) fn project_esp(
    preset: &EspPreset,
    target: [f32; 3],
    camera: [f32; 3],
    yaw: f32,
    pitch: f32,
    aspect: f32,
) -> Option<EspProjection> {
    if !target.into_iter().chain(camera).all(f32::is_finite)
        || !yaw.is_finite()
        || !pitch.is_finite()
        || !aspect.is_finite()
    {
        return None;
    }
    let dx = target[0] - camera[0];
    let dy = target[1] - camera[1];
    let dz = target[2] - camera[2];
    let h_mult = if preset.height_scale.is_finite() && preset.height_scale != 0.0 {
        preset.height_scale
    } else {
        1.0
    };
    let (forward_a, forward_b, mut vertical) = match preset.horizontal_plane {
        EspHorizontalPlane::Xy => (dx, dy, (dz + preset.target_vertical_offset) * h_mult),
        EspHorizontalPlane::Xz => (dx, dz, (dy + preset.target_vertical_offset) * h_mult),
    };
    if preset.invert_vertical {
        vertical = -vertical;
    }
    let horizontal_distance = forward_a.hypot(forward_b);
    let distance = horizontal_distance.hypot(vertical);
    if distance <= f32::EPSILON {
        return None;
    }
    let mut yaw = esp_angle_to_radians(yaw, preset.yaw_unit);
    let mut pitch = esp_pitch_to_radians(preset, pitch);
    if preset.invert_camera_yaw {
        yaw = -yaw;
    }
    if preset.invert_camera_pitch {
        pitch = -pitch;
    }
    yaw += preset.yaw_offset_degrees.to_radians();
    pitch += preset.pitch_offset_degrees.to_radians();
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let camera_forward = forward_a * cos_yaw + forward_b * sin_yaw;
    let mut camera_right = -forward_a * sin_yaw + forward_b * cos_yaw;
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    let camera_depth = camera_forward * cos_pitch + vertical * sin_pitch;
    let mut camera_up = vertical * cos_pitch - camera_forward * sin_pitch;
    if preset.invert_yaw {
        camera_right = -camera_right;
    }
    if preset.invert_pitch {
        camera_up = -camera_up;
    }
    let half_fov_x = (preset.horizontal_fov.clamp(1.0, 179.0).to_radians() * 0.5).max(0.001);
    let half_fov_y = (half_fov_x.tan() / aspect.max(0.01)).atan();
    let x = camera_right / (camera_depth * half_fov_x.tan());
    let vertical_projection_multiplier = if preset.vertical_projection_multiplier.is_finite() {
        preset.vertical_projection_multiplier.max(0.0)
    } else {
        1.0
    };
    let y = camera_up / (camera_depth * half_fov_y.tan()) * vertical_projection_multiplier;
    if !x.is_finite() || !y.is_finite() || !distance.is_finite() {
        return None;
    }
    let in_front = camera_depth > f32::EPSILON;
    Some(EspProjection {
        normalized_x: x,
        normalized_y: y,
        distance,
        in_front,
        on_screen: in_front && x.abs() < 1.0 && y.abs() < 1.0,
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
    let projection = project_esp(preset, target, camera, yaw, pitch, aspect)?;
    projection.on_screen.then_some((
        projection.normalized_x,
        projection.normalized_y,
        projection.distance,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_presets_default_to_geometry_markers() {
        let mut value = serde_json::to_value(EspPreset::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("marker_source");
        object.remove("marker_asset_path");
        object.remove("marker_svg_source");
        object.remove("marker_text");
        object.remove("text_offset_x");
        object.remove("text_offset_y");
        object.remove("text_font_size");
        object.remove("text_opacity");
        object.remove("scale_with_distance");
        object.remove("distance_reference");
        object.remove("marker_size_offset_percent");
        object.remove("marker_billboard_3d");
        object.remove("marker_offset_x");
        object.remove("marker_offset_y");
        object.remove("svg_width");
        object.remove("svg_height");
        object.remove("image_width");
        object.remove("image_height");
        object.remove("target_audio_enabled");
        object.remove("target_audio_path");
        object.remove("target_audio_loop");
        object.remove("target_audio_volume");
        object.remove("target_audio_full_volume_distance");
        object.remove("target_audio_max_distance");
        object.remove("pitch_input");
        object.remove("vertical_projection_multiplier");

        let preset: EspPreset = serde_json::from_value(value).unwrap();
        assert_eq!(preset.marker_source, EspMarkerSource::Geometry);
        assert!(preset.marker_asset_path.is_empty());
        assert!(preset.marker_svg_source.is_empty());
        assert_eq!(preset.marker_text, "Target");
        assert_eq!(preset.text_offset_x, 0.0);
        assert_eq!(preset.text_offset_y, 0.0);
        assert_eq!(preset.text_font_size, 18.0);
        assert_eq!(preset.text_opacity, 1.0);
        assert!(!preset.scale_with_distance);
        assert_eq!(preset.distance_reference, 100.0);
        assert_eq!(preset.marker_size_offset_percent, 0.0);
        assert!(!preset.marker_billboard_3d);
        assert_eq!(preset.marker_offset_x, 0.0);
        assert_eq!(preset.marker_offset_y, 0.0);
        assert_eq!(preset.svg_width, 44.0);
        assert_eq!(preset.svg_height, 88.0);
        assert_eq!(preset.image_width, 44.0);
        assert_eq!(preset.image_height, 88.0);
        assert!(!preset.target_audio_enabled);
        assert!(preset.target_audio_path.is_empty());
        assert!(preset.target_audio_loop);
        assert_eq!(preset.target_audio_volume, 1.0);
        assert_eq!(preset.pitch_input, EspPitchInput::Angle);
        assert_eq!(preset.vertical_projection_multiplier, 1.0);
    }

    #[test]
    fn marker_distance_scale_uses_reference_distance_and_size_offset() {
        let mut preset = EspPreset::default();
        preset.scale_with_distance = true;
        preset.distance_reference = 100.0;
        assert!((esp_marker_scale(&preset, 50.0) - 2.0).abs() < f32::EPSILON);
        assert!((esp_marker_scale(&preset, 200.0) - 0.5).abs() < f32::EPSILON);

        preset.marker_size_offset_percent = 25.0;
        assert!((esp_marker_scale(&preset, 100.0) - 1.25).abs() < f32::EPSILON);

        preset.marker_size_offset_percent = 0.0;
        preset.distance_scale_strength_percent = 50.0;
        assert!((esp_marker_scale(&preset, 400.0) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn billboard_enables_perspective_scaling() {
        let mut preset = EspPreset::default();
        preset.marker_billboard_3d = true;
        preset.distance_reference = 100.0;
        assert!((esp_marker_scale(&preset, 50.0) - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn spatial_audio_is_full_nearby_and_silent_past_max_distance() {
        let mut preset = EspPreset::default();
        preset.target_audio_volume = 0.8;
        preset.target_audio_full_volume_distance = 10.0;
        preset.target_audio_max_distance = 110.0;
        assert!((esp_spatial_audio_gain(&preset, 5.0) - 0.8).abs() < f32::EPSILON);
        assert!((esp_spatial_audio_gain(&preset, 60.0) - 0.2).abs() < 0.001);
        assert_eq!(esp_spatial_audio_gain(&preset, 120.0), 0.0);
    }

    #[test]
    fn spatial_audio_pan_tracks_target_side_outside_the_visible_fov() {
        let preset = EspPreset::default();
        assert!(esp_spatial_audio_pan(&preset, [0.0, 1.0, 0.0], [0.0; 3], 0.0) > 0.9);
        assert!(esp_spatial_audio_pan(&preset, [0.0, -1.0, 0.0], [0.0; 3], 0.0) < -0.9);
    }

    #[test]
    fn legacy_svg_source_moves_out_of_image_path() {
        let mut preset = EspPreset::default();
        preset.marker_source = EspMarkerSource::Svg;
        preset.marker_asset_path = "<svg/>".to_owned();

        assert!(preset.migrate_marker_sources());
        assert_eq!(preset.marker_svg_source, "<svg/>");
        assert!(preset.marker_asset_path.is_empty());
        assert!(!preset.migrate_marker_sources());
    }

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
    fn horizontal_camera_strafe_does_not_move_target_vertically() {
        let preset = EspPreset::default();
        let stationary = project_esp_normalized(
            &preset,
            [10.0, 0.0, 2.0],
            [0.0, 0.0, 0.0],
            0.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        let strafed = project_esp_normalized(
            &preset,
            [10.0, 0.0, 2.0],
            [0.0, -2.0, 0.0],
            0.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        assert!((stationary.1 - strafed.1).abs() < 0.001);
        assert!((stationary.0 - strafed.0).abs() > 0.01);
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
        let result = solve_esp_calibration(&samples, false, false, true, false).unwrap();
        assert!(!result.invert_camera_yaw);
        assert!(result.invert_yaw);
        assert!((result.yaw_offset_degrees - 30.0).abs() < 0.01);
        assert!(result.yaw_error_degrees < 0.01);
    }

    #[test]
    fn direction_pair_reconstructs_yaw_and_preserves_pitch() {
        let mut preset = EspPreset::default();
        preset.yaw_unit = EspAngleUnit::Radians;
        preset.pitch_unit = EspAngleUnit::Radians;
        let (yaw, pitch) = esp_orientation_from_direction_pair(&preset, [1.0, 1.0], -0.29).unwrap();
        assert!((yaw - std::f32::consts::FRAC_PI_4).abs() < 0.001);
        assert!((pitch + 0.29).abs() < 0.001);

        preset.swap_direction_pair = true;
        preset.invert_direction_a = true;
        let (yaw, _) = esp_orientation_from_direction_pair(&preset, [1.0, 0.0], -0.29).unwrap();
        assert!((yaw + std::f32::consts::FRAC_PI_2).abs() < 0.001);

        preset.swap_direction_pair = false;
        preset.invert_direction_a = false;
        preset.direction_multiplier = 2.0;
        let (yaw, _) = esp_orientation_from_direction_pair(&preset, [1.0, 1.0], 0.0).unwrap();
        assert!((yaw - 2.0_f32.atan()).abs() < 0.001);
    }

    #[test]
    fn pitch_multiplier_scales_a_presets_raw_pitch() {
        let mut preset = EspPreset::default();
        preset.pitch_unit = EspAngleUnit::Degrees;
        preset.pitch_multiplier = 0.5;
        assert!(
            (esp_pitch_to_radians(&preset, 180.0) - std::f32::consts::FRAC_PI_2).abs()
                < 0.001
        );

        preset.pitch_multiplier = f32::NAN;
        assert!(
            (esp_pitch_to_radians(&preset, 180.0) - std::f32::consts::PI).abs() < 0.001
        );

        preset.pitch_multiplier = 1.0;
        preset.pitch_input = EspPitchInput::SineComponent;
        assert!(
            (esp_pitch_to_radians(&preset, 1.0) - std::f32::consts::FRAC_PI_2).abs()
                < 0.001
        );
        preset.pitch_input = EspPitchInput::TangentComponent;
        assert!(
            (esp_pitch_to_radians(&preset, 1.0) - std::f32::consts::FRAC_PI_4).abs()
                < 0.001
        );
    }

    #[test]
    fn vertical_projection_scale_does_not_change_screen_x() {
        let mut preset = EspPreset::default();
        let normal = project_esp(
            &preset,
            [10.0, 2.0, 2.0],
            [0.0; 3],
            0.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        preset.vertical_projection_multiplier = 0.5;
        let scaled = project_esp(
            &preset,
            [10.0, 2.0, 2.0],
            [0.0; 3],
            0.0,
            0.0,
            16.0 / 9.0,
        )
        .unwrap();
        assert!((normal.normalized_x - scaled.normalized_x).abs() < 0.001);
        assert!((normal.normalized_y * 0.5 - scaled.normalized_y).abs() < 0.001);
    }
}
