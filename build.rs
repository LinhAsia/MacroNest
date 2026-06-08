use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use image::{ColorType, ImageEncoder, codecs::ico::IcoEncoder};
use tiny_skia::Pixmap;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=assets/app-icon.svg");
    println!("cargo:rerun-if-changed=assets/app-icon-disabled.svg");
    let build_tag = normalize_version_tag(&env::var("CARGO_PKG_VERSION").unwrap_or_default());
    if !build_tag.is_empty() {
        println!("cargo:rustc-env=MACRONEST_BUILD_TAG={build_tag}");
    }

    #[cfg(windows)]
    {
        embed_windows_icon()?;
        println!("cargo:rustc-link-arg=/DELAYLOAD:opencv_world4100.dll");
        println!("cargo:rustc-link-arg=delayimp.lib");
    }

    Ok(())
}

fn normalize_version_tag(version: &str) -> String {
    let mut parts: Vec<&str> = version.split('.').collect();
    while parts.last() == Some(&"0") {
        parts.pop();
    }
    parts.join(".")
}

#[cfg(windows)]
fn embed_windows_icon() -> Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let ico_path = out_dir.join("macronest-app.ico");
    let rc_path = out_dir.join("macronest-app.rc");
    let res_path = out_dir.join("macronest-app.res");
    let package_name = env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "MacroNest".to_owned());
    let company_name = "NBaoLinh".to_owned();
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "1.1.0".to_owned());
    let version_parts = parse_version_components(&version);
    let file_version = format!(
        "{},{},{},{}",
        version_parts.0, version_parts.1, version_parts.2, version_parts.3
    );
    let version_string = format!(
        "{}.{}.{}.{}",
        version_parts.0, version_parts.1, version_parts.2, version_parts.3
    );

    render_svg_icon_to_ico(&manifest_dir.join("assets/app-icon.svg"), &ico_path, 256)?;

    fs::write(
        &rc_path,
        format!(
            r#"
1 ICON "{icon_path}"
1 VERSIONINFO
FILEVERSION {file_version}
PRODUCTVERSION {file_version}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "CompanyName", "{company_name}"
            VALUE "FileDescription", "{package_name}"
            VALUE "FileVersion", "{version_string}"
            VALUE "InternalName", "{package_name}"
            VALUE "LegalCopyright", "Copyright (c) 2026 {company_name}"
            VALUE "OriginalFilename", "{package_name}.exe"
            VALUE "ProductName", "{package_name}"
            VALUE "ProductVersion", "{version_string}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#,
            icon_path = ico_path.display().to_string().replace('\\', "/"),
            file_version = file_version,
            version_string = version_string,
            company_name = company_name,
            package_name = package_name,
        ),
    )
    .with_context(|| format!("Failed to write resource file {}", rc_path.display()))?;

    compile_resource(&rc_path, &res_path)?;
    println!("cargo:rustc-link-arg={}", res_path.display());
    Ok(())
}

fn parse_version_components(version: &str) -> (u16, u16, u16, u16) {
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .collect::<Vec<_>>();
    while parts.len() < 4 {
        parts.push(0);
    }
    (parts[0], parts[1], parts[2], parts[3])
}

#[cfg(windows)]
fn compile_resource(rc_path: &Path, res_path: &Path) -> Result<()> {
    let status = Command::new("llvm-rc")
        .args(["/nologo", "/FO"])
        .arg(res_path)
        .arg(rc_path)
        .status()
        .context("Failed to launch llvm-rc")?;
    if status.success() {
        return Ok(());
    }

    let status = Command::new("windres")
        .args([
            "--input-format=rc",
            "--output-format=res",
            "-o",
            &res_path.display().to_string(),
            &rc_path.display().to_string(),
        ])
        .status()
        .context("Failed to launch windres")?;
    if status.success() {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "Failed to compile Windows resources with llvm-rc or windres"
    ))
}

#[cfg(windows)]
fn render_svg_icon_to_ico(svg_path: &Path, ico_path: &Path, size: u32) -> Result<()> {
    let svg = fs::read_to_string(svg_path)
        .with_context(|| format!("Failed to read SVG icon {}", svg_path.display()))?;
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(&svg, &options)
        .context("Failed to parse app icon SVG for exe resource")?;
    let scale = (size as f32 / tree.size().width()).min(size as f32 / tree.size().height());
    let width = (tree.size().width() * scale).round().max(1.0) as u32;
    let height = (tree.size().height() * scale).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(width, height).context("Failed to create icon pixmap")?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let file = fs::File::create(ico_path)
        .with_context(|| format!("Failed to create icon file {}", ico_path.display()))?;
    let encoder = IcoEncoder::new(file);
    encoder.write_image(
        pixmap.data(),
        pixmap.width(),
        pixmap.height(),
        ColorType::Rgba8.into(),
    )?;
    Ok(())
}
