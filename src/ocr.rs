use anyhow::{Result, bail};
use once_cell::sync::Lazy;
use parking_lot::Mutex;

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
static AVAILABLE_OCR_LANGUAGES_CACHE: Lazy<Mutex<Option<Vec<String>>>> =
    Lazy::new(|| Mutex::new(None));

pub const OCR_SUPPORTED_LANGUAGE_CATALOG: &[(&str, &str, &str)] = &[
    ("en-US", "English (en-US)", "Install via Windows OCR capabilities"),
    ("zh-CN", "Chinese Simplified (zh-CN)", "Install via Windows OCR capabilities"),
    ("zh-HK", "Chinese Traditional Hong Kong (zh-HK)", "Install via Windows OCR capabilities"),
    ("zh-TW", "Chinese Traditional Taiwan (zh-TW)", "Install via Windows OCR capabilities"),
    ("ja-JP", "Japanese (ja-JP)", "Install via Windows OCR capabilities"),
    ("ko-KR", "Korean (ko-KR)", "Install via Windows OCR capabilities"),
    ("de-DE", "German (de-DE)", "Install via Windows OCR capabilities"),
    ("fr-FR", "French (fr-FR)", "Install via Windows OCR capabilities"),
    ("fr-CA", "French Canada (fr-CA)", "Install via Windows OCR capabilities"),
    ("es-ES", "Spanish (es-ES)", "Install via Windows OCR capabilities"),
    ("es-MX", "Spanish Mexico (es-MX)", "Install via Windows OCR capabilities"),
    ("it-IT", "Italian (it-IT)", "Install via Windows OCR capabilities"),
    ("pt-BR", "Portuguese Brazil (pt-BR)", "Install via Windows OCR capabilities"),
    ("pt-PT", "Portuguese Portugal (pt-PT)", "Install via Windows OCR capabilities"),
    ("ru-RU", "Russian (ru-RU)", "Install via Windows OCR capabilities"),
    ("ar-SA", "Arabic (ar-SA)", "Install via Windows OCR capabilities"),
    ("bg-BG", "Bulgarian (bg-BG)", "Install via Windows OCR capabilities"),
    ("bs-LATN-BA", "Bosnian Latin (bs-LATN-BA)", "Install via Windows OCR capabilities"),
    ("cs-CZ", "Czech (cs-CZ)", "Install via Windows OCR capabilities"),
    ("da-DK", "Danish (da-DK)", "Install via Windows OCR capabilities"),
    ("el-GR", "Greek (el-GR)", "Install via Windows OCR capabilities"),
    ("fi-FI", "Finnish (fi-FI)", "Install via Windows OCR capabilities"),
    ("hr-HR", "Croatian (hr-HR)", "Install via Windows OCR capabilities"),
    ("hu-HU", "Hungarian (hu-HU)", "Install via Windows OCR capabilities"),
    ("nb-NO", "Norwegian Bokmal (nb-NO)", "Install via Windows OCR capabilities"),
    ("nl-NL", "Dutch (nl-NL)", "Install via Windows OCR capabilities"),
    ("pl-PL", "Polish (pl-PL)", "Install via Windows OCR capabilities"),
    ("ro-RO", "Romanian (ro-RO)", "Install via Windows OCR capabilities"),
    ("sk-SK", "Slovak (sk-SK)", "Install via Windows OCR capabilities"),
    ("sl-SI", "Slovenian (sl-SI)", "Install via Windows OCR capabilities"),
    ("sr-CYRL-RS", "Serbian Cyrillic (sr-CYRL-RS)", "Install via Windows OCR capabilities"),
    ("sr-LATN-RS", "Serbian Latin (sr-LATN-RS)", "Install via Windows OCR capabilities"),
    ("sv-SE", "Swedish (sv-SE)", "Install via Windows OCR capabilities"),
    ("tr-TR", "Turkish (tr-TR)", "Install via Windows OCR capabilities"),
];

#[cfg(windows)]
pub fn available_ocr_languages() -> Vec<String> {
    if let Some(cached) = AVAILABLE_OCR_LANGUAGES_CACHE.lock().clone() {
        return cached;
    }

    use windows::Media::Ocr::OcrEngine;
    let languages = match OcrEngine::AvailableRecognizerLanguages() {
        Ok(langs) => langs
            .into_iter()
            .filter_map(|l| l.LanguageTag().ok().map(|t| t.to_string()))
            .collect(),
        Err(_) => vec![],
    };

    *AVAILABLE_OCR_LANGUAGES_CACHE.lock() = Some(languages.clone());
    languages
}

#[cfg(windows)]
pub fn clear_available_ocr_languages_cache() {
    *AVAILABLE_OCR_LANGUAGES_CACHE.lock() = None;
}

#[cfg(not(windows))]
pub fn clear_available_ocr_languages_cache() {}

pub fn ocr_capability_name(lang_code: &str) -> Option<String> {
    let code = lang_code.trim();
    if code.is_empty() {
        None
    } else {
        Some(format!("Language.OCR~~~{}~0.0.1.0", code))
    }
}

pub fn language_tag_matches(tags: &[String], code: &str) -> bool {
    let code_lower = code.to_lowercase();
    tags.iter().any(|tag| {
        let tag_lower = tag.to_lowercase();
        tag_lower == code_lower
            || tag_lower.starts_with(&(code_lower.clone() + "-"))
            || code_lower.starts_with(&(tag_lower + "-"))
    })
}

#[cfg(not(windows))]
pub fn available_ocr_languages() -> Vec<String> {
    vec![]
}

#[cfg(windows)]
fn friendly_lang_not_installed_msg(language_code: &str) -> String {
    let lang_name = match language_code {
        "en" | "en-US" => "English",
        "vi" => "Tiếng Việt",
        "zh-Hans" => "简体中文 (Simplified Chinese)",
        "zh-Hant" => "繁體中文 (Traditional Chinese)",
        "ja" => "日本語 (Japanese)",
        "ko" => "한국어 (Korean)",
        "fr" => "Français (French)",
        "de" => "Deutsch (German)",
        "es" => "Español (Spanish)",
        "ru" => "Русский (Russian)",
        "th" => "ไทย (Thai)",
        other => other,
    };
    format!(
        "Ngôn ngữ OCR '{}' chưa được cài đặt trên Windows.\n\
        Vui lòng vào Settings → Time & Language → Language & Region → Add a language\n\
        và cài thêm gói Optional features → Basic Typing / OCR cho ngôn ngữ đó.",
        lang_name
    )
}

#[cfg(windows)]
pub fn perform_ocr(rgba_bytes: &[u8], width: u32, height: u32, lang: &str) -> Result<OcrResult> {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::core::HSTRING;

    if rgba_bytes.is_empty() || width == 0 || height == 0 {
        bail!("Empty image or invalid dimensions");
    }

    let mut w = width;
    let mut h = height;
    let mut rgba_vec = rgba_bytes.to_vec();
    let mut scale_factor = 1;

    // Windows OCR requires width and height of the image to be at least 40 pixels.
    // If the captured region is too small, we upscale it to improve detection accuracy.
    if w < 120 || h < 120 {
        scale_factor = if w < 40 || h < 40 { 4 } else { 2 };
        let new_w = w * scale_factor;
        let new_h = h * scale_factor;
        if let Some(img) =
            image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, rgba_vec.clone())
        {
            let resized_img =
                image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Triangle);
            rgba_vec = resized_img.into_raw();
            w = new_w;
            h = new_h;
        }
    }

    // Convert RGBA to PNG in memory
    let mut png_bytes = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, rgba_vec)
            .ok_or_else(|| anyhow::anyhow!("Failed to create ImageBuffer from raw pixels"))?;
        img.write_to(&mut cursor, image::ImageFormat::Png)?;
    }

    // Create InMemoryRandomAccessStream
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(&png_bytes)?;
    writer.StoreAsync()?.get()?;
    writer.FlushAsync()?.get()?;

    // Seek to beginning of stream
    stream.Seek(0)?;

    // Create SoftwareBitmap via BitmapDecoder
    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;

    // Initialize Windows OCR Engine
    let ocr_engine = if lang.trim().is_empty() {
        // Try creating from user preferred languages
        match OcrEngine::TryCreateFromUserProfileLanguages() {
            Ok(engine) => engine,
            Err(_) => {
                // Fallback to English
                let language = Language::CreateLanguage(&HSTRING::from("en-US"))?;
                // Check if English is supported before trying
                if !OcrEngine::IsLanguageSupported(&language).unwrap_or(false) {
                    bail!(
                        "No OCR language pack is installed on this Windows system. Please go to Settings → Time & Language → Language & Region to install a language with OCR support."
                    );
                }
                match OcrEngine::TryCreateFromLanguage(&language) {
                    Ok(engine) => engine,
                    Err(e) => bail!("Failed to create OCR engine for English: {}", e),
                }
            }
        }
    } else {
        let language_code = lang.trim();
        let language = Language::CreateLanguage(&HSTRING::from(language_code))?;

        // Check if language pack is installed before trying to create engine
        if !OcrEngine::IsLanguageSupported(&language).unwrap_or(false) {
            bail!("{}", friendly_lang_not_installed_msg(language_code));
        }

        match OcrEngine::TryCreateFromLanguage(&language) {
            Ok(engine) => engine,
            Err(e) => {
                bail!(
                    "{}\n(Details: {})",
                    friendly_lang_not_installed_msg(language_code),
                    e
                );
            }
        }
    };

    // Recognize text
    let ocr_result_async = ocr_engine.RecognizeAsync(&bitmap)?;
    let ocr_result = ocr_result_async.get()?;

    let text = ocr_result.Text()?.to_string();
    let lines = ocr_result.Lines()?;
    let mut words = Vec::new();

    for line in lines {
        let line_words = line.Words()?;
        for word in line_words {
            let word_text = word.Text()?.to_string();
            let rect = word.BoundingRect()?;
            words.push(OcrWord {
                text: word_text,
                x: rect.X / scale_factor as f32,
                y: rect.Y / scale_factor as f32,
                width: rect.Width / scale_factor as f32,
                height: rect.Height / scale_factor as f32,
            });
        }
    }

    Ok(OcrResult { text, words })
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
