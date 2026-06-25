use serde::{Deserialize, Serialize};

use super::{
    default_audio_sense_duration_ms, default_audio_sense_min_confidence,
    default_audio_sense_min_level, default_audio_sense_output_level_var,
    default_audio_sense_output_note_var, default_audio_sense_updates_per_second,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum AudioSensePresetKind {
    #[default]
    #[serde(alias = "Spatial", alias = "Surround")]
    Pitch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum AudioSenseSource {
    #[default]
    System,
    Microphone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudioSenseMonitorSettings {
    pub source: AudioSenseSource,
    pub input_device_name: Option<String>,
    #[serde(default = "default_audio_sense_updates_per_second")]
    pub updates_per_second: u32,
    #[serde(default, alias = "listen_forever")]
    pub permanent: bool,
    #[serde(default = "default_audio_sense_duration_ms")]
    pub duration_ms: u64,
}

impl Default for AudioSenseMonitorSettings {
    fn default() -> Self {
        Self {
            source: AudioSenseSource::System,
            input_device_name: None,
            updates_per_second: default_audio_sense_updates_per_second(),
            permanent: false,
            duration_ms: default_audio_sense_duration_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PitchAudioSenseSettings {
    pub monitor: AudioSenseMonitorSettings,
    pub show_sharps: bool,
    #[serde(default = "default_audio_sense_output_note_var")]
    pub output_note_var: String,
    #[serde(default = "default_audio_sense_output_level_var")]
    pub output_level_var: String,
    #[serde(default = "default_audio_sense_min_confidence")]
    pub min_confidence: u32,
    #[serde(default = "default_audio_sense_min_level")]
    pub min_level: u32,
}

impl Default for PitchAudioSenseSettings {
    fn default() -> Self {
        Self {
            monitor: AudioSenseMonitorSettings::default(),
            show_sharps: true,
            output_note_var: default_audio_sense_output_note_var(),
            output_level_var: default_audio_sense_output_level_var(),
            min_confidence: default_audio_sense_min_confidence(),
            min_level: default_audio_sense_min_level(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudioSensePreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub kind: AudioSensePresetKind,
    pub pitch: PitchAudioSenseSettings,
}

impl AudioSensePreset {
    pub fn new_pitch(id: u32) -> Self {
        Self {
            id,
            name: format!("Pitch Detect {id}"),
            enabled: true,
            collapsed: true,
            kind: AudioSensePresetKind::Pitch,
            pitch: PitchAudioSenseSettings::default(),
        }
    }
}

impl Default for AudioSensePreset {
    fn default() -> Self {
        Self::new_pitch(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudioSenseSpec {
    pub kind: AudioSensePresetKind,
    pub pitch: PitchAudioSenseSettings,
}

impl Default for AudioSenseSpec {
    fn default() -> Self {
        Self {
            kind: AudioSensePresetKind::Pitch,
            pitch: PitchAudioSenseSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioClipSettings {
    pub enabled: bool,
    pub file_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub volume: f32,
    pub speed: f32,
}

impl Default for AudioClipSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            file_path: String::new(),
            start_ms: 0,
            end_ms: 0,
            volume: 1.0,
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioSettings {
    pub startup: AudioClipSettings,
    pub exit: AudioClipSettings,
    pub library: Vec<SoundLibraryItem>,
    pub next_library_item_id: u32,
    pub presets: Vec<SoundPreset>,
    pub next_preset_id: u32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            startup: AudioClipSettings::default(),
            exit: AudioClipSettings::default(),
            library: Vec::new(),
            next_library_item_id: 1,
            presets: Vec::new(),
            next_preset_id: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SoundLibraryItem {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub clip: AudioClipSettings,
}

impl SoundLibraryItem {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Library Sound {id}"),
            collapsed: true,
            clip: AudioClipSettings::default(),
        }
    }
}

impl Default for SoundLibraryItem {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SoundPreset {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub clip: AudioClipSettings,
    pub sequence_library_ids: Vec<u32>,
}

impl SoundPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Sound {id}"),
            collapsed: true,
            clip: AudioClipSettings {
                enabled: true,
                ..AudioClipSettings::default()
            },
            sequence_library_ids: Vec::new(),
        }
    }
}

impl Default for SoundPreset {
    fn default() -> Self {
        Self::new(1)
    }
}
