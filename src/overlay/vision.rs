use anyhow::{Context, Result, bail};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use opencv::{
    core::{self as cv, Mat, Size},
    imgproc,
    prelude::*,
};

use super::{
    HOOK_STATE, UiCommand, resolve_text_variable_value, send_mouse_left_click_backend,
    set_text_variable_value, set_variable_value, settle_image_search_mouse_move,
};
use crate::model::{RgbaColor, VisionPreset};
use crate::window_list;

#[derive(Clone, Copy, Debug)]
pub(crate) struct TemplateMatchHit {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) scale: f32,
    pub(crate) confidence: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VisionRegion {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) is_circle: bool,
    pub(crate) angle_offset_deg: Option<f32>,
    pub(crate) angle_span_deg: Option<f32>,
}

#[derive(Clone)]
pub(crate) struct CachedTemplate {
    pub(crate) rgba: Vec<u8>,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) modified: Option<std::time::SystemTime>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ColorMatchHit {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) score: u32,
    pub(crate) distance_sq: i32,
    pub(crate) matched_color: RgbaColor,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ColorMatchPixel {
    pub(crate) target_index: usize,
    pub(crate) score: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct ConnectedColorClusterHit {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) score: u32,
    pub(crate) distance_sq: i32,
    pub(crate) matched_color: RgbaColor,
}

pub(crate) static TEMPLATE_CACHE: Lazy<Mutex<HashMap<u32, CachedTemplate>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
pub(crate) static IMAGE_SEARCH_WAIT_GENERATIONS: Lazy<Mutex<HashMap<u32, u64>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(crate) fn image_search_following_is_active(preset_id: u32) -> bool {
    HOOK_STATE
        .lock()
        .vision_following_presets
        .contains(&preset_id)
}

pub(crate) fn image_search_wait_generation(preset_id: u32) -> u64 {
    let gens = IMAGE_SEARCH_WAIT_GENERATIONS.lock();
    gens.get(&preset_id).copied().unwrap_or(0)
}

pub(crate) fn set_image_search_following_active(preset_id: u32, active: bool) {
    let mut state = HOOK_STATE.lock();
    if active {
        state.vision_following_presets.insert(preset_id);
    } else {
        state.vision_following_presets.remove(&preset_id);
    }
}

pub(crate) fn bump_image_search_wait_generation(preset_id: u32) {
    let mut gens = IMAGE_SEARCH_WAIT_GENERATIONS.lock();
    let entry = gens.entry(preset_id).or_insert(0);
    *entry = entry.wrapping_add(1);
}

pub(crate) fn vision_preset_by_id(spec: &str) -> Result<VisionPreset> {
    let spec = spec.trim();
    if spec.is_empty() {
        bail!("Vision preset id is invalid");
    }

    let hook_state = HOOK_STATE.lock();
    let by_id = spec.parse::<u32>().ok().and_then(|preset_id| {
        hook_state
            .vision_presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .cloned()
    });

    by_id
        .or_else(|| {
            hook_state
                .vision_presets
                .iter()
                .find(|preset| preset.name.trim().eq_ignore_ascii_case(spec))
                .cloned()
        })
        .context("Vision preset was not found")
}

pub(crate) fn start_vision_following(spec: &str, variable_override: Option<&str>) -> Result<()> {
    let preset = vision_preset_by_id(spec)?;
    if image_search_following_is_active(preset.id) {
        return Ok(());
    }

    let ui_tx = HOOK_STATE.lock().ui_tx.clone();
    set_image_search_following_active(preset.id, true);
    let var_override = variable_override.map(|s| s.to_string());
    thread::spawn(move || run_image_search_follow_loop(preset, ui_tx, var_override));
    Ok(())
}

pub(crate) fn stop_vision_following(spec: &str) -> Result<()> {
    let preset = vision_preset_by_id(spec)?;
    set_image_search_following_active(preset.id, false);
    Ok(())
}

pub(crate) fn stop_vision_following_ids(preset_ids: &[u32]) {
    for preset_id in preset_ids {
        set_image_search_following_active(*preset_id, false);
    }
}

pub(crate) fn image_search_template_file(preset_id: u32) -> PathBuf {
    let hook_state = HOOK_STATE.lock();
    hook_state
        .vision_dir
        .join(format!("preset-{preset_id}.png"))
}

pub(crate) fn configured_image_search_region(preset: &VisionPreset) -> Option<VisionRegion> {
    let (Some(region_x), Some(region_y), Some(region_width), Some(region_height)) = (
        preset.search_region_screen_x,
        preset.search_region_screen_y,
        preset.search_region_width,
        preset.search_region_height,
    ) else {
        return None;
    };
    if region_width <= 0 || region_height <= 0 {
        return None;
    }

    let (virtual_left, virtual_top, virtual_width, virtual_height) =
        window_list::virtual_screen_bounds();
    let virtual_right = virtual_left + virtual_width;
    let virtual_bottom = virtual_top + virtual_height;
    let left = region_x.max(virtual_left);
    let top = region_y.max(virtual_top);
    let right = (region_x + region_width).min(virtual_right);
    let bottom = (region_y + region_height).min(virtual_bottom);
    let width = right - left;
    let height = bottom - top;
    if width <= 0 || height <= 0 {
        return None;
    }

    Some(VisionRegion {
        left,
        top,
        width,
        height,
        is_circle: preset.search_region_is_circle,
        angle_offset_deg: None,
        angle_span_deg: None,
    })
}

pub(crate) fn image_search_region_contains_point(
    region: Option<&VisionRegion>,
    x: i32,
    y: i32,
) -> bool {
    let Some(region) = region else {
        return true;
    };
    let inside_rect = x >= region.left
        && y >= region.top
        && x < region.left + region.width
        && y < region.top + region.height;
    if !inside_rect {
        return false;
    }

    if !region.is_circle {
        return true;
    }

    let center_x = region.left as f32 + region.width as f32 * 0.5;
    let center_y = region.top as f32 + region.height as f32 * 0.5;
    let radius_x = (region.width as f32 * 0.5).max(1.0);
    let radius_y = (region.height as f32 * 0.5).max(1.0);
    let dx = (x as f32 + 0.5 - center_x) / radius_x;
    let dy = (y as f32 + 0.5 - center_y) / radius_y;
    dx * dx + dy * dy <= 1.0
}

pub(crate) fn expand_search_region_to_fit(
    region: VisionRegion,
    min_width: i32,
    min_height: i32,
) -> VisionRegion {
    let VisionRegion {
        left,
        top,
        width,
        height,
        is_circle,
        angle_offset_deg,
        angle_span_deg,
    } = region;
    let target_width = width.max(min_width.max(1));
    let target_height = height.max(min_height.max(1));
    if target_width == width && target_height == height {
        return region;
    }

    let center_x = left + width / 2;
    let center_y = top + height / 2;
    let mut next_left = center_x - target_width / 2;
    let mut next_top = center_y - target_height / 2;
    let (virtual_left, virtual_top, virtual_width, virtual_height) =
        window_list::virtual_screen_bounds();
    let virtual_right = virtual_left + virtual_width;
    let virtual_bottom = virtual_top + virtual_height;
    if next_left < virtual_left {
        next_left = virtual_left;
    }

    if next_top < virtual_top {
        next_top = virtual_top;
    }

    let mut next_right = (next_left + target_width).min(virtual_right);
    let mut next_bottom = (next_top + target_height).min(virtual_bottom);
    if next_right - next_left < target_width {
        next_left = (next_right - target_width).max(virtual_left);
        next_right = (next_left + target_width).min(virtual_right);
    }

    if next_bottom - next_top < target_height {
        next_top = (next_bottom - target_height).max(virtual_top);
        next_bottom = (next_top + target_height).min(virtual_bottom);
    }

    VisionRegion {
        left: next_left,
        top: next_top,
        width: (next_right - next_left).max(1),
        height: (next_bottom - next_top).max(1),
        is_circle,
        angle_offset_deg,
        angle_span_deg,
    }
}

pub(crate) fn capture_near_last_image_search_region(
    capture_x: i32,
    capture_y: i32,
    template_width: usize,
    template_height: usize,
) -> Option<window_list::ScreenCaptureFrame> {
    let padding_x = (template_width as i32 * 2).clamp(160, 480);
    let padding_y = (template_height as i32 * 2).clamp(160, 480);
    let desired_left = capture_x - (template_width as i32 / 2) - padding_x;
    let desired_top = capture_y - (template_height as i32 / 2) - padding_y;
    let desired_right = capture_x + (template_width as i32 / 2) + padding_x;
    let desired_bottom = capture_y + (template_height as i32 / 2) + padding_y;
    let (virtual_left, virtual_top, virtual_width, virtual_height) =
        window_list::virtual_screen_bounds();
    let left = desired_left.max(virtual_left);
    let top = desired_top.max(virtual_top);
    let right = desired_right.min(virtual_left + virtual_width);
    let bottom = desired_bottom.min(virtual_top + virtual_height);
    let width = (right - left).max(template_width as i32);
    let height = (bottom - top).max(template_height as i32);
    window_list::capture_virtual_screen_region(left, top, width, height)
}

pub(crate) fn find_template_match_exact_rgba(
    screen: &window_list::ScreenCaptureFrame,
    template_rgba: &[u8],
    template_width: usize,
    template_height: usize,
    max_average_diff: f32,
    anchor_hint_screen: Option<(i32, i32)>,
    search_region: Option<&VisionRegion>,
) -> Option<TemplateMatchHit> {
    if template_width == 0
        || template_height == 0
        || screen.width < template_width
        || screen.height < template_height
    {
        return None;
    }

    let anchor_hint = anchor_hint_screen.map(|(x, y)| (x - screen.screen_x, y - screen.screen_y));
    let mut best_hit: Option<TemplateMatchHit> = None;
    for y in 0..=(screen.height - template_height) {
        for x in 0..=(screen.width - template_width) {
            let center_x = screen.screen_x + x as i32 + (template_width as i32 / 2);
            let center_y = screen.screen_y + y as i32 + (template_height as i32 / 2);
            if !image_search_region_contains_point(search_region, center_x, center_y) {
                continue;
            }

            let mut total_diff = 0u64;
            let mut over_budget = false;
            for row in 0..template_height {
                let screen_row = ((y + row) * screen.width + x) * 4;
                let template_row = row * template_width * 4;
                for col in 0..template_width {
                    let screen_idx = screen_row + col * 4;
                    let template_idx = template_row + col * 4;
                    let dr = screen.rgba[screen_idx].abs_diff(template_rgba[template_idx]) as u64;
                    let dg = screen.rgba[screen_idx + 1].abs_diff(template_rgba[template_idx + 1])
                        as u64;
                    let db = screen.rgba[screen_idx + 2].abs_diff(template_rgba[template_idx + 2])
                        as u64;
                    total_diff += dr + dg + db;
                    let processed = ((row * template_width) + (col + 1)) as f32;
                    let average = total_diff as f32 / processed / 3.0;
                    if average > max_average_diff {
                        over_budget = true;
                        break;
                    }
                }

                if over_budget {
                    break;
                }
            }

            if over_budget {
                continue;
            }

            let pixel_count = (template_width * template_height) as f32;
            let avg_diff = total_diff as f32 / pixel_count / 3.0;
            let candidate = TemplateMatchHit {
                x: x as i32,
                y: y as i32,
                width: template_width as i32,
                height: template_height as i32,
                scale: 1.0,
                confidence: (1.0 - (avg_diff / 255.0)).clamp(0.0, 1.0),
            };
            if select_better_template_match(candidate, best_hit, anchor_hint) {
                best_hit = Some(candidate);
            }
        }
    }

    best_hit
}

pub(crate) fn find_template_match_opencv(
    screen: &window_list::ScreenCaptureFrame,
    template_rgba: &[u8],
    template_width: usize,
    template_height: usize,
    scales: &[f32],
    anchor_hint_screen: Option<(i32, i32)>,
    use_color_matching: bool,
    search_region: Option<&VisionRegion>,
) -> Result<Option<TemplateMatchHit>> {
    let screen_mat = if use_color_matching {
        rgba_to_color_mat(&screen.rgba, screen.width, screen.height)?
    } else {
        rgba_to_gray_mat(&screen.rgba, screen.width, screen.height)?
    };
    let template_mat = if use_color_matching {
        rgba_to_color_mat(template_rgba, template_width, template_height)?
    } else {
        rgba_to_gray_mat(template_rgba, template_width, template_height)?
    };
    let anchor_hint = anchor_hint_screen
        .map(|(screen_x, screen_y)| (screen_x - screen.screen_x, screen_y - screen.screen_y));
    let mut best_hit: Option<TemplateMatchHit> = None;
    for &scale in scales {
        let scaled_width = ((template_width as f32) * scale).round().max(1.0) as i32;
        let scaled_height = ((template_height as f32) * scale).round().max(1.0) as i32;
        if scaled_width > screen.width as i32 || scaled_height > screen.height as i32 {
            continue;
        }

        let scaled_template = if (scale - 1.0).abs() < f32::EPSILON {
            template_mat
                .try_clone()
                .context("Failed to clone template Mat.")?
        } else {
            let mut resized = Mat::default();
            imgproc::resize(
                &template_mat,
                &mut resized,
                Size::new(scaled_width, scaled_height),
                0.0,
                0.0,
                imgproc::INTER_LINEAR,
            )
            .context("Failed to resize template for OpenCV matching.")?;
            resized
        };
        let result_cols = screen_mat.cols() - scaled_template.cols() + 1;
        let result_rows = screen_mat.rows() - scaled_template.rows() + 1;
        if result_cols <= 0 || result_rows <= 0 {
            continue;
        }

        let mut result = Mat::default();
        imgproc::match_template(
            &screen_mat,
            &scaled_template,
            &mut result,
            imgproc::TM_CCOEFF_NORMED,
            &cv::no_array(),
        )
        .context("OpenCV matchTemplate failed.")?;
        let result_data = result
            .data_typed::<f32>()
            .context("OpenCV result matrix was not readable.")?;
        let result_width = result.cols().max(0) as usize;
        let result_height = result.rows().max(0) as usize;
        for y in 0..result_height {
            for x in 0..result_width {
                let confidence = result_data[y * result_width + x];
                let center_x = screen.screen_x + x as i32 + scaled_width / 2;
                let center_y = screen.screen_y + y as i32 + scaled_height / 2;
                if !image_search_region_contains_point(search_region, center_x, center_y) {
                    continue;
                }

                let candidate = TemplateMatchHit {
                    x: x as i32,
                    y: y as i32,
                    width: scaled_width,
                    height: scaled_height,
                    scale,
                    confidence,
                };
                if select_better_template_match(candidate, best_hit, anchor_hint) {
                    best_hit = Some(candidate);
                }
            }
        }
    }

    Ok(best_hit)
}

pub(crate) fn rgba_to_color_mat(rgba: &[u8], width: usize, height: usize) -> Result<Mat> {
    if !HOOK_STATE.lock().opencv_dll_path.exists() {
        bail!("OpenCV library not found. Please install it in Settings.");
    }

    let expected_len = width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(4))
        .context("Image buffer is too large.")?;
    if rgba.len() != expected_len {
        bail!("Image buffer size does not match width/height.");
    }

    let flat = Mat::from_slice(rgba).context("Failed to create OpenCV Mat from RGBA slice.")?;
    let rgba_mat = flat
        .reshape(4, height as i32)
        .context("Failed to reshape RGBA buffer into OpenCV Mat.")?;
    let mut bgr = Mat::default();
    imgproc::cvt_color(&rgba_mat, &mut bgr, imgproc::COLOR_RGBA2BGR, 0)
        .context("Failed to convert RGBA image to BGR.")?;
    Ok(bgr)
}

pub(crate) fn rgba_to_gray_mat(rgba: &[u8], width: usize, height: usize) -> Result<Mat> {
    let rgba_mat = rgba_to_color_mat(rgba, width, height)?;
    let mut gray = Mat::default();
    imgproc::cvt_color(&rgba_mat, &mut gray, imgproc::COLOR_BGR2GRAY, 0)
        .context("Failed to convert BGR image to grayscale.")?;
    Ok(gray)
}

pub(crate) fn select_better_template_match(
    candidate: TemplateMatchHit,
    current: Option<TemplateMatchHit>,
    anchor_hint: Option<(i32, i32)>,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    if candidate.confidence > current.confidence + 0.002 {
        return true;
    }

    if current.confidence > candidate.confidence + 0.002 {
        return false;
    }

    if let Some((anchor_x, anchor_y)) = anchor_hint {
        let candidate_center_x = candidate.x + candidate.width / 2;
        let candidate_center_y = candidate.y + candidate.height / 2;
        let current_center_x = current.x + current.width / 2;
        let current_center_y = current.y + current.height / 2;
        let candidate_distance =
            (candidate_center_x - anchor_x).pow(2) + (candidate_center_y - anchor_y).pow(2);
        let current_distance =
            (current_center_x - anchor_x).pow(2) + (current_center_y - anchor_y).pow(2);
        return candidate_distance < current_distance;
    }

    false
}

pub(crate) fn count_matching_pixels(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: u8,
    region: Option<&VisionRegion>,
) -> i32 {
    let width = screen.width as i32;
    let height = screen.height as i32;
    if width <= 0 || height <= 0 || targets.is_empty() {
        return 0;
    }

    let tolerance = tolerance as i16;
    let mut count = 0;
    for y in 0..height {
        for x in 0..width {
            if !image_search_region_contains_point(region, screen.screen_x + x, screen.screen_y + y)
            {
                continue;
            }

            let index = ((y as usize) * screen.width + (x as usize)) * 4;
            if index + 3 >= screen.rgba.len() {
                continue;
            }

            let r = screen.rgba[index] as i16;
            let g = screen.rgba[index + 1] as i16;
            let b = screen.rgba[index + 2] as i16;
            for target in targets {
                let dr = (r - target.r as i16).abs();
                let dg = (g - target.g as i16).abs();
                let db = (b - target.b as i16).abs();
                if dr <= tolerance && dg <= tolerance && db <= tolerance {
                    count += 1;
                    break;
                }
            }
        }
    }

    count
}

pub(crate) fn image_search_target_colors(preset: &VisionPreset) -> Vec<RgbaColor> {
    if !preset.target_colors.is_empty() {
        return preset.target_colors.clone();
    }

    preset.target_color.into_iter().collect()
}

pub(crate) fn color_match_pixel_for_coordinate(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: i16,
    x: i32,
    y: i32,
    region: Option<&VisionRegion>,
) -> Option<ColorMatchPixel> {
    if x < 0 || y < 0 || x >= screen.width as i32 || y >= screen.height as i32 {
        return None;
    }

    if !image_search_region_contains_point(region, screen.screen_x + x, screen.screen_y + y) {
        return None;
    }

    let index = ((y as usize) * screen.width + (x as usize)) * 4;
    if index + 3 >= screen.rgba.len() {
        return None;
    }

    let r = screen.rgba[index] as i16;
    let g = screen.rgba[index + 1] as i16;
    let b = screen.rgba[index + 2] as i16;
    let mut best_match: Option<ColorMatchPixel> = None;
    for (target_index, target) in targets.iter().enumerate() {
        let dr = (r - target.r as i16).abs();
        let dg = (g - target.g as i16).abs();
        let db = (b - target.b as i16).abs();
        if dr > tolerance || dg > tolerance || db > tolerance {
            continue;
        }

        let candidate = ColorMatchPixel {
            target_index,
            score: (dr as u32) + (dg as u32) + (db as u32),
        };
        let replace = match best_match {
            None => true,
            Some(current) => candidate.score < current.score,
        };
        if replace {
            best_match = Some(candidate);
        }
    }

    best_match
}

pub(crate) fn build_color_match_pixel_map(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: i16,
    region: Option<&VisionRegion>,
) -> Vec<Option<ColorMatchPixel>> {
    let width = screen.width as i32;
    let height = screen.height as i32;
    let mut pixel_map = vec![None; screen.width * screen.height];
    for y in 0..height {
        for x in 0..width {
            let index = (y as usize) * screen.width + (x as usize);
            pixel_map[index] =
                color_match_pixel_for_coordinate(screen, targets, tolerance, x, y, region);
        }
    }

    pixel_map
}

pub(crate) fn find_connected_color_match(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: u8,
    region: Option<&VisionRegion>,
    reference: Option<(i32, i32)>,
) -> Option<ColorMatchHit> {
    let width = screen.width as i32;
    let height = screen.height as i32;
    if width <= 0 || height <= 0 || targets.len() < 2 {
        return None;
    }

    let tolerance = tolerance as i16;
    let pixel_map = build_color_match_pixel_map(screen, targets, tolerance, region);
    let mut visited = vec![false; pixel_map.len()];
    let mut stack = Vec::new();
    let mut best_hit: Option<ConnectedColorClusterHit> = None;
    let reference = reference.unwrap_or((width / 2, height / 2));
    for y in 0..height {
        for x in 0..width {
            let start_index = (y as usize) * screen.width + (x as usize);
            if visited[start_index] || pixel_map[start_index].is_none() {
                continue;
            }

            stack.clear();
            stack.push((x, y));
            visited[start_index] = true;
            let mut seen_targets = vec![false; targets.len()];
            let mut unique_target_count = 0usize;
            let mut score_sum = 0u32;
            let mut pixel_count = 0i32;
            let mut sum_x = 0i32;
            let mut sum_y = 0i32;
            let mut first_target_index = 0usize;
            while let Some((cx, cy)) = stack.pop() {
                let index = (cy as usize) * screen.width + (cx as usize);
                let Some(pixel_match) = pixel_map[index] else {
                    continue;
                };
                if pixel_count == 0 {
                    first_target_index = pixel_match.target_index;
                }

                if !seen_targets[pixel_match.target_index] {
                    seen_targets[pixel_match.target_index] = true;
                    unique_target_count += 1;
                }

                score_sum = score_sum.saturating_add(pixel_match.score);
                pixel_count += 1;
                sum_x += cx;
                sum_y += cy;
                for ny in (cy - 1).max(0)..=(cy + 1).min(height - 1) {
                    for nx in (cx - 1).max(0)..=(cx + 1).min(width - 1) {
                        if nx == cx && ny == cy {
                            continue;
                        }

                        let neighbor_index = (ny as usize) * screen.width + (nx as usize);
                        if visited[neighbor_index] || pixel_map[neighbor_index].is_none() {
                            continue;
                        }

                        visited[neighbor_index] = true;
                        stack.push((nx, ny));
                    }
                }
            }

            if unique_target_count != targets.len() || pixel_count <= 0 {
                continue;
            }

            let cluster_x = sum_x / pixel_count;
            let cluster_y = sum_y / pixel_count;
            let avg_score = score_sum / (pixel_count as u32);
            let distance_sq = (cluster_x - reference.0).pow(2) + (cluster_y - reference.1).pow(2);
            let candidate = ConnectedColorClusterHit {
                x: cluster_x,
                y: cluster_y,
                score: avg_score,
                distance_sq,
                matched_color: targets[first_target_index],
            };
            let replace = match best_hit.as_ref() {
                None => true,
                Some(current) if candidate.score < current.score => true,
                Some(current) if candidate.score == current.score => {
                    candidate.distance_sq < current.distance_sq
                }
                _ => false,
            };
            if replace {
                best_hit = Some(candidate);
            }
        }
    }

    best_hit.map(|hit| ColorMatchHit {
        x: hit.x,
        y: hit.y,
        score: hit.score,
        distance_sq: hit.distance_sq,
        matched_color: hit.matched_color,
    })
}

pub(crate) fn color_match_candidate_for_pixel(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: i16,
    x: i32,
    y: i32,
    reference_x: i32,
    reference_y: i32,
    region: Option<&VisionRegion>,
) -> Option<ColorMatchHit> {
    if x < 0 || y < 0 || x >= screen.width as i32 || y >= screen.height as i32 {
        return None;
    }

    if !image_search_region_contains_point(region, screen.screen_x + x, screen.screen_y + y) {
        return None;
    }

    let index = ((y as usize) * screen.width + (x as usize)) * 4;
    if index + 3 >= screen.rgba.len() {
        return None;
    }

    let r = screen.rgba[index] as i16;
    let g = screen.rgba[index + 1] as i16;
    let b = screen.rgba[index + 2] as i16;
    let mut best_hit: Option<ColorMatchHit> = None;
    for target in targets {
        let dr = (r - target.r as i16).abs();
        let dg = (g - target.g as i16).abs();
        let db = (b - target.b as i16).abs();
        if dr > tolerance || dg > tolerance || db > tolerance {
            continue;
        }

        let score = (dr as u32) + (dg as u32) + (db as u32);
        let distance_sq = (x - reference_x).pow(2) + (y - reference_y).pow(2);
        let candidate = ColorMatchHit {
            x,
            y,
            score,
            distance_sq,
            matched_color: *target,
        };
        let replace = match best_hit {
            None => true,
            Some(current) if candidate.score < current.score => true,
            Some(current) if candidate.score == current.score => {
                candidate.distance_sq < current.distance_sq
            }
            _ => false,
        };
        if replace {
            best_hit = Some(candidate);
        }
    }

    best_hit
}

pub(crate) fn find_color_match_in_range(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: u8,
    x_start: usize,
    x_end: usize,
    region: Option<&VisionRegion>,
) -> Option<ColorMatchHit> {
    let width = screen.width as i32;
    let height = screen.height as i32;
    if width <= 0 || height <= 0 || targets.is_empty() {
        return None;
    }

    let x_start = x_start.min(screen.width);
    let x_end = x_end.min(screen.width);
    if x_start >= x_end {
        return None;
    }

    let center_x = width / 2;
    let center_y = height / 2;
    let tolerance = tolerance as i16;
    let mut best_hit: Option<ColorMatchHit> = None;
    for y in 0..height {
        for x in x_start as i32..x_end as i32 {
            let candidate = color_match_candidate_for_pixel(
                screen, targets, tolerance, x, y, center_x, center_y, region,
            );
            if let Some(candidate) = candidate {
                let replace = match best_hit {
                    None => true,
                    Some(current) if candidate.score < current.score => true,
                    Some(current) if candidate.score == current.score => {
                        candidate.distance_sq < current.distance_sq
                    }
                    _ => false,
                };
                if replace {
                    best_hit = Some(candidate);
                }
            }
        }
    }

    best_hit
}

pub(crate) fn find_color_match(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: u8,
    region: Option<&VisionRegion>,
) -> Option<ColorMatchHit> {
    find_color_match_in_range(screen, targets, tolerance, 0, screen.width, region)
}

pub(crate) fn find_dual_color_midpoint_match(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: u8,
    region: Option<&VisionRegion>,
) -> Option<ColorMatchHit> {
    let mid = (screen.width / 2).max(1);
    let (left_hit, right_hit) = thread::scope(|scope| {
        let left =
            scope.spawn(|| find_color_match_in_range(screen, targets, tolerance, 0, mid, region));
        let right = scope.spawn(|| {
            find_color_match_in_range(screen, targets, tolerance, mid, screen.width, region)
        });
        (left.join().ok().flatten(), right.join().ok().flatten())
    });
    match (left_hit, right_hit) {
        (Some(left), Some(right)) => Some(ColorMatchHit {
            x: ((left.x + right.x) / 2).max(0),
            y: ((left.y + right.y) / 2).max(0),
            score: left.score.min(right.score),
            distance_sq: left.distance_sq.min(right.distance_sq),
            matched_color: left.matched_color,
        }),
        (Some(hit), None) | (None, Some(hit)) => Some(hit),
        (None, None) => None,
    }
}

pub(crate) fn find_color_average_centroid_match(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: u8,
    region: Option<&VisionRegion>,
) -> Option<ColorMatchHit> {
    let width = screen.width as i32;
    let height = screen.height as i32;
    if width <= 0 || height <= 0 || targets.is_empty() {
        return None;
    }

    let tolerance = tolerance as i16;
    let mut sum_x = 0i64;
    let mut sum_y = 0i64;
    let mut match_count = 0i64;
    let mut score_sum = 0u64;
    let mut matched_color = targets[0];

    for y in 0..height {
        for x in 0..width {
            if !image_search_region_contains_point(region, screen.screen_x + x, screen.screen_y + y)
            {
                continue;
            }

            let index = ((y as usize) * screen.width + (x as usize)) * 4;
            if index + 3 >= screen.rgba.len() {
                continue;
            }

            let r = screen.rgba[index] as i16;
            let g = screen.rgba[index + 1] as i16;
            let b = screen.rgba[index + 2] as i16;

            for target in targets {
                let dr = (r - target.r as i16).abs();
                let dg = (g - target.g as i16).abs();
                let db = (b - target.b as i16).abs();
                if dr <= tolerance && dg <= tolerance && db <= tolerance {
                    sum_x += x as i64;
                    sum_y += y as i64;
                    score_sum += (dr + dg + db) as u64;
                    if match_count == 0 {
                        matched_color = *target;
                    }
                    match_count += 1;
                    break;
                }
            }
        }
    }

    if match_count > 0 {
        let avg_x = (sum_x / match_count) as i32;
        let avg_y = (sum_y / match_count) as i32;
        let avg_score = (score_sum / match_count as u64) as u32;
        let center_x = width / 2;
        let center_y = height / 2;
        let distance_sq = (avg_x - center_x).pow(2) + (avg_y - center_y).pow(2);
        Some(ColorMatchHit {
            x: avg_x,
            y: avg_y,
            score: avg_score,
            distance_sq,
            matched_color,
        })
    } else {
        None
    }
}

pub(crate) fn find_color_match_from_anchor(
    screen: &window_list::ScreenCaptureFrame,
    targets: &[RgbaColor],
    tolerance: u8,
    anchor_x: i32,
    anchor_y: i32,
    region: Option<&VisionRegion>,
) -> Option<ColorMatchHit> {
    let width = screen.width as i32;
    let height = screen.height as i32;
    if width <= 0 || height <= 0 || targets.is_empty() {
        return None;
    }

    if anchor_x < 0 || anchor_y < 0 || anchor_x >= width || anchor_y >= height {
        return None;
    }

    let tolerance = tolerance as i16;
    let max_radius = (anchor_x)
        .max(width - 1 - anchor_x)
        .max(anchor_y)
        .max(height - 1 - anchor_y);
    for radius in 0..=max_radius {
        let left = (anchor_x - radius).max(0);
        let right = (anchor_x + radius).min(width - 1);
        let top = (anchor_y - radius).max(0);
        let bottom = (anchor_y + radius).min(height - 1);
        let mut best_in_radius: Option<ColorMatchHit> = None;
        for x in left..=right {
            for y in [top, bottom] {
                if let Some(candidate) = color_match_candidate_for_pixel(
                    screen, targets, tolerance, x, y, anchor_x, anchor_y, region,
                ) {
                    let replace = match best_in_radius {
                        None => true,
                        Some(current) if candidate.score < current.score => true,
                        Some(current) if candidate.score == current.score => {
                            candidate.distance_sq < current.distance_sq
                        }
                        _ => false,
                    };
                    if replace {
                        best_in_radius = Some(candidate);
                    }
                }
            }
        }

        if top + 1 <= bottom.saturating_sub(1) {
            for y in (top + 1)..bottom {
                for x in [left, right] {
                    if let Some(candidate) = color_match_candidate_for_pixel(
                        screen, targets, tolerance, x, y, anchor_x, anchor_y, region,
                    ) {
                        let replace = match best_in_radius {
                            None => true,
                            Some(current) if candidate.score < current.score => true,
                            Some(current) if candidate.score == current.score => {
                                candidate.distance_sq < current.distance_sq
                            }
                            _ => false,
                        };
                        if replace {
                            best_in_radius = Some(candidate);
                        }
                    }
                }
            }
        }

        if best_in_radius.is_some() {
            return best_in_radius;
        }
    }

    None
}

#[derive(Debug, Clone)]
pub(crate) struct VisionRunOutcome {
    pub(crate) matched: bool,
    pub(crate) status: String,
}

pub(crate) fn run_vision_once_with_options(
    preset: &VisionPreset,
    move_cursor: bool,
    fire_click: bool,
    variable_override: Option<&str>,
    pos_var_x: Option<&str>,
    pos_var_y: Option<&str>,
    found_var: Option<&str>,
) -> Result<VisionRunOutcome> {
    let set_found_var = |matched: bool| {
        if let Some(var_name) = found_var.filter(|s| !s.trim().is_empty()) {
            set_variable_value(var_name.trim(), if matched { 1.0 } else { 0.0 });
        }
    };
    if preset.is_pixel_counter {
        let target_colors = image_search_target_colors(preset);
        if target_colors.is_empty() {
            bail!("No target colors have been picked yet.");
        }

        let configured_region = configured_image_search_region(preset);
        let screen = if let Some(region) = configured_region {
            window_list::capture_virtual_screen_region(
                region.left,
                region.top,
                region.width,
                region.height,
            )
            .context("Failed to capture the selected search area")?
        } else if preset.target_window_title.is_some()
            || !preset.extra_target_window_titles.is_empty()
        {
            window_list::capture_window_region_with_candidates(
                preset.target_window_title.as_deref(),
                &preset.extra_target_window_titles,
                preset.match_duplicate_window_titles,
            )
            .context("Failed to capture the target window")?
        } else {
            let (left, top, width, height) = window_list::virtual_screen_bounds();
            window_list::capture_virtual_screen_region(left, top, width, height)
                .context("Failed to capture the screen")?
        };
        let count = count_matching_pixels(
            &screen,
            &target_colors,
            preset.color_tolerance,
            configured_region.as_ref(),
        );
        let var_name = if let Some(over) = variable_override.filter(|s| !s.trim().is_empty()) {
            over.trim().to_string()
        } else if preset.pixel_counter_variable_name.is_empty() {
            format!("pixel_count_{}", preset.id)
        } else {
            preset.pixel_counter_variable_name.clone()
        };
        set_variable_value(&var_name, count as f64);
        set_found_var(count > 0);

        return Ok(VisionRunOutcome {
            matched: count > 0,
            status: format!("Saved pixel count {count} to variable '{var_name}'"),
        });
    }

    if preset.use_color_matching && preset.search_region_is_single_pixel {
        let configured_region = configured_image_search_region(preset);
        let Some(region) = configured_region else {
            bail!("No pixel has been picked yet.");
        };

        let screen = window_list::capture_virtual_screen_region(region.left, region.top, 1, 1)
            .context("Failed to capture the selected pixel")?;

        if screen.rgba.len() < 4 {
            bail!("Failed to read captured pixel color.");
        }

        let r = screen.rgba[0];
        let g = screen.rgba[1];
        let b = screen.rgba[2];
        let hex_color = format!("#{:02X}{:02X}{:02X}", r, g, b);

        let var_name = if let Some(over) = variable_override.filter(|s| !s.trim().is_empty()) {
            over.trim().to_string()
        } else if preset.pixel_counter_variable_name.is_empty() {
            format!("color_code_{}", preset.id)
        } else {
            preset.pixel_counter_variable_name.clone()
        };

        set_text_variable_value(&var_name, &hex_color);

        return Ok(VisionRunOutcome {
            matched: true,
            status: format!("Saved pixel color {hex_color} to variable '{var_name}'"),
        });
    }

    if preset.use_color_matching {
        let target_colors = image_search_target_colors(preset);
        if target_colors.is_empty() {
            bail!("No target colors have been picked yet.");
        }

        let configured_region = configured_image_search_region(preset);
        let anchor = {
            let state = HOOK_STATE.lock();
            if state.vision_capture_mouse_blocked {
                state.vision_capture_anchor
            } else {
                None
            }
        };

        let screen = if let Some(region) = configured_region {
            window_list::capture_virtual_screen_region(
                region.left,
                region.top,
                region.width,
                region.height,
            )
            .context("Failed to capture the selected search area")?
        } else if preset.target_window_title.is_some()
            || !preset.extra_target_window_titles.is_empty()
        {
            window_list::capture_window_region_with_candidates(
                preset.target_window_title.as_deref(),
                &preset.extra_target_window_titles,
                preset.match_duplicate_window_titles,
            )
            .context("Failed to capture the target window")?
        } else if let Some((anchor_x, anchor_y)) = anchor {
            // Anchor-based color matching scan region padding
            let (virtual_left, virtual_top, virtual_width, virtual_height) =
                window_list::virtual_screen_bounds();
            let desired_left = anchor_x - 300;
            let desired_top = anchor_y - 300;
            let desired_right = anchor_x + 300;
            let desired_bottom = anchor_y + 300;
            let left = desired_left.max(virtual_left);
            let top = desired_top.max(virtual_top);
            let right = desired_right.min(virtual_left + virtual_width);
            let bottom = desired_bottom.min(virtual_top + virtual_height);
            let width = (right - left).max(1);
            let height = (bottom - top).max(1);
            window_list::capture_virtual_screen_region(left, top, width, height)
                .context("Failed to capture the anchor screen area")?
        } else {
            let (left, top, width, height) = window_list::virtual_screen_bounds();
            window_list::capture_virtual_screen_region(left, top, width, height)
                .context("Failed to capture the screen")?
        };

        let hit = if let Some((anchor_x, anchor_y)) = anchor {
            let relative_anchor_x = anchor_x - screen.screen_x;
            let relative_anchor_y = anchor_y - screen.screen_y;
            find_color_match_from_anchor(
                &screen,
                &target_colors,
                preset.color_tolerance,
                relative_anchor_x,
                relative_anchor_y,
                configured_region.as_ref(),
            )
        } else if preset.color_scan_average_centroid {
            find_color_average_centroid_match(
                &screen,
                &target_colors,
                preset.color_tolerance,
                configured_region.as_ref(),
            )
        } else if preset.require_connected_target_colors && target_colors.len() >= 2 {
            find_connected_color_match(
                &screen,
                &target_colors,
                preset.color_tolerance,
                configured_region.as_ref(),
                None,
            )
        } else if preset.dual_color_scan_midpoint {
            find_dual_color_midpoint_match(
                &screen,
                &target_colors,
                preset.color_tolerance,
                configured_region.as_ref(),
            )
        } else {
            find_color_match(
                &screen,
                &target_colors,
                preset.color_tolerance,
                configured_region.as_ref(),
            )
        };

        let Some(hit) = hit else {
            set_found_var(false);
            return Ok(VisionRunOutcome {
                matched: false,
                status: "No color match found.".to_owned(),
            });
        };

        let center_x = screen.screen_x + hit.x;
        let center_y = screen.screen_y + hit.y;
        let moved_x = center_x + preset.move_offset_x;
        let moved_y = center_y + preset.move_offset_y;

        if let Some(var_x) = pos_var_x.filter(|s| !s.trim().is_empty()) {
            set_variable_value(var_x, moved_x as f64);
        }

        if let Some(var_y) = pos_var_y.filter(|s| !s.trim().is_empty()) {
            set_variable_value(var_y, moved_y as f64);
        }
        set_found_var(true);

        if move_cursor {
            settle_image_search_mouse_move(
                moved_x,
                moved_y,
                preset.non_interception_move_passes,
                preset.non_interception_move_delay_ms,
            )?;
        }

        if fire_click {
            thread::sleep(Duration::from_millis(12));
            send_mouse_left_click_backend()?;
        }

        return Ok(VisionRunOutcome {
            matched: true,
            status: if anchor.is_some() {
                format!(
                    "Matched colors from priority point at {moved_x}, {moved_y} with tolerance {} and offset {:+}, {:+}.",
                    preset.color_tolerance, preset.move_offset_x, preset.move_offset_y
                )
            } else if preset.color_scan_average_centroid {
                format!(
                    "Matched colors centroid at {moved_x}, {moved_y} with tolerance {} and offset {:+}, {:+}.",
                    preset.color_tolerance, preset.move_offset_x, preset.move_offset_y
                )
            } else if preset.require_connected_target_colors && target_colors.len() >= 2 {
                format!(
                    "Matched connected colors at {moved_x}, {moved_y} with tolerance {} and offset {:+}, {:+}.",
                    preset.color_tolerance, preset.move_offset_x, preset.move_offset_y
                )
            } else if preset.dual_color_scan_midpoint {
                format!(
                    "Matched colors midpoint at {moved_x}, {moved_y} with tolerance {} and offset {:+}, {:+}.",
                    preset.color_tolerance, preset.move_offset_x, preset.move_offset_y
                )
            } else {
                format!(
                    "Matched color #{:02X}{:02X}{:02X} at {moved_x}, {moved_y} with tolerance {} and offset {:+}, {:+}.",
                    hit.matched_color.r,
                    hit.matched_color.g,
                    hit.matched_color.b,
                    preset.color_tolerance,
                    preset.move_offset_x,
                    preset.move_offset_y
                )
            },
        });
    }

    let template_file = image_search_template_file(preset.id);
    if !template_file.exists() {
        bail!("No image template has been captured yet.");
    }

    let current_modified = std::fs::metadata(&template_file)
        .and_then(|meta| meta.modified())
        .ok();
    let cached_data = {
        let cache = TEMPLATE_CACHE.lock();
        cache.get(&preset.id).cloned()
    };
    let use_cache = if let Some(ref cached) = cached_data {
        cached.modified == current_modified
    } else {
        false
    };
    let (template_rgba, template_width, template_height) = if use_cache {
        let cached = cached_data.unwrap();
        (cached.rgba, cached.width, cached.height)
    } else {
        let template = image::open(&template_file)
            .with_context(|| format!("Failed to open template {}", template_file.display()))?
            .to_rgba8();
        let w = template.width() as usize;
        let h = template.height() as usize;
        let rgba = template.into_raw();
        let new_cached = CachedTemplate {
            rgba: rgba.clone(),
            width: w,
            height: h,
            modified: current_modified,
        };
        let mut cache = TEMPLATE_CACHE.lock();
        cache.insert(preset.id, new_cached);
        (rgba, w, h)
    };
    let anchor_hint = match (preset.last_capture_screen_x, preset.last_capture_screen_y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    let configured_region = configured_image_search_region(preset);
    let used_roi_capture = configured_region.is_some()
        || (preset.target_window_title.is_none()
            && preset.extra_target_window_titles.is_empty()
            && anchor_hint.is_some());
    let screen = if let Some(region) = configured_region {
        let region =
            expand_search_region_to_fit(region, template_width as i32, template_height as i32);
        window_list::capture_virtual_screen_region(
            region.left,
            region.top,
            region.width,
            region.height,
        )
        .context("Failed to capture the selected search area")?
    } else if preset.target_window_title.is_some() || !preset.extra_target_window_titles.is_empty()
    {
        window_list::capture_window_region_with_candidates(
            preset.target_window_title.as_deref(),
            &preset.extra_target_window_titles,
            preset.match_duplicate_window_titles,
        )
        .context("Failed to capture the target window")?
    } else if let Some((capture_x, capture_y)) = anchor_hint {
        capture_near_last_image_search_region(capture_x, capture_y, template_width, template_height)
            .context("Failed to capture the area near the original template")?
    } else {
        let (left, top, width, height) = window_list::virtual_screen_bounds();
        window_list::capture_virtual_screen_region(left, top, width, height)
            .context("Failed to capture the screen")?
    };
    let fallback_average_diff =
        ((1.0 - preset.confidence_threshold.clamp(0.35, 0.99)) * 48.0).clamp(2.0, 18.0);
    let exact_hit = if used_roi_capture
        || configured_region.is_some()
        || (screen.width <= 960 && screen.height <= 960)
    {
        find_template_match_exact_rgba(
            &screen,
            &template_rgba,
            template_width,
            template_height,
            fallback_average_diff,
            anchor_hint,
            configured_region.as_ref(),
        )
    } else {
        None
    };
    let scales = [1.0_f32, 0.9, 1.1, 0.8, 1.2, 1.33];
    let opencv_hit = find_template_match_opencv(
        &screen,
        &template_rgba,
        template_width,
        template_height,
        &scales,
        anchor_hint,
        false,
        configured_region.as_ref(),
    )?;
    let hit = match (exact_hit, opencv_hit) {
        (Some(exact), Some(opencv)) => {
            if exact.confidence > opencv.confidence + 0.08 {
                exact
            } else {
                opencv
            }
        }

        (Some(exact), None) => exact,
        (None, Some(opencv)) => opencv,
        (None, None) => {
            if configured_region.is_some() {
                set_found_var(false);
                return Ok(VisionRunOutcome {
                    matched: false,
                    status: "No match found inside the selected search area.".to_owned(),
                });
            }

            if used_roi_capture {
                set_found_var(false);
                return Ok(VisionRunOutcome {
                    matched: false,
                    status: "No match found near the captured area.".to_owned(),
                });
            }

            set_found_var(false);
            return Ok(VisionRunOutcome {
                matched: false,
                status: "No match found on screen.".to_owned(),
            });
        }
    };
    let center_x = screen.screen_x + hit.x + (hit.width / 2);
    let center_y = screen.screen_y + hit.y + (hit.height / 2);
    let moved_x = center_x + preset.move_offset_x;
    let moved_y = center_y + preset.move_offset_y;
    let required_confidence = preset.confidence_threshold.clamp(0.35, 0.99);
    if hit.confidence < required_confidence {
        set_found_var(false);
        return Ok(VisionRunOutcome {
            matched: false,
            status: format!(
                "Best match near {moved_x}, {moved_y} scored {:.3} at scale {:.2}x, below threshold {:.2}.",
                hit.confidence, hit.scale, required_confidence
            ),
        });
    }

    if let Some(var_x) = pos_var_x.filter(|s| !s.trim().is_empty()) {
        set_variable_value(var_x, moved_x as f64);
    }

    if let Some(var_y) = pos_var_y.filter(|s| !s.trim().is_empty()) {
        set_variable_value(var_y, moved_y as f64);
    }
    set_found_var(true);

    if move_cursor {
        settle_image_search_mouse_move(
            moved_x,
            moved_y,
            preset.non_interception_move_passes,
            preset.non_interception_move_delay_ms,
        )?;
    }

    if fire_click {
        thread::sleep(Duration::from_millis(12));
        send_mouse_left_click_backend()?;
    }

    Ok(VisionRunOutcome {
        matched: true,
        status: format!(
            "OpenCV matched at {moved_x}, {moved_y} with confidence {:.3} on {:.2}x (offset {:+}, {:+}).",
            hit.confidence, hit.scale, preset.move_offset_x, preset.move_offset_y
        ),
    })
}

pub(crate) fn run_vision_once(preset: &VisionPreset) -> Result<String> {
    run_vision_once_with_options(
        preset,
        true,
        preset.click_after_move,
        None,
        None,
        None,
        None,
    )
    .map(|outcome| outcome.status)
}

pub(crate) fn run_image_search_follow_loop(
    preset: VisionPreset,
    ui_tx: Option<crossbeam_channel::Sender<UiCommand>>,
    variable_override: Option<String>,
) {
    if let Some(tx) = ui_tx.as_ref() {
        let _ = tx.send(UiCommand::VisionFinished(format!(
            "{}: repeat mode started. Press the hotkey again to stop.",
            preset.name
        )));
    }

    while image_search_following_is_active(preset.id) {
        match run_vision_once_with_options(
            &preset,
            true,
            false,
            variable_override.as_deref(),
            None,
            None,
            None,
        ) {
            Ok(_) => {}

            Err(error) => {
                if let Some(tx) = ui_tx.as_ref() {
                    let _ = tx.send(UiCommand::VisionFinished(format!(
                        "{}: Vision search failed: {error}",
                        preset.name
                    )));
                }

                break;
            }
        }

        let rate_hz = preset.color_scan_rate_hz.max(1);
        let sleep_duration = Duration::from_nanos(1_000_000_000 / rate_hz as u64);
        thread::sleep(sleep_duration);
    }

    set_image_search_following_active(preset.id, false);
    if let Some(tx) = ui_tx {
        let _ = tx.send(UiCommand::VisionFinished(format!(
            "{}: repeat mode stopped.",
            preset.name
        )));
    }
}

pub(crate) fn trigger_vision_move(spec: &str) -> Result<()> {
    let preset = vision_preset_by_id(spec)?;
    let status = run_vision_once(&preset)?;
    if let Some(tx) = HOOK_STATE.lock().ui_tx.clone() {
        let _ = tx.send(UiCommand::VisionFinished(format!(
            "{}: {status}",
            preset.name
        )));
    }

    Ok(())
}

use super::{
    MacroRunFlow, MouseMoveLockMask, STOP_REQUESTED_MACRO_PRESETS, macro_runtime_target_matches,
    trigger_nested_macro_preset,
};

pub(crate) fn trigger_vision_move_with_options(
    preset: &VisionPreset,
    move_cursor: bool,
    wait_until_found: bool,
    trigger_macro_enabled: bool,
    trigger_macro_preset_id: Option<u32>,
    macro_preset_id: u32,
    press_locked_keys: &mut Vec<String>,
    press_locked_mouse_masks: &mut Vec<MouseMoveLockMask>,
    stop_immediately_on_retrigger: bool,
    target_window_title: Option<&str>,
    extra_target_window_titles: &[String],
    match_duplicate_window_titles: bool,
    variable_override: Option<&str>,
) -> MacroRunFlow {
    let ui_tx = HOOK_STATE.lock().ui_tx.clone();
    let wait_generation = image_search_wait_generation(preset.id);
    let mut sent_wait_status = false;
    loop {
        if !macro_runtime_target_matches(
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
        ) {
            return MacroRunFlow::StopExecution;
        }

        if stop_immediately_on_retrigger
            && STOP_REQUESTED_MACRO_PRESETS
                .lock()
                .contains(&macro_preset_id)
        {
            return MacroRunFlow::StopExecution;
        }

        if image_search_wait_generation(preset.id) != wait_generation {
            if let Some(tx) = ui_tx.as_ref() {
                let _ = tx.send(UiCommand::VisionFinished(format!(
                    "{}: waiting cancelled.",
                    preset.name
                )));
            }

            return MacroRunFlow::Continue;
        }

        let outcome = match run_vision_once_with_options(
            preset,
            move_cursor,
            false,
            variable_override,
            None,
            None,
            None,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!("Vision macro step failed: {error}");
                return MacroRunFlow::Continue;
            }
        };
        if outcome.matched {
            if let Some(tx) = ui_tx.as_ref() {
                let _ = tx.send(UiCommand::VisionFinished(format!(
                    "{}: {}",
                    preset.name, outcome.status
                )));
            }

            if trigger_macro_enabled {
                if let Some(trigger_preset_id) = trigger_macro_preset_id {
                    let _ = trigger_nested_macro_preset(
                        &trigger_preset_id.to_string(),
                        press_locked_keys,
                        press_locked_mouse_masks,
                        stop_immediately_on_retrigger,
                        target_window_title,
                        extra_target_window_titles,
                        match_duplicate_window_titles,
                        true,
                    );
                }
            }

            return MacroRunFlow::Continue;
        }

        if !wait_until_found {
            return MacroRunFlow::Continue;
        }

        if !sent_wait_status {
            if let Some(tx) = ui_tx.as_ref() {
                let _ = tx.send(UiCommand::VisionFinished(format!(
                    "{}: waiting...",
                    preset.name
                )));
            }

            sent_wait_status = true;
        }

        thread::sleep(Duration::from_millis(25));
    }
}
