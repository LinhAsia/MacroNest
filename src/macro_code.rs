use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

use crate::model::{
    AudioSensePreset, CommandPreset, EspPreset, GeometryPreset, HudPreset, MacroGroup, MacroPreset,
    MacroStep, MousePathPreset, MouseSensitivityPreset, OcrPreset, PinPreset, ProfileRecord,
    TimerPreset, VisionPreset, WindowFocusPreset, WindowLayout, WindowPreset, ZoomPreset,
};

const PREFIX_STEP: &str = "MN_STEP:";
const PREFIX_PRESET: &str = "MN_PRESET:";
const PREFIX_GROUP: &str = "MN_GROUP:";

const PREFIX_STEP_V2: &str = "MN2_STEP:";
const PREFIX_PRESET_V2: &str = "MN2_PRESET:";
const PREFIX_GROUP_V2: &str = "MN2_GROUP:";

const PREFIX_STEP_V3: &str = "MN3_STEP:";
const PREFIX_PRESET_V3: &str = "MN3_PRESET:";
const PREFIX_GROUP_V3: &str = "MN3_GROUP:";

const PREFIX_STEP_V4: &str = "MN4_STEP:";
const PREFIX_STEP_V5: &str = "MN5_STEP:";
const PREFIX_PRESET_V5: &str = "MN5_PRESET:";
const PREFIX_GROUP_V5: &str = "MN5_GROUP:";

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
            let digit = z85_value(byte).ok_or_else(|| {
                anyhow::anyhow!("The encoded payload contains invalid characters")
            })?;
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
    let compressed =
        z85_decode(encoded).with_context(|| format!("Failed to decode the {kind} code"))?;
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

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SharedVisionPreset {
    pub preset: VisionPreset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_png: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct MacroShareResources {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crosshair_profiles: Vec<ProfileRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub window_presets: Vec<WindowPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub window_layouts: Vec<WindowLayout>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub window_focus_presets: Vec<WindowFocusPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pin_presets: Vec<PinPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mouse_path_presets: Vec<MousePathPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mouse_sensitivity_presets: Vec<MouseSensitivityPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zoom_presets: Vec<ZoomPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hud_presets: Vec<HudPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_presets: Vec<CommandPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub geometry_presets: Vec<GeometryPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vision_presets: Vec<SharedVisionPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ocr_presets: Vec<OcrPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_sense_presets: Vec<AudioSensePreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timer_presets: Vec<TimerPreset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub esp_presets: Vec<EspPreset>,
}

impl MacroShareResources {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SharedMacroStep {
    pub step: MacroStep,
    #[serde(default, skip_serializing_if = "MacroShareResources::is_empty")]
    pub resources: MacroShareResources,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
struct CompactSharedMacroStep {
    #[serde(rename = "s")]
    step: serde_json::Map<String, serde_json::Value>,
    #[serde(
        rename = "r",
        default,
        skip_serializing_if = "MacroShareResources::is_empty"
    )]
    resources: MacroShareResources,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
struct SparseSharedMacroStep {
    #[serde(rename = "s")]
    step: Vec<(u32, serde_json::Value)>,
    #[serde(
        rename = "r",
        default,
        skip_serializing_if = "MacroShareResources::is_empty"
    )]
    resources: MacroShareResources,
}

type SparseFields = Vec<(u32, serde_json::Value)>;

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
struct SparseMacroPreset {
    #[serde(rename = "m", default, skip_serializing_if = "Vec::is_empty")]
    metadata: SparseFields,
    #[serde(rename = "s", default, skip_serializing_if = "Vec::is_empty")]
    steps: Vec<SparseFields>,
    #[serde(rename = "h", default, skip_serializing_if = "Option::is_none")]
    hold_stop_step: Option<SparseFields>,
    #[serde(rename = "p", default, skip_serializing_if = "Option::is_none")]
    press_stop_step: Option<SparseFields>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
struct SparseSharedMacroPreset {
    #[serde(rename = "p")]
    preset: SparseMacroPreset,
    #[serde(
        rename = "r",
        default,
        skip_serializing_if = "MacroShareResources::is_empty"
    )]
    resources: MacroShareResources,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
struct SparseSharedMacroGroup {
    #[serde(rename = "g", default, skip_serializing_if = "Vec::is_empty")]
    group: SparseFields,
    #[serde(rename = "p", default, skip_serializing_if = "Vec::is_empty")]
    presets: Vec<SparseMacroPreset>,
    #[serde(
        rename = "r",
        default,
        skip_serializing_if = "MacroShareResources::is_empty"
    )]
    resources: MacroShareResources,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SharedMacroPreset {
    pub preset: MacroPreset,
    #[serde(default, skip_serializing_if = "MacroShareResources::is_empty")]
    pub resources: MacroShareResources,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SharedMacroGroup {
    pub group: MacroGroup,
    #[serde(default, skip_serializing_if = "MacroShareResources::is_empty")]
    pub resources: MacroShareResources,
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

pub fn encode_shared_step(shared: &SharedMacroStep) -> Result<String> {
    let compact = SparseSharedMacroStep {
        step: compact_step(&shared.step)?,
        resources: shared.resources.clone(),
    };
    encode_v2(&compact, PREFIX_STEP_V5, "step")
}

pub fn decode_shared_step(code: &str) -> Result<SharedMacroStep> {
    let payload = code.trim();
    if let Some(encoded) = payload.strip_prefix(PREFIX_STEP_V5) {
        let compact: SparseSharedMacroStep = decode_v2(encoded, "step")?;
        return Ok(SharedMacroStep {
            step: expand_step(compact.step)?,
            resources: compact.resources,
        });
    }
    if let Some(encoded) = payload.strip_prefix(PREFIX_STEP_V4) {
        let compact: CompactSharedMacroStep = decode_v2(encoded, "step")?;
        let serde_json::Value::Object(mut step) =
            serde_json::to_value(MacroStep::default()).context("Failed to load step defaults")?
        else {
            return Err(anyhow::anyhow!("The step defaults are invalid"));
        };
        step.extend(compact.step);
        return Ok(SharedMacroStep {
            step: serde_json::from_value(serde_json::Value::Object(step))
                .context("The step code contents are invalid")?,
            resources: compact.resources,
        });
    }
    if payload.starts_with(PREFIX_STEP_V3) {
        return decode_any(code, PREFIX_STEP_V3, PREFIX_STEP_V3, "step");
    }
    decode_step(code).map(|step| SharedMacroStep {
        step,
        resources: MacroShareResources::default(),
    })
}

fn stable_field_id(name: &str) -> u32 {
    name.bytes().fold(0x811c9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x01000193)
    })
}

fn object_fields<T: Serialize>(
    value: T,
    kind: &str,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let serde_json::Value::Object(fields) =
        serde_json::to_value(value).with_context(|| format!("Failed to serialize {kind}"))?
    else {
        return Err(anyhow::anyhow!("The {kind} contents are invalid"));
    };
    Ok(fields)
}

fn compact_fields(
    fields: serde_json::Map<String, serde_json::Value>,
    defaults: &serde_json::Map<String, serde_json::Value>,
) -> SparseFields {
    fields
        .into_iter()
        .filter(|(name, value)| defaults.get(name) != Some(value))
        .map(|(name, value)| (stable_field_id(&name), value))
        .collect()
}

fn expand_fields(
    sparse: SparseFields,
    mut defaults: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let names: std::collections::HashMap<_, _> = defaults
        .keys()
        .map(|name| (stable_field_id(name), name.clone()))
        .collect();
    for (field_id, value) in sparse {
        if let Some(name) = names.get(&field_id) {
            defaults.insert(name.clone(), value);
        }
    }
    defaults
}

fn compact_step(step: &MacroStep) -> Result<SparseFields> {
    let fields = object_fields(step, "step")?;
    let defaults = object_fields(MacroStep::default(), "step defaults")?;
    Ok(fields
        .into_iter()
        .filter(|(name, value)| {
            defaults.get(name) != Some(value) && step_field_is_relevant(step.action, name)
        })
        .map(|(name, value)| (stable_field_id(&name), value))
        .collect())
}

fn expand_step(sparse: SparseFields) -> Result<MacroStep> {
    serde_json::from_value(serde_json::Value::Object(expand_fields(
        sparse,
        object_fields(MacroStep::default(), "step defaults")?,
    )))
    .context("The step code contents are invalid")
}

fn compact_preset(preset: &MacroPreset) -> Result<SparseMacroPreset> {
    let mut metadata = object_fields(preset, "preset")?;
    let mut defaults = object_fields(MacroPreset::default(), "preset defaults")?;
    metadata.remove("steps");
    defaults.remove("steps");
    let hold_stop_step = metadata
        .remove("hold_stop_step")
        .map(serde_json::from_value::<MacroStep>)
        .transpose()
        .context("The preset hold-stop step is invalid")?
        .map(|step| compact_step(&step))
        .transpose()?;
    let press_stop_step = metadata
        .remove("press_stop_step")
        .map(serde_json::from_value::<MacroStep>)
        .transpose()
        .context("The preset press-stop step is invalid")?
        .map(|step| compact_step(&step))
        .transpose()?;
    defaults.remove("hold_stop_step");
    defaults.remove("press_stop_step");
    Ok(SparseMacroPreset {
        metadata: compact_fields(metadata, &defaults),
        steps: preset
            .steps
            .iter()
            .map(compact_step)
            .collect::<Result<_>>()?,
        hold_stop_step,
        press_stop_step,
    })
}

fn expand_preset(compact: SparseMacroPreset) -> Result<MacroPreset> {
    let mut preset = object_fields(MacroPreset::default(), "preset defaults")?;
    preset.remove("steps");
    preset.remove("hold_stop_step");
    preset.remove("press_stop_step");
    let mut preset = expand_fields(compact.metadata, preset);
    preset.insert(
        "steps".to_owned(),
        serde_json::to_value(
            compact
                .steps
                .into_iter()
                .map(expand_step)
                .collect::<Result<Vec<_>>>()?,
        )?,
    );
    if let Some(step) = compact.hold_stop_step {
        preset.insert(
            "hold_stop_step".to_owned(),
            serde_json::to_value(expand_step(step)?)?,
        );
    }
    if let Some(step) = compact.press_stop_step {
        preset.insert(
            "press_stop_step".to_owned(),
            serde_json::to_value(expand_step(step)?)?,
        );
    }
    serde_json::from_value(serde_json::Value::Object(preset))
        .context("The preset code contents are invalid")
}

fn step_field_is_relevant(action: crate::model::MacroAction, field: &str) -> bool {
    use crate::model::MacroAction;

    if field == "if_variable_name" {
        return matches!(action, MacroAction::IfStart | MacroAction::SetVariable);
    }
    if field.starts_with("if_") || field == "extra_conditions" {
        return action == MacroAction::IfStart;
    }
    if field.starts_with("vision_") {
        return matches!(
            action,
            MacroAction::StartVisionSearch
                | MacroAction::ScanVisionOnce
                | MacroAction::StopVision
                | MacroAction::TriggerVisionTiming
                | MacroAction::StartVisionTiming
                | MacroAction::StopVisionTiming
        );
    }
    if field.starts_with("audio_sense_") {
        return matches!(
            action,
            MacroAction::StartAudioSensePreset | MacroAction::StopAudioSense
        );
    }
    if field.starts_with("geometry_") {
        return matches!(
            action,
            MacroAction::DrawGeometry
                | MacroAction::ShowGeometryPreset
                | MacroAction::HideGeometryPreset
        );
    }
    if field.starts_with("esp_") {
        return matches!(
            action,
            MacroAction::EnableEspPreset
                | MacroAction::DisableEspPreset
                | MacroAction::StartEspScan
                | MacroAction::StopEspScan
                | MacroAction::ReadEspTarget
                | MacroAction::Esp3DAimLock
        );
    }
    if field.starts_with("ocr_") {
        return action == MacroAction::OcrSearch;
    }
    if field.starts_with("memory_") {
        return matches!(action, MacroAction::ReadMemory | MacroAction::WriteMemory);
    }
    if field.starts_with("timer_") {
        return matches!(
            action,
            MacroAction::StartTimerPreset
                | MacroAction::PauseTimerPreset
                | MacroAction::StopTimerPreset
                | MacroAction::ReadTimerPreset
        );
    }
    if field.starts_with("ai_response_") {
        return action == MacroAction::AiResponse;
    }
    true
}

pub fn encode_shared_preset(shared: &SharedMacroPreset) -> Result<String> {
    encode_v2(
        &SparseSharedMacroPreset {
            preset: compact_preset(&shared.preset)?,
            resources: shared.resources.clone(),
        },
        PREFIX_PRESET_V5,
        "preset",
    )
}

pub fn decode_shared_preset(code: &str) -> Result<SharedMacroPreset> {
    let payload = code.trim();
    if let Some(encoded) = payload.strip_prefix(PREFIX_PRESET_V5) {
        let compact: SparseSharedMacroPreset = decode_v2(encoded, "preset")?;
        return Ok(SharedMacroPreset {
            preset: expand_preset(compact.preset)?,
            resources: compact.resources,
        });
    }
    if payload.starts_with(PREFIX_PRESET_V3) {
        return decode_any(code, PREFIX_PRESET_V3, PREFIX_PRESET_V3, "preset");
    }
    decode_preset(code).map(|preset| SharedMacroPreset {
        preset,
        resources: MacroShareResources::default(),
    })
}

pub fn encode_shared_group(shared: &SharedMacroGroup) -> Result<String> {
    let mut group = object_fields(&shared.group, "group")?;
    let mut defaults = object_fields(MacroGroup::default(), "group defaults")?;
    group.remove("presets");
    defaults.remove("presets");
    encode_v2(
        &SparseSharedMacroGroup {
            group: compact_fields(group, &defaults),
            presets: shared
                .group
                .presets
                .iter()
                .map(compact_preset)
                .collect::<Result<_>>()?,
            resources: shared.resources.clone(),
        },
        PREFIX_GROUP_V5,
        "group",
    )
}

pub fn decode_shared_group(code: &str) -> Result<SharedMacroGroup> {
    let payload = code.trim();
    if let Some(encoded) = payload.strip_prefix(PREFIX_GROUP_V5) {
        let compact: SparseSharedMacroGroup = decode_v2(encoded, "group")?;
        let mut group = object_fields(MacroGroup::default(), "group defaults")?;
        group.remove("presets");
        let mut group = expand_fields(compact.group, group);
        group.insert(
            "presets".to_owned(),
            serde_json::to_value(
                compact
                    .presets
                    .into_iter()
                    .map(expand_preset)
                    .collect::<Result<Vec<_>>>()?,
            )?,
        );
        return Ok(SharedMacroGroup {
            group: serde_json::from_value(serde_json::Value::Object(group))
                .context("The group code contents are invalid")?,
            resources: compact.resources,
        });
    }
    if payload.starts_with(PREFIX_GROUP_V3) {
        return decode_any(code, PREFIX_GROUP_V3, PREFIX_GROUP_V3, "group");
    }
    decode_group(code).map(|group| SharedMacroGroup {
        group,
        resources: MacroShareResources::default(),
    })
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

    #[test]
    fn shared_preset_round_trip_v3() {
        let shared = SharedMacroPreset {
            preset: MacroPreset::default(),
            resources: MacroShareResources {
                crosshair_profiles: vec![ProfileRecord::default()],
                vision_presets: vec![SharedVisionPreset {
                    preset: VisionPreset::default(),
                    template_png: Some(vec![1, 2, 3, 4]),
                }],
                ..MacroShareResources::default()
            },
        };
        let encoded = encode_shared_preset(&shared).expect("encode shared preset");
        let decoded = decode_shared_preset(&encoded).expect("decode shared preset");
        assert_eq!(decoded, shared);
    }

    #[test]
    fn shared_step_uses_compact_v4_and_still_reads_v3() {
        let shared = SharedMacroStep {
            step: MacroStep {
                key: "Space".to_owned(),
                ..MacroStep::default()
            },
            resources: MacroShareResources::default(),
        };

        let encoded = encode_shared_step(&shared).expect("encode compact step");
        assert!(encoded.starts_with(PREFIX_STEP_V5));
        assert!(encoded.len() < 160, "simple step code was too long");
        assert_eq!(
            decode_shared_step(&encoded).expect("decode compact step"),
            shared
        );

        let legacy = encode_v2(&shared, PREFIX_STEP_V3, "step").expect("encode v3 step");
        assert_eq!(decode_shared_step(&legacy).expect("decode v3 step"), shared);
    }

    #[test]
    fn compact_step_ignores_stale_settings_from_other_action_families() {
        let shared = SharedMacroStep {
            step: MacroStep {
                key: "F".to_owned(),
                if_condition_type: crate::model::IfConditionType::PixelColor,
                if_mouse_axis: String::new(),
                if_mouse_button: String::new(),
                vision_color_scan_rate_hz: 24,
                vision_move_cursor_on_match: true,
                vision_move_delay_ms: 10,
                vision_move_passes: 3,
                ..MacroStep::default()
            },
            resources: MacroShareResources::default(),
        };

        let encoded = encode_shared_step(&shared).expect("encode stale key step");
        assert!(encoded.len() < 160, "stale settings leaked into step code");
        let decoded = decode_shared_step(&encoded).expect("decode stale key step");
        assert_eq!(decoded.step.key, "F");
        assert_eq!(decoded.step.action, crate::model::MacroAction::KeyPress);
        assert_eq!(decoded.step.if_condition_type, Default::default());
        assert!(!decoded.step.vision_move_cursor_on_match);
    }
}
