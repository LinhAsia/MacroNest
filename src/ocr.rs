use anyhow::{Context, Result, anyhow, bail};
use image::{DynamicImage, ImageBuffer, Rgba, imageops::FilterType};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const OCR_DEFAULT_CODE: &str = "multilingual";

const OCR_MODELS_BASE_URL: &str =
    "https://github.com/NBaoLinh/MacroNest/releases/download/tools";
const OCR_DET_MODEL_FILE: &str = "PP-OCRv5_mobile_det.mnn";
const OCR_MULTILINGUAL_REC_FILE: &str = "PP-OCRv5_mobile_rec.mnn";
const OCR_MULTILINGUAL_CHARSET_FILE: &str = "ppocr_keys_v5.txt";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcrLanguagePack {
    pub code: &'static str,
    pub label: &'static str,
    rec_model_file: &'static str,
    charset_file: &'static str,
}

const OCR_LANGUAGE_PACKS: &[OcrLanguagePack] = &[
    OcrLanguagePack {
        code: "multilingual",
        label: "Chinese / English / Japanese",
        rec_model_file: OCR_MULTILINGUAL_REC_FILE,
        charset_file: OCR_MULTILINGUAL_CHARSET_FILE,
    },
    OcrLanguagePack {
        code: "latin",
        label: "Latin / Vietnamese / European",
        rec_model_file: "latin_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_latin.txt",
    },
    OcrLanguagePack {
        code: "korean",
        label: "Korean / English",
        rec_model_file: "korean_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_korean.txt",
    },
    OcrLanguagePack {
        code: "th",
        label: "Thai / English",
        rec_model_file: "th_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_th.txt",
    },
    OcrLanguagePack {
        code: "cyrillic",
        label: "Cyrillic",
        rec_model_file: "cyrillic_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_cyrillic.txt",
    },
    OcrLanguagePack {
        code: "arabic",
        label: "Arabic / Persian / Urdu",
        rec_model_file: "arabic_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_arabic.txt",
    },
    OcrLanguagePack {
        code: "devanagari",
        label: "Hindi / Devanagari",
        rec_model_file: "devanagari_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_devanagari.txt",
    },
    OcrLanguagePack {
        code: "el",
        label: "Greek / English",
        rec_model_file: "el_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_el.txt",
    },
    OcrLanguagePack {
        code: "ta",
        label: "Tamil / English",
        rec_model_file: "ta_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_ta.txt",
    },
    OcrLanguagePack {
        code: "te",
        label: "Telugu / English",
        rec_model_file: "te_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_te.txt",
    },
    OcrLanguagePack {
        code: "en",
        label: "English",
        rec_model_file: "en_PP-OCRv5_mobile_rec_infer.mnn",
        charset_file: "ppocr_keys_en.txt",
    },
];

#[derive(Debug, Clone)]
pub struct OcrWord {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub struct OcrResult {
    pub text: String,
    pub words: Vec<OcrWord>,
}

#[cfg(windows)]
struct OcrEngineBundle {
    engine: Arc<Mutex<ocr_rs::OcrEngine>>,
}

#[cfg(windows)]
static OCR_ENGINE_CACHE: Lazy<Mutex<HashMap<String, OcrEngineBundle>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn ocr_language_packs() -> &'static [OcrLanguagePack] {
    OCR_LANGUAGE_PACKS
}

pub fn normalize_language_code(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "active" {
        return OCR_DEFAULT_CODE.to_owned();
    }
    if OCR_LANGUAGE_PACKS.iter().any(|pack| pack.code == normalized) {
        normalized
    } else {
        OCR_DEFAULT_CODE.to_owned()
    }
}

pub fn label_for_language_code(value: &str) -> &'static str {
    let normalized = normalize_language_code(value);
    OCR_LANGUAGE_PACKS
        .iter()
        .find(|pack| pack.code == normalized)
        .map(|pack| pack.label)
        .unwrap_or(OCR_LANGUAGE_PACKS[0].label)
}

pub fn display_label_for_language_code(value: &str) -> String {
    let label = label_for_language_code(value);
    if is_language_pack_installed(value) {
        label.to_owned()
    } else {
        format!("{label} [not installed]")
    }
}

pub fn compact_label_for_language_code(value: &str) -> &'static str {
    match normalize_language_code(value).as_str() {
        "multilingual" => "CJK",
        "latin" => "Latin",
        "korean" => "Korean",
        "th" => "Thai",
        "cyrillic" => "Cyrillic",
        "arabic" => "Arabic",
        "devanagari" => "Hindi",
        "el" => "Greek",
        "ta" => "Tamil",
        "te" => "Telugu",
        "en" => "English",
        _ => "OCR",
    }
}

pub fn language_pack_for_code_public(value: &str) -> OcrLanguagePack {
    #[cfg(windows)]
    {
        *language_pack_for_code(value)
    }
    #[cfg(not(windows))]
    {
        OCR_LANGUAGE_PACKS[0]
    }
}

#[cfg(windows)]
fn resolve_requested_language(value: &str) -> String {
    if value.trim().is_empty() {
        OCR_DEFAULT_CODE.to_owned()
    } else {
        normalize_language_code(value)
    }
}

#[cfg(windows)]
fn language_pack_for_code(value: &str) -> &'static OcrLanguagePack {
    let normalized = normalize_language_code(value);
    OCR_LANGUAGE_PACKS
        .iter()
        .find(|pack| pack.code == normalized)
        .unwrap_or(&OCR_LANGUAGE_PACKS[0])
}

#[cfg(windows)]
fn ocr_models_dir() -> Result<PathBuf> {
    let dir = crate::storage::AppPaths::discover()?.ocr_dir;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(windows)]
fn model_paths_for_pack(pack: &OcrLanguagePack) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let root = ocr_models_dir()?;
    Ok((
        root.join(OCR_DET_MODEL_FILE),
        root.join(pack.rec_model_file),
        root.join(pack.charset_file),
    ))
}

#[cfg(windows)]
pub fn is_language_pack_installed(value: &str) -> bool {
    let pack = language_pack_for_code(value);
    model_paths_for_pack(pack)
        .map(|(det_path, rec_path, charset_path)| {
            [det_path, rec_path, charset_path]
                .iter()
                .all(|path| path.exists() && path.metadata().map(|meta| meta.len() > 0).unwrap_or(false))
        })
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_language_pack_installed(_value: &str) -> bool {
    false
}

#[cfg(windows)]
pub fn installed_language_pack_size(value: &str) -> u64 {
    let pack = language_pack_for_code(value);
    model_paths_for_pack(pack)
        .map(|(det_path, rec_path, charset_path)| {
            [det_path, rec_path, charset_path]
                .iter()
                .filter_map(|path| path.metadata().ok().map(|meta| meta.len()))
                .sum()
        })
        .unwrap_or(0)
}

#[cfg(not(windows))]
pub fn installed_language_pack_size(_value: &str) -> u64 {
    0
}

#[cfg(windows)]
pub fn install_language_pack<F>(value: &str, mut progress: F) -> Result<()>
where
    F: FnMut(u64, u64),
{
    let pack = language_pack_for_code(value);
    let files = {
        let (det_path, rec_path, charset_path) = model_paths_for_pack(pack)?;
        [
            (det_path, OCR_DET_MODEL_FILE),
            (rec_path, pack.rec_model_file),
            (charset_path, pack.charset_file),
        ]
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent("MacroNest")
        .build()
        .context("Failed to create OCR download client")?;

    let mut total_size = 0_u64;
    for (path, file_name) in &files {
        if path.exists() && path.metadata().map(|meta| meta.len() > 0).unwrap_or(false) {
            total_size = total_size.saturating_add(path.metadata().map(|meta| meta.len()).unwrap_or(0));
            continue;
        }
        let url = format!("{OCR_MODELS_BASE_URL}/{file_name}");
        let size = client
            .head(&url)
            .send()
            .ok()
            .and_then(|response| response.headers().get(reqwest::header::CONTENT_LENGTH).cloned())
            .and_then(|value| value.to_str().ok()?.parse::<u64>().ok())
            .unwrap_or(0);
        total_size = total_size.saturating_add(size.max(1));
    }
    total_size = total_size.max(1);

    let mut downloaded = 0_u64;
    progress(downloaded, total_size);

    for (path, file_name) in files {
        if path.exists() && path.metadata().map(|meta| meta.len() > 0).unwrap_or(false) {
            downloaded = downloaded.saturating_add(path.metadata().map(|meta| meta.len()).unwrap_or(0));
            progress(downloaded.min(total_size), total_size);
            continue;
        }

        let url = format!("{OCR_MODELS_BASE_URL}/{file_name}");
        let mut response = client
            .get(&url)
            .send()
            .with_context(|| format!("Failed to download OCR asset from {url}"))?
            .error_for_status()
            .with_context(|| format!("OCR asset download returned an error for {url}"))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path)
            .with_context(|| format!("Failed to create temporary OCR asset file {}", temp_path.display()))?;

        use std::io::Read;
        let mut buffer = [0u8; 16384];
        loop {
            let count = response.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])?;
            downloaded = downloaded.saturating_add(count as u64);
            progress(downloaded.min(total_size), total_size);
        }
        file.flush()?;
        drop(file);
        fs::rename(&temp_path, &path)
            .with_context(|| format!("Failed to move OCR asset into place {}", path.display()))?;
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn install_language_pack<F>(_value: &str, _progress: F) -> Result<()>
where
    F: FnMut(u64, u64),
{
    bail!("OCR is only supported on Windows.");
}

#[cfg(windows)]
pub fn delete_language_pack(value: &str) -> Result<()> {
    let pack = language_pack_for_code(value);
    OCR_ENGINE_CACHE
        .lock()
        .map_err(|_| anyhow!("OCR engine cache lock was poisoned"))?
        .remove(pack.code);
    let (det_path, rec_path, charset_path) = model_paths_for_pack(pack)?;
    let shared_det_is_still_needed = OCR_LANGUAGE_PACKS
        .iter()
        .filter(|other| other.code != pack.code)
        .any(|other| is_language_pack_installed(other.code));
    let _ = fs::remove_file(&rec_path);
    let _ = fs::remove_file(&charset_path);
    if !shared_det_is_still_needed {
        let _ = fs::remove_file(&det_path);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn delete_language_pack(_value: &str) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
fn build_engine(pack: &OcrLanguagePack) -> Result<ocr_rs::OcrEngine> {
    let (det_path, rec_path, charset_path) = model_paths_for_pack(pack)?;
    for path in [&det_path, &rec_path, &charset_path] {
        if !path.exists() || path.metadata().map(|meta| meta.len() == 0).unwrap_or(true) {
            bail!(
                "OCR pack '{}' is not installed yet. Open Settings > Downloaded Tools and install it first.",
                pack.label
            );
        }
    }
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .min(4) as i32;

    let det_options = ocr_rs::DetOptions::fast()
        .with_max_side_len(1280)
        .with_box_threshold(0.42)
        .with_score_threshold(0.22)
        .with_min_area(6)
        .with_box_border(2);
    let rec_options = ocr_rs::RecOptions::default()
        .with_target_height(48)
        .with_min_score(0.18)
        .with_punct_min_score(0.08)
        .with_batch(false);
    let config = ocr_rs::OcrEngineConfig::fast()
        .with_threads(threads)
        .with_parallel(false)
        .with_min_result_confidence(0.18)
        .with_det_options(det_options)
        .with_rec_options(rec_options);

    ocr_rs::OcrEngine::new(det_path, rec_path, charset_path, Some(config))
        .map_err(|error| anyhow!("Failed to initialize PaddleOCR engine for {}: {error}", pack.label))
}

#[cfg(windows)]
fn engine_for_language(value: &str) -> Result<Arc<Mutex<ocr_rs::OcrEngine>>> {
    let requested = resolve_requested_language(value);
    if let Some(existing) = OCR_ENGINE_CACHE
        .lock()
        .map_err(|_| anyhow!("OCR engine cache lock was poisoned"))?
        .get(&requested)
        .map(|bundle| bundle.engine.clone())
    {
        return Ok(existing);
    }

    let pack = language_pack_for_code(&requested);
    let engine = Arc::new(Mutex::new(build_engine(pack)?));

    let mut cache = OCR_ENGINE_CACHE
        .lock()
        .map_err(|_| anyhow!("OCR engine cache lock was poisoned"))?;
    let entry = cache
        .entry(requested)
        .or_insert_with(|| OcrEngineBundle {
            engine: engine.clone(),
        });
    Ok(entry.engine.clone())
}

#[cfg(windows)]
fn upscale_for_small_text(
    rgba_bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<(DynamicImage, f32)> {
    let image = ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, rgba_bytes.to_vec())
        .ok_or_else(|| anyhow!("Failed to create OCR image buffer from raw pixels"))?;
    let mut scale = 1.0_f32;
    let min_side = width.min(height);

    let dynamic = if min_side < 48 {
        scale = 4.0;
        DynamicImage::ImageRgba8(image::imageops::resize(
            &image,
            width.saturating_mul(4),
            height.saturating_mul(4),
            FilterType::CatmullRom,
        ))
    } else if min_side < 96 {
        scale = 2.0;
        DynamicImage::ImageRgba8(image::imageops::resize(
            &image,
            width.saturating_mul(2),
            height.saturating_mul(2),
            FilterType::CatmullRom,
        ))
    } else {
        DynamicImage::ImageRgba8(image)
    };

    Ok((dynamic, scale))
}

#[cfg(windows)]
pub fn perform_ocr(rgba_bytes: &[u8], width: u32, height: u32, lang: &str) -> Result<OcrResult> {
    if rgba_bytes.is_empty() || width == 0 || height == 0 {
        bail!("Empty image or invalid dimensions");
    }

    let (image, scale) = upscale_for_small_text(rgba_bytes, width, height)?;
    let engine = engine_for_language(lang)?;
    let results = engine
        .lock()
        .map_err(|_| anyhow!("OCR engine lock was poisoned"))?
        .recognize(&image)
        .map_err(|error| anyhow!("PaddleOCR scan failed: {error}"))?;

    let mut text_lines = Vec::new();
    let mut words = Vec::new();

    for item in results {
        if item.text.trim().is_empty() {
            continue;
        }
        let rect = item.bbox.rect;
        text_lines.push(item.text.clone());
        words.push(OcrWord {
            text: item.text,
            x: rect.left() as f32 / scale,
            y: rect.top() as f32 / scale,
            width: rect.width() as f32 / scale,
            height: rect.height() as f32 / scale,
        });
    }

    Ok(OcrResult {
        text: text_lines.join("\n"),
        words,
    })
}

#[cfg(not(windows))]
pub fn perform_ocr(
    _rgba_bytes: &[u8],
    _width: u32,
    _height: u32,
    _lang: &str,
) -> Result<OcrResult> {
    bail!("OCR is only supported on Windows.");
}
