use anyhow::{Context, Result};
use std::thread;
use std::time::Duration;

use super::{HOOK_STATE, is_ui_in_foreground, set_text_variable_value, set_variable_value};
use crate::audiosense;
use crate::model::{AudioSensePreset, AudioSenseSpec, MacroAction, MacroStep};

pub(crate) fn audio_sense_monitor_key_for_preset(preset_id: u32) -> String {
    format!("preset:{preset_id}")
}

pub(crate) fn custom_audio_sense_monitor_key(
    macro_preset_id: u32,
    step_index: usize,
    is_hold_stop: bool,
) -> String {
    format!("custom:{macro_preset_id}:{step_index}:{is_hold_stop}:pitch")
}

pub(crate) fn audio_sense_is_active(key: &str) -> bool {
    HOOK_STATE.lock().active_audio_sense_keys.contains(key)
}

pub(crate) fn set_audio_sense_active(key: &str, active: bool) {
    let mut hook_state = HOOK_STATE.lock();
    if active {
        hook_state.active_audio_sense_keys.insert(key.to_owned());
    } else {
        hook_state.active_audio_sense_keys.remove(key);
        hook_state.active_audio_sense_snapshots.remove(key);
    }
}

pub(crate) fn stop_all_audio_sense() {
    let mut hook_state = HOOK_STATE.lock();
    hook_state.active_audio_sense_keys.clear();
    hook_state.active_audio_sense_snapshots.clear();
}

pub(crate) fn audio_sense_preset_by_id(spec: &str) -> Result<AudioSensePreset> {
    let preset_id = spec
        .trim()
        .parse::<u32>()
        .context("AudioSense preset id is invalid")?;
    HOOK_STATE
        .lock()
        .audio_sense_presets
        .iter()
        .find(|preset| preset.id == preset_id)
        .cloned()
        .context("AudioSense preset was not found")
}

pub(crate) fn write_pitch_snapshot_vars(
    settings: &crate::model::PitchAudioSenseSettings,
    snapshot: &audiosense::PitchSnapshot,
) {
    if !settings.output_note_var.trim().is_empty() {
        set_text_variable_value(&settings.output_note_var, &snapshot.note);
    }
    if !settings.output_level_var.trim().is_empty() {
        set_variable_value(&settings.output_level_var, (snapshot.level * 1000.0) as f64);
    }
}

pub(crate) fn run_pitch_monitor_loop(
    monitor_key: String,
    config: crate::model::PitchAudioSenseSettings,
    stop_when_ui_foreground: bool,
    is_preview: bool,
) {
    let mut monitor = audiosense::PitchMonitor::new();
    if let Err(error) = monitor.start(config.clone()) {
        eprintln!("AudioSense pitch start failed: {error}");
        set_audio_sense_active(&monitor_key, false);
        return;
    }

    let update_hz = config.monitor.updates_per_second;
    while audio_sense_is_active(&monitor_key) {
        if stop_when_ui_foreground && is_ui_in_foreground() {
            break;
        }

        let snapshot = monitor.snapshot();
        if let Some(error) = snapshot.error.as_ref() {
            eprintln!("AudioSense pitch error: {error}");
            break;
        }
        if !snapshot.running {
            break;
        }

        HOOK_STATE
            .lock()
            .active_audio_sense_snapshots
            .insert(monitor_key.clone(), snapshot.clone());
        if !is_preview {
            write_pitch_snapshot_vars(&config, &snapshot);
        }
        audiosense::sleep_detection_interval(update_hz);
    }

    monitor.stop();
    set_audio_sense_active(&monitor_key, false);
}

pub(crate) fn start_audio_sense_preset(spec: &str, stop_when_ui_foreground: bool) -> Result<()> {
    let preset = audio_sense_preset_by_id(spec)?;
    let monitor_key = audio_sense_monitor_key_for_preset(preset.id);
    if audio_sense_is_active(&monitor_key) {
        return Ok(());
    }

    set_audio_sense_active(&monitor_key, true);
    thread::spawn(move || {
        run_pitch_monitor_loop(monitor_key, preset.pitch, stop_when_ui_foreground, false)
    });
    Ok(())
}

pub(crate) fn stop_audio_sense_preset(spec: &str) -> Result<()> {
    let preset = audio_sense_preset_by_id(spec)?;
    let monitor_key = audio_sense_monitor_key_for_preset(preset.id);
    set_audio_sense_active(&monitor_key, false);
    Ok(())
}

pub(crate) fn start_custom_audio_sense(
    monitor_key: String,
    spec: AudioSenseSpec,
    stop_when_ui_foreground: bool,
    is_preview: bool,
) {
    if audio_sense_is_active(&monitor_key) {
        return;
    }

    set_audio_sense_active(&monitor_key, true);
    thread::spawn(move || {
        run_pitch_monitor_loop(monitor_key, spec.pitch, stop_when_ui_foreground, is_preview)
    });
}

pub(crate) fn start_audio_sense_from_step(
    step: &MacroStep,
    macro_preset_id: u32,
    step_index: usize,
    is_hold_stop: bool,
    stop_when_ui_foreground: bool,
    is_preview: bool,
) {
    match step.action {
        MacroAction::StartAudioSensePreset => {
            if let Some(preset_id) = step.audio_sense_preset_id {
                if let Ok(mut preset) = audio_sense_preset_by_id(&preset_id.to_string()) {
                    if !step
                        .audio_sense_spec
                        .pitch
                        .output_note_var
                        .trim()
                        .is_empty()
                    {
                        preset.pitch.output_note_var =
                            step.audio_sense_spec.pitch.output_note_var.clone();
                    }
                    if !step
                        .audio_sense_spec
                        .pitch
                        .output_level_var
                        .trim()
                        .is_empty()
                    {
                        preset.pitch.output_level_var =
                            step.audio_sense_spec.pitch.output_level_var.clone();
                    }
                    let monitor_key = audio_sense_monitor_key_for_preset(preset.id);
                    if !audio_sense_is_active(&monitor_key) {
                        set_audio_sense_active(&monitor_key, true);
                        thread::spawn(move || {
                            run_pitch_monitor_loop(
                                monitor_key,
                                preset.pitch,
                                stop_when_ui_foreground,
                                is_preview,
                            )
                        });
                    }
                }
            } else {
                let spec = step.audio_sense_spec.clone();
                let monitor_key =
                    custom_audio_sense_monitor_key(macro_preset_id, step_index, is_hold_stop);
                start_custom_audio_sense(monitor_key, spec, stop_when_ui_foreground, is_preview);
            }
        }
        _ => {}
    }
}

pub(crate) fn stop_audio_sense_from_step(
    step: &MacroStep,
    macro_preset_id: u32,
    step_index: usize,
    is_hold_stop: bool,
) {
    match step.action {
        MacroAction::StopAudioSense => {
            if step.audio_sense_stop_all {
                stop_all_audio_sense();
            } else if let Some(preset_id) = step.audio_sense_preset_id {
                let _ = stop_audio_sense_preset(&preset_id.to_string());
            } else {
                let pitch_key =
                    custom_audio_sense_monitor_key(macro_preset_id, step_index, is_hold_stop);
                set_audio_sense_active(&pitch_key, false);
            }
        }
        _ => {}
    }
}

pub(crate) fn is_audio_sense_active(
    preset_id: Option<u32>,
    macro_preset_id: u32,
    step_index: usize,
    is_hold_stop: bool,
) -> bool {
    let key = if let Some(id) = preset_id {
        audio_sense_monitor_key_for_preset(id)
    } else {
        custom_audio_sense_monitor_key(macro_preset_id, step_index, is_hold_stop)
    };
    audio_sense_is_active(&key)
}

pub(crate) fn start_audio_sense_preview(
    step: &MacroStep,
    macro_preset_id: u32,
    step_index: usize,
    is_hold_stop: bool,
) {
    start_audio_sense_from_step(step, macro_preset_id, step_index, is_hold_stop, true, true);
}

pub(crate) fn stop_audio_sense(
    preset_id: Option<u32>,
    macro_preset_id: u32,
    step_index: usize,
    is_hold_stop: bool,
) {
    if let Some(id) = preset_id {
        let _ = stop_audio_sense_preset(&id.to_string());
    } else {
        let pitch_key = custom_audio_sense_monitor_key(macro_preset_id, step_index, is_hold_stop);
        set_audio_sense_active(&pitch_key, false);
    }
}

pub(crate) fn get_audio_sense_snapshot(
    preset_id: Option<u32>,
    macro_preset_id: u32,
    step_index: usize,
    is_hold_stop: bool,
) -> Option<crate::audiosense::PitchSnapshot> {
    let key = if let Some(id) = preset_id {
        audio_sense_monitor_key_for_preset(id)
    } else {
        custom_audio_sense_monitor_key(macro_preset_id, step_index, is_hold_stop)
    };
    HOOK_STATE
        .lock()
        .active_audio_sense_snapshots
        .get(&key)
        .cloned()
}
