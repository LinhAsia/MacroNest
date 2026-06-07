use std::{fs, path::Path};

use anyhow::{Context, Result};
use eframe::egui::IconData;
use image::{codecs::ico::IcoEncoder, ColorType, ImageEncoder};
use tiny_skia::Pixmap;

const APP_ICON_SVG: &str = include_str!("../assets/app-icon.svg");

const APP_ICON_DISABLED_SVG: &str = include_str!("../assets/app-icon-disabled.svg");

pub fn icon_data(size: u32) -> Result<IconData> {
    let pixmap = render_pixmap(size, false)?;
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
