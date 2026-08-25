use std::{fs, path::Path};

use anyhow::{Context, Result};
use eframe::egui::IconData;
use image::{ColorType, ImageEncoder, codecs::ico::IcoEncoder};
use tiny_skia::Pixmap;

const APP_ICON_SVG: &str = include_str!("../assets/app-icon.svg");

const APP_ICON_DISABLED_SVG: &str = include_str!("../assets/app-icon-disabled.svg");

/// Load icon data from an already-rendered .ico file on disk.
/// Much faster than `icon_data` since it skips SVG parsing and rendering.
pub fn icon_data_from_ico_file(path: &Path) -> Result<IconData> {
    let bytes = fs::read(path).context("Failed to read icon file")?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Ico)
        .context("Failed to decode icon file")?
        .into_rgba8();
    let width = image.width();
    let height = image.height();
    Ok(IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

pub fn icon_data(size: u32) -> Result<IconData> {
    let pixmap = render_pixmap(size, false)?;
    Ok(IconData {
        rgba: pixmap.data().to_vec(),
        width: pixmap.width(),
        height: pixmap.height(),
    })
}

pub fn recording_overlay_badge_icon_data(size: u32) -> Result<IconData> {
    let size = size.clamp(16, 64);
    let mut pixmap = Pixmap::new(size, size).context("Failed to create badge pixmap")?;
    let center = size as f32 * 0.5;
    let radius = (size as f32 * 0.42).max(4.0);

    let mut paint = tiny_skia::Paint::default();
    paint.anti_alias = true;

    // Dark outline ring for high contrast
    paint.set_color_rgba8(15, 23, 42, 230);
    let outer_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_circle(center, center, radius + 1.5);
        pb.finish()
    };
    if let Some(path) = outer_path {
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    // Glowing vibrant red recording circle
    paint.set_color_rgba8(239, 68, 68, 255);
    let inner_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_circle(center, center, radius);
        pb.finish()
    };
    if let Some(path) = inner_path {
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    // Specular shine dot in top-left of circle
    paint.set_color_rgba8(255, 255, 255, 220);
    let shine_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_circle(
            center - radius * 0.3,
            center - radius * 0.3,
            radius * 0.32,
        );
        pb.finish()
    };
    if let Some(path) = shine_path {
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    Ok(IconData {
        rgba: pixmap.data().to_vec(),
        width: pixmap.width(),
        height: pixmap.height(),
    })
}

pub fn ensure_ico_file(path: &Path, size: u32) -> Result<()> {
    ensure_ico_file_variant(path, size, false)
}

pub fn ensure_disabled_ico_file(path: &Path, size: u32) -> Result<()> {
    ensure_ico_file_variant(path, size, true)
}

fn ensure_ico_file_variant(path: &Path, size: u32, disabled: bool) -> Result<()> {
    if path.is_file()
        && fs::metadata(path)
            .map(|meta| meta.len() > 0)
            .unwrap_or(false)
    {
        return Ok(());
    }
    let pixmap = render_pixmap(size, disabled)?;
    let file = fs::File::create(path)
        .with_context(|| format!("Failed to create icon file {}", path.display()))?;
    let encoder = IcoEncoder::new(file);
    encoder.write_image(
        pixmap.data(),
        pixmap.width(),
        pixmap.height(),
        ColorType::Rgba8.into(),
    )?;
    Ok(())
}

fn render_pixmap(size: u32, disabled: bool) -> Result<Pixmap> {
    let options = resvg::usvg::Options::default();
    let svg = if disabled {
        APP_ICON_DISABLED_SVG
    } else {
        APP_ICON_SVG
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .context("Failed to parse the embedded icon SVG")?;
    let scale = (size as f32 / tree.size().width()).min(size as f32 / tree.size().height());
    let width = (tree.size().width() * scale).round().max(1.0) as u32;
    let height = (tree.size().height() * scale).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(width, height).context("Failed to create icon pixmap")?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}
