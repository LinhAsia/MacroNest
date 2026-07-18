use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{BufWriter, Write},
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
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect},
    UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow},
};

use crate::{
    hotkey,
    model::{HotkeyBinding, QuickVideoRecordMode},
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
        }
    }
}

struct RecordingProcess {
    child: Child,
    output_path: PathBuf,
    log_path: PathBuf,
    region_border: Option<RegionBorder>,
    audio_stop: Arc<AtomicBool>,
    audio_thread: Option<JoinHandle<Result<(), String>>>,
    audio_path: PathBuf,
    copy_after_recording: bool,
    ffmpeg_exe: PathBuf,
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
static HARDWARE_ENCODING: Lazy<Mutex<Option<(String, bool)>>> = Lazy::new(|| Mutex::new(None));

pub fn set_config(config: VideoRecorderConfig) {
    *CONFIG.lock() = config;
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
    let matches = config.enabled
        && config
            .hotkey
            .as_ref()
            .is_some_and(|trigger| {
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
        REGION_CAPTURE_ACTIVE.store(false, Ordering::Release);
        PRESS_HANDLED_ON_DOWN.store(false, Ordering::Release);
        let press_id = HOTKEY_PRESS_ID.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        if ACTIVE.load(Ordering::Acquire) || BUSY.load(Ordering::Acquire) {
            PRESS_HANDLED_ON_DOWN.store(true, Ordering::Release);
            toggle_async();
            return true;
        }
        let trigger = binding.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(105));
            if HOTKEY_DOWN.load(Ordering::Acquire)
                && HOTKEY_PRESS_ID.load(Ordering::Acquire) == press_id
                && !ACTIVE.load(Ordering::Acquire)
                && !BUSY.load(Ordering::Acquire)
            {
                REGION_CAPTURE_ACTIVE.store(true, Ordering::Release);
                if !crate::overlay::screen_draw_begin_video_region_capture(trigger) {
                    REGION_CAPTURE_ACTIVE.store(false, Ordering::Release);
                }
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

    let (source, border_rect) = capture_source(&config)?;
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let output_path = unique_output_path(&config.output_dir, &format!("MacroNest_{timestamp}"));
    let audio_path = config.output_dir.join(format!(
        ".macronest-video-audio-{}-{timestamp}.f32le",
        std::process::id()
    ));
    let log_path = config.output_dir.join(".macronest-video-recorder.log");
    let log = File::create(&log_path)
        .map_err(|error| format!("Could not create the recorder log: {error}"))?;

    let hardware_encoding = hardware_encoding_available(&config.ffmpeg_exe);
    let (audio_stop, audio_thread, audio_start) = start_system_audio_capture(&audio_path)?;
    let mut command = Command::new(&config.ffmpeg_exe);
    command
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &source,
            "-vf",
            "hwdownload,format=bgra,format=nv12",
            "-an",
            "-c:v",
            "h264_mf",
        ]);
    if hardware_encoding {
        command.args(["-hw_encoding", "1"]);
    }
    command
        .args([
            "-rate_control",
            "quality",
            "-quality",
            "90",
            "-scenario",
            "archive",
            "-movflags",
            "+faststart",
        ])
        .arg(&output_path);

    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            audio_stop.store(true, Ordering::Release);
            let _ = audio_start.send(());
            let _ = audio_thread.join();
            let _ = fs::remove_file(&audio_path);
            return Err(format!("Could not start FFmpeg: {error}"));
        }
    };
    let _ = audio_start.send(());
    let region_border = border_rect.and_then(RegionBorder::start);
    let session_id = SESSION_ID.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    *PROCESS.lock() = Some(RecordingProcess {
        child,
        output_path: output_path.clone(),
        log_path,
        region_border,
        audio_stop,
        audio_thread: Some(audio_thread),
        audio_path,
        copy_after_recording: config.copy_after_recording,
        ffmpeg_exe: config.ffmpeg_exe,
    });
    ACTIVE.store(true, Ordering::Release);
    *STATUS.lock() = format!("Recording: {}", output_path.display());
    spawn_exit_watchdog(session_id);
    Ok(())
}

fn stop_recording_inner() {
    let Some(mut recording) = PROCESS.lock().take() else {
        ACTIVE.store(false, Ordering::Release);
        return;
    };
    recording.region_border.take();
    ACTIVE.store(false, Ordering::Release);
    *STATUS.lock() = "Finishing video...".to_owned();
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
            Ok(()) => status.push_str(" - copied"),
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

fn hardware_encoding_available(ffmpeg_exe: &Path) -> bool {
    let signature = ffmpeg_signature(ffmpeg_exe);
    let mut cached = HARDWARE_ENCODING.lock();
    if let Some((cached_signature, available)) = cached.as_ref()
        && cached_signature == &signature
    {
        return *available;
    }
    let cache_path = ffmpeg_exe.with_file_name("ffmpeg-hardware-encoding.cache");
    if let Ok(value) = fs::read_to_string(&cache_path)
        && let Some((cached_signature, available)) = value.trim().rsplit_once('|')
        && cached_signature == signature
        && let Ok(available) = available.parse::<bool>()
    {
        *cached = Some((signature, available));
        return available;
    }
    let mut child = match Command::new(ffmpeg_exe)
        .creation_flags(CREATE_NO_WINDOW)
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
            "-vf",
            "format=nv12",
            "-c:v",
            "h264_mf",
            "-hw_encoding",
            "1",
            "-f",
            "null",
            "-",
        ])
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            cache_hardware_encoding(&cache_path, &signature, false);
            *cached = Some((signature, false));
            return false;
        }
    };
    let deadline = Instant::now() + Duration::from_millis(1500);
    let available = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            Err(_) => break false,
        }
    };
    cache_hardware_encoding(&cache_path, &signature, available);
    *cached = Some((signature, available));
    available
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

fn cache_hardware_encoding(cache_path: &Path, signature: &str, available: bool) {
    let _ = fs::write(cache_path, format!("{signature}|{available}"));
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
        audio_client
            .start_stream()
            .map_err(|error| error.to_string())?;
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
        let _ = audio_client.stop_stream();
        return Ok(());
    }

    let mut samples = VecDeque::new();
    while capture
        .get_next_packet_size()
        .map_err(|error| error.to_string())?
        .unwrap_or(0)
        > 0
    {
        capture
            .read_from_device_to_deque(&mut samples)
            .map_err(|error| error.to_string())?;
        samples.clear();
    }

    while !stop.load(Ordering::Acquire) {
        if capture
            .get_next_packet_size()
            .map_err(|error| error.to_string())?
            .unwrap_or(0)
            > 0
        {
            capture
                .read_from_device_to_deque(&mut samples)
                .map_err(|error| error.to_string())?;
            let (first, second) = samples.as_slices();
            output.write_all(first).map_err(|error| error.to_string())?;
            output
                .write_all(second)
                .map_err(|error| error.to_string())?;
            samples.clear();
        }
        let _ = event.wait_for_event(50);
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

fn copy_video_to_clipboard(video_path: &Path) -> Result<(), String> {
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

fn capture_source(config: &VideoRecorderConfig) -> Result<(String, Option<RECT>), String> {
    let fps = config.fps.clamp(1, 240);
    match config.mode {
        QuickVideoRecordMode::FullScreen => Ok((
            format!("gfxcapture=monitor_idx=0:capture_cursor=1:display_border=1:max_framerate={fps}:width=-2:height=-2"),
            None,
        )),
        QuickVideoRecordMode::FocusedWindow => window_source(unsafe { GetForegroundWindow() }, fps),
        QuickVideoRecordMode::SelectedWindow => {
            let hwnd = selector_hwnd(&config.target_window)
                .ok_or_else(|| "Select a window to record first.".to_owned())?;
            window_source(hwnd, fps)
        }
        QuickVideoRecordMode::Region => {
            let (x, y, width, height) = config
                .region
                .ok_or_else(|| "Select a screen region to record first.".to_owned())?;
            region_source(RECT {
                left: x,
                top: y,
                right: x.saturating_add(width.max(2)),
                bottom: y.saturating_add(height.max(2)),
            }, fps)
        }
    }
}

fn window_source(hwnd: HWND, fps: u32) -> Result<(String, Option<RECT>), String> {
    if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)).as_bool() } {
        return Err("The selected window is no longer available.".to_owned());
    }
    Ok((
        format!(
            "gfxcapture=hwnd={}:monitor_idx=window:capture_cursor=1:capture_border=1:display_border=1:max_framerate={fps}:width=-2:height=-2",
            hwnd.0 as usize
        ),
        None,
    ))
}

fn region_source(mut region: RECT, fps: u32) -> Result<(String, Option<RECT>), String> {
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
    region.right = region.right.clamp(region.left + 2, info.rcMonitor.right);
    region.bottom = region.bottom.clamp(region.top + 2, info.rcMonitor.bottom);
    if (region.right - region.left) % 2 != 0 {
        region.right -= 1;
    }
    if (region.bottom - region.top) % 2 != 0 {
        region.bottom -= 1;
    }
    let crop_left = region.left - info.rcMonitor.left;
    let crop_top = region.top - info.rcMonitor.top;
    let crop_right = info.rcMonitor.right - region.right;
    let crop_bottom = info.rcMonitor.bottom - region.bottom;
    Ok((
        format!(
            "gfxcapture=hmonitor={}:capture_cursor=1:display_border=0:max_framerate={fps}:crop_left={crop_left}:crop_top={crop_top}:crop_right={crop_right}:crop_bottom={crop_bottom}:width=-2:height=-2",
            monitor.0 as usize
        ),
        Some(region),
    ))
}

fn selector_hwnd(selector: &str) -> Option<HWND> {
    let marker = selector.rfind("(0x")?;
    let hex = selector.get(marker + 3..selector.len().checked_sub(1)?)?;
    let raw = usize::from_str_radix(hex, 16).ok()?;
    Some(HWND(raw as *mut _))
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
struct RegionBorder {
    stop: std::sync::Arc<AtomicBool>,
}

#[cfg(windows)]
impl RegionBorder {
    fn start(rect: RECT) -> Option<Self> {
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        thread::spawn(move || run_region_border(rect, thread_stop));
        Some(Self { stop })
    }
}

#[cfg(windows)]
impl Drop for RegionBorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(windows)]
fn run_region_border(rect: RECT, stop: std::sync::Arc<AtomicBool>) {
    use windows::{
        Win32::{
            Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
            Graphics::Gdi::{
                CombineRgn, CreateRectRgn, CreateSolidBrush, DeleteObject, HGDIOBJ, RGN_DIFF,
                SetWindowRgn,
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

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
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
                hbrBackground: CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x0000_CCFF)),
                ..Default::default()
            };
            RegisterClassW(&class) != 0
        }) {
            return;
        }
        let width = (rect.right - rect.left).max(2);
        let height = (rect.bottom - rect.top).max(2);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class_name,
            PCWSTR::null(),
            WS_POPUP,
            rect.left,
            rect.top,
            width,
            height,
            None,
            None,
            Some(HINSTANCE(module.0)),
            None,
        ) else {
            return;
        };
        let outer = CreateRectRgn(0, 0, width, height);
        let inner = CreateRectRgn(3, 3, (width - 3).max(3), (height - 3).max(3));
        let _ = CombineRgn(Some(outer), Some(outer), Some(inner), RGN_DIFF);
        let _ = DeleteObject(HGDIOBJ(inner.0));
        let _ = SetWindowRgn(hwnd, Some(outer), true);
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let mut message = MSG::default();
        while !stop.load(Ordering::Acquire) {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            thread::sleep(Duration::from_millis(16));
        }
        let _ = DestroyWindow(hwnd);
    }
}
