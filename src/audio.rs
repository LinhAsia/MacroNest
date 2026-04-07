use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, OutputStreamBuilder, Sink, Source};
use symphonia::core::{
    audio::{AudioBufferRef, SampleBuffer},
    codecs::DecoderOptions,
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use symphonia::default::{get_codecs, get_probe};

use crate::model::AudioClipSettings;

struct CachedAudio {
    path: PathBuf,
    channels: u16,
    sample_rate: u32,
    samples: Arc<[f32]>,
}

struct SharedSamplesSource {
    samples: Arc<[f32]>,
    index: usize,
    end: usize,
    channels: u16,
    sample_rate: u32,
}

impl Iterator for SharedSamplesSource {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.end {
            return None;
        }
        let sample = self.samples.get(self.index).copied();
        self.index = self.index.saturating_add(1);
        sample
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.end.saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl Source for SharedSamplesSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.end.saturating_sub(self.index) / self.channels.max(1) as usize)
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        let frames = self.end.saturating_sub(self.index) / self.channels.max(1) as usize;
        Some(Duration::from_secs_f32(
            frames as f32 / self.sample_rate.max(1) as f32,
        ))
    }
}

struct PreviewState {
    _stream: rodio::OutputStream,
    cached_audio: Option<CachedAudio>,
    sink: Option<Sink>,
    clip: AudioClipSettings,
    start_position_ms: u64,
    started_at: Option<Instant>,
    current_speed: f32,
}

impl PreviewState {
    fn new() -> Result<Self> {
        let stream = OutputStreamBuilder::open_default_stream()
            .context("Could not open the default audio output")?;
        Ok(Self {
            _stream: stream,
            cached_audio: None,
            sink: None,
            clip: AudioClipSettings::default(),
            start_position_ms: 0,
            started_at: None,
            current_speed: 1.0,
        })
    }

    fn cleanup(&mut self) {
        if self.sink.as_ref().is_some_and(|sink| sink.empty()) {
            self.stop();
        }
    }

    fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.clip = AudioClipSettings::default();
        self.start_position_ms = 0;
        self.started_at = None;
        self.current_speed = 1.0;
    }

    fn ensure_cached_audio(&mut self, path: &str) -> Result<()> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            bail!("Choose an audio file first");
        }
        if self
            .cached_audio
            .as_ref()
            .is_some_and(|cached| cached.path.as_path() == Path::new(trimmed))
        {
            return Ok(());
        }
        self.cached_audio = Some(load_cached_audio(trimmed)?);
        Ok(())
    }

    fn play(&mut self, clip: AudioClipSettings, start_position_ms: u64) -> Result<()> {
        if !clip.enabled || clip.file_path.trim().is_empty() {
            bail!("Choose an audio file first");
        }

        self.stop();
        self.ensure_cached_audio(&clip.file_path)?;
        let cached = self
            .cached_audio
            .as_ref()
            .expect("cached audio should exist after ensure_cached_audio");
        let channels = cached.channels.max(1);
        let sample_rate = cached.sample_rate.max(1);
        let clip_start_ms = clip.start_ms;
        let clip_end_ms = clip.end_ms.max(clip_start_ms + 1);
        let start_position_ms = start_position_ms.clamp(clip_start_ms, clip_end_ms);
        let start_frame =
            ((start_position_ms as f32 / 1000.0) * sample_rate as f32).floor() as usize;
        let start_sample = start_frame.saturating_mul(channels as usize);
        let end_frame = ((clip_end_ms as f32 / 1000.0) * sample_rate as f32).ceil() as usize;
        let end_sample = (end_frame.saturating_mul(channels as usize)).min(cached.samples.len());
        let total_duration_ms = ((end_sample.saturating_sub(start_sample)) as f32
            / channels as f32
            / sample_rate as f32
            * 1000.0)
            .round()
            .max(0.0) as u64;
        let start_position_ms =
            start_position_ms.min(clip_start_ms.saturating_add(total_duration_ms));
        let source = SharedSamplesSource {
            samples: Arc::clone(&cached.samples),
            index: start_sample.min(end_sample),
            end: end_sample,
            channels,
            sample_rate,
        }
        .speed(clip.speed.clamp(0.25, 3.0));

        let sink = Sink::connect_new(self._stream.mixer());
        sink.set_volume(clip.volume.clamp(0.0, 2.0));
        sink.append(source);
        sink.play();

        self.clip = clip;
        self.start_position_ms = start_position_ms;
        self.started_at = Some(Instant::now());
        self.current_speed = self.clip.speed.clamp(0.25, 3.0);
        self.sink = Some(sink);
        Ok(())
    }

    fn is_previewing(&mut self, clip: &AudioClipSettings) -> bool {
        self.cleanup();
        self.clip == *clip && self.sink.is_some()
    }

    fn position_ms(&mut self, clip: &AudioClipSettings) -> Option<u64> {
        self.cleanup();
        if self.clip != *clip {
            return None;
        }
        let started_at = self.started_at?;
        let elapsed_ms = started_at.elapsed().as_millis() as u64;
        let played_ms = ((elapsed_ms as f32) * self.current_speed).round().max(0.0) as u64;
        Some(
            self.start_position_ms
                .saturating_add(played_ms)
                .min(self.clip.end_ms.max(self.clip.start_ms + 1)),
        )
    }
}

static PREVIEW_STATE: Lazy<Mutex<Option<PreviewState>>> = Lazy::new(|| Mutex::new(None));
static VIDEO_PREVIEW_STATE: Lazy<Mutex<Option<VideoPreviewAudioState>>> =
    Lazy::new(|| Mutex::new(None));

struct VideoPreviewAudioState {
    _stream: rodio::OutputStream,
    sink: Option<Sink>,
    path: String,
    start_ms: u64,
    end_ms: u64,
}

impl VideoPreviewAudioState {
    fn new() -> Result<Self> {
        let stream = OutputStreamBuilder::open_default_stream()
            .context("Could not open the default audio output")?;
        Ok(Self {
            _stream: stream,
            sink: None,
            path: String::new(),
            start_ms: 0,
            end_ms: 0,
        })
    }

    fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.path.clear();
        self.start_ms = 0;
        self.end_ms = 0;
    }
}

pub fn load_duration_ms(path: &str) -> Result<u64> {
    let trimmed = path.trim();
    let mut decoder = open_decoder(trimmed)?;
    if let Some(duration) = decoder.total_duration() {
        return Ok(duration.as_millis() as u64);
    }

    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    let samples: Vec<f32> = decoder.by_ref().collect();
    computed_duration_ms(samples.len(), channels, sample_rate)
        .context("Could not determine the audio duration")
}

pub fn preload_preview_audio(path: &str) -> Result<()> {
    let mut state = preview_state()?;
    state
        .as_mut()
        .expect("preview state should be initialized")
        .ensure_cached_audio(path)
}

pub fn play_clip_async(clip: AudioClipSettings) {
    let _ = try_play_clip_async(clip);
}

pub fn play_clip_sequence_async(clips: Vec<AudioClipSettings>) {
    let _ = try_play_clip_sequence_async(clips);
}

pub fn try_play_clip_async(clip: AudioClipSettings) -> Result<()> {
    try_play_clip_sequence_async(vec![clip])
}

pub fn try_play_clip_sequence_async(clips: Vec<AudioClipSettings>) -> Result<()> {
    let clips = clips
        .into_iter()
        .filter(|clip| clip.enabled && !clip.file_path.trim().is_empty())
        .collect::<Vec<_>>();
    if clips.is_empty() {
        return Ok(());
    }
    for clip in &clips {
        let path = clip.file_path.trim();
        if !Path::new(path).exists() {
            bail!("Audio file was not found");
        }
    }

    let stream = OutputStreamBuilder::open_default_stream()
        .context("Could not open the default audio output")?;
    let sink = Sink::connect_new(stream.mixer());
    for clip in &clips {
        sink.set_volume(clip.volume.clamp(0.0, 2.0));
        sink.append(clipped_source_from_ms(clip, clip.start_ms)?);
    }
    sink.play();
    thread::spawn(move || {
        sink.sleep_until_end();
        drop(sink);
        drop(stream);
    });
    Ok(())
}

pub fn play_clip_blocking(clip: &AudioClipSettings) -> Result<()> {
    play_clip_sequence_blocking(std::slice::from_ref(clip))
}

pub fn play_clip_sequence_blocking(clips: &[AudioClipSettings]) -> Result<()> {
    let clips = clips
        .iter()
        .filter(|clip| clip.enabled && !clip.file_path.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if clips.is_empty() {
        return Ok(());
    }
    for clip in &clips {
        let path = clip.file_path.trim();
        if !Path::new(path).exists() {
            bail!("Audio file was not found");
        }
    }

    let stream = OutputStreamBuilder::open_default_stream()
        .context("Could not open the default audio output")?;
    let sink = Sink::connect_new(stream.mixer());
    for clip in &clips {
        sink.set_volume(clip.volume.clamp(0.0, 2.0));
        sink.append(clipped_source_from_ms(clip, clip.start_ms)?);
    }
    sink.sleep_until_end();
    Ok(())
}

pub fn toggle_preview(clip: AudioClipSettings) -> Result<bool> {
    let start_ms = clip.start_ms;
    toggle_preview_from_ms(clip, start_ms)
}

pub fn start_preview_from_ms(clip: AudioClipSettings, start_position_ms: u64) -> Result<()> {
    let mut state = preview_state()?;
    let start_position_ms =
        start_position_ms.clamp(clip.start_ms, clip.end_ms.max(clip.start_ms + 1));
    state
        .as_mut()
        .expect("preview state should be initialized")
        .play(clip, start_position_ms)?;
    Ok(())
}

pub fn toggle_preview_from_ms(mut clip: AudioClipSettings, start_position_ms: u64) -> Result<bool> {
    if !clip.enabled || clip.file_path.trim().is_empty() {
        bail!("Choose an audio file first");
    }
    clip.enabled = true;
    let start_position_ms =
        start_position_ms.clamp(clip.start_ms, clip.end_ms.max(clip.start_ms + 1));

    let mut state = preview_state()?;
    state
        .as_mut()
        .expect("preview state should be initialized")
        .cleanup();

    if state
        .as_ref()
        .expect("preview state should be initialized")
        .sink
        .is_some()
        && state
            .as_ref()
            .expect("preview state should be initialized")
            .clip
            == clip
        && state
            .as_ref()
            .expect("preview state should be initialized")
            .start_position_ms
            == start_position_ms
    {
        state
            .as_mut()
            .expect("preview state should be initialized")
            .stop();
        return Ok(false);
    }

    state
        .as_mut()
        .expect("preview state should be initialized")
        .play(clip, start_position_ms)?;
    Ok(true)
}

pub fn stop_preview() {
    if let Ok(mut state) = preview_state() {
        state
            .as_mut()
            .expect("preview state should be initialized")
            .stop();
    }
}

pub fn is_previewing(clip: &AudioClipSettings) -> bool {
    let Ok(mut state) = preview_state() else {
        return false;
    };
    state
        .as_mut()
        .expect("preview state should be initialized")
        .is_previewing(clip)
}

pub fn preview_position_ms(clip: &AudioClipSettings) -> Option<u64> {
    let Ok(mut state) = preview_state() else {
        return None;
    };
    state
        .as_mut()
        .expect("preview state should be initialized")
        .position_ms(clip)
}

pub fn play_video_audio_preview(path: &str, start_ms: u64, end_ms: u64) -> Result<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("Choose a video file first");
    }

    let mut state = video_preview_state()?;
    let state = state
        .as_mut()
        .expect("video preview state should be initialized");
    state.stop();

    let decoded = decode_media_audio(trimmed)?;
    let channels = decoded.channels.max(1);
    let sample_rate = decoded.sample_rate.max(1);
    let clip_end_ms = end_ms.max(start_ms.saturating_add(1));
    let start_frame = ((start_ms as f64 / 1000.0) * sample_rate as f64)
        .floor()
        .max(0.0) as usize;
    let end_frame = ((clip_end_ms as f64 / 1000.0) * sample_rate as f64)
        .ceil()
        .max(0.0) as usize;
    let start_sample = start_frame.saturating_mul(channels as usize);
    let end_sample = end_frame
        .saturating_mul(channels as usize)
        .min(decoded.samples.len());
    if start_sample >= end_sample {
        return Ok(());
    }

    let sink = Sink::connect_new(state._stream.mixer());
    sink.append(SamplesBuffer::new(
        channels,
        sample_rate,
        decoded.samples[start_sample..end_sample].to_vec(),
    ));
    sink.play();
    state.path = trimmed.to_owned();
    state.start_ms = start_ms;
    state.end_ms = clip_end_ms;
    state.sink = Some(sink);
    Ok(())
}

pub fn stop_video_audio_preview() {
    if let Ok(mut state) = video_preview_state() {
        state
            .as_mut()
            .expect("video preview state should be initialized")
            .stop();
    }
}

pub fn load_waveform(path: &str, buckets: usize) -> Result<Vec<f32>> {
    let path = path.trim();
    if path.is_empty() {
        bail!("Choose an audio file first");
    }
    if !Path::new(path).exists() {
        bail!("Audio file was not found");
    }

    let bucket_count = buckets.max(32);

    if let Ok(mut decoder) = open_decoder(path) {
        let estimated_total_samples = decoder.total_duration().map(|duration| {
            (duration.as_secs_f64() * decoder.sample_rate() as f64 * decoder.channels() as f64)
                .round() as usize
        });
        let samples_per_bucket = estimated_total_samples
            .map(|total| (total / bucket_count).max(1))
            .unwrap_or(2048);
        let mut samples = Vec::new();
        for sample in decoder.by_ref() {
            samples.push(sample);
        }
        return Ok(normalize_waveform_peaks(
            &samples,
            bucket_count,
            samples_per_bucket,
        ));
    }

    let decoded = decode_media_audio(path)?;
    let estimated_total_samples = Some(decoded.samples.len());
    let samples_per_bucket = estimated_total_samples
        .map(|total| (total / bucket_count).max(1))
        .unwrap_or(2048);
    Ok(normalize_waveform_peaks(
        &decoded.samples,
        bucket_count,
        samples_per_bucket,
    ))
}

fn normalize_waveform_peaks(
    samples: &[f32],
    bucket_count: usize,
    samples_per_bucket: usize,
) -> Vec<f32> {
    let mut peaks = vec![0.0f32; bucket_count];
    if samples.is_empty() {
        return peaks;
    }

    for (sample_index, sample) in samples.iter().enumerate() {
        let bucket = (sample_index / samples_per_bucket).min(bucket_count - 1);
        peaks[bucket] = peaks[bucket].max(sample.abs());
    }

    let peak_max = peaks
        .iter()
        .copied()
        .fold(0.0f32, |best, current| best.max(current));
    if peak_max > 0.0 {
        for peak in &mut peaks {
            *peak /= peak_max;
        }
    }

    peaks
}

fn clipped_source_from_ms(
    clip: &AudioClipSettings,
    start_ms: u64,
) -> Result<Box<dyn Source<Item = rodio::Sample> + Send>> {
    let decoder = open_decoder(&clip.file_path)?;
    let start = Duration::from_millis(start_ms.max(clip.start_ms));
    let speed = clip.speed.clamp(0.25, 3.0);
    if clip.end_ms <= start_ms {
        Ok(Box::new(
            decoder
                .skip_duration(start)
                .take_duration(Duration::ZERO)
                .speed(speed),
        ))
    } else if clip.end_ms <= clip.start_ms {
        Ok(Box::new(
            decoder
                .skip_duration(start)
                .take_duration(Duration::ZERO)
                .speed(speed),
        ))
    } else {
        let end = Duration::from_millis(clip.end_ms);
        let length = end.saturating_sub(start);
        Ok(Box::new(
            decoder
                .skip_duration(start)
                .take_duration(length)
                .speed(speed),
        ))
    }
}

fn preview_state() -> Result<parking_lot::MutexGuard<'static, Option<PreviewState>>> {
    let mut state = PREVIEW_STATE.lock();
    if state.is_none() {
        *state = Some(PreviewState::new()?);
    }
    Ok(state)
}

fn video_preview_state() -> Result<parking_lot::MutexGuard<'static, Option<VideoPreviewAudioState>>>
{
    let mut state = VIDEO_PREVIEW_STATE.lock();
    if state.is_none() {
        *state = Some(VideoPreviewAudioState::new()?);
    }
    Ok(state)
}

fn load_cached_audio(path: &str) -> Result<CachedAudio> {
    let mut decoder = open_decoder(path)?;
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    let samples: Vec<f32> = decoder.by_ref().collect();
    Ok(CachedAudio {
        path: Path::new(path).to_path_buf(),
        channels,
        sample_rate,
        samples: Arc::from(samples.into_boxed_slice()),
    })
}

fn open_decoder(path: &str) -> Result<Decoder<BufReader<File>>> {
    let file = File::open(path).with_context(|| format!("Failed to open audio file: {path}"))?;
    Decoder::new(BufReader::new(file)).context("Failed to decode the audio file")
}

fn computed_duration_ms(sample_count: usize, channels: u16, sample_rate: u32) -> Option<u64> {
    let channel_count = usize::from(channels.max(1));
    let rate = sample_rate.max(1);
    if sample_count == 0 {
        return Some(0);
    }

    let frame_count = sample_count / channel_count;
    Some(((frame_count as f64 / rate as f64) * 1000.0).round().max(0.0) as u64)
}

fn decode_media_audio(path: &str) -> Result<CachedAudio> {
    let file = File::open(path).with_context(|| format!("Failed to open media file: {path}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = Path::new(path).extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }
    let probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("Failed to probe media file")?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .context("No default audio track was found")?;
    let track_id = track.id;
    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("Failed to create media audio decoder")?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let mut channels = track
        .codec_params
        .channels
        .map(|layout| layout.count() as u16)
        .unwrap_or(2);
    let mut samples = Vec::<f32>::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                bail!("Media decoder requested a reset")
            }
            Err(error) => return Err(anyhow::anyhow!("Failed to read media packet: {error}")),
        };
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(anyhow::anyhow!("Failed to decode media audio: {error}")),
        };

        sample_rate = decoded.spec().rate;
        channels = decoded.spec().channels.count() as u16;
        let mut sample_buffer =
            SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        match decoded {
            AudioBufferRef::F32(_) => {}
            _ => {}
        }
        sample_buffer.copy_interleaved_ref(decoded);
        samples.extend_from_slice(sample_buffer.samples());
    }

    Ok(CachedAudio {
        path: Path::new(path).to_path_buf(),
        channels,
        sample_rate,
        samples: Arc::from(samples.into_boxed_slice()),
    })
}
