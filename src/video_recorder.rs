use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, UNIX_EPOCH},
};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::{
        Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect},
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic, IsWindow, SW_RESTORE, ShowWindow},
};

use crate::{
    hotkey,
    model::{HotkeyBinding, QuickVideoRecordMode},
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

#[cfg(windows)]
#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

#[derive(Clone)]
pub struct VideoRecorderConfig {
    pub enabled: bool,
    pub hotkey: Option<HotkeyBinding>,
    pub mode: QuickVideoRecordMode,
    pub target_window: String,
    pub region: Option<(i32, i32, i32, i32)>,
    pub output_dir: PathBuf,
    pub fps: u32,
    pub copy_after_recording: bool,
    pub ffmpeg_exe: PathBuf,
    pub ui_language: crate::model::UiLanguage,
}

impl Default for VideoRecorderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: None,
            mode: QuickVideoRecordMode::FullScreen,
            target_window: String::new(),
            region: None,
            output_dir: PathBuf::new(),
            fps: 60,
            copy_after_recording: true,
            ffmpeg_exe: PathBuf::new(),
            ui_language: crate::model::UiLanguage::English,
        }
    }
}

struct RecordingProcess {
    child: Child,
    output_path: PathBuf,
    log_path: PathBuf,
    region_border: Option<RegionBorder>,
    region_rect: Option<RECT>,
    audio_stop: Arc<AtomicBool>,
    audio_thread: Option<JoinHandle<Result<(), String>>>,
    audio_path: PathBuf,
    copy_after_recording: bool,
    ffmpeg_exe: PathBuf,
    ui_language: crate::model::UiLanguage,
    stream_stop: Option<Arc<AtomicBool>>,
    stream_thread: Option<JoinHandle<()>>,
}

static CONFIG: Lazy<Mutex<VideoRecorderConfig>> =
    Lazy::new(|| Mutex::new(VideoRecorderConfig::default()));
static PROCESS: Lazy<Mutex<Option<RecordingProcess>>> = Lazy::new(|| Mutex::new(None));
static STATUS: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new("Ready".to_owned()));
static ACTIVE: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
static HOTKEY_DOWN: AtomicBool = AtomicBool::new(false);
static HOTKEY_PRESS_ID: AtomicU64 = AtomicU64::new(0);
static REGION_CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
static PRESS_HANDLED_ON_DOWN: AtomicBool = AtomicBool::new(false);
static SESSION_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HardwareEncoderKind {
    Qsv,
    MediaFoundation,
    Software,
}

static HARDWARE_ENCODER: Lazy<Mutex<Option<(String, HardwareEncoderKind)>>> =
    Lazy::new(|| Mutex::new(None));
static PREPARED_FFMPEG: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
static VIDEO_EDIT_BUSY: AtomicBool = AtomicBool::new(false);
static VIDEO_EDIT_PROGRESS: AtomicU64 = AtomicU64::new(0);
static VIDEO_EDIT_STATUS: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new("Ready".to_owned()));
const LIBRARY_PLAYBACK_WIDTH: usize = 640;
const LIBRARY_PLAYBACK_HEIGHT: usize = 360;
const LIBRARY_PLAYBACK_FPS: u64 = 60;

#[derive(Clone)]
pub struct VideoLibraryPreview {
    pub duration_seconds: f64,
    pub file_size: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Option<Vec<u8>>,
}

pub enum VideoPlaybackEvent {
    Frame {
        rgba: Vec<u8>,
        position_seconds: f64,
    },
    Finished,
    Error(String),
}

pub struct VideoPlaybackSession {
    receiver: Receiver<VideoPlaybackEvent>,
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    play: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl VideoPlaybackSession {
    pub fn try_recv(&self) -> Option<VideoPlaybackEvent> {
        self.receiver.try_recv().ok()
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(mut child) = self.child.lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn play(&self) {
        self.play.store(true, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

impl Drop for VideoPlaybackSession {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn set_config(config: VideoRecorderConfig) {
    prepare_hardware_encoding_async(&config.ffmpeg_exe);
    *CONFIG.lock() = config;
}

pub fn set_region(region: Option<(i32, i32, i32, i32)>) {
    let mut config = CONFIG.lock();
    config.region = region;
    config.mode = QuickVideoRecordMode::Region;
}

fn prepare_hardware_encoding_async(ffmpeg_exe: &Path) {
    if !ffmpeg_exe.exists() {
        return;
    }
    let signature = ffmpeg_signature(ffmpeg_exe);
    let mut prepared = PREPARED_FFMPEG.lock();
    if prepared.as_ref() == Some(&signature) {
        return;
    }
    *prepared = Some(signature);
    let ffmpeg_exe = ffmpeg_exe.to_owned();
    thread::spawn(move || {
        detect_hardware_encoder(&ffmpeg_exe);
    });
}

pub fn status() -> String {
    STATUS.lock().clone()
}

pub fn is_recording() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

pub fn is_busy() -> bool {
    BUSY.load(Ordering::Acquire)
}

pub fn is_editing() -> bool {
    VIDEO_EDIT_BUSY.load(Ordering::Acquire)
}

pub fn edit_status() -> String {
    VIDEO_EDIT_STATUS.lock().clone()
}

pub fn edit_progress() -> Option<f32> {
    is_editing().then(|| VIDEO_EDIT_PROGRESS.load(Ordering::Acquire) as f32 / 1000.0)
}

pub fn recorded_videos(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut videos = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        matches!(
                            extension.to_ascii_lowercase().as_str(),
                            "mp4" | "mkv" | "mov" | "webm" | "avi"
                        )
                    })
        })
        .collect::<Vec<_>>();
    videos.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH)
    });
    videos.reverse();
    videos
}

pub fn inspect_recorded_video(
    ffmpeg_exe: &Path,
    video_path: &Path,
    preview_at_seconds: f64,
) -> Result<VideoLibraryPreview, String> {
    if !ffmpeg_exe.exists() {
        return Err("FFmpeg is not installed.".to_owned());
    }
    let file_size = fs::metadata(video_path)
        .map_err(|error| format!("Could not read video: {error}"))?
        .len();
    let duration_seconds = probe_video_duration(ffmpeg_exe, video_path).unwrap_or(0.0);
    let preview_at_seconds = preview_at_seconds
        .max(0.0)
        .min((duration_seconds - 0.05).max(0.0));
    let output = Command::new(ffmpeg_exe)
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{preview_at_seconds:.3}"),
            "-i",
        ])
        .arg(video_path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=560:-2",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("Could not create video preview: {error}"))?;
    let rgba = if output.status.success() && !output.stdout.is_empty() {
        let image = image::load_from_memory(&output.stdout)
            .map_err(|error| format!("Could not decode video preview: {error}"))?
            .to_rgba8();
        Some((image.width(), image.height(), image.into_raw()))
    } else {
        return Err("FFmpeg could not extract a preview frame from this video.".to_owned());
    };
    let (width, height, rgba) = rgba
        .map(|(width, height, rgba)| (width, height, Some(rgba)))
        .unwrap_or((0, 0, None));
    Ok(VideoLibraryPreview {
        duration_seconds,
        file_size,
        width,
        height,
        rgba,
    })
}

pub fn inspect_recorded_video_thumbnail(
    ffmpeg_exe: &Path,
    video_path: &Path,
) -> Result<VideoLibraryPreview, String> {
    if !ffmpeg_exe.exists() {
        return Err("FFmpeg is not installed.".to_owned());
    }
    let file_size = fs::metadata(video_path)
        .map_err(|error| format!("Could not read video: {error}"))?
        .len();
    let duration_seconds = probe_video_duration(ffmpeg_exe, video_path).unwrap_or(0.0);
    let seek_time = if duration_seconds > 0.3 { "0.100" } else { "0.000" };
    let mut output = Command::new(ffmpeg_exe)
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
        .args(["-hide_banner", "-loglevel", "error", "-ss", seek_time, "-i"])
        .arg(video_path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=320:180:force_original_aspect_ratio=decrease,pad=320:180:(ow-iw)/2:(oh-ih)/2",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("Could not create video thumbnail: {error}"))?;

    if (!output.status.success() || output.stdout.len() != 320 * 180 * 4) && seek_time != "0.000" {
        if let Ok(retry) = Command::new(ffmpeg_exe)
            .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
            .args(["-hide_banner", "-loglevel", "error", "-ss", "0.000", "-i"])
            .arg(video_path)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "scale=320:180:force_original_aspect_ratio=decrease,pad=320:180:(ow-iw)/2:(oh-ih)/2",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            if retry.status.success() && retry.stdout.len() == 320 * 180 * 4 {
                output = retry;
            }
        }
    }

    let rgba = if output.status.success() && output.stdout.len() == 320 * 180 * 4 {
        Some(output.stdout)
    } else {
        None
    };

    Ok(VideoLibraryPreview {
        duration_seconds,
        file_size,
        width: 320,
        height: 180,
        rgba,
    })
}

pub fn start_video_library_playback(
    ffmpeg_exe: PathBuf,
    video_path: PathBuf,
    start_seconds: f64,
    end_seconds: f64,
) -> Result<VideoPlaybackSession, String> {
    start_video_library_playback_inner(ffmpeg_exe, video_path, start_seconds, end_seconds, true)
}

pub fn prepare_video_library_playback(
    ffmpeg_exe: PathBuf,
    video_path: PathBuf,
    start_seconds: f64,
    end_seconds: f64,
) -> Result<VideoPlaybackSession, String> {
    start_video_library_playback_inner(ffmpeg_exe, video_path, start_seconds, end_seconds, false)
}

fn start_video_library_playback_inner(
    ffmpeg_exe: PathBuf,
    video_path: PathBuf,
    start_seconds: f64,
    end_seconds: f64,
    play_immediately: bool,
) -> Result<VideoPlaybackSession, String> {
    if !ffmpeg_exe.exists() {
        return Err("FFmpeg is not installed.".to_owned());
    }
    if !video_path.is_file() {
        return Err("Video file was not found.".to_owned());
    }
    let has_duration_limit = end_seconds > start_seconds;
    let duration = end_seconds - start_seconds;
    let (sender, receiver) = sync_channel(2);
    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let play = Arc::new(AtomicBool::new(play_immediately));
    let ready = Arc::new(AtomicBool::new(false));
    let child_holder = Arc::new(Mutex::new(None));
    let worker_child = child_holder.clone();
    let worker_stop = stop.clone();
    let worker_finished = finished.clone();
    let worker_play = play.clone();
    let worker_ready = ready.clone();
    thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let mut cmd = Command::new(ffmpeg_exe);
            cmd.creation_flags(CREATE_NO_WINDOW)
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-probesize",
                    "64K",
                    "-analyzeduration",
                    "0",
                    "-ss",
                ])
                .arg(format!("{:.3}", start_seconds.max(0.0)))
                .args(["-i"])
                .arg(video_path);
            if has_duration_limit {
                cmd.args(["-t", &format!("{duration:.3}")]);
            }
            cmd.args([
                "-an",
                "-vf",
                "scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2,fps=60",
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "pipe:1",
            ]);
            let mut child = cmd
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| format!("Could not start embedded video playback: {error}"))?;
            let mut stdout = child
                .stdout
                .take()
                .ok_or_else(|| "FFmpeg playback output was unavailable.".to_owned())?;
            *worker_child.lock() = Some(child);
            let mut frame = vec![0_u8; LIBRARY_PLAYBACK_WIDTH * LIBRARY_PLAYBACK_HEIGHT * 4];
            let mut frame_index = 0_u64;
            let mut started_at = None;
            loop {
                if worker_stop.load(Ordering::Acquire) {
                    if let Some(mut child) = worker_child.lock().take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    break;
                }
                match stdout.read_exact(&mut frame) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(error) => return Err(format!("Could not decode video frame: {error}")),
                }
                if frame_index == 0 {
                    worker_ready.store(true, Ordering::Release);
                    if sender
                        .send(VideoPlaybackEvent::Frame {
                            rgba: frame.clone(),
                            position_seconds: start_seconds,
                        })
                        .is_err()
                    {
                        if let Some(mut child) = worker_child.lock().take() {
                            let _ = child.kill();
                            let _ = child.wait();
                        }
                        break;
                    }
                    while !worker_play.load(Ordering::Acquire) {
                        if worker_stop.load(Ordering::Acquire) {
                            if let Some(mut child) = worker_child.lock().take() {
                                let _ = child.kill();
                                let _ = child.wait();
                            }
                            return Ok(());
                        }
                        thread::sleep(Duration::from_millis(2));
                    }
                    started_at = Some(Instant::now());
                    frame_index = 1;
                    continue;
                }
                let due = Duration::from_secs_f64(frame_index as f64 / LIBRARY_PLAYBACK_FPS as f64);
                if let Some(wait) =
                    due.checked_sub(started_at.unwrap_or_else(Instant::now).elapsed())
                {
                    thread::sleep(wait);
                }
                let event = VideoPlaybackEvent::Frame {
                    rgba: frame.clone(),
                    position_seconds: start_seconds
                        + frame_index as f64 / LIBRARY_PLAYBACK_FPS as f64,
                };
                if sender.send(event).is_err() {
                    if let Some(mut child) = worker_child.lock().take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    break;
                }
                frame_index += 1;
            }
            if let Some(mut child) = worker_child.lock().take() {
                let _ = child.wait();
            }
            Ok(())
        })();
        let _ = sender.send(match result {
            Ok(()) => VideoPlaybackEvent::Finished,
            Err(error) => VideoPlaybackEvent::Error(error),
        });
        worker_finished.store(true, Ordering::Release);
    });
    Ok(VideoPlaybackSession {
        receiver,
        stop,
        finished,
        play,
        ready,
        child: child_holder,
    })
}

pub fn export_trim_async(
    ffmpeg_exe: PathBuf,
    input_path: PathBuf,
    output_dir: PathBuf,
    start_seconds: f64,
    end_seconds: f64,
    target_size_mb: Option<u32>,
) {
    if VIDEO_EDIT_BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    VIDEO_EDIT_PROGRESS.store(0, Ordering::Release);
    *VIDEO_EDIT_STATUS.lock() = "Preparing video…".to_owned();
    thread::spawn(move || {
        let result = export_trim(
            &ffmpeg_exe,
            &input_path,
            &output_dir,
            start_seconds,
            end_seconds,
            target_size_mb,
        );
        *VIDEO_EDIT_STATUS.lock() = match result {
            Ok(path) => {
                VIDEO_EDIT_PROGRESS.store(1000, Ordering::Release);
                format!("Saved: {}", path.display())
            }
            Err(error) => format!("Video export failed: {error}"),
        };
        VIDEO_EDIT_BUSY.store(false, Ordering::Release);
    });
}

fn probe_video_duration(ffmpeg_exe: &Path, video_path: &Path) -> Option<f64> {
    let ffprobe_exe = ffmpeg_exe.with_file_name("ffprobe.exe");
    if ffprobe_exe.exists() {
        let output = Command::new(ffprobe_exe)
            .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(video_path)
            .output()
            .ok()?;
        return String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<f64>()
            .ok();
    }

    // ponytail: MacroNest's bundled FFmpeg does not include ffprobe, so reuse the
    // available executable instead of requiring another tool just to read duration.
    let output = Command::new(ffmpeg_exe)
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
        .args(["-hide_banner", "-i"])
        .arg(video_path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stderr);
    let duration = text.split("Duration: ").nth(1)?.split(',').next()?.trim();
    let mut parts = duration.split(':').map(|part| part.parse::<f64>().ok());
    Some(parts.next()?? * 3600.0 + parts.next()?? * 60.0 + parts.next()??)
}

fn export_trim(
    ffmpeg_exe: &Path,
    input_path: &Path,
    output_dir: &Path,
    start_seconds: f64,
    end_seconds: f64,
    target_size_mb: Option<u32>,
) -> Result<PathBuf, String> {
    if !ffmpeg_exe.exists() {
        return Err("FFmpeg is not installed.".to_owned());
    }
    let duration = end_seconds - start_seconds;
    if !duration.is_finite() || duration <= 0.05 {
        return Err("Choose an end time after the start time.".to_owned());
    }
    fs::create_dir_all(output_dir).map_err(|error| error.to_string())?;
    let stem = input_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("video");
    let suffix = if target_size_mb.is_some() {
        "compressed"
    } else {
        "trimmed"
    };
    let output_path = unique_output_path(output_dir, &format!("{stem}_{suffix}"));
    let mut command = Command::new(ffmpeg_exe);
    command
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-threads", "0", "-ss"])
        .arg(format!("{:.3}", start_seconds.max(0.0)))
        .args(["-i"])
        .arg(input_path)
        .args([
            "-t",
            &format!("{duration:.3}"),
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
        ]);
    if let Some(target_size_mb) = target_size_mb {
        command.args([
            "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-b:a", "96k",
        ]);
        let target_kbps = ((target_size_mb as f64 * 8192.0 / duration) - 96.0)
            .round()
            .clamp(100.0, 80_000.0) as u32;
        command.args([
            "-b:v",
            &format!("{target_kbps}k"),
            "-maxrate",
            &format!("{target_kbps}k"),
            "-bufsize",
            &format!("{}k", target_kbps.saturating_mul(2)),
        ]);
    } else {
        // ponytail: a normal trim is a stream copy; use Compress when re-encoding is required.
        command.args(["-c", "copy"]);
    }
    let mut child = command
        .args(["-movflags", "+faststart", "-progress", "pipe:1", "-nostats"])
        .arg(&output_path)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start FFmpeg: {error}"))?;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(value) = line
                .strip_prefix("out_time_us=")
                .and_then(|value| value.parse::<u64>().ok())
            {
                let progress = (value as f64 / (duration * 1_000_000.0)).clamp(0.0, 1.0);
                VIDEO_EDIT_PROGRESS.store((progress * 1000.0).round() as u64, Ordering::Release);
            }
        }
    }
    let status = child
        .wait()
        .map_err(|error| format!("Could not wait for FFmpeg: {error}"))?;
    if !status.success() || fs::metadata(&output_path).map_or(true, |metadata| metadata.len() == 0)
    {
        let _ = fs::remove_file(&output_path);
        return Err("FFmpeg could not export this video.".to_owned());
    }
    Ok(output_path)
}

pub fn toggle_async() {
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    thread::spawn(|| {
        if ACTIVE.load(Ordering::Acquire) {
            stop_recording_inner();
        } else if let Err(error) = start_recording_inner() {
            *STATUS.lock() = error;
            ACTIVE.store(false, Ordering::Release);
        }
        BUSY.store(false, Ordering::Release);
    });
}

pub fn start_region_async(region: (i32, i32, i32, i32)) {
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    thread::spawn(move || {
        let mut config = CONFIG.lock().clone();
        config.mode = QuickVideoRecordMode::Region;
        config.region = Some(region);
        if let Err(error) = start_recording_with_config(config) {
            *STATUS.lock() = error;
            ACTIVE.store(false, Ordering::Release);
        }
        BUSY.store(false, Ordering::Release);
    });
}

pub fn stop_blocking() {
    if ACTIVE.load(Ordering::Acquire) || PROCESS.lock().is_some() {
        stop_recording_inner();
    }
}

pub fn process_hotkey(binding: &HotkeyBinding, is_down: bool, is_repeat: bool) -> bool {
    let config = CONFIG.lock();
    let matches = config.hotkey.as_ref().is_some_and(|trigger| {
        if is_down {
            hotkey::binding_matches(trigger, binding)
        } else {
            trigger.key.eq_ignore_ascii_case(&binding.key)
        }
    });
    drop(config);
    if !matches {
        return false;
    }
    if is_down {
        if is_repeat || HOTKEY_DOWN.swap(true, Ordering::AcqRel) {
            return true;
        }
        PRESS_HANDLED_ON_DOWN.store(false, Ordering::Release);
        if ACTIVE.load(Ordering::Acquire) || BUSY.load(Ordering::Acquire) {
            PRESS_HANDLED_ON_DOWN.store(true, Ordering::Release);
            toggle_async();
            return true;
        }
        let press_id = HOTKEY_PRESS_ID.fetch_add(1, Ordering::AcqRel) + 1;
        let trigger = binding.clone();
        let ui_lang = crate::overlay::current_ui_language();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            if HOTKEY_DOWN.load(Ordering::Acquire)
                && HOTKEY_PRESS_ID.load(Ordering::Acquire) == press_id
                && !ACTIVE.load(Ordering::Acquire)
                && !BUSY.load(Ordering::Acquire)
            {
                REGION_CAPTURE_ACTIVE.store(true, Ordering::Release);
                crate::overlay::native_capture::run_native_video_record_region_overlay(
                    Some(trigger),
                    ui_lang,
                );
            }
        });
    } else {
        let was_down = HOTKEY_DOWN.swap(false, Ordering::AcqRel);
        HOTKEY_PRESS_ID.fetch_add(1, Ordering::AcqRel);
        if REGION_CAPTURE_ACTIVE.swap(false, Ordering::AcqRel) {
            crate::overlay::screen_draw_release_video_region_capture();
        } else if was_down
            && !PRESS_HANDLED_ON_DOWN.swap(false, Ordering::AcqRel)
            && !ACTIVE.load(Ordering::Acquire)
            && !BUSY.load(Ordering::Acquire)
        {
            toggle_async();
        }
    }
    true
}

fn start_recording_inner() -> Result<(), String> {
    start_recording_with_config(CONFIG.lock().clone())
}

enum ActiveCaptureSession {
    Wgc(crate::window_list::WgcSession),
    Game(crate::game_capture::GameCaptureSession),
}

impl ActiveCaptureSession {
    fn poll_into_buffer(&mut self, buffer: &mut Vec<u8>, w: usize, h: usize) -> anyhow::Result<bool> {
        match self {
            Self::Wgc(s) => s.poll_into_buffer(buffer, w, h),
            Self::Game(s) => s.poll_into_buffer(buffer, w, h),
        }
    }

    fn has_nvenc(&self) -> bool {
        match self {
            Self::Game(s) => s.has_nvenc(),
            _ => false,
        }
    }

    fn poll_encoded_frame(&mut self, force_idr: bool) -> anyhow::Result<Option<&'static [u8]>> {
        match self {
            Self::Game(s) => s.poll_encoded_frame(force_idr),
            _ => anyhow::bail!("Not an NVENC session"),
        }
    }
}

fn start_recording_with_config(config: VideoRecorderConfig) -> Result<(), String> {
    if !config.ffmpeg_exe.exists() {
        return Err(
            "FFmpeg is not installed. Install it in Settings > Downloaded Tools.".to_owned(),
        );
    }
    if config.output_dir.as_os_str().is_empty() {
        return Err("Choose a video save folder first.".to_owned());
    }
    fs::create_dir_all(&config.output_dir)
        .map_err(|error| format!("Could not create the video folder: {error}"))?;

    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let output_path = unique_output_path(&config.output_dir, &format!("MacroNest_{timestamp}"));
    let audio_path = config.output_dir.join(format!(
        ".macronest-video-audio-{}-{timestamp}.f32le",
        std::process::id()
    ));
    let (audio_stop, audio_thread, audio_start) = start_system_audio_capture(&audio_path)?;

    let source = match capture_source(&config) {
        Ok(res) => res,
        Err(err) => {
            audio_stop.store(true, Ordering::Release);
            let _ = audio_start.send(());
            let _ = audio_thread.join();
            let _ = fs::remove_file(&audio_path);
            return Err(err);
        }
    };
    let border_rect = match &source {
        CaptureSource::Desktop { .. } => {
            let (left, top, width, height) = crate::window_list::virtual_screen_bounds();
            Some((
                RECT {
                    left,
                    top,
                    right: left + width,
                    bottom: top + height,
                },
                false,
            ))
        }
        CaptureSource::WgcWindow { hwnd, .. } => {
            let mut r = RECT::default();
            unsafe {
                if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(*hwnd, &mut r).is_ok() {
                    Some((r, false))
                } else {
                    None
                }
            }
        }
        CaptureSource::Region { region, .. } => Some((*region, false)),
        CaptureSource::GameCapture { hwnd, .. } => {
            let mut r = RECT::default();
            unsafe {
                if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(*hwnd, &mut r).is_ok() {
                    Some((r, true))
                } else {
                    None
                }
            }
        }
    };
    let (region_border, recording_active_signal) = match border_rect {
        Some((rect, badge_only)) => {
            let (border, signal) = RegionBorder::start(rect, config.ui_language, badge_only);
            (Some(border), Some(signal))
        }
        None => (None, None),
    };

    let log_path = config.output_dir.join(".macronest-video-recorder.log");
    let log = match File::create(&log_path) {
        Ok(file) => file,
        Err(error) => {
            audio_stop.store(true, Ordering::Release);
            let _ = audio_start.send(());
            let _ = audio_thread.join();
            let _ = fs::remove_file(&audio_path);
            return Err(format!("Could not create the recorder log: {error}"));
        }
    };

    let gop_size = (config.fps.clamp(1, 240) * 2).to_string();
    let mut command = Command::new(&config.ffmpeg_exe);
    command
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));

    let (mut session_opt, initial_frame_opt) = match &source {
        CaptureSource::WgcWindow { hwnd, .. } => {
            let mut session = match crate::window_list::init_wgc_session(*hwnd) {
                Ok(s) => s,
                Err(err) => {
                    audio_stop.store(true, Ordering::Release);
                    let _ = audio_start.send(());
                    let _ = audio_thread.join();
                    let _ = fs::remove_file(&audio_path);
                    return Err(format!("Could not initialize window capture: {err}"));
                }
            };
            let initial_frame = match session.get_next_frame() {
                Ok(f) => f,
                Err(err) => {
                    audio_stop.store(true, Ordering::Release);
                    let _ = audio_start.send(());
                    let _ = audio_thread.join();
                    let _ = fs::remove_file(&audio_path);
                    return Err(format!("Could not capture initial window frame: {err}"));
                }
            };
            (Some(ActiveCaptureSession::Wgc(session)), Some(initial_frame))
        }
        CaptureSource::GameCapture { hwnd, .. } => {
            let paths = crate::storage::AppPaths::discover().map_err(|e| format!("{e}"))?;
            let mut session = match crate::game_capture::GameCaptureSession::start(*hwnd, &paths) {
                Ok(s) => s,
                Err(err) => {
                    audio_stop.store(true, Ordering::Release);
                    let _ = audio_start.send(());
                    let _ = audio_thread.join();
                    let _ = fs::remove_file(&audio_path);
                    return Err(format!("Could not initialize Game Capture (OBS Hook): {err}"));
                }
            };
            let (width, height) = session.dimensions();
            let mut initial_rgba = Vec::new();
            if !session.has_nvenc() {
                if let Err(err) = session.poll_into_buffer(&mut initial_rgba, width as usize, height as usize) {
                    audio_stop.store(true, Ordering::Release);
                    let _ = audio_start.send(());
                    let _ = audio_thread.join();
                    let _ = fs::remove_file(&audio_path);
                    return Err(format!("Could not capture initial game frame: {err}"));
                }
            }
            let initial_frame = crate::window_list::ScreenCaptureFrame {
                screen_x: 0,
                screen_y: 0,
                width: width as usize,
                height: height as usize,
                rgba: initial_rgba,
            };
            (Some(ActiveCaptureSession::Game(session)), Some(initial_frame))
        }
        _ => (None, None),
    };

    let use_nvenc = session_opt.as_ref().map(|s| s.has_nvenc()).unwrap_or(false);

    match &source {
        CaptureSource::Desktop { fps } => {
            command.args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-thread_queue_size",
                "1024",
                "-rtbufsize",
                "512M",
                "-f",
                "gdigrab",
                "-draw_mouse",
                "1",
                "-framerate",
                &fps.to_string(),
                "-i",
                "desktop",
            ]);
        }
        CaptureSource::Region { region, fps } => {
            let width = region.right - region.left;
            let height = region.bottom - region.top;
            command.args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-thread_queue_size",
                "1024",
                "-rtbufsize",
                "512M",
                "-f",
                "gdigrab",
                "-draw_mouse",
                "1",
                "-framerate",
                &fps.to_string(),
                "-offset_x",
                &region.left.to_string(),
                "-offset_y",
                &region.top.to_string(),
                "-video_size",
                &format!("{width}x{height}"),
                "-i",
                "desktop",
            ]);
        }
        CaptureSource::WgcWindow { fps, .. } | CaptureSource::GameCapture { fps, .. } => {
            if use_nvenc {
                command.args([
                    "-y",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-thread_queue_size",
                    "32",
                    "-f",
                    "h264",
                    "-r",
                    &fps.to_string(),
                    "-i",
                    "pipe:0",
                ]);
            } else {
                let initial = initial_frame_opt.as_ref().unwrap();
                let width = initial.width;
                let height = initial.height;
                command.args([
                    "-y",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-thread_queue_size",
                    "32",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "bgra",
                    "-s",
                    &format!("{width}x{height}"),
                    "-r",
                    &fps.to_string(),
                    "-i",
                    "pipe:0",
                ]);
            }
        }
    }

    if use_nvenc {
        command.args([
            "-an",
            "-c:v",
            "copy",
            "-fps_mode",
            "cfr",
            "-movflags",
            "+faststart",
        ]);
    } else {
        let encoder_kind = detect_hardware_encoder(&config.ffmpeg_exe);
        match encoder_kind {
            HardwareEncoderKind::Qsv => {
                command.args([
                    "-threads",
                    "2",
                    "-vf",
                    "format=nv12",
                    "-an",
                    "-c:v",
                    "h264_qsv",
                    "-preset",
                    "veryfast",
                    "-scenario",
                    "displayremoting",
                    "-async_depth",
                    "2",
                    "-look_ahead",
                    "0",
                    "-b:v",
                    "12M",
                    "-g",
                    &gop_size,
                    "-fps_mode",
                    "cfr",
                    "-avoid_negative_ts",
                    "make_zero",
                    "-movflags",
                    "+faststart",
                ]);
            }
            HardwareEncoderKind::MediaFoundation => {
                command.args([
                    "-vf",
                    "format=yuv420p",
                    "-an",
                    "-c:v",
                    "h264_mf",
                    "-b:v",
                    "12M",
                    "-g",
                    &gop_size,
                    "-fps_mode",
                    "cfr",
                    "-avoid_negative_ts",
                    "make_zero",
                    "-movflags",
                    "+faststart",
                ]);
            }
            HardwareEncoderKind::Software => {
                command.args([
                    "-vf",
                    "format=yuv420p",
                    "-an",
                    "-c:v",
                    "libx264",
                    "-preset",
                    "ultrafast",
                    "-tune",
                    "zerolatency",
                    "-crf",
                    "20",
                    "-g",
                    &gop_size,
                    "-bf",
                    "0",
                    "-fps_mode",
                    "cfr",
                    "-avoid_negative_ts",
                    "make_zero",
                    "-movflags",
                    "+faststart",
                ]);
            }
        }
    }
    command.arg(&output_path);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            audio_stop.store(true, Ordering::Release);
            let _ = audio_start.send(());
            let _ = audio_thread.join();
            let _ = fs::remove_file(&audio_path);
            return Err(format!("Could not start FFmpeg: {error}"));
        }
    };

    let (stream_stop, stream_thread) = if let Some(session) = session_opt.take() {
        let initial = initial_frame_opt.unwrap();
        let width = initial.width;
        let height = initial.height;
        let fps = match source {
            CaptureSource::WgcWindow { fps, .. } | CaptureSource::GameCapture { fps, .. } => fps,
            _ => 60,
        };
        let stdin = match child.stdin.take() {
            Some(pipe) => pipe,
            None => {
                audio_stop.store(true, Ordering::Release);
                let _ = audio_start.send(());
                let _ = audio_thread.join();
                let _ = fs::remove_file(&audio_path);
                return Err("Could not connect to FFmpeg video stream.".to_owned());
            }
        };
        let stop_signal = Arc::new(AtomicBool::new(false));
        let feeder_stop = stop_signal.clone();
        let feeder_fps = fps.max(1) as u64;
        let audio_start_clone = audio_start.clone();
        let thread_handle = thread::spawn(move || {
            #[cfg(windows)]
            unsafe {
                let _ = timeBeginPeriod(1);
            }
            let mut active_session = session;
            let use_nvenc = active_session.has_nvenc();
            let mut last_frame = initial.rgba;
            let mut pipe = std::io::BufWriter::with_capacity(8 * 1024 * 1024, stdin);

            if use_nvenc {
                if let Ok(Some(first_packet)) = active_session.poll_encoded_frame(true) {
                    if pipe.write_all(first_packet).is_ok() && pipe.flush().is_ok() {
                        let _ = audio_start_clone.send(());
                    } else {
                        #[cfg(windows)]
                        unsafe {
                            let _ = timeEndPeriod(1);
                        }
                        return;
                    }
                } else {
                    #[cfg(windows)]
                    unsafe {
                        let _ = timeEndPeriod(1);
                    }
                    return;
                }
            } else {
                if pipe.write_all(&last_frame).is_ok() && pipe.flush().is_ok() {
                    let _ = audio_start_clone.send(());
                } else {
                    #[cfg(windows)]
                    unsafe {
                        let _ = timeEndPeriod(1);
                    }
                    return;
                }
            }

            let frame_duration = Duration::from_micros(1_000_000 / feeder_fps);
            let mut start_time = Instant::now();
            let mut frame_count: u64 = 0;

            'feeder: while !feeder_stop.load(Ordering::Acquire) {
                frame_count += 1;
                let target_time = start_time + Duration::from_micros(frame_count * 1_000_000 / feeder_fps);
                let now = Instant::now();
                if target_time > now {
                    let diff = target_time - now;
                    thread::sleep(diff);
                } else if now.saturating_duration_since(target_time) > frame_duration * 3 {
                    // Heavily lagged behind, resync time anchor
                    start_time = now;
                    frame_count = 0;
                }

                if use_nvenc {
                    let force_idr = (frame_count % (feeder_fps * 2)) == 0;
                    if let Ok(Some(packet)) = active_session.poll_encoded_frame(force_idr) {
                        if !packet.is_empty() {
                            if pipe.write_all(packet).is_err() {
                                break 'feeder;
                            }
                        }
                    }
                } else {
                    let _ = active_session.poll_into_buffer(&mut last_frame, width, height);

                    if pipe.write_all(&last_frame).is_err() {
                        break 'feeder;
                    }
                }
            }
            let _ = pipe.flush();
            #[cfg(windows)]
            unsafe {
                let _ = timeEndPeriod(1);
            }
        });
        (Some(stop_signal), Some(thread_handle))
    } else {
        let _ = audio_start.send(());
        (None, None)
    };

    if let Some(signal) = recording_active_signal {
        signal.store(true, Ordering::Release);
    }
    let session_id = SESSION_ID.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    *PROCESS.lock() = Some(RecordingProcess {
        child,
        output_path: output_path.clone(),
        log_path,
        region_border,
        region_rect: border_rect.map(|(r, _)| r),
        audio_stop,
        audio_thread: Some(audio_thread),
        audio_path,
        copy_after_recording: config.copy_after_recording,
        ffmpeg_exe: config.ffmpeg_exe,
        ui_language: config.ui_language,
        stream_stop,
        stream_thread,
    });
    ACTIVE.store(true, Ordering::Release);
    crate::platform::update_native_taskbar_recording_state(true);
    crate::overlay::request_ui_repaint();
    *STATUS.lock() = format!("Recording: {}", output_path.display());
    spawn_exit_watchdog(session_id);
    Ok(())
}

fn stop_recording_inner() {
    let Some(mut recording) = PROCESS.lock().take() else {
        ACTIVE.store(false, Ordering::Release);
        crate::platform::update_native_taskbar_recording_state(false);
        crate::overlay::request_ui_repaint();
        return;
    };
    recording.region_border.take();
    ACTIVE.store(false, Ordering::Release);
    crate::platform::update_native_taskbar_recording_state(false);
    crate::overlay::request_ui_repaint();
    *STATUS.lock() = "Finishing video...".to_owned();
    recording.audio_stop.store(true, Ordering::Release);
    if let Some(stop) = recording.stream_stop.take() {
        stop.store(true, Ordering::Release);
    }
    if let Some(thread) = recording.stream_thread.take() {
        let _ = thread.join();
    }
    if let Some(stdin) = recording.child.stdin.as_mut() {
        let _ = stdin.write_all(b"q\n");
        let _ = stdin.flush();
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match recording.child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = recording.child.kill();
                let _ = recording.child.wait();
                break;
            }
        }
    }
    recording.audio_stop.store(true, Ordering::Release);
    let audio_result = recording
        .audio_thread
        .take()
        .map(|worker| {
            worker
                .join()
                .map_err(|_| "System audio capture stopped unexpectedly.".to_owned())?
        })
        .unwrap_or(Ok(()));
    let mux_result = audio_result.and_then(|_| {
        mux_system_audio(
            &recording.ffmpeg_exe,
            &recording.audio_path,
            &recording.output_path,
        )
    });
    let _ = fs::remove_file(&recording.audio_path);
    let mut status = match mux_result {
        Ok(()) => format!(
            "Saved with system audio: {}",
            recording.output_path.display()
        ),
        Err(error) => format!(
            "Saved without system audio: {} ({error})",
            recording.output_path.display()
        ),
    };
    if recording.copy_after_recording {
        match copy_video_to_clipboard(&recording.output_path) {
            Ok(()) => {
                status.push_str(" - copied");
                show_video_copy_toast_async(recording.region_rect, recording.ui_language);
            }
            Err(error) => status.push_str(&format!(" - copy failed: {error}")),
        }
    }
    *STATUS.lock() = status;
    let _ = fs::remove_file(recording.log_path);
}

fn spawn_exit_watchdog(session_id: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(600));
        loop {
            if SESSION_ID.load(Ordering::Acquire) != session_id || !ACTIVE.load(Ordering::Acquire) {
                return;
            }
            let exited = {
                let mut guard = PROCESS.lock();
                guard
                    .as_mut()
                    .and_then(|recording| recording.child.try_wait().ok().flatten())
                    .is_some()
            };
            if exited {
                if let Some(mut recording) = PROCESS.lock().take() {
                    recording.region_border.take();
                    recording.audio_stop.store(true, Ordering::Release);
                    if let Some(worker) = recording.audio_thread.take() {
                        let _ = worker.join();
                    }
                    let error = fs::read_to_string(&recording.log_path).unwrap_or_default();
                    let _ = fs::remove_file(&recording.log_path);
                    let _ = fs::remove_file(&recording.audio_path);
                    let _ = fs::remove_file(&recording.output_path);
                    *STATUS.lock() = if error.trim().is_empty() {
                        "Video recording stopped unexpectedly.".to_owned()
                    } else {
                        format!("Video recording failed: {}", concise_ffmpeg_error(&error))
                    };
                }
                ACTIVE.store(false, Ordering::Release);
                return;
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
}

fn concise_ffmpeg_error(log: &str) -> String {
    let detail = log
        .lines()
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("error") || lower.contains("failed")
        })
        .or_else(|| log.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or("FFmpeg stopped unexpectedly")
        .trim();
    detail.chars().take(180).collect()
}

fn detect_hardware_encoder(ffmpeg_exe: &Path) -> HardwareEncoderKind {
    let signature = ffmpeg_signature(ffmpeg_exe);
    let mut cached = HARDWARE_ENCODER.lock();
    if let Some((cached_signature, kind)) = cached.as_ref()
        && cached_signature == &signature
    {
        return *kind;
    }
    let cache_path = ffmpeg_exe.with_file_name("ffmpeg-hardware-encoder-choice.cache");
    if let Ok(value) = fs::read_to_string(&cache_path)
        && let Some((cached_signature, kind_str)) = value.trim().rsplit_once('|')
        && cached_signature == signature
    {
        let kind = match kind_str {
            "qsv" => HardwareEncoderKind::Qsv,
            "mf" => HardwareEncoderKind::MediaFoundation,
            _ => HardwareEncoderKind::Software,
        };
        *cached = Some((signature, kind));
        return kind;
    }

    let exe = ffmpeg_exe.to_path_buf();
    let mf_check = {
        let mut cmd = Command::new(&exe);
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .args([
                "-hide_banner",
                "-loglevel",
                "quiet",
                "-f",
                "lavfi",
                "-i",
                "color=size=64x64:rate=1",
                "-frames:v",
                "1",
                "-c:v",
                "h264_mf",
                "-f",
                "null",
                "-",
            ]);
        cmd.status().map_or(false, |s| s.success())
    };

    let kind = if mf_check {
        HardwareEncoderKind::MediaFoundation
    } else {
        let qsv_check = {
            let mut cmd = Command::new(&exe);
            cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "quiet",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=size=64x64:rate=1",
                    "-frames:v",
                    "1",
                    "-c:v",
                    "h264_qsv",
                    "-f",
                    "null",
                    "-",
                ]);
            cmd.status().map_or(false, |s| s.success())
        };
        if qsv_check {
            HardwareEncoderKind::Qsv
        } else {
            HardwareEncoderKind::Software
        }
    };

    let kind_str = match kind {
        HardwareEncoderKind::Qsv => "qsv",
        HardwareEncoderKind::MediaFoundation => "mf",
        HardwareEncoderKind::Software => "software",
    };
    let _ = fs::write(&cache_path, format!("{signature}|{kind_str}"));
    *cached = Some((signature, kind));
    kind
}

fn ffmpeg_signature(ffmpeg_exe: &Path) -> String {
    let Ok(metadata) = fs::metadata(ffmpeg_exe) else {
        return ffmpeg_exe.display().to_string();
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs());
    format!("{}:{}:{}", ffmpeg_exe.display(), metadata.len(), modified)
}

fn start_system_audio_capture(
    audio_path: &Path,
) -> Result<
    (
        Arc<AtomicBool>,
        JoinHandle<Result<(), String>>,
        SyncSender<()>,
    ),
    String,
> {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker_path = audio_path.to_path_buf();
    let (ready_tx, ready_rx) = sync_channel(1);
    let (start_tx, start_rx) = sync_channel(1);
    let worker =
        thread::spawn(move || capture_system_audio(&worker_path, worker_stop, ready_tx, start_rx));
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => Ok((stop, worker, start_tx)),
        Ok(Err(error)) => {
            stop.store(true, Ordering::Release);
            let _ = start_tx.send(());
            let _ = worker.join();
            let _ = fs::remove_file(audio_path);
            Err(format!("Could not start system audio capture: {error}"))
        }
        Err(_) => {
            stop.store(true, Ordering::Release);
            let _ = start_tx.send(());
            let _ = worker.join();
            let _ = fs::remove_file(audio_path);
            Err("System audio capture did not start in time.".to_owned())
        }
    }
}

fn capture_system_audio(
    audio_path: &Path,
    stop: Arc<AtomicBool>,
    ready: SyncSender<Result<(), String>>,
    start: Receiver<()>,
) -> Result<(), String> {
    use wasapi::{
        Direction, SampleType, StreamMode, WaveFormat, get_default_device, initialize_mta,
    };

    let initialized = (|| -> Result<_, String> {
        let _ = initialize_mta();
        let device = get_default_device(&Direction::Render).map_err(|error| error.to_string())?;
        let mut audio_client = device
            .get_iaudioclient()
            .map_err(|error| error.to_string())?;
        let format = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None);
        let (_, min_time) = audio_client
            .get_device_period()
            .map_err(|error| error.to_string())?;
        audio_client
            .initialize_client(
                &format,
                &Direction::Capture,
                &StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: min_time,
                },
            )
            .map_err(|error| error.to_string())?;
        let event = audio_client
            .set_get_eventhandle()
            .map_err(|error| error.to_string())?;
        let capture = audio_client
            .get_audiocaptureclient()
            .map_err(|error| error.to_string())?;
        let file = File::create(audio_path).map_err(|error| error.to_string())?;
        Ok((audio_client, capture, event, BufWriter::new(file)))
    })();

    let (mut audio_client, capture, event, mut output) = match initialized {
        Ok(values) => {
            let _ = ready.send(Ok(()));
            values
        }
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    if start.recv_timeout(Duration::from_secs(5)).is_err() {
        return Ok(());
    }
    audio_client
        .start_stream()
        .map_err(|error| error.to_string())?;

    let audio_start_time = Instant::now();
    let mut total_bytes_written: u64 = 0;
    const BYTES_PER_SAMPLE_FRAME: usize = 8;
    const BYTES_PER_SEC: usize = 48_000 * BYTES_PER_SAMPLE_FRAME;
    const SILENCE_MARGIN_BYTES: usize = (48_000 * 20 / 1000) * BYTES_PER_SAMPLE_FRAME;
    let silence_chunk = [0u8; 8192];
    let mut samples = VecDeque::new();

    while !stop.load(Ordering::Acquire) {
        let _ = event.wait_for_event(20);

        while let Ok(Some(packet_size)) = capture.get_next_packet_size() {
            if packet_size == 0 {
                break;
            }
            capture
                .read_from_device_to_deque(&mut samples)
                .map_err(|error| error.to_string())?;
            let (first, second) = samples.as_slices();
            if !first.is_empty() {
                output.write_all(first).map_err(|error| error.to_string())?;
                total_bytes_written += first.len() as u64;
            }
            if !second.is_empty() {
                output.write_all(second).map_err(|error| error.to_string())?;
                total_bytes_written += second.len() as u64;
            }
            samples.clear();
        }

        let elapsed_micros = audio_start_time.elapsed().as_micros();
        let expected_bytes = ((elapsed_micros * BYTES_PER_SEC as u128) / 1_000_000) as u64;
        let expected_bytes = (expected_bytes / BYTES_PER_SAMPLE_FRAME as u64) * BYTES_PER_SAMPLE_FRAME as u64;
        if expected_bytes > total_bytes_written + SILENCE_MARGIN_BYTES as u64 {
            let mut gap = (expected_bytes - total_bytes_written) as usize;
            gap = (gap / BYTES_PER_SAMPLE_FRAME) * BYTES_PER_SAMPLE_FRAME;
            while gap > 0 {
                let to_write = gap.min(silence_chunk.len());
                output.write_all(&silence_chunk[..to_write]).map_err(|error| error.to_string())?;
                total_bytes_written += to_write as u64;
                gap -= to_write;
            }
        }
    }

    let elapsed_micros = audio_start_time.elapsed().as_micros();
    let expected_bytes = ((elapsed_micros * BYTES_PER_SEC as u128) / 1_000_000) as u64;
    let expected_bytes = (expected_bytes / BYTES_PER_SAMPLE_FRAME as u64) * BYTES_PER_SAMPLE_FRAME as u64;
    if expected_bytes > total_bytes_written {
        let mut gap = (expected_bytes - total_bytes_written) as usize;
        gap = (gap / BYTES_PER_SAMPLE_FRAME) * BYTES_PER_SAMPLE_FRAME;
        while gap > 0 {
            let to_write = gap.min(silence_chunk.len());
            let _ = output.write_all(&silence_chunk[..to_write]);
            gap -= to_write;
        }
    }

    let _ = audio_client.stop_stream();
    output.flush().map_err(|error| error.to_string())
}

fn mux_system_audio(ffmpeg_exe: &Path, audio_path: &Path, video_path: &Path) -> Result<(), String> {
    if fs::metadata(audio_path).map_or(true, |metadata| metadata.len() < 1_024) {
        return Err("No system audio samples were captured.".to_owned());
    }
    let stem = video_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("MacroNest");
    let muxed_path = video_path.with_file_name(format!(".{stem}.muxing.mp4"));
    let status = Command::new(ffmpeg_exe)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "f32le",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-i",
        ])
        .arg(audio_path)
        .arg("-i")
        .arg(video_path)
        .args([
            "-map",
            "1:v:0",
            "-map",
            "0:a:0",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            "-avoid_negative_ts",
            "make_zero",
            "-movflags",
            "+faststart",
        ])
        .arg(&muxed_path)
        .status()
        .map_err(|error| format!("Could not mux system audio: {error}"))?;
    if !status.success() || fs::metadata(&muxed_path).map_or(true, |metadata| metadata.len() == 0) {
        let _ = fs::remove_file(&muxed_path);
        return Err("FFmpeg could not add system audio.".to_owned());
    }
    fs::remove_file(video_path).map_err(|error| error.to_string())?;
    fs::rename(&muxed_path, video_path).map_err(|error| error.to_string())
}

pub fn copy_video_to_clipboard(video_path: &Path) -> Result<(), String> {
    let mut last_error = None;
    for _ in 0..3 {
        match crate::platform::copy_folder_to_clipboard(video_path) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error.to_string()),
        }
        thread::sleep(Duration::from_millis(40));
    }
    Err(last_error.unwrap_or_else(|| "Could not open the clipboard.".to_owned()))
}

enum CaptureSource {
    Desktop { fps: u32 },
    Region { region: RECT, fps: u32 },
    WgcWindow { hwnd: HWND, fps: u32 },
    GameCapture { hwnd: HWND, fps: u32 },
}

fn capture_source(config: &VideoRecorderConfig) -> Result<CaptureSource, String> {
    let fps = config.fps.clamp(1, 240);
    match config.mode {
        QuickVideoRecordMode::FullScreen => Ok(CaptureSource::Desktop { fps }),
        QuickVideoRecordMode::FocusedWindow => {
            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)).as_bool() } {
                return Err("No focused window found.".to_owned());
            }
            Ok(CaptureSource::WgcWindow { hwnd, fps })
        }
        QuickVideoRecordMode::SelectedWindow => {
            let hwnd = selector_hwnd(&config.target_window)
                .ok_or_else(|| "Select a window to record first.".to_owned())?;
            if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)).as_bool() } {
                return Err("The selected window is no longer available.".to_owned());
            }
            Ok(CaptureSource::WgcWindow { hwnd, fps })
        }
        QuickVideoRecordMode::GameCapture => {
            let hwnd = if let Some(hwnd) = selector_hwnd(&config.target_window) {
                if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)).as_bool() } {
                    return Err("The selected game window is no longer available.".to_owned());
                }
                hwnd
            } else {
                let hwnd = unsafe { GetForegroundWindow() };
                let mut fg_pid: u32 = 0;
                unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut fg_pid)) };
                if hwnd.0.is_null() || fg_pid == std::process::id() || !unsafe { IsWindow(Some(hwnd)).as_bool() } {
                    return Err("Select a game window from the dropdown first.".to_owned());
                }
                hwnd
            };
            Ok(CaptureSource::GameCapture { hwnd, fps })
        }
        QuickVideoRecordMode::Region => {
            let (x, y, width, height) = config
                .region
                .ok_or_else(|| "Select a screen region to record first.".to_owned())?;
            region_source(
                RECT {
                    left: x,
                    top: y,
                    right: x.saturating_add(width.max(2)),
                    bottom: y.saturating_add(height.max(2)),
                },
                fps,
            )
        }
    }
}

fn region_source(mut region: RECT, fps: u32) -> Result<CaptureSource, String> {
    let monitor = unsafe { MonitorFromRect(&region, MONITOR_DEFAULTTONEAREST) };
    if monitor.0.is_null() {
        return Err("Could not find the monitor for this region.".to_owned());
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return Err("Could not read monitor bounds.".to_owned());
    }
    region.left = region
        .left
        .clamp(info.rcMonitor.left, info.rcMonitor.right - 2);
    region.top = region
        .top
        .clamp(info.rcMonitor.top, info.rcMonitor.bottom - 2);
    let mut width = (region.right - region.left).max(2);
    let mut height = (region.bottom - region.top).max(2);
    if width % 2 != 0 {
        width -= 1;
    }
    if height % 2 != 0 {
        height -= 1;
    }
    region.right = region.left + width;
    region.bottom = region.top + height;
    Ok(CaptureSource::Region { region, fps })
}

fn selector_hwnd(selector: &str) -> Option<HWND> {
    crate::window_list::find_window_handle(Some(selector)).or_else(|| {
        let marker = selector.rfind("(0x")?;
        let hex = selector.get(marker + 3..selector.len().checked_sub(1)?)?;
        let raw = usize::from_str_radix(hex, 16).ok()?;
        Some(HWND(raw as *mut _))
    })
}

fn unique_output_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.mp4"));
    if !first.exists() {
        return first;
    }
    (2..=999)
        .map(|index| dir.join(format!("{stem}_{index}.mp4")))
        .find(|path| !path.exists())
        .unwrap_or_else(|| dir.join(format!("{stem}_{}.mp4", std::process::id())))
}

#[cfg(windows)]
#[cfg(windows)]
struct RegionBorder {
    stop: std::sync::Arc<AtomicBool>,
}

#[cfg(windows)]
impl RegionBorder {
    fn start(rect: RECT, language: crate::model::UiLanguage, badge_only: bool) -> (Self, std::sync::Arc<AtomicBool>) {
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let recording_active = std::sync::Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_active = recording_active.clone();
        thread::spawn(move || run_region_border(rect, thread_stop, thread_active, language, badge_only));
        (Self { stop }, recording_active)
    }
}

#[cfg(windows)]
impl Drop for RegionBorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(windows)]
fn run_region_border(
    rect: RECT,
    stop: std::sync::Arc<AtomicBool>,
    recording_active: std::sync::Arc<AtomicBool>,
    language: crate::model::UiLanguage,
    badge_only: bool,
) {
    use windows::{
        Win32::{
            Foundation::{COLORREF, HINSTANCE, LPARAM, LRESULT, WPARAM},
            Graphics::Gdi::{
                CombineRgn, CreateFontW, CreateRectRgn, CreateSolidBrush, DT_CENTER, DT_SINGLELINE,
                DT_VCENTER, DeleteObject, DrawTextW, FONT_CHARSET, FONT_CLIP_PRECISION,
                FONT_OUTPUT_PRECISION, FONT_QUALITY, FW_SEMIBOLD, FillRect, GetDC, HGDIOBJ,
                RGN_DIFF, RGN_OR, ReleaseDC, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
                PeekMessageW, RegisterClassW, SW_SHOWNOACTIVATE, SetWindowDisplayAffinity,
                ShowWindow, TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WNDCLASSW, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
        core::{PCWSTR, w},
    };

    let start_instant = Instant::now();
    let width = (rect.right - rect.left).max(10);
    let height = (rect.bottom - rect.top).max(10);

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == windows::Win32::UI::WindowsAndMessaging::WM_NCHITTEST {
            return LRESULT(windows::Win32::UI::WindowsAndMessaging::HTTRANSPARENT as isize);
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class_name = w!("MacroNestVideoRegionBorder");
        static CLASS_REGISTERED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*CLASS_REGISTERED.get_or_init(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: HINSTANCE(module.0),
                lpszClassName: class_name,
                hbrBackground: CreateSolidBrush(COLORREF(0x0000_CCFF)),
                ..Default::default()
            };
            RegisterClassW(&class) != 0
        }) {
            return;
        }

        let (win_x, win_y, win_w, win_h, prep_badge_w, badge_h) = if badge_only {
            (rect.left + 8, rect.top + 8, 145, 24, 145, 24)
        } else {
            (rect.left, rect.top, width, height, 145.min(width - 6), 24.min(height - 6))
        };

        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class_name,
            PCWSTR::null(),
            WS_POPUP,
            win_x,
            win_y,
            win_w,
            win_h,
            None,
            None,
            Some(HINSTANCE(module.0)),
            None,
        ) else {
            return;
        };

        if !badge_only {
            let outer_rgn = CreateRectRgn(0, 0, width, height);
            let inner_rgn = CreateRectRgn(3, 3, (width - 3).max(3), (height - 3).max(3));
            let badge_rgn = CreateRectRgn(3, 3, 3 + prep_badge_w, 3 + badge_h);

            let _ = CombineRgn(Some(outer_rgn), Some(outer_rgn), Some(inner_rgn), RGN_DIFF);
            let _ = CombineRgn(Some(outer_rgn), Some(outer_rgn), Some(badge_rgn), RGN_OR);
            let _ = DeleteObject(HGDIOBJ(inner_rgn.0));
            let _ = DeleteObject(HGDIOBJ(badge_rgn.0));

            let _ = SetWindowRgn(hwnd, Some(outer_rgn), true);
        }
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let font = CreateFontW(
            13,
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            FONT_CHARSET(0),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(0),
            0,
            w!("Segoe UI"),
        );

        let dark_brush = CreateSolidBrush(COLORREF(0x001A_1A1A));
        let red_brush = CreateSolidBrush(COLORREF(0x0033_33FF));
        let prep_text_color = COLORREF(0x0000_E6FF);

        let mut rec_start: Option<Instant> = None;
        let mut last_rendered_secs = u64::MAX;
        let mut last_prep_frame = usize::MAX;

        let mut message = MSG::default();

        let (b_left, b_top) = if badge_only { (0, 0) } else { (3, 3) };

        while !stop.load(Ordering::Acquire) {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            let is_active = recording_active.load(Ordering::Acquire);
            if is_active {
                let rec_instant = *rec_start.get_or_insert_with(Instant::now);
                let elapsed_secs = rec_instant.elapsed().as_secs();

                if elapsed_secs != last_rendered_secs {
                    last_rendered_secs = elapsed_secs;
                    let hdc = GetDC(Some(hwnd));
                    if !hdc.0.is_null() {
                        let full_badge_rect = RECT {
                            left: b_left,
                            top: b_top,
                            right: b_left + prep_badge_w,
                            bottom: b_top + badge_h,
                        };
                        FillRect(hdc, &full_badge_rect, dark_brush);

                        let dot_rect = RECT {
                            left: b_left + 6,
                            top: b_top + 7,
                            right: b_left + 14,
                            bottom: b_top + 15,
                        };
                        FillRect(hdc, &dot_rect, red_brush);

                        let mins = elapsed_secs / 60;
                        let secs = elapsed_secs % 60;
                        let mut time_str: Vec<u16> = format!("{mins:02}:{secs:02}")
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        let mut text_rect = RECT {
                            left: b_left + 17,
                            top: b_top,
                            right: b_left + prep_badge_w,
                            bottom: b_top + badge_h,
                        };
                        let old_font =
                            windows::Win32::Graphics::Gdi::SelectObject(hdc, HGDIOBJ(font.0));
                        SetBkMode(hdc, TRANSPARENT);
                        SetTextColor(hdc, COLORREF(0x00FF_FF_FF));
                        DrawTextW(
                            hdc,
                            &mut time_str,
                            &mut text_rect,
                            DT_SINGLELINE | DT_VCENTER | DT_CENTER,
                        );
                        windows::Win32::Graphics::Gdi::SelectObject(hdc, old_font);
                        let _ = ReleaseDC(Some(hwnd), hdc);
                    }
                }
            } else {
                let prep_frame = (start_instant.elapsed().as_millis() / 250) as usize % 3;
                if prep_frame != last_prep_frame {
                    last_prep_frame = prep_frame;
                    let hdc = GetDC(Some(hwnd));
                    if !hdc.0.is_null() {
                        let badge_rect = RECT {
                            left: b_left,
                            top: b_top,
                            right: b_left + prep_badge_w,
                            bottom: b_top + badge_h,
                        };
                        FillRect(hdc, &badge_rect, dark_brush);

                        let dots = match prep_frame {
                            0 => ".",
                            1 => "..",
                            _ => "...",
                        };
                        let msg = if language == crate::model::UiLanguage::Vietnamese {
                            format!("Đang chuẩn bị{dots}")
                        } else {
                            format!("Preparing{dots}")
                        };
                        let mut msg_utf16: Vec<u16> =
                            msg.encode_utf16().chain(std::iter::once(0)).collect();
                        let mut text_rect = RECT {
                            left: b_left,
                            top: b_top,
                            right: b_left + prep_badge_w,
                            bottom: b_top + badge_h,
                        };
                        let old_font =
                            windows::Win32::Graphics::Gdi::SelectObject(hdc, HGDIOBJ(font.0));
                        SetBkMode(hdc, TRANSPARENT);
                        SetTextColor(hdc, prep_text_color);
                        DrawTextW(
                            hdc,
                            &mut msg_utf16,
                            &mut text_rect,
                            DT_SINGLELINE | DT_VCENTER | DT_CENTER,
                        );
                        windows::Win32::Graphics::Gdi::SelectObject(hdc, old_font);
                        let _ = ReleaseDC(Some(hwnd), hdc);
                    }
                }
            }

            thread::sleep(Duration::from_millis(50));
        }

        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteObject(HGDIOBJ(dark_brush.0));
        let _ = DeleteObject(HGDIOBJ(red_brush.0));
        let _ = DestroyWindow(hwnd);
    }
}

#[cfg(windows)]
fn show_video_copy_toast_async(rect: Option<RECT>, language: crate::model::UiLanguage) {
    thread::spawn(move || run_video_copy_toast(rect, language));
}

#[cfg(windows)]
fn run_video_copy_toast(rect: Option<RECT>, language: crate::model::UiLanguage) {
    use windows::{
        Win32::{
            Foundation::{COLORREF, HINSTANCE, LPARAM, LRESULT, WPARAM},
            Graphics::Gdi::{
                CreateFontW, CreateSolidBrush, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject,
                DrawTextW, FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY,
                FW_BOLD, FillRect, GetDC, HGDIOBJ, ReleaseDC, SetBkMode, SetTextColor, TRANSPARENT,
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetSystemMetrics,
                LWA_ALPHA, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN,
                SW_SHOWNOACTIVATE, SetLayeredWindowAttributes, ShowWindow, TranslateMessage,
                WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
        core::{PCWSTR, w},
    };

    let toast_w = 260;
    let toast_h = 44;

    let (center_x, center_y) = if let Some(r) = rect {
        ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
    } else {
        let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        (sw / 2, sh / 2)
    };

    let toast_x = center_x - toast_w / 2;
    let toast_y = center_y - toast_h / 2;

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == windows::Win32::UI::WindowsAndMessaging::WM_NCHITTEST {
            return LRESULT(windows::Win32::UI::WindowsAndMessaging::HTTRANSPARENT as isize);
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class_name = w!("MacroNestVideoCopyToast");
        static CLASS_REGISTERED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*CLASS_REGISTERED.get_or_init(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: HINSTANCE(module.0),
                lpszClassName: class_name,
                hbrBackground: CreateSolidBrush(COLORREF(0x0022_2222)),
                ..Default::default()
            };
            RegisterClassW(&class) != 0
        }) {
            return;
        }

        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED,
            class_name,
            PCWSTR::null(),
            WS_POPUP,
            toast_x,
            toast_y,
            toast_w,
            toast_h,
            None,
            None,
            Some(HINSTANCE(module.0)),
            None,
        ) else {
            return;
        };

        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let font = CreateFontW(
            14,
            0,
            0,
            0,
            FW_BOLD.0 as i32,
            0,
            0,
            0,
            FONT_CHARSET(0),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(0),
            0,
            w!("Segoe UI"),
        );

        let bg_brush = CreateSolidBrush(COLORREF(0x002A_2620));
        let border_brush = CreateSolidBrush(COLORREF(0x0000_CCFF));

        let hdc = GetDC(Some(hwnd));
        if !hdc.0.is_null() {
            let full_rect = RECT {
                left: 0,
                top: 0,
                right: toast_w,
                bottom: toast_h,
            };
            FillRect(hdc, &full_rect, bg_brush);

            let top_b = RECT {
                left: 0,
                top: 0,
                right: toast_w,
                bottom: 2,
            };
            let bot_b = RECT {
                left: 0,
                top: toast_h - 2,
                right: toast_w,
                bottom: toast_h,
            };
            let left_b = RECT {
                left: 0,
                top: 0,
                right: 2,
                bottom: toast_h,
            };
            let right_b = RECT {
                left: toast_w - 2,
                top: 0,
                right: toast_w,
                bottom: toast_h,
            };
            FillRect(hdc, &top_b, border_brush);
            FillRect(hdc, &bot_b, border_brush);
            FillRect(hdc, &left_b, border_brush);
            FillRect(hdc, &right_b, border_brush);

            let msg = if language == crate::model::UiLanguage::Vietnamese {
                "✓ Đã sao chép video vào bộ nhớ tạm"
            } else {
                "✓ Video Copied to Clipboard"
            };
            let mut text_utf16: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
            let mut text_rect = RECT {
                left: 6,
                top: 2,
                right: toast_w - 6,
                bottom: toast_h - 2,
            };
            let old_font = windows::Win32::Graphics::Gdi::SelectObject(hdc, HGDIOBJ(font.0));
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00FF_FF_FF));
            DrawTextW(
                hdc,
                &mut text_utf16,
                &mut text_rect,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER,
            );
            windows::Win32::Graphics::Gdi::SelectObject(hdc, old_font);
            let _ = ReleaseDC(Some(hwnd), hdc);
        }

        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteObject(HGDIOBJ(bg_brush.0));
        let _ = DeleteObject(HGDIOBJ(border_brush.0));

        let mut message = MSG::default();

        let start_fade_in = Instant::now();
        while start_fade_in.elapsed() < Duration::from_millis(150) {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let progress = (start_fade_in.elapsed().as_secs_f32() / 0.15).clamp(0.0, 1.0);
            let alpha = (progress * 240.0) as u8;
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
            thread::sleep(Duration::from_millis(16));
        }
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 240, LWA_ALPHA);

        let start_hold = Instant::now();
        while start_hold.elapsed() < Duration::from_millis(1200) {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            thread::sleep(Duration::from_millis(30));
        }

        let start_fade_out = Instant::now();
        while start_fade_out.elapsed() < Duration::from_millis(200) {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let progress = (1.0 - start_fade_out.elapsed().as_secs_f32() / 0.20).clamp(0.0, 1.0);
            let alpha = (progress * 240.0) as u8;
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
            thread::sleep(Duration::from_millis(16));
        }

        let _ = DestroyWindow(hwnd);
    }
}
