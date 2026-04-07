use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

use crate::model::{MacroGroup, MacroPreset, MacroStep};

const PREFIX_STEP: &str = "MN_STEP:";
const PREFIX_PRESET: &str = "MN_PRESET:";
const PREFIX_GROUP: &str = "MN_GROUP:";

const PREFIX_STEP_V2: &str = "MN2_STEP:";
const PREFIX_PRESET_V2: &str = "MN2_PRESET:";
const PREFIX_GROUP_V2: &str = "MN2_GROUP:";

const Z85_ALPHABET: &[u8; 85] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-:+=^!/*?&<>()[]{}@%$#";

fn compress_bytes(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

fn decompress_bytes(data: &[u8], kind: &str) -> Result<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(data);
    let mut decoded = Vec::new();
    decoder
        .read_to_end(&mut decoded)
        .with_context(|| format!("Failed to decompress the {kind} code"))?;
    Ok(decoded)
}

fn z85_encode(bytes: &[u8]) -> String {
    let mut payload = Vec::with_capacity(bytes.len() + 4);
    payload.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    payload.extend_from_slice(bytes);
    while payload.len() % 4 != 0 {
        payload.push(0);
    }

    let mut output = String::with_capacity((payload.len() / 4) * 5);
    for chunk in payload.chunks_exact(4) {
        let mut value = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let mut encoded = [0u8; 5];
        for index in (0..5).rev() {
            encoded[index] = Z85_ALPHABET[(value % 85) as usize];
            value /= 85;
        }
        output.push_str(std::str::from_utf8(&encoded).unwrap_or_default());
    }
    output
}

fn z85_value(byte: u8) -> Option<u32> {
    Z85_ALPHABET
        .iter()
        .position(|candidate| *candidate == byte)
        .map(|index| index as u32)
}

fn z85_decode(encoded: &str) -> Result<Vec<u8>> {
    let bytes = encoded.as_bytes();
    if bytes.len() % 5 != 0 {
        return Err(anyhow::anyhow!("The encoded payload length is invalid"));
    }

    let mut decoded = Vec::with_capacity((bytes.len() / 5) * 4);
    for chunk in bytes.chunks_exact(5) {
        let mut value = 0u32;
        for &byte in chunk {
            let digit = z85_value(byte)
                .ok_or_else(|| anyhow::anyhow!("The encoded payload contains invalid characters"))?;
            value = value
                .checked_mul(85)
                .and_then(|current| current.checked_add(digit))
                .ok_or_else(|| anyhow::anyhow!("The encoded payload is out of range"))?;
        }
        decoded.extend_from_slice(&value.to_be_bytes());
    }

    if decoded.len() < 4 {
        return Err(anyhow::anyhow!("The decoded payload is incomplete"));
    }

    let expected_len =
        u32::from_be_bytes([decoded[0], decoded[1], decoded[2], decoded[3]]) as usize;
    let payload = &decoded[4..];
    if payload.len() < expected_len {
        return Err(anyhow::anyhow!("The decoded payload is truncated"));
    }
    Ok(payload[..expected_len].to_vec())
}

fn encode_v2<T: Serialize>(value: &T, prefix: &str, kind: &str) -> Result<String> {
    let binary = rmp_serde::to_vec_named(value)
        .with_context(|| format!("Failed to serialize the {kind}"))?;
    let compressed = compress_bytes(&binary)?;
    Ok(format!("{prefix}{}", z85_encode(&compressed)))
}

fn decode_v1<T: DeserializeOwned>(encoded: &str, kind: &str) -> Result<T> {
    let compressed = URL_SAFE_NO_PAD
        .decode(encoded)
        .with_context(|| format!("Failed to decode the {kind} code"))?;
    let json = decompress_bytes(&compressed, kind)?;
    serde_json::from_slice(&json).with_context(|| format!("The {kind} code contents are invalid"))
}

fn decode_v2<T: DeserializeOwned>(encoded: &str, kind: &str) -> Result<T> {
    let compressed = z85_decode(encoded).with_context(|| format!("Failed to decode the {kind} code"))?;
    let binary = decompress_bytes(&compressed, kind)?;
    rmp_serde::from_slice(&binary).with_context(|| format!("The {kind} code contents are invalid"))
}

fn decode_any<T: DeserializeOwned>(
    code: &str,
    v2_prefix: &str,
    v1_prefix: &str,
    kind: &str,
) -> Result<T> {
    let payload = code.trim();
    if let Some(encoded) = payload.strip_prefix(v2_prefix) {
        return decode_v2(encoded, kind);
    }
    let encoded = payload
        .strip_prefix(v1_prefix)
        .ok_or_else(|| anyhow::anyhow!("The {kind} code format is invalid"))?;
    decode_v1(encoded, kind)
}

pub fn encode_step(step: &MacroStep) -> Result<String> {
    encode_v2(step, PREFIX_STEP_V2, "step")
}

pub fn decode_step(code: &str) -> Result<MacroStep> {
    decode_any(code, PREFIX_STEP_V2, PREFIX_STEP, "step")
}

pub fn encode_preset(preset: &MacroPreset) -> Result<String> {
    encode_v2(preset, PREFIX_PRESET_V2, "preset")
}

pub fn decode_preset(code: &str) -> Result<MacroPreset> {
    decode_any(code, PREFIX_PRESET_V2, PREFIX_PRESET, "preset")
}

pub fn encode_group(group: &MacroGroup) -> Result<String> {
    encode_v2(group, PREFIX_GROUP_V2, "group")
}

pub fn decode_group(code: &str) -> Result<MacroGroup> {
    decode_any(code, PREFIX_GROUP_V2, PREFIX_GROUP, "group")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_round_trip_v2() {
        let step = MacroStep::default();
        let encoded = encode_step(&step).expect("encode step");
        let decoded = decode_step(&encoded).expect("decode step");
        assert_eq!(decoded, step);
    }

    #[test]
    fn preset_round_trip_v2() {
        let preset = MacroPreset::default();
        let encoded = encode_preset(&preset).expect("encode preset");
        let decoded = decode_preset(&encoded).expect("decode preset");
        assert_eq!(decoded, preset);
    }

    #[test]
    fn group_round_trip_v2() {
        let group = MacroGroup::default();
        let encoded = encode_group(&group).expect("encode group");
        let decoded = decode_group(&encoded).expect("decode group");
        assert_eq!(decoded, group);
    }
}
