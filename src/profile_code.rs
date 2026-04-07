use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use std::io::{Read, Write};

use crate::model::CrosshairStyle;

const PREFIX: &str = "CH1:";

pub fn encode_style(style: &CrosshairStyle) -> Result<String> {
    let json = serde_json::to_vec(style).context("Failed to serialize the crosshair")?;
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&json)?;
    let compressed = encoder.finish()?;
    Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(compressed)))
}

pub fn decode_style(code: &str) -> Result<CrosshairStyle> {
    let payload = code.trim();
    let encoded = payload
        .strip_prefix(PREFIX)
        .ok_or_else(|| anyhow::anyhow!("The crosshair code format is invalid"))?;
    let compressed = URL_SAFE_NO_PAD
        .decode(encoded)
        .context("Failed to decode the crosshair code")?;
    let mut decoder = DeflateDecoder::new(compressed.as_slice());
    let mut json = Vec::new();
    decoder
        .read_to_end(&mut json)
        .context("Failed to decompress the crosshair code")?;
    let style: CrosshairStyle =
        serde_json::from_slice(&json).context("The crosshair code contents are invalid")?;

    if style.horizontal_length < 0.0
        || style.vertical_length < 0.0
        || style.arm_length < 0.0
        || style.thickness < 0.0
        || style.gap < 0.0
    {
        bail!("The crosshair code contains invalid values");
    }

    Ok(style)
}
