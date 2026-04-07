use std::{fs, path::Path};

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView, ImageReader, imageops::FilterType};
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform};

use crate::model::{CrosshairStyle, RgbaColor};

pub struct RenderedCrosshair {
    pub width: u32,
    pub height: u32,
    pub center_x: i32,
    pub center_y: i32,
    pub rgba: Vec<u8>,
}

pub fn render_crosshair(
    style: &CrosshairStyle,
    custom_asset: Option<&Path>,
) -> Result<RenderedCrosshair> {
    if let Some(asset) = custom_asset {
        return render_custom_asset(style, asset);
    }
    render_builtin(style)
}

fn render_builtin(style: &CrosshairStyle) -> Result<RenderedCrosshair> {
    let outline = if style.outline_enabled {
        style.outline_thickness.max(0.0)
    } else {
        0.0
    };
    let horizontal_length = style.horizontal_length.max(0.0);
    let vertical_length = style.vertical_length.max(0.0);
    let extent = style.gap.max(0.0)
        + horizontal_length.max(vertical_length)
        + style.thickness.max(1.0)
        + outline
        + style.center_dot_size.max(0.0);
    let canvas_size = ((extent * 2.0) + 24.0).ceil().max(64.0) as u32;
    let mut pixmap = Pixmap::new(canvas_size, canvas_size).context("Failed to create a pixmap")?;
    let cx = (canvas_size / 2) as f32;
    let cy = (canvas_size / 2) as f32;

    let fill_color = style.color.with_alpha(style.opacity);
    let outline_color = style.outline_color.with_alpha(style.opacity);
    let thickness = style.thickness.max(1.0);
    let gap = style.gap.max(0.0);

    let arms = [
        (
            cx - gap - horizontal_length,
            cy - thickness / 2.0,
            horizontal_length,
            thickness,
        ),
        (cx + gap, cy - thickness / 2.0, horizontal_length, thickness),
        (
            cx - thickness / 2.0,
            cy - gap - vertical_length,
            thickness,
            vertical_length,
        ),
        (cx - thickness / 2.0, cy + gap, thickness, vertical_length),
    ];

    if style.outline_enabled && outline > 0.0 {
        for (x, y, w, h) in arms {
            fill_rect(
                &mut pixmap,
                x - outline,
                y - outline,
                w + outline * 2.0,
                h + outline * 2.0,
                outline_color,
            )?;
        }
    }

    for (x, y, w, h) in arms {
        fill_rect(&mut pixmap, x, y, w, h, fill_color)?;
    }

    if style.center_dot {
        let dot = style.center_dot_size.max(1.0);
        if style.outline_enabled && outline > 0.0 {
            fill_rect(
                &mut pixmap,
                cx - dot / 2.0 - outline,
                cy - dot / 2.0 - outline,
                dot + outline * 2.0,
                dot + outline * 2.0,
                outline_color,
            )?;
        }
        fill_rect(
            &mut pixmap,
            cx - dot / 2.0,
            cy - dot / 2.0,
            dot,
            dot,
            fill_color,
        )?;
    }

    Ok(RenderedCrosshair {
        width: canvas_size,
        height: canvas_size,
        center_x: (canvas_size / 2) as i32,
        center_y: (canvas_size / 2) as i32,
        rgba: pixmap.data().to_vec(),
    })
}

fn render_custom_asset(style: &CrosshairStyle, path: &Path) -> Result<RenderedCrosshair> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();

    let target_size = style.custom_scale.clamp(16.0, 512.0) as u32;

    let pixmap = if ext == "svg" {
        render_svg(path, target_size)?
    } else {
        render_raster(path, target_size)?
    };

    let mut rgba = pixmap.data().to_vec();
    apply_global_alpha(&mut rgba, style.opacity);

    Ok(RenderedCrosshair {
        width: pixmap.width(),
        height: pixmap.height(),
        center_x: (pixmap.width() / 2) as i32,
        center_y: (pixmap.height() / 2) as i32,
        rgba,
    })
}

fn render_svg(path: &Path, target_size: u32) -> Result<Pixmap> {
    let options = resvg::usvg::Options::default();
    let bytes = fs::read(path).with_context(|| format!("Failed to read SVG {}", path.display()))?;
    let tree = resvg::usvg::Tree::from_data(&bytes, &options)
        .with_context(|| format!("Invalid SVG {}", path.display()))?;
    let size = tree.size();
    let scale = (target_size as f32 / size.width()).min(target_size as f32 / size.height());
    let width = (size.width() * scale).round().max(1.0) as u32;
    let height = (size.height() * scale).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(width, height).context("Failed to create an SVG pixmap")?;
    let transform = Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}

fn render_raster(path: &Path, target_size: u32) -> Result<Pixmap> {
    let image = ImageReader::open(path)
        .with_context(|| format!("Failed to open image {}", path.display()))?
        .decode()
        .with_context(|| format!("Failed to decode image {}", path.display()))?;

    let resized = fit_image(image, target_size);
    let rgba = resized.to_rgba8();
    let (width, height) = resized.dimensions();
    let mut pixmap = Pixmap::new(width, height).context("Failed to create a raster pixmap")?;
    pixmap.data_mut().copy_from_slice(rgba.as_raw());
    Ok(pixmap)
}

fn fit_image(image: DynamicImage, target_size: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    let scale = (target_size as f32 / width.max(1) as f32)
        .min(target_size as f32 / height.max(1) as f32)
        .max(0.01);
    let resized_width = (width as f32 * scale).round().max(1.0) as u32;
    let resized_height = (height as f32 * scale).round().max(1.0) as u32;
    image.resize_exact(resized_width, resized_height, FilterType::CatmullRom)
}

fn fill_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: RgbaColor,
) -> Result<()> {
    let rect = Rect::from_xywh(x, y, width, height).context("Invalid rectangle dimensions")?;
    let mut paint = Paint::default();
    paint.set_color(to_skia_color(color));
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    Ok(())
}

fn to_skia_color(color: RgbaColor) -> Color {
    Color::from_rgba8(color.r, color.g, color.b, color.a)
}

fn apply_global_alpha(rgba: &mut [u8], alpha: f32) {
    let factor = alpha.clamp(0.0, 1.0);
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = (pixel[3] as f32 * factor).round() as u8;
    }
}
