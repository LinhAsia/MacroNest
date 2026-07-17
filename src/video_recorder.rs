use std::{
    fs::{self, File},
    io::Write,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
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
    pub hotkey: Option<HotkeyBinding>,
    pub mode: QuickVideoRecordMode,
    pub target_window: String,
    pub region: Option<(i32, i32, i32, i32)>,
    pub output_dir: PathBuf,
    pub ffmpeg_exe: PathBuf,
}

impl Default for VideoRecorderConfig {
    fn default() -> Self {
        Self {
            hotkey: None,
            mode: QuickVideoRecordMode::FullScreen,
            target_window: String::new(),
            region: None,
            output_dir: PathBuf::new(),
            ffmpeg_exe: PathBuf::new(),
        }
    }
}

struct RecordingProcess {
    child: Child,
    output_path: PathBuf,
    log_path: PathBuf,
    region_border: Option<RegionBorder>,
}

static CONFIG: Lazy<Mutex<VideoRecorderConfig>> =
    Lazy::new(|| Mutex::new(VideoRecorderConfig::default()));
static PROCESS: Lazy<Mutex<Option<RecordingProcess>>> = Lazy::new(|| Mutex::new(None));
static STATUS: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new("Ready".to_owned()));
static ACTIVE: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
static HOTKEY_DOWN: AtomicBool = AtomicBool::new(false);
static SESSION_ID: AtomicU64 = AtomicU64::new(0);

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

pub fn stop_blocking() {
    if ACTIVE.load(Ordering::Acquire) || PROCESS.lock().is_some() {
        stop_recording_inner();
    }
}

pub fn process_hotkey(binding: &HotkeyBinding, is_down: bool, is_repeat: bool) -> bool {
    let matches = CONFIG
        .lock()
        .hotkey
        .as_ref()
        .is_some_and(|trigger| hotkey::binding_matches(trigger, binding));
    if !matches {
        return false;
    }
    if is_down {
        if !is_repeat && !HOTKEY_DOWN.swap(true, Ordering::AcqRel) {
            toggle_async();
        }
    } else {
        HOTKEY_DOWN.store(false, Ordering::Release);
    }
    true
}

fn start_recording_inner() -> Result<(), String> {
    let config = CONFIG.lock().clone();
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
    let log_path = config.output_dir.join(".macronest-video-recorder.log");
    let log = File::create(&log_path)
        .map_err(|error| format!("Could not create the recorder log: {error}"))?;

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
            "-an",
            "-c:v",
            "h264_mf",
            "-rate_control",
            "quality",
            "-quality",
            "75",
            "-scenario",
            "archive",
            "-movflags",
            "+faststart",
        ])
        .arg(&output_path);

    let child = command
        .spawn()
        .map_err(|error| format!("Could not start FFmpeg: {error}"))?;
    let region_border = border_rect.and_then(RegionBorder::start);
    let session_id = SESSION_ID.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    *PROCESS.lock() = Some(RecordingProcess {
        child,
        output_path: output_path.clone(),
        log_path,
        region_border,
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
    recording.region_border.take();
    ACTIVE.store(false, Ordering::Release);
    *STATUS.lock() = format!("Saved: {}", recording.output_path.display());
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
                    let error = fs::read_to_string(&recording.log_path).unwrap_or_default();
                    let _ = fs::remove_file(&recording.log_path);
                    let _ = fs::remove_file(&recording.output_path);
                    *STATUS.lock() = if error.trim().is_empty() {
                        "Video recording stopped unexpectedly.".to_owned()
                    } else {
                        format!("Video recording failed: {}", error.trim())
                    };
                }
                ACTIVE.store(false, Ordering::Release);
                return;
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
}

fn capture_source(config: &VideoRecorderConfig) -> Result<(String, Option<RECT>), String> {
    match config.mode {
        QuickVideoRecordMode::FullScreen => Ok((
            "gfxcapture=monitor_idx=0:capture_cursor=1:display_border=1:max_framerate=60:width=-2:height=-2".to_owned(),
            None,
        )),
        QuickVideoRecordMode::FocusedWindow => window_source(unsafe { GetForegroundWindow() }),
        QuickVideoRecordMode::SelectedWindow => {
            let hwnd = selector_hwnd(&config.target_window)
                .ok_or_else(|| "Select a window to record first.".to_owned())?;
            window_source(hwnd)
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
            })
        }
    }
}

fn window_source(hwnd: HWND) -> Result<(String, Option<RECT>), String> {
    if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)).as_bool() } {
        return Err("The selected window is no longer available.".to_owned());
    }
    Ok((
        format!(
            "gfxcapture=hwnd={}:monitor_idx=window:capture_cursor=1:capture_border=1:display_border=1:max_framerate=60:width=-2:height=-2",
            hwnd.0 as usize
        ),
        None,
    ))
}

fn region_source(mut region: RECT) -> Result<(String, Option<RECT>), String> {
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
            "gfxcapture=hmonitor={}:capture_cursor=1:display_border=0:max_framerate=60:crop_left={crop_left}:crop_top={crop_top}:crop_right={crop_right}:crop_bottom={crop_bottom}:width=-2:height=-2",
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
