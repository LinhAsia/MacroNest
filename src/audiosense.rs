#[cfg(not(windows))]
use anyhow::bail;
use anyhow::Result;
#[cfg(windows)]
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
#[cfg(windows)]
use std::time::{Duration, Instant};

use crate::model::{
    AudioSenseMonitorSettings, AudioSenseSource, PitchAudioSenseSettings,
};

#[derive(Clone, Debug)]
pub struct PitchSnapshot {
    pub running: bool,
    pub note: String,
    pub confidence: f32,
    pub level: f32,
    pub waveform: Vec<f32>,
    pub error: Option<String>,
}

impl Default for PitchSnapshot {
    fn default() -> Self {
        Self {
            running: false,
            note: "None".to_owned(),
            confidence: 0.0,
            level: 0.0,
            waveform: Vec::new(),
            error: None,
        }
    }
}

pub struct PitchMonitor {
    state: Arc<Mutex<PitchSnapshot>>,
    stop_flag: Option<Arc<AtomicBool>>,
    worker: Option<JoinHandle<()>>,
}

#[cfg(windows)]
const ANALYSIS_SAMPLE_RATE: u32 = 44_100;
#[cfg(windows)]
const PITCH_ANALYSIS_SAMPLES: usize = 8192;
#[cfg(windows)]
const PITCH_HOLD_TIME: Duration = Duration::from_millis(360);
#[cfg(windows)]
const YIN_THRESHOLD: f32 = 0.16;

impl PitchMonitor {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(PitchSnapshot::default())),
            stop_flag: None,
            worker: None,
        }
    }

    pub fn snapshot(&self) -> PitchSnapshot {
        self.state.lock().unwrap().clone()
    }

    pub fn start(&mut self, config: PitchAudioSenseSettings) -> Result<()> {
        if self.worker.is_some() {
            self.stop();
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let state = Arc::clone(&self.state);
        let stop_for_thread = Arc::clone(&stop_flag);

        {
            let mut snapshot = state.lock().unwrap();
            *snapshot = PitchSnapshot {
                running: true,
                ..PitchSnapshot::default()
            };
        }

        let worker = thread::Builder::new()
            .name("audiosense-pitch".to_owned())
            .spawn(move || {
                let result = run_pitch_loop(Arc::clone(&state), Arc::clone(&stop_for_thread), config);
                let mut snapshot = state.lock().unwrap();
                snapshot.running = false;
                if let Err(error) = result {
                    snapshot.error = Some(error.to_string());
                }
            })?;

        self.stop_flag = Some(stop_flag);
        self.worker = Some(worker);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(stop_flag) = self.stop_flag.take() {
            stop_flag.store(true, Ordering::Relaxed);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let mut snapshot = self.state.lock().unwrap();
        *snapshot = PitchSnapshot::default();
    }
}

impl Drop for PitchMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(windows)]
pub fn list_capture_devices() -> Result<Vec<String>> {
    use wasapi::{DeviceCollection, Direction, initialize_mta};

    let _ = initialize_mta();
    let devices = DeviceCollection::new(&Direction::Capture)?;
    let mut names = Vec::new();
    for device in &devices {
        let device = device?;
        names.push(device.get_friendlyname()?);
    }
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(not(windows))]
pub fn list_capture_devices() -> Result<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(windows)]
fn resolve_device(
    settings: &AudioSenseMonitorSettings,
) -> Result<(
    wasapi::AudioClient,
    wasapi::AudioCaptureClient,
    wasapi::Handle,
    usize,
)> {
    use wasapi::{
        DeviceCollection, Direction, SampleType, StreamMode, WaveFormat, get_default_device,
        initialize_mta,
    };

    let _ = initialize_mta();
    let device = match settings.source {
        AudioSenseSource::System => get_default_device(&Direction::Render)?,
        AudioSenseSource::Microphone => {
            if let Some(device_name) = settings.input_device_name.as_deref() {
                DeviceCollection::new(&Direction::Capture)?.get_device_with_name(device_name)?
            } else {
                get_default_device(&Direction::Capture)?
            }
        }
    };

    let mut audio_client = device.get_iaudioclient()?;
    let desired_format = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        ANALYSIS_SAMPLE_RATE as usize,
        2,
        None,
    );
    let blockalign = desired_format.get_blockalign() as usize;
    let (_, min_time) = audio_client.get_device_period()?;
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: min_time,
    };

    audio_client.initialize_client(&desired_format, &Direction::Capture, &mode)?;
    let event_handle = audio_client.set_get_eventhandle()?;
    let capture_client = audio_client.get_audiocaptureclient()?;
    Ok((audio_client, capture_client, event_handle, blockalign))
}

#[cfg(windows)]
fn publish_interval(updates_per_second: u32) -> Duration {
    let hz = updates_per_second.clamp(1, 60) as f32;
    Duration::from_secs_f32((1.0 / hz).max(1.0 / 60.0))
}

#[cfg(windows)]
fn max_duration(settings: &AudioSenseMonitorSettings) -> Option<Duration> {
    if settings.permanent {
        None
    } else {
        Some(Duration::from_millis(settings.duration_ms.max(100)))
    }
}

#[cfg(windows)]
fn run_pitch_loop(
    state: Arc<Mutex<PitchSnapshot>>,
    stop_flag: Arc<AtomicBool>,
    config: PitchAudioSenseSettings,
) -> Result<()> {
    let (mut audio_client, capture_client, event_handle, blockalign) =
        resolve_device(&config.monitor)?;
    let buffer_frame_count = audio_client.get_buffer_size()? as usize;
    let chunk_frames = 512usize;
    let chunk_bytes = chunk_frames * blockalign;
    let mut sample_queue =
        VecDeque::with_capacity(blockalign * (chunk_frames + buffer_frame_count * 4));
    let mut pitch_samples = VecDeque::with_capacity(PITCH_ANALYSIS_SAMPLES);
    let mut smoothed_level = 0.04f32;
    let interval = publish_interval(config.monitor.updates_per_second);
    let stop_after = max_duration(&config.monitor);
    let started_at = Instant::now();
    let mut last_publish = Instant::now()
        .checked_sub(interval)
        .unwrap_or_else(Instant::now);
    let mut last_note = "None".to_owned();
    let mut last_confidence = 0.0f32;
    let mut last_detected_at = Instant::now()
        .checked_sub(PITCH_HOLD_TIME)
        .unwrap_or_else(Instant::now);
    let mut waveform = VecDeque::with_capacity(160);

    audio_client.start_stream()?;
    while !stop_flag.load(Ordering::Relaxed) {
        if stop_after.is_some_and(|limit| started_at.elapsed() >= limit) {
            break;
        }

        let new_frames = capture_client.get_next_packet_size()?.unwrap_or(0);
        if new_frames > 0 {
            let additional = (new_frames as usize * blockalign)
                .saturating_sub(sample_queue.capacity().saturating_sub(sample_queue.len()));
            sample_queue.reserve(additional);
            capture_client.read_from_device_to_deque(&mut sample_queue)?;
        }

        while sample_queue.len() >= chunk_bytes {
            let mut chunk = vec![0u8; chunk_bytes];
            for value in &mut chunk {
                *value = sample_queue.pop_front().unwrap_or_default();
            }

            let mono = bytes_to_mono_samples(&chunk, 2);
            let raw_level = rms_level(&mono);
            let level_visual = level_to_visual(raw_level);
            smoothed_level = smoothed_level * 0.78 + level_visual * 0.22;
            let waveform_level = (raw_level * 18.0).clamp(0.0, 1.0);
            waveform.push_back(waveform_level);
            while waveform.len() > 160 {
                let _ = waveform.pop_front();
            }
            for sample in mono {
                pitch_samples.push_back(sample);
                while pitch_samples.len() > PITCH_ANALYSIS_SAMPLES {
                    let _ = pitch_samples.pop_front();
                }
            }

            if last_publish.elapsed() >= interval {
                let analysis_window = pitch_samples.iter().copied().collect::<Vec<_>>();
                let accept_confidence = (config.min_confidence as f32 / 1000.0).max(0.0).min(1.0);
                let level_gate = (config.min_level as f32 / 1000.0).max(0.0);
                if let Some((frequency, confidence)) =
                    detect_pitch(&analysis_window, ANALYSIS_SAMPLE_RATE, accept_confidence, level_gate)
                {
                    let candidate_note = pitch_to_spn(frequency, config.show_sharps);
                    let sustain_confidence = accept_confidence * 0.75;
                    let sustained_note =
                        candidate_note == last_note && confidence >= sustain_confidence;
                    if confidence >= accept_confidence || sustained_note {
                        last_note = candidate_note;
                        last_confidence = confidence;
                        last_detected_at = Instant::now();
                    }
                } else if smoothed_level > 0.08
                    && last_note != "None"
                    && last_detected_at.elapsed() <= PITCH_HOLD_TIME
                {
                    last_confidence = (last_confidence * 0.92).clamp(0.0, 1.0);
                } else {
                    last_note = "None".to_owned();
                    last_confidence = 0.0;
                }

                let mut snapshot = state.lock().unwrap();
                snapshot.running = true;
                snapshot.note = last_note.clone();
                snapshot.confidence = last_confidence;
                snapshot.level = smoothed_level;
                snapshot.waveform = waveform.iter().copied().collect();
                snapshot.error = None;
                last_publish = Instant::now();
            }
        }

        let _ = event_handle.wait_for_event(200);
    }

    let _ = audio_client.stop_stream();
    Ok(())
}

#[cfg(not(windows))]
fn run_pitch_loop(
    _state: Arc<Mutex<PitchSnapshot>>,
    _stop_flag: Arc<AtomicBool>,
    _config: PitchAudioSenseSettings,
) -> Result<()> {
    bail!("AudioSense pitch detection is only available on Windows")
}

#[cfg(windows)]
pub fn sleep_detection_interval(updates_per_second: u32) {
    let interval = publish_interval(updates_per_second);
    thread::sleep(interval.min(Duration::from_millis(250)));
}

#[cfg(not(windows))]
pub fn sleep_detection_interval(_updates_per_second: u32) {
    thread::sleep(std::time::Duration::from_millis(120));
}

fn level_to_visual(level: f32) -> f32 {
    (level * 8.0).clamp(0.0, 1.0).powf(0.55).clamp(0.04, 1.0)
}

fn bytes_to_mono_samples(bytes: &[u8], channels: usize) -> Vec<f32> {
    let frame_width = channels * std::mem::size_of::<f32>();
    let mut mono = Vec::with_capacity(bytes.len() / frame_width.max(1));

    for frame in bytes.chunks_exact(frame_width.max(4)) {
        let mut mixed = 0.0f32;
        let mut used = 0usize;
        for sample in frame.chunks_exact(4).take(channels) {
            mixed += f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]);
            used += 1;
        }
        mono.push(mixed / used.max(1) as f32);
    }

    mono
}

fn rms_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}

fn detect_pitch(samples: &[f32], sample_rate: u32, accept_confidence: f32, level_gate: f32) -> Option<(f32, f32)> {
    if samples.len() < 1024 {
        return None;
    }

    let analysis_len = samples.len().min(PITCH_ANALYSIS_SAMPLES);
    let frame = &samples[samples.len() - analysis_len..];
    let mean = frame.iter().sum::<f32>() / frame.len() as f32;
    let centered = frame
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            let t = index as f32 / (frame.len().saturating_sub(1).max(1)) as f32;
            let window = 0.5 - 0.5 * (std::f32::consts::TAU * t).cos();
            (sample - mean) * window
        })
        .collect::<Vec<_>>();
    let level = rms_level(&centered);
    if level < level_gate {
        return None;
    }

    let min_freq = 55.0f32;
    let max_freq = 1_760.0f32;
    let min_tau = ((sample_rate as f32 / max_freq).floor() as usize).max(2);
    let max_tau =
        ((sample_rate as f32 / min_freq).ceil() as usize).min(centered.len().saturating_div(2));
    if max_tau <= min_tau + 2 {
        return None;
    }

    let mut diff = vec![0.0f32; max_tau + 1];
    for tau in min_tau..=max_tau {
        let mut sum = 0.0f32;
        for index in 0..(centered.len() - tau) {
            let delta = centered[index] - centered[index + tau];
            sum += delta * delta;
        }
        diff[tau] = sum;
    }

    let mut cmndf = vec![1.0f32; max_tau + 1];
    let mut running_sum = 0.0f32;
    for tau in min_tau..=max_tau {
        running_sum += diff[tau];
        if running_sum > 1e-9 {
            cmndf[tau] = diff[tau] * tau as f32 / running_sum;
        }
    }

    let mut best_tau = min_tau;
    let mut best_value = f32::MAX;
    for tau in min_tau..=max_tau {
        let value = cmndf[tau];
        if value < best_value {
            best_value = value;
            best_tau = tau;
        }
        if value < YIN_THRESHOLD {
            let mut candidate = tau;
            while candidate < max_tau && cmndf[candidate + 1] < cmndf[candidate] {
                candidate += 1;
            }
            best_tau = candidate;
            best_value = cmndf[candidate];
            break;
        }
    }

    let confidence = (1.0 - best_value).clamp(0.0, 1.0);
    let detection_floor = (accept_confidence - 0.10).max(0.0);
    if confidence < detection_floor {
        return None;
    }

    let refined_tau = if best_tau > min_tau && best_tau < max_tau {
        let prev = cmndf[best_tau - 1];
        let curr = cmndf[best_tau];
        let next = cmndf[best_tau + 1];
        let denom = prev - 2.0 * curr + next;
        if denom.abs() > 1e-6 {
            best_tau as f32 + 0.5 * (prev - next) / denom
        } else {
            best_tau as f32
        }
    } else {
        best_tau as f32
    };

    let frequency = sample_rate as f32 / refined_tau.max(1.0);
    if !(min_freq..=max_freq).contains(&frequency) {
        return None;
    }
    Some((frequency, confidence))
}

fn pitch_to_spn(frequency: f32, show_sharps: bool) -> String {
    const NOTE_NAMES: [(&str, Option<&str>); 12] = [
        ("C", None),
        ("C#", Some("Db")),
        ("D", None),
        ("D#", Some("Eb")),
        ("E", None),
        ("F", None),
        ("F#", Some("Gb")),
        ("G", None),
        ("G#", Some("Ab")),
        ("A", None),
        ("A#", Some("Bb")),
        ("B", None),
    ];

    let midi = (69.0 + 12.0 * (frequency / 440.0).log2()).round() as i32;
    let note_index = midi.rem_euclid(12) as usize;
    let octave = midi.div_euclid(12) - 1;
    let (sharp, flat) = NOTE_NAMES[note_index];
    if show_sharps {
        format!("{sharp}{octave}")
    } else {
        match flat {
            Some(flat_name) => format!("{flat_name}{octave}"),
            None => format!("{sharp}{octave}"),
        }
    }
}
