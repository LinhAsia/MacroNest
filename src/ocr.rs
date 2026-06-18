use anyhow::{Result, bail};

pub const OCR_ENGLISH_CODE: &str = "en-US";
pub const OCR_ENGLISH_LABEL: &str = "English (en-US)";

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
pub fn perform_ocr(
    rgba_bytes: &[u8],
    width: u32,
    height: u32,
    _lang: &str,
) -> Result<OcrResult> {
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

    // Windows OCR needs a reasonable minimum image size to produce stable results.
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

    let mut png_bytes = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, rgba_vec)
            .ok_or_else(|| anyhow::anyhow!("Failed to create ImageBuffer from raw pixels"))?;
        img.write_to(&mut cursor, image::ImageFormat::Png)?;
    }

    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(&png_bytes)?;
    writer.StoreAsync()?.get()?;
    writer.FlushAsync()?.get()?;
    stream.Seek(0)?;

    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;

    let language = Language::CreateLanguage(&HSTRING::from(OCR_ENGLISH_CODE))?;
    if !OcrEngine::IsLanguageSupported(&language).unwrap_or(false) {
        bail!("English OCR is not available on this Windows system.");
    }
    let ocr_engine = OcrEngine::TryCreateFromLanguage(&language)?;

    let ocr_result = ocr_engine.RecognizeAsync(&bitmap)?.get()?;
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
