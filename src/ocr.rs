use anyhow::{Context, Result, anyhow, bail};
use directories::ProjectDirs;
use image::{DynamicImage, ImageBuffer, Rgba, imageops::FilterType};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const OCR_ACTIVE_CODE: &str = "active";
pub const OCR_DEFAULT_CODE: &str = "multilingual";

const OCR_MODELS_BASE_URL: &str =
    "https://raw.githubusercontent.com/zibo-chen/rust-paddle-ocr/main/models";
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

static ACTIVE_LANGUAGE_CODE: Lazy<Mutex<String>> =
    Lazy::new(|| Mutex::new(OCR_DEFAULT_CODE.to_owned()));

pub fn ocr_language_packs() -> &'static [OcrLanguagePack] {
    OCR_LANGUAGE_PACKS
}

pub fn normalize_language_code(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == OCR_ACTIVE_CODE {
        return active_language_code();
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

pub fn active_language_code() -> String {
    ACTIVE_LANGUAGE_CODE
        .lock()
        .map(|value| value.clone())
        .unwrap_or_else(|_| OCR_DEFAULT_CODE.to_owned())
}

pub fn active_language_label() -> &'static str {
    label_for_language_code(&active_language_code())
}

pub fn set_active_language_code(value: &str) {
    let normalized = normalize_language_code(value);
    if let Ok(mut active) = ACTIVE_LANGUAGE_CODE.lock() {
        *active = normalized;
    }
}

#[cfg(windows)]
fn resolve_requested_language(value: &str) -> String {
    if value.trim().is_empty() || value == OCR_ACTIVE_CODE {
        active_language_code()
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
    let dirs = ProjectDirs::from("com", "", "MacroNest")
        .context("Failed to locate the MacroNest data folder for OCR models")?;
    let dir = dirs.data_local_dir().join("ocr-models");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(windows)]
fn ensure_file_downloaded(path: &Path, file_name: &str) -> Result<()> {
    if path.exists() && path.metadata().map(|meta| meta.len() > 0).unwrap_or(false) {
        return Ok(());
    }

    let url = format!("{OCR_MODELS_BASE_URL}/{file_name}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent("MacroNest")
        .build()
        .context("Failed to create OCR download client")?;
    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("Failed to download OCR model from {url}"))?
        .error_for_status()
        .with_context(|| format!("OCR model download returned an error for {url}"))?;

    let bytes = response
        .bytes()
        .with_context(|| format!("Failed to read OCR model payload from {url}"))?;
    if bytes.is_empty() {
        bail!("Downloaded OCR model file is empty: {file_name}");
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("tmp");
    let mut file = fs::File::create(&temp_path)
        .with_context(|| format!("Failed to create temporary OCR model file {}", temp_path.display()))?;
    file.write_all(bytes.as_ref())
        .with_context(|| format!("Failed to write OCR model file {}", temp_path.display()))?;
    file.flush()?;
    drop(file);
    fs::rename(&temp_path, path)
        .with_context(|| format!("Failed to move OCR model file into place {}", path.display()))?;
    Ok(())
}

#[cfg(windows)]
fn ensure_model_files(pack: &OcrLanguagePack) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let root = ocr_models_dir()?;
    let det_path = root.join(OCR_DET_MODEL_FILE);
    let rec_path = root.join(pack.rec_model_file);
    let charset_path = root.join(pack.charset_file);

    ensure_file_downloaded(&det_path, OCR_DET_MODEL_FILE)?;
    ensure_file_downloaded(&rec_path, pack.rec_model_file)?;
    ensure_file_downloaded(&charset_path, pack.charset_file)?;

    Ok((det_path, rec_path, charset_path))
}

#[cfg(windows)]
fn build_engine(pack: &OcrLanguagePack) -> Result<ocr_rs::OcrEngine> {
    let (det_path, rec_path, charset_path) = ensure_model_files(pack)?;
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
