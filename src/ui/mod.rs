use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU32},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::Result;
use arboard::Clipboard;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use eframe::egui::{
    self, Button, Color32, ColorImage, FontFamily, Frame, Grid, Image, Margin, Order, RichText,
    Sense, Shadow, Stroke, StrokeKind, TextureHandle, TextureOptions, pos2, vec2,
};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use resvg::usvg;

use crate::{
    ai, audio, audiosense, hotkey,
    model::{
        AppPanel, AppState, AudioClipSettings, AudioSensePreset, AudioSettings, CaptureRequest,
        CapturedInput, CommandPreset, CrosshairStyle, EspPreset, FocusHighlightDecoration,
        GeometryPreset, GeometrySpec, GroqSettings, HotkeyBinding, HudPreset, MacroAction,
        MacroFolder, MacroGroup, MacroPreset, MacroStep, MacroTriggerMode, MascotStyle,
        MasterMacroGroupState, MasterMacroPresetState, MasterPreset, MasterWindowFocusPresetState,
        MasterWindowPresetState, MasterZoomPresetState, MousePathEvent, MousePathEventKind,
        MousePathPreset, MouseSensitivityPreset, OcrPreset, PinPreset, ProfileRecord,
        QuickKeyDisplayMode, QuickScreenDrawTool, QuickVideoRecordMode, RgbaColor, SoundPreset,
        TimerPreset, UiLanguage, UiThemeMode, VietnameseInputMode, VisionPreset, VisionSettings,
        WindowAnchor, WindowExpandDirection, WindowPreset,
    },
    overlay::{OverlayCommand, UiCommand},
    storage::AppPaths,
    window_list::{self, WindowInfo},
};
use vi::{self, TELEX, VNI};

mod app_shell;
mod audiosense_panel;
mod command_panel;
mod crosshair_panel;
mod esp_panel;
mod geometry_panel;
mod hud_panel;
mod layout;
mod macro_panel;
mod macro_panel_ocr;
mod memory_panel;
mod mouse_panel;
mod navigation;
mod network_panel;
mod ocr_panel;
mod settings_panel;
mod sound_panel;
mod state_sync;
mod theme;
mod vision_panel;
mod widgets;
mod window_panel;

pub(crate) use theme::MATERIAL_ICONS_FONT;
pub(crate) use theme::configure_theme;
pub use theme::{configure_fonts, text_has_cjk};

#[cfg(windows)]
pub(crate) use windows::Win32::{
    Foundation::POINT,
    Graphics::Dwm::DwmFlush,
    UI::{
        Input::KeyboardAndMouse::GetAsyncKeyState,
        WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
    },
};

#[derive(Default)]
pub(crate) struct AudioCardOutcome {
    changed: bool,
    choose_file: bool,
    open_editor: bool,
    status: Option<String>,
}

#[derive(Default)]
pub(crate) struct VietnameseInputSession {
    mode: VietnameseInputMode,
    prefix: String,
    raw_tail: String,
    last_output: String,
}

static VIETNAMESE_INPUT_SESSION: Lazy<Mutex<VietnameseInputSession>> =
    Lazy::new(|| Mutex::new(VietnameseInputSession::default()));

#[derive(Clone, Copy)]
pub(crate) struct VietnameseInputConfig {
    pub(crate) enabled: bool,
    pub(crate) mode: VietnameseInputMode,
}

pub(crate) static VIETNAMESE_INPUT_CONFIG: Lazy<Mutex<VietnameseInputConfig>> = Lazy::new(|| {
    Mutex::new(VietnameseInputConfig {
        enabled: false,
        mode: VietnameseInputMode::Telex,
    })
});
static LIVE_WINDOW_TARGET_COMBO_WINDOWS: Lazy<Mutex<Option<Vec<WindowInfo>>>> =
    Lazy::new(|| Mutex::new(None));
static PROCESS_ICON_TEXTURES: Lazy<Mutex<HashMap<String, Option<TextureHandle>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static PROCESS_PATHS: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Available(String, String, String), // version, body, download_url
    Downloading,
    ReadyToRestart(String), // new_exe_path
    Error(String),
    UpToDate,
}

#[derive(Debug, Clone)]
pub(crate) struct UpdateNotice {
    message: String,
    expires_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AudioEditorTarget {
    Startup,
    Exit,
    Library(u32),
    Preset(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum TitlebarQuickActionKind {
    #[default]
    Taskbar,
    WindowsKey,
    WindowPin,
    FocusHighlight,
    FocusMode,
    WindowOpacity,
    Protractor,
    Ruler,
    GetCoordinates,
    GetColor,
    KeyDisplay,
    ScreenDraw,
    VideoRecord,
    ClearOverlays,
    KeySound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoTrimHandle {
    Start,
    End,
    Playhead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisionCaptureMode {
    Template,
    SearchRegion,
    RegionAdjust,
    ColorSample,
    ColorPriorityAnchor,
    SinglePixel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrosshairColorTarget {
    Main,
    Outline,
    Ring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisionCaptureTarget {
    Preset(u32),
    CrosshairProfileColor {
        profile_index: usize,
        target: CrosshairColorTarget,
    },
    GeometryColor,
    OcrPreset(u32),
    /// Custom OCR region directly on a macro step (no separate OcrPreset needed)
    OcrStepRegion {
        group_id: u32,
        preset_id: u32,
        step_index: usize,
    },
    /// Color pick targeting a DrawGeometry macro step's geometry spec
    MacroStepGeometryColor {
        group_id: u32,
        preset_id: u32,
        step_index: usize,
        is_fill: bool,
        is_hold_stop: bool,
    },
    PinPresetColor(u32),
    PinPresetRegion(u32),
    PinPresetSourceCrop(u32),
    HudPresetRegion(u32),
    QuickActionsCoordinates,
    QuickActionsColor,
    QuickActionsKeyDisplayPosition,
    QuickActionsVideoRegion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MouseCaptureKind {
    #[default]
    MoveMouseAbsolute,
    GeometryPrimaryPos,
    GeometrySecondaryPos,
    IfStartMousePos,
    IfStartPixelColor,
    ExtraCondMousePos,
    ExtraCondPixelColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MouseMoveAbsoluteCaptureTarget {
    group_id: Option<u32>,
    preset_id: u32,
    step_index: usize,
    capture_kind: MouseCaptureKind,
    extra_cond_index: Option<usize>,
    is_hold_stop: bool,
}

#[derive(Clone)]
pub(crate) struct MacroStepDragPayload {
    group_id: u32,
    preset_id: u32,
    indices: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacroGroupFavoriteFilter {
    All,
    Star,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MacroShareCodeKind {
    #[default]
    None,
    Step,
    Preset,
    Group,
}

#[derive(Default)]
struct MacroShareCollectSeen {
    crosshair_profiles: HashSet<String>,
    window_presets: HashSet<u32>,
    window_layouts: HashSet<u32>,
    window_focus_presets: HashSet<u32>,
    pin_presets: HashSet<u32>,
    mouse_path_presets: HashSet<u32>,
    mouse_sensitivity_presets: HashSet<u32>,
    zoom_presets: HashSet<u32>,
    hud_presets: HashSet<u32>,
    command_presets: HashSet<u32>,
    geometry_presets: HashSet<u32>,
    vision_presets: HashSet<u32>,
    ocr_presets: HashSet<u32>,
    audio_sense_presets: HashSet<u32>,
    timer_presets: HashSet<u32>,
}

#[derive(Default)]
struct ImportedMacroShareMaps {
    crosshair_profiles: HashMap<String, String>,
    window_presets: HashMap<u32, u32>,
    window_layouts: HashMap<u32, u32>,
    window_focus_presets: HashMap<u32, u32>,
    pin_presets: HashMap<u32, u32>,
    mouse_path_presets: HashMap<u32, u32>,
    mouse_sensitivity_presets: HashMap<u32, u32>,
    zoom_presets: HashMap<u32, u32>,
    hud_presets: HashMap<u32, u32>,
    command_presets: HashMap<u32, u32>,
    geometry_presets: HashMap<u32, u32>,
    vision_presets: HashMap<u32, u32>,
    ocr_presets: HashMap<u32, u32>,
    audio_sense_presets: HashMap<u32, u32>,
    timer_presets: HashMap<u32, u32>,
}

#[derive(Clone)]
pub(crate) struct CommandAiDialog {
    preset_id: u32,
    prompt: String,
}

pub(crate) struct CommandAiJob {
    token: u64,
    preset_id: u32,
    receiver: crossbeam_channel::Receiver<CommandAiJobResult>,
}

#[derive(Debug)]
pub(crate) struct CommandAiJobResult {
    token: u64,
    preset_id: u32,
    outcome: Result<ai::CommandPresetPatch, String>,
}

#[derive(Clone, Default)]
pub(crate) struct MacroStepInlineFeedback {
    message: String,
    open_groq_settings: bool,
}

struct StartupSplashState {
    started_at: Option<f64>,
    duration_sec: f64,
}

#[derive(Clone)]
pub(crate) struct ZoomPreviewView {
    texture: TextureHandle,
    filtered_texture: Option<TextureHandle>,
    title: String,
    screen_x: i32,
    screen_y: i32,
    logical_width: i32,
    logical_height: i32,
}

pub(crate) struct ZoomPreviewCache {
    updated_at: Instant,
    source_window_key: Option<String>,
    source_window_extra_keys: Vec<String>,
    match_duplicate_window_titles: bool,
    view: ZoomPreviewView,
}

#[derive(Clone)]
pub(crate) struct VisionPreviewView {
    texture: TextureHandle,
    file_name: String,
    width: usize,
    height: usize,
}

pub(crate) struct VisionPreviewCache {
    updated_at: Instant,
    source_path: PathBuf,
    source_modified: Option<std::time::SystemTime>,
    view: VisionPreviewView,
}

const OPEN_WINDOWS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const AUDIO_SENSE_DEVICES_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const PERSIST_DEBOUNCE: Duration = Duration::from_millis(180);
const MAX_UI_COMMANDS_PER_FRAME: usize = 256;

#[derive(Clone)]
pub(crate) struct PersistSnapshot {
    profiles: Vec<ProfileRecord>,
    state: AppState,
}

fn spawn_persist_worker(paths: AppPaths, ui_tx: Sender<UiCommand>) -> Sender<PersistSnapshot> {
    let (tx, rx) = crossbeam_channel::unbounded::<PersistSnapshot>();
    std::thread::spawn(move || {
        while let Ok(mut snapshot) = rx.recv() {
            // ponytail: collapse rapid-fire UI edits into the newest snapshot; if saves ever need
            // stronger ordering guarantees, switch this worker to versioned acknowledgements.
            while let Ok(newer_snapshot) = rx.recv_timeout(PERSIST_DEBOUNCE) {
                snapshot = newer_snapshot;
            }
            let result = paths
                .save_profiles(&snapshot.profiles)
                .and_then(|_| paths.save_state(&snapshot.state));
            if let Err(error) = result {
                let _ = ui_tx.send(UiCommand::PersistFailed(format!(
                    "Failed to save app state: {error}"
                )));
            }
        }
    });
    tx
}

pub fn build_runtime_macro_groups(state: &AppState) -> Vec<MacroGroup> {
    let mut macro_groups = state.macro_groups.clone();
    for group in &mut macro_groups {
        if let Some(folder_id) = group.folder_id
            && let Some(folder) = state.macro_folders.iter().find(|f| f.id == folder_id)
            && !folder.enabled
        {
            group.enabled = false;
        }
    }
    CrosshairApp::sort_macro_groups(&mut macro_groups);
    macro_groups
}

#[derive(Clone, Copy)]
pub enum PopupBlobKind {
    AlreadyRunning,
}

pub struct PopupBlobApp {
    kind: PopupBlobKind,
    theme: UiThemeMode,
    started_at: Option<f64>,
    duration_sec: f64,
    center_next_frame: bool,
}

impl PopupBlobApp {
    pub fn new(kind: PopupBlobKind, theme: UiThemeMode) -> Self {
        Self {
            kind,
            theme,
            started_at: None,
            duration_sec: 1.55,
            center_next_frame: true,
        }
    }

    fn popup_palette(&self) -> (Color32, Color32, Color32, Color32, Color32) {
        match self.theme {
            UiThemeMode::Dark => (
                Color32::from_rgb(108, 244, 226),
                Color32::from_rgb(255, 120, 186),
                Color32::from_rgb(112, 170, 255),
                Color32::from_rgba_premultiplied(4, 8, 18, 230),
                Color32::from_rgba_premultiplied(12, 18, 30, 188),
            ),
            UiThemeMode::Light => (
                Color32::from_rgb(58, 196, 182),
                Color32::from_rgb(236, 102, 152),
                Color32::from_rgb(92, 144, 238),
                Color32::from_rgba_premultiplied(245, 250, 255, 228),
                Color32::from_rgba_premultiplied(220, 236, 246, 190),
            ),
        }
    }

    fn render_message_popup(&self, ctx: &egui::Context, progress: f32) {
        let rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("message-popup"),
        ));
        let center = rect.center();
        let time = ctx.input(|input| input.time) as f32;
        let ease_in = 1.0 - (1.0 - (progress / 0.32).clamp(0.0, 1.0)).powi(3);
        let shatter = ((progress - 0.48) / 0.52).clamp(0.0, 1.0);
        let shatter = 1.0 - (1.0 - shatter).powi(3);
        let scale = egui::lerp(0.18..=1.0, ease_in) * (1.0 - shatter * 0.28);
        let fade = 1.0 - shatter * 0.82;
        let (neon_cyan, neon_pink, neon_blue, dark_fill, mid_fill) = self.popup_palette();
        let (title, message) = match self.kind {
            PopupBlobKind::AlreadyRunning => ("MacroNest", "Already running"),
        };

        for layer in 0..3 {
            let layer_t = layer as f32 / 2.0;
            let radius_x = rect.width() * (0.22 + layer_t * 0.12) * scale;
            let radius_y = rect.height() * (0.24 + layer_t * 0.08) * scale;
            let mut points = Vec::with_capacity(96);
            for step in 0..96 {
                let angle = step as f32 / 96.0 * std::f32::consts::TAU;
                let wobble = 1.0
                    + 0.13 * (angle * 3.0 + time * (0.9 + layer_t * 0.3)).sin()
                    + 0.07 * (angle * 5.0 - time * (0.65 + layer_t * 0.22)).cos();
                let blast = 1.0 + shatter * (0.12 + layer_t * 0.08);
                points.push(egui::pos2(
                    center.x + angle.cos() * radius_x * wobble * blast,
                    center.y + angle.sin() * radius_y * wobble * blast,
                ));
            }
            let fill = if layer == 0 {
                Color32::from_rgba_premultiplied(
                    dark_fill.r(),
                    dark_fill.g(),
                    dark_fill.b(),
                    (230.0 * fade) as u8,
                )
            } else if layer == 1 {
                Color32::from_rgba_premultiplied(
                    mid_fill.r(),
                    mid_fill.g(),
                    mid_fill.b(),
                    (168.0 * fade) as u8,
                )
            } else {
                Color32::from_rgba_premultiplied(
                    neon_pink.r(),
                    neon_pink.g(),
                    neon_pink.b(),
                    (52.0 * fade) as u8,
                )
            };
            let stroke = if layer == 2 { neon_pink } else { neon_blue };
            painter.add(egui::Shape::convex_polygon(
                points,
                fill,
                egui::Stroke::new(
                    1.4 - layer as f32 * 0.2,
                    Color32::from_rgba_premultiplied(
                        stroke.r(),
                        stroke.g(),
                        stroke.b(),
                        (110.0 * fade) as u8,
                    ),
                ),
            ));
        }

        for shard_index in 0..18 {
            let frac = shard_index as f32 / 18.0;
            let angle = frac * std::f32::consts::TAU + time * 0.6;
            let distance = rect.width().min(rect.height()) * 0.28 * shatter;
            let pos = egui::pos2(
                center.x + angle.cos() * distance,
                center.y + angle.sin() * distance * 0.72,
            );
            let color = if shard_index % 2 == 0 {
                neon_cyan
            } else {
                neon_pink
            };
            painter.circle_filled(
                pos,
                (1.2 + (shard_index % 4) as f32 * 0.45) * (0.8 + shatter * 0.4),
                Color32::from_rgba_premultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    (140.0 * (1.0 - shatter * 0.35)) as u8,
                ),
            );
        }

        painter.text(
            egui::pos2(center.x, rect.top() + rect.height() * 0.38),
            egui::Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(26.0),
            Color32::from_rgba_premultiplied(244, 247, 255, (255.0 * fade) as u8),
        );
        painter.text(
            egui::pos2(center.x, rect.top() + rect.height() * 0.62),
            egui::Align2::CENTER_CENTER,
            message,
            egui::FontId::proportional(16.0),
            Color32::from_rgba_premultiplied(208, 220, 255, (220.0 * fade) as u8),
        );
    }
}

impl eframe::App for PopupBlobApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.center_next_frame {
            if let Some(center_cmd) = egui::ViewportCommand::center_on_screen(ctx) {
                ctx.send_viewport_cmd(center_cmd);
                self.center_next_frame = false;
            }
        }
        let now = ctx.input(|input| input.time);
        let started_at = self.started_at.get_or_insert(now);
        let progress = ((now - *started_at) / self.duration_sec).clamp(0.0, 1.0) as f32;
        if progress >= 1.0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(33));
        self.render_message_popup(ctx, progress);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacroActionSubmenuKind {
    Macro,
    Mouse,
    Memory,
    ImageSearch,
    Timer,
    If,
    Geometry,
    Esp,
    AudioSense,
    Funny,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacroGroupClipboardFeedback {
    Copy,
    Paste,
    Cut,
}

#[derive(Clone)]
enum PresetClipboard {
    Command(CommandPreset),
    Window(WindowPreset),
    Pin(PinPreset),
    MousePath(MousePathPreset),
    MouseSensitivity(MouseSensitivityPreset),
    Vision(VisionPreset),
    AudioSense(AudioSensePreset),
    Ocr(OcrPreset),
    Geometry(GeometryPreset),
    Esp(EspPreset),
    Sound(SoundPreset),
    Hud(HudPreset),
    Timer(TimerPreset),
    WindowLayout(crate::model::WindowLayout),
}

pub struct CrosshairApp {
    pub paths: AppPaths,
    pub state: AppState,
    overlay_tx: Sender<OverlayCommand>,
    ui_tx: Sender<UiCommand>,
    ui_rx: Receiver<UiCommand>,
    status: String,
    save_name: String,
    open_window_infos: Vec<WindowInfo>,
    open_windows_loaded_once: bool,
    open_windows_loading: bool,
    quit_requested: bool,
    capture_target: Option<CaptureRequest>,
    startup_clip_duration_ms: Option<u64>,
    exit_clip_duration_ms: Option<u64>,
    persist_tx: Sender<PersistSnapshot>,
    persist_dirty: bool,
    persist_requested_at: Option<Instant>,
    show_startup_audio_editor: bool,
    show_exit_audio_editor: bool,
    audio_waveforms: HashMap<String, Vec<f32>>,
    last_synced_profiles: Option<Vec<ProfileRecord>>,
    last_synced_audio_settings: Option<AudioSettings>,
    last_synced_groq_settings: Option<GroqSettings>,
    last_synced_vision_settings: Option<VisionSettings>,
    last_synced_macro_groups: Option<Vec<MacroGroup>>,
    last_synced_window_presets: Option<Vec<WindowPreset>>,
    last_synced_window_focus_presets: Option<Vec<crate::model::WindowFocusPreset>>,
    last_synced_pin_presets: Option<Vec<crate::model::PinPreset>>,
    last_synced_mouse_path_presets: Option<Vec<crate::model::MousePathPreset>>,
    last_synced_window_layouts: Option<Vec<crate::model::WindowLayout>>,
    last_synced_vision_presets: Option<Vec<crate::model::VisionPreset>>,
    last_synced_ocr_presets: Option<Vec<crate::model::OcrPreset>>,
    last_synced_hud_presets: Option<Vec<crate::model::HudPreset>>,
    last_synced_command_presets: Option<Vec<CommandPreset>>,
    last_synced_timer_presets: Option<Vec<TimerPreset>>,
    last_synced_audio_sense_presets: Option<Vec<crate::model::AudioSensePreset>>,
    last_synced_geometry_presets: Option<Vec<crate::model::GeometryPreset>>,
    last_synced_esp_presets: Option<Vec<EspPreset>>,
    last_synced_mouse_sensitivity_presets: Option<Vec<crate::model::MouseSensitivityPreset>>,
    last_synced_macro_delays: Option<(u32, u32)>,
    last_synced_focus_highlight_config: Option<(RgbaColor, FocusHighlightDecoration)>,
    last_synced_focus_mode_config: Option<(bool, bool, String, u8, bool)>,
    last_synced_window_opacity_config: Option<(bool, bool, String, u8)>,
    last_synced_quick_key_display_config: Option<(
        bool,
        i32,
        i32,
        f32,
        QuickKeyDisplayMode,
        MascotStyle,
        Vec<MascotStyle>,
        Vec<(MascotStyle, i32, i32)>,
    )>,
    last_synced_quick_screen_draw_config: Option<(
        bool,
        Option<HotkeyBinding>,
        RgbaColor,
        f32,
        bool,
        f32,
        bool,
        bool,
        QuickScreenDrawTool,
        bool,
    )>,
    last_synced_quick_key_sound_config: Option<(bool, u32, f32)>,
    last_synced_macro_master_hotkey: Option<Option<HotkeyBinding>>,
    last_synced_macros_master_enabled: Option<bool>,
    last_synced_windows_key_locked: Option<bool>,
    last_synced_native_focus_highlight_enabled: Option<bool>,
    last_synced_vietnamese_input_enabled: Option<bool>,
    screen_draw_color_picker_open: bool,
    screen_draw_color_pick_pending_at: Option<Instant>,
    last_synced_active_macro_folder_scope: Option<crate::overlay::MacroFolderScope>,
    last_synced_protractor_enabled: Option<bool>,
    last_synced_protractor_config: Option<(f32, f32, f32, i32, i32, f32, bool, UiLanguage)>,
    sound_preset_clip_duration_ms: HashMap<u32, Option<u64>>,
    show_sound_preset_audio_editor: HashSet<u32>,
    library_clip_duration_ms: HashMap<u32, Option<u64>>,
    show_library_audio_editor: HashSet<u32>,
    active_audio_editor: Option<AudioEditorTarget>,
    trim_timeline_zoom: f32,
    preview_cursor: Option<(AudioEditorTarget, u64)>,
    capture_ignored_keys: HashSet<u32>,
    capture_hotkey_combo_keys: Option<Vec<String>>,
    capture_hotkey_combo_vks: HashSet<u32>,
    capture_suppress_next_poll: bool,
    capture_wait_for_mouse_release: bool,
    capture_ignore_mouse_until_release: bool,
    capture_suppress_polls_remaining: u8,
    capture_mouse_guard_until: Option<Instant>,
    mouse_move_absolute_capture_target: Option<MouseMoveAbsoluteCaptureTarget>,
    mouse_move_absolute_capture_raise_window: bool,
    mouse_move_absolute_restore_inner_size: Option<egui::Vec2>,
    mouse_move_absolute_restore_outer_pos: Option<egui::Pos2>,
    mouse_path_draw_capture_preset_id: Option<u32>,
    mouse_path_draw_capture_restore_inner_size: Option<egui::Vec2>,
    mouse_path_draw_capture_restore_outer_pos: Option<egui::Pos2>,
    mouse_path_step_preview_preset_id: Option<u32>,
    mouse_path_timeline_initialized: HashSet<u32>,
    mouse_path_merge_selection: HashMap<u32, u32>,
    macro_step_copy_feedback_target: Option<(u32, u32, usize)>,
    macro_step_copy_feedback_until: Option<Instant>,
    macro_selected_steps_copy_feedback_target: Option<(u32, u32)>,
    macro_selected_steps_copy_feedback_until: Option<std::time::Instant>,
    macro_preset_copy_feedback_target: Option<u32>,
    macro_preset_copy_feedback_until: Option<std::time::Instant>,

    pub(crate) captured_freeze_frame: Option<crate::window_list::ScreenCaptureFrame>,
    pub(crate) captured_freeze_texture: Option<egui::TextureHandle>,
    pub(crate) captured_freeze_pos: egui::Pos2,
    vision_capture_active: bool,
    vision_capture_target: Option<VisionCaptureTarget>,
    vision_capture_mode: Option<VisionCaptureMode>,
    vision_capture_anchor: Option<egui::Pos2>,
    vision_capture_current: Option<egui::Pos2>,
    vision_capture_screen_region_preview: Option<(i32, i32, i32, i32)>,
    vision_restore_inner_size: Option<egui::Vec2>,
    vision_restore_outer_pos: Option<egui::Pos2>,
    selected_macro_steps: HashSet<(u32, u32, usize)>,
    selected_macro_groups: HashSet<u32>,
    macro_groups_favorite_filter: MacroGroupFavoriteFilter,
    macro_preset_search_query: String,
    macro_group_clipboard: Vec<u32>,
    macro_group_clipboard_is_cut: bool,
    macro_group_clipboard_feedback: Option<MacroGroupClipboardFeedback>,
    macro_group_clipboard_feedback_until: Option<Instant>,
    macro_preset_clipboard: Option<MacroPreset>,
    macro_step_clipboard: Vec<MacroStep>,
    pending_macro_group_scroll_target: Option<u32>,
    crosshair_profile_clipboard: Option<ProfileRecord>,
    preset_clipboard: Option<PresetClipboard>,
    crosshair_editor_dirty: bool,
    crosshair_preview_last_sync_at: Option<Instant>,
    crosshair_preview_dirty_index: Option<usize>,
    crosshair_preview_dirty_generation: u64,
    crosshair_preview_applied_generation: u64,
    crosshair_link_lengths: bool,
    confirm_delete_folder_id: Option<u32>,
    confirm_release_folder_id: Option<u32>,
    confirm_delete_macro_group_id: Option<u32>,
    pending_macro_infinite_loop_enable: Option<(u32, u32)>,
    enforce_square_window_frames: u8,
    last_window_refresh_at: Instant,
    last_audio_sense_devices_refresh_at: Instant,
    last_active_panel: AppPanel,
    macro_drag_select_anchor: Option<(u32, u32, usize)>,
    last_selected_macro_step: Option<(u32, u32, usize)>,
    active_macro_folder_view: Option<u32>,
    macro_folders_panel_open: bool,
    startup_splash: StartupSplashState,
    settings_popup_open: bool,
    focus_groq_api_key_pending: bool,
    advanced_settings_open: bool,
    downloaded_tools_open: bool,
    zoom_preview_cache: HashMap<u32, ZoomPreviewCache>,
    vision_preview_cache: HashMap<u32, VisionPreviewCache>,
    window_preview_requested: HashMap<u32, Instant>,
    window_preview_loading: HashSet<u32>,
    vietnamese_input_enabled_texture: Option<TextureHandle>,
    vietnamese_input_disabled_texture: Option<TextureHandle>,
    titlebar_app_icon_texture: Option<TextureHandle>,
    guides_author_logo_texture: Option<TextureHandle>,
    active_mouse_record_preset_id: Option<u32>,
    active_macro_record_preset_id: Option<u32>,
    active_hud_preview_preset_id: Option<u32>,
    active_timer_preview_preset_id: Option<u32>,
    quick_action_window_selector: String,
    quick_action_pinned_windows: HashSet<String>,
    command_ai_dialog: Option<CommandAiDialog>,
    command_ai_job: Option<CommandAiJob>,
    command_ai_next_token: u64,
    command_ai_feedback: Option<String>,
    command_ai_step_target: Option<(u32, u32, Option<usize>)>,
    last_applied_theme: Option<UiThemeMode>,
    native_shadow_applied: bool,
    native_transitions_disabled_applied: bool,
    startup_show_pending: bool,
    startup_hide_to_tray_pending: bool,
    startup_gate_release_pending: bool,
    startup_gate_frames_remaining: u8,
    startup_shell_frames_remaining: u8,
    startup_overlay_sync_pending: bool,
    startup_state_persist_pending: bool,
    startup_cjk_font_check_pending: bool,
    startup_state_needs_cjk_fallback: bool,
    background_panel_preload_index: usize,
    startup_gate: Option<std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>>,
    panel_warmup_target: Option<AppPanel>,
    panel_warmup_frames_remaining: u8,
    warmed_panels: Vec<AppPanel>,
    update_status: UpdateStatus,
    startup_update_check_pending: bool,
    update_check_was_automatic: bool,
    update_notice: Option<UpdateNotice>,
    update_download_progress: Arc<AtomicU32>,
    interception_status: String,
    opencv_download_job: Option<JoinHandle<Result<()>>>,
    opencv_download_progress: Arc<AtomicU32>,
    opencv_installed: bool,
    ffmpeg_download_job: Option<JoinHandle<Result<()>>>,
    ffmpeg_download_progress: Arc<AtomicU32>,
    ffmpeg_installed: bool,
    frida_download_job: Option<JoinHandle<Result<()>>>,
    frida_download_progress: Arc<AtomicU32>,
    frida_installed: bool,
    video_library_open: bool,
    video_library_selected: Option<PathBuf>,
    video_library_preview: Option<crate::video_recorder::VideoLibraryPreview>,
    video_library_preview_texture: Option<TextureHandle>,
    video_library_preview_rx: Option<
        Receiver<(
            PathBuf,
            Result<crate::video_recorder::VideoLibraryPreview, String>,
        )>,
    >,
    video_library_thumbnails: HashMap<PathBuf, TextureHandle>,
    video_library_thumbnail_tx: Sender<(
        PathBuf,
        Result<crate::video_recorder::VideoLibraryPreview, String>,
    )>,
    video_library_thumbnail_rx: Receiver<(
        PathBuf,
        Result<crate::video_recorder::VideoLibraryPreview, String>,
    )>,
    video_library_thumbnail_jobs: HashSet<PathBuf>,
    video_library_playback: Option<crate::video_recorder::VideoPlaybackSession>,
    video_library_preloaded_playback: Option<(
        PathBuf,
        f64,
        f64,
        crate::video_recorder::VideoPlaybackSession,
    )>,
    video_library_playback_path: Option<PathBuf>,
    video_library_playback_position_seconds: f64,
    video_library_pending_preview: Option<(PathBuf, f64)>,
    video_library_trim_start_seconds: f64,
    video_library_trim_end_seconds: f64,
    video_library_target_size_mb: u32,
    video_library_copy_feedback: Option<(PathBuf, Instant)>,
    video_library_delete_rx: Option<Receiver<(PathBuf, std::result::Result<(), String>)>>,
    ocr_download_job: Option<JoinHandle<Result<()>>>,
    ocr_download_progress: Arc<AtomicU32>,
    interception_download_job: Option<JoinHandle<Result<()>>>,
    interception_download_progress: Arc<AtomicU32>,
    interception_package_downloaded: bool,
    interception_driver_checked: bool,
    interception_driver_installed: bool,
    interception_driver_needs_restart: bool,
    interception_install_job: Option<JoinHandle<Result<()>>>,
    interception_uninstall_job: Option<JoinHandle<Result<()>>>,
    arduino_download_job: Option<JoinHandle<Result<()>>>,
    arduino_download_progress: Arc<std::sync::atomic::AtomicU32>,
    arduino_tools_downloaded: bool,
    arduino_flash_status: String,
    arduino_flash_running: bool,
    arduino_restore_emulation_after_flash: bool,
    arduino_flash_result: Arc<parking_lot::Mutex<Option<Result<(), String>>>>,
    arduino_flash_progress: Arc<parking_lot::Mutex<Option<String>>>,
    interception_installed: bool,
    copy_folder_feedback_until: Option<Instant>,
    macro_group_export_feedback_until: Option<Instant>,
    macro_group_export_feedback_target: Option<u32>,
    macro_preset_export_feedback_until: Option<Instant>,
    macro_preset_export_feedback_target: Option<u32>,
    macro_step_export_feedback_until: Option<Instant>,
    macro_step_export_feedback_target: Option<(u32, usize)>,
    macro_share_clipboard_kind: MacroShareCodeKind,
    macro_share_clipboard_checked_at: Option<Instant>,
    vision_manual_color: RgbaColor,
    vision_manual_color_hex: String,
    geometry_color_pick_target: Option<(u32, u32, bool)>,
    geometry_preview_target: Option<(u32, u32)>,
    geometry_preset_preview_target: Option<u32>,
    geometry_preview_sent: Option<GeometrySpec>,
    esp_calibration_feedback: HashMap<u32, String>,
    show_geometry_preset_preview_target: Option<(u32, u32, usize, bool)>,
    show_geometry_preset_preview_sent: Option<Option<u32>>,
    audio_sense_devices: Vec<String>,
    audio_sense_devices_loaded_once: bool,
    audio_sense_devices_loading: bool,
    pitch_monitor: audiosense::PitchMonitor,
    audio_sense_test_settings: crate::model::AudioSenseMonitorSettings,
    audio_sense_test_pitch_settings: crate::model::PitchAudioSenseSettings,
    audio_sense_test_active: bool,
    active_pitch_preview_preset_id: Option<u32>,
    /// Target for color picking a DrawGeometry macro step spec (group_id, preset_id, step_index, is_fill, is_hold_stop)
    macro_step_geometry_color_pick_target: Option<(u32, u32, usize, bool, bool)>,
    /// Which DrawGeometry macro step is currently being previewed on overlay (group_id, preset_id, step_index, is_hold_stop)
    draw_geometry_step_preview_target: Option<(u32, u32, usize, bool)>,
    draw_geometry_step_preview_sent: Option<crate::model::GeometrySpec>,
    macro_step_inline_feedback: HashMap<(u32, usize), MacroStepInlineFeedback>,
    memory_panel: memory_panel::MemoryPanelState,
    network_panel: network_panel::NetworkPanelState,

    macro_referenced_variables_cache: Option<Vec<String>>,
    variable_inspector_open: bool,
    titlebar_guides_open: bool,
    pub show_share_buttons: bool,
    arduino_available_ports: Vec<String>,
    arduino_ports_last_refresh: Option<Instant>,
    mouse_input_normal_open: bool,
    mouse_input_arduino_open: bool,
    mouse_input_interception_open: bool,
    window_layout_tab: usize,
    selected_layout_cell: Option<(u32, usize, usize)>,
    drag_start_layout_cell: Option<(u32, usize, usize)>,
    protractor_picking_active: bool,
    protractor_calibration_points: Option<Vec<(i32, i32)>>,
    distance_measurement_active: bool,
    native_capture_in_progress: bool,
}

impl CrosshairApp {
    pub(crate) fn apply_fixed_variable_to_overlay(name: &str, value: &str) {
        let trimmed = value.trim();
        if let Ok(parsed) = trimmed.parse::<f64>() {
            crate::overlay::RUNTIME_VARIABLES
                .lock()
                .insert(name.to_owned(), parsed);
            crate::overlay::TEXT_VARIABLES.lock().remove(name);
        } else {
            crate::overlay::RUNTIME_VARIABLES.lock().remove(name);
            crate::overlay::TEXT_VARIABLES
                .lock()
                .insert(name.to_owned(), value.to_owned());
        }
    }

    pub(crate) fn remove_fixed_variable_from_overlay(name: &str) {
        crate::overlay::RUNTIME_VARIABLES.lock().remove(name);
        crate::overlay::TEXT_VARIABLES.lock().remove(name);
    }

    pub fn new(
        paths: AppPaths,
        state: AppState,
        overlay_tx: Sender<OverlayCommand>,
        ui_tx: Sender<UiCommand>,
        ui_rx: Receiver<UiCommand>,
        startup_state_dirty: bool,
        startup_gate: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
        start_hidden_to_tray: bool,
    ) -> Self {
        let save_name = state.selected_profile.clone().unwrap_or_default();
        let initial_active_panel = state.active_panel;
        crate::overlay::set_memory_pointer_entries(&state.memory_pointer_list);
        crate::overlay::set_memory_code_entries(&state.memory_code_list);
        let memory_panel = memory_panel::MemoryPanelState::with_hotkeys(
            &state.memory_scan_hotkeys,
            &state.memory_pointer_list,
        );
        let network_panel = network_panel::NetworkPanelState::new(
            paths.root.join("network-proxy-recovery.json"),
            state.network_decrypt_https,
        );
        let persist_tx = spawn_persist_worker(paths.clone(), ui_tx.clone());
        let (video_library_thumbnail_tx, video_library_thumbnail_rx) =
            crossbeam_channel::unbounded();

        let opencv_installed = paths.opencv_dll.exists();
        let ffmpeg_installed = paths.ffmpeg_exe.exists();
        let frida_installed = paths.frida_helper_exe.exists();
        let interception_pending_marker = paths.bin_dir.join("interception.install.pending");
        if interception_pending_marker.exists() {
            if let Ok(metadata) = std::fs::metadata(&interception_pending_marker) {
                if let Ok(created) = metadata.created().or_else(|_| metadata.modified()) {
                    if let Ok(elapsed) = created.elapsed() {
                        let uptime = crate::platform::get_system_uptime();
                        if elapsed > uptime {
                            let _ = std::fs::remove_file(&interception_pending_marker);
                        }
                    }
                }
            }
        }
        let interception_driver_needs_restart = interception_pending_marker.exists();
        let interception_driver_installed = interception_driver_needs_restart;
        let mut app = Self {
            paths: paths.clone(),
            state,
            overlay_tx,
            ui_tx,
            ui_rx,
            status: String::new(),
            save_name,
            open_window_infos: Vec::new(),
            open_windows_loaded_once: false,
            open_windows_loading: false,
            quit_requested: false,
            capture_target: None,
            startup_clip_duration_ms: None,
            exit_clip_duration_ms: None,
            persist_tx,
            persist_dirty: false,
            persist_requested_at: None,
            show_startup_audio_editor: false,
            show_exit_audio_editor: false,
            audio_waveforms: HashMap::new(),
            last_synced_profiles: None,
            last_synced_audio_settings: None,
            last_synced_groq_settings: None,
            last_synced_vision_settings: None,
            last_synced_macro_groups: None,
            last_synced_window_presets: None,
            last_synced_window_focus_presets: None,
            last_synced_pin_presets: None,
            last_synced_mouse_path_presets: None,
            last_synced_window_layouts: None,
            last_synced_vision_presets: None,
            last_synced_ocr_presets: None,
            last_synced_hud_presets: None,
            last_synced_command_presets: None,
            last_synced_timer_presets: None,
            last_synced_audio_sense_presets: None,
            last_synced_geometry_presets: None,
            last_synced_esp_presets: None,
            last_synced_mouse_sensitivity_presets: None,
            last_synced_macro_delays: None,
            last_synced_focus_highlight_config: None,
            last_synced_focus_mode_config: None,
            last_synced_window_opacity_config: None,
            last_synced_quick_key_display_config: None,
            last_synced_quick_screen_draw_config: None,
            last_synced_quick_key_sound_config: None,
            last_synced_macro_master_hotkey: None,
            last_synced_macros_master_enabled: None,
            last_synced_windows_key_locked: None,
            last_synced_native_focus_highlight_enabled: None,
            last_synced_vietnamese_input_enabled: None,
            screen_draw_color_picker_open: false,
            screen_draw_color_pick_pending_at: None,
            last_synced_active_macro_folder_scope: None,
            last_synced_protractor_enabled: None,
            last_synced_protractor_config: None,
            sound_preset_clip_duration_ms: HashMap::new(),
            show_sound_preset_audio_editor: HashSet::new(),
            library_clip_duration_ms: HashMap::new(),
            show_library_audio_editor: HashSet::new(),
            active_audio_editor: None,
            trim_timeline_zoom: 1.0,
            preview_cursor: None,
            capture_ignored_keys: HashSet::new(),
            capture_hotkey_combo_keys: None,
            capture_hotkey_combo_vks: HashSet::new(),
            capture_suppress_next_poll: false,
            capture_wait_for_mouse_release: false,
            capture_ignore_mouse_until_release: false,
            capture_suppress_polls_remaining: 0,
            capture_mouse_guard_until: None,
            mouse_move_absolute_capture_target: None,
            mouse_move_absolute_capture_raise_window: false,
            mouse_move_absolute_restore_inner_size: None,
            mouse_move_absolute_restore_outer_pos: None,
            mouse_path_draw_capture_preset_id: None,
            mouse_path_draw_capture_restore_inner_size: None,
            mouse_path_draw_capture_restore_outer_pos: None,
            mouse_path_step_preview_preset_id: None,
            mouse_path_timeline_initialized: HashSet::new(),
            mouse_path_merge_selection: HashMap::new(),
            macro_step_copy_feedback_target: None,
            macro_step_copy_feedback_until: None,
            macro_selected_steps_copy_feedback_target: None,
            macro_selected_steps_copy_feedback_until: None,
            macro_preset_copy_feedback_target: None,
            macro_preset_copy_feedback_until: None,

            captured_freeze_frame: None,
            captured_freeze_texture: None,
            captured_freeze_pos: egui::Pos2::ZERO,
            vision_capture_active: false,
            vision_capture_target: None,
            vision_capture_mode: None,
            protractor_picking_active: false,
            protractor_calibration_points: None,
            distance_measurement_active: false,
            native_capture_in_progress: false,
            vision_capture_anchor: None,
            vision_capture_current: None,
            vision_capture_screen_region_preview: None,
            vision_restore_inner_size: None,
            vision_restore_outer_pos: None,
            selected_macro_steps: HashSet::new(),
            selected_macro_groups: HashSet::new(),
            macro_groups_favorite_filter: MacroGroupFavoriteFilter::All,
            macro_preset_search_query: String::new(),
            macro_group_clipboard: Vec::new(),
            macro_group_clipboard_is_cut: false,
            macro_group_clipboard_feedback: None,
            macro_group_clipboard_feedback_until: None,
            macro_preset_clipboard: None,
            macro_step_clipboard: Vec::new(),
            pending_macro_group_scroll_target: None,
            crosshair_profile_clipboard: None,
            preset_clipboard: None,
            crosshair_editor_dirty: false,
            crosshair_preview_last_sync_at: None,
            crosshair_preview_dirty_index: None,
            crosshair_preview_dirty_generation: 0,
            crosshair_preview_applied_generation: 0,
            crosshair_link_lengths: false,
            confirm_delete_folder_id: None,
            confirm_release_folder_id: None,
            confirm_delete_macro_group_id: None,
            pending_macro_infinite_loop_enable: None,
            enforce_square_window_frames: 0,
            last_window_refresh_at: Instant::now(),
            last_audio_sense_devices_refresh_at: Instant::now(),
            last_active_panel: initial_active_panel,
            macro_drag_select_anchor: None,
            last_selected_macro_step: None,
            active_macro_folder_view: None,
            macro_folders_panel_open: false,
            startup_splash: StartupSplashState {
                started_at: None,
                duration_sec: 0.0,
            },
            settings_popup_open: false,
            focus_groq_api_key_pending: false,
            advanced_settings_open: false,
            downloaded_tools_open: false,
            zoom_preview_cache: HashMap::new(),
            vision_preview_cache: HashMap::new(),
            window_preview_requested: HashMap::new(),
            window_preview_loading: HashSet::new(),
            vietnamese_input_enabled_texture: None,
            vietnamese_input_disabled_texture: None,
            titlebar_app_icon_texture: None,
            guides_author_logo_texture: None,
            active_mouse_record_preset_id: None,
            active_macro_record_preset_id: None,
            active_hud_preview_preset_id: None,
            active_timer_preview_preset_id: None,
            quick_action_window_selector: String::new(),
            quick_action_pinned_windows: HashSet::new(),
            command_ai_dialog: None,
            command_ai_job: None,
            command_ai_next_token: 1,
            command_ai_feedback: None,
            command_ai_step_target: None,
            last_applied_theme: None,
            native_shadow_applied: false,
            native_transitions_disabled_applied: false,
            startup_show_pending: true,
            startup_hide_to_tray_pending: start_hidden_to_tray,
            startup_gate_release_pending: false,
            startup_gate_frames_remaining: 1,
            startup_shell_frames_remaining: 0,
            startup_overlay_sync_pending: true,
            startup_state_persist_pending: false,
            startup_cjk_font_check_pending: true,
            startup_state_needs_cjk_fallback: false,
            background_panel_preload_index: 0,
            startup_gate: Some(startup_gate),
            update_status: UpdateStatus::Idle,
            startup_update_check_pending: true,
            update_check_was_automatic: false,
            update_notice: None,
            update_download_progress: Arc::new(AtomicU32::new(0)),
            interception_status: "Interception: Unavailable".to_owned(),
            opencv_download_job: None,
            opencv_download_progress: Arc::new(AtomicU32::new(0)),
            opencv_installed,
            ffmpeg_download_job: None,
            ffmpeg_download_progress: Arc::new(AtomicU32::new(0)),
            ffmpeg_installed,
            frida_download_job: None,
            frida_download_progress: Arc::new(AtomicU32::new(0)),
            frida_installed,
            video_library_open: false,
            video_library_selected: None,
            video_library_preview: None,
            video_library_preview_texture: None,
            video_library_preview_rx: None,
            video_library_thumbnails: HashMap::new(),
            video_library_thumbnail_tx,
            video_library_thumbnail_rx,
            video_library_thumbnail_jobs: HashSet::new(),
            video_library_playback: None,
            video_library_preloaded_playback: None,
            video_library_playback_path: None,
            video_library_playback_position_seconds: 0.0,
            video_library_pending_preview: None,
            video_library_trim_start_seconds: 0.0,
            video_library_trim_end_seconds: 0.0,
            video_library_target_size_mb: 12,
            video_library_copy_feedback: None,
            video_library_delete_rx: None,
            ocr_download_job: None,
            ocr_download_progress: Arc::new(AtomicU32::new(0)),
            interception_download_job: None,
            interception_download_progress: Arc::new(AtomicU32::new(0)),
            interception_package_downloaded: paths.interception_zip.exists()
                || paths.interception_package_dir.exists()
                || paths.interception_installer_exe.exists(),
            interception_driver_checked: interception_driver_needs_restart,
            interception_driver_installed,
            interception_driver_needs_restart,
            interception_install_job: None,
            interception_uninstall_job: None,
            arduino_download_job: None,
            arduino_download_progress: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            arduino_tools_downloaded: paths.avrdude_exe.exists()
                && paths.avrdude_conf.exists()
                && paths.arduino_firmware_hex.exists(),
            arduino_flash_status: String::new(),
            arduino_flash_running: false,
            arduino_restore_emulation_after_flash: false,
            arduino_flash_result: Arc::new(parking_lot::Mutex::new(None)),
            arduino_flash_progress: Arc::new(parking_lot::Mutex::new(None)),
            interception_installed: false, // will update below
            copy_folder_feedback_until: None,
            macro_group_export_feedback_until: None,
            macro_group_export_feedback_target: None,
            macro_preset_export_feedback_until: None,
            macro_preset_export_feedback_target: None,
            macro_step_export_feedback_until: None,
            macro_step_export_feedback_target: None,
            macro_share_clipboard_kind: MacroShareCodeKind::None,
            macro_share_clipboard_checked_at: None,
            vision_manual_color: RgbaColor {
                r: 0,
                g: 255,
                b: 170,
                a: 255,
            },
            vision_manual_color_hex: "00FFAA".to_owned(),
            geometry_color_pick_target: None,
            geometry_preview_target: None,
            geometry_preset_preview_target: None,
            geometry_preview_sent: None,
            esp_calibration_feedback: HashMap::new(),
            show_geometry_preset_preview_target: None,
            show_geometry_preset_preview_sent: None,
            audio_sense_devices: Vec::new(),
            audio_sense_devices_loaded_once: false,
            audio_sense_devices_loading: false,
            pitch_monitor: audiosense::PitchMonitor::new(),
            audio_sense_test_settings: crate::model::AudioSenseMonitorSettings::default(),
            audio_sense_test_pitch_settings: crate::model::PitchAudioSenseSettings::default(),
            audio_sense_test_active: false,
            active_pitch_preview_preset_id: None,
            macro_step_geometry_color_pick_target: None,
            draw_geometry_step_preview_target: None,
            draw_geometry_step_preview_sent: None,
            macro_step_inline_feedback: HashMap::new(),
            memory_panel,
            network_panel,
            macro_referenced_variables_cache: None,

            variable_inspector_open: false,
            titlebar_guides_open: false,
            show_share_buttons: false,
            arduino_available_ports: Vec::new(),
            arduino_ports_last_refresh: None,
            mouse_input_normal_open: false,
            mouse_input_arduino_open: false,
            mouse_input_interception_open: false,
            window_layout_tab: 0,
            selected_layout_cell: None,
            drag_start_layout_cell: None,
            panel_warmup_target: Some(initial_active_panel),
            panel_warmup_frames_remaining: 1,
            warmed_panels: Vec::new(),
        };
        app.state.ocr_language = crate::ocr::OCR_DEFAULT_CODE.to_owned();
        app.interception_installed = app.paths.interception_dll.exists();
        let mut pending_startup_persist = startup_state_dirty;
        if app.apply_startup_state_adjustments() {
            pending_startup_persist = true;
        }
        app.startup_state_persist_pending = pending_startup_persist;
        crate::overlay::RUNTIME_VARIABLES.lock().clear();
        crate::overlay::TEXT_VARIABLES.lock().clear();
        for (name, val) in &app.state.global_constants {
            Self::apply_fixed_variable_to_overlay(name, val);
        }
        app
    }

    fn add_profile(&mut self) {
        let name = self.unique_profile_name("Profile");
        let default_style = CrosshairStyle::default();
        self.state.active_style = default_style.clone();
        self.state.profiles.push(ProfileRecord {
            name: name.clone(),
            enabled: self.state.active_style.enabled,
            collapsed: true,
            style: self.state.active_style.clone(),
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
        });
        self.state.selected_profile = Some(name.clone());
        self.save_name = name.clone();
        self.sync_profiles();
        self.persist();
        self.status = format!("Added profile: {name}");
    }

    fn unique_profile_name(&self, base: &str) -> String {
        let mut counter = self.state.profiles.len().max(1) + 1;
        loop {
            let candidate = format!("{base} {counter}");
            if self
                .state
                .profiles
                .iter()
                .all(|profile| profile.name != candidate)
            {
                return candidate;
            }
            counter += 1;
        }
    }

    fn clone_crosshair_profile_with_new_name(&self, source: &ProfileRecord) -> ProfileRecord {
        let mut copied = source.clone();
        copied.name = self.unique_profile_name(&format!("{} Copy", source.name));
        copied.collapsed = true;
        copied
    }

    fn copy_crosshair_profile(&mut self, profile: &ProfileRecord) {
        self.crosshair_profile_clipboard = Some(profile.clone());
        self.status = format!("Copied crosshair preset: {}.", profile.name);
    }

    fn mark_crosshair_profile_dirty(&mut self, index: usize) {
        self.crosshair_preview_dirty_index = Some(index);
        self.crosshair_preview_dirty_generation =
            self.crosshair_preview_dirty_generation.wrapping_add(1);
        self.crosshair_editor_dirty = true;
    }

    fn flush_crosshair_profile_dirty(&mut self, force: bool) {
        const CROSSHAIR_PREVIEW_SYNC_INTERVAL: Duration = Duration::from_millis(16);
        let Some(index) = self.crosshair_preview_dirty_index else {
            return;
        };
        if !force {
            if self.crosshair_preview_applied_generation == self.crosshair_preview_dirty_generation
            {
                return;
            }
            if let Some(last_sync_at) = self.crosshair_preview_last_sync_at {
                if last_sync_at.elapsed() < CROSSHAIR_PREVIEW_SYNC_INTERVAL {
                    return;
                }
            }
        }
        if let Some(profile) = self.state.profiles.get(index).cloned() {
            self.sync_crosshair_profile(index, &profile);
        }
        self.crosshair_preview_last_sync_at = Some(Instant::now());
        self.crosshair_preview_applied_generation = self.crosshair_preview_dirty_generation;
        if !force {
            return;
        }
        self.crosshair_preview_dirty_index = None;
        if self.crosshair_editor_dirty {
            self.persist();
            self.crosshair_editor_dirty = false;
        }
    }

    fn paste_crosshair_profile_after(&mut self, index: usize) {
        let Some(source) = self.crosshair_profile_clipboard.clone() else {
            self.status = "No crosshair preset in clipboard.".to_owned();
            return;
        };
        let copied = self.clone_crosshair_profile_with_new_name(&source);
        let insert_at = (index + 1).min(self.state.profiles.len());
        let name = copied.name.clone();
        self.state.profiles.insert(insert_at, copied.clone());
        self.state.selected_profile = Some(name.clone());
        self.save_name = name.clone();
        self.state.active_style = copied.style.clone();
        self.sync_crosshair();
        self.persist();
        self.status = format!("Pasted crosshair preset: {}.", name);
    }

    fn apply_drawn_crosshair_asset(
        &mut self,
        profile_name: &str,
        asset_name: Option<String>,
        asset_scale: Option<f32>,
    ) {
        let Some(index) = self
            .state
            .profiles
            .iter()
            .position(|profile| profile.name == profile_name)
        else {
            return;
        };

        if let Some(asset_name) = asset_name {
            self.state.profiles[index].style.custom_asset = Some(asset_name);
            if let Some(asset_scale) = asset_scale {
                self.state.profiles[index].style.custom_scale = asset_scale.clamp(16.0, 4096.0);
            }
        }

        if self.state.selected_profile.as_deref() == Some(profile_name) {
            self.state.active_style = self.state.profiles[index].style.clone();
            self.state.active_style.enabled = self.state.profiles[index].enabled;
        }

        self.mark_crosshair_profile_dirty(index);
        self.flush_crosshair_profile_dirty(true);
    }

    fn find_named_item_by_spec<'a, T>(
        items: &'a [T],
        spec: &str,
        id_of: impl Fn(&T) -> u32,
        name_of: impl Fn(&T) -> &str,
    ) -> Option<&'a T> {
        let trimmed = spec.trim();
        if let Ok(id) = trimmed.parse::<u32>() {
            return items.iter().find(|item| id_of(item) == id);
        }
        items
            .iter()
            .find(|item| Self::trimmed_eq_ignore_ascii_case(name_of(item), trimmed))
    }

    fn collect_macro_share_resources_for_step(
        &self,
        step: &MacroStep,
    ) -> crate::macro_code::MacroShareResources {
        let mut resources = crate::macro_code::MacroShareResources::default();
        let mut seen = MacroShareCollectSeen::default();
        self.collect_macro_share_resources_from_step(step, &mut resources, &mut seen);
        resources
    }

    fn collect_macro_share_resources_for_preset(
        &self,
        preset: &MacroPreset,
    ) -> crate::macro_code::MacroShareResources {
        let mut resources = crate::macro_code::MacroShareResources::default();
        let mut seen = MacroShareCollectSeen::default();
        self.collect_macro_share_resources_from_step(
            &preset.hold_stop_step,
            &mut resources,
            &mut seen,
        );
        self.collect_macro_share_resources_from_step(
            &preset.press_stop_step,
            &mut resources,
            &mut seen,
        );
        for step in &preset.steps {
            self.collect_macro_share_resources_from_step(step, &mut resources, &mut seen);
        }
        resources
    }

    fn collect_macro_share_resources_for_group(
        &self,
        group: &MacroGroup,
    ) -> crate::macro_code::MacroShareResources {
        let mut resources = crate::macro_code::MacroShareResources::default();
        let mut seen = MacroShareCollectSeen::default();
        for preset in &group.presets {
            self.collect_macro_share_resources_from_step(
                &preset.hold_stop_step,
                &mut resources,
                &mut seen,
            );
            self.collect_macro_share_resources_from_step(
                &preset.press_stop_step,
                &mut resources,
                &mut seen,
            );
            for step in &preset.steps {
                self.collect_macro_share_resources_from_step(step, &mut resources, &mut seen);
            }
        }
        resources
    }

    fn collect_macro_share_resources_from_step(
        &self,
        step: &MacroStep,
        resources: &mut crate::macro_code::MacroShareResources,
        seen: &mut MacroShareCollectSeen,
    ) {
        let trimmed_key = step.key.trim();
        match step.action {
            MacroAction::ApplyWindowPreset => {
                if let Some(layout_spec) = trimmed_key.strip_prefix("layout:") {
                    if let Some(layout) = Self::find_named_item_by_spec(
                        &self.state.window_layouts,
                        layout_spec,
                        |item| item.id,
                        |item| &item.name,
                    ) && seen.window_layouts.insert(layout.id)
                    {
                        resources.window_layouts.push(layout.clone());
                    }
                } else if let Some(preset) = Self::find_named_item_by_spec(
                    &self.state.window_presets,
                    trimmed_key,
                    |item| item.id,
                    |item| &item.name,
                ) && seen.window_presets.insert(preset.id)
                {
                    resources.window_presets.push(preset.clone());
                }
            }
            MacroAction::FocusWindowPreset => {
                if let Some(preset) = Self::find_named_item_by_spec(
                    &self.state.window_focus_presets,
                    trimmed_key,
                    |item| item.id,
                    |item| &item.name,
                ) && seen.window_focus_presets.insert(preset.id)
                {
                    resources.window_focus_presets.push(preset.clone());
                }
            }
            MacroAction::TriggerCommandPreset => {
                if let Some(preset_id) =
                    Self::command_preset_id_from_key(&self.state.command_presets, trimmed_key)
                    && let Some(preset) = self
                        .state
                        .command_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.command_presets.insert(preset.id)
                {
                    resources.command_presets.push(preset.clone());
                }
            }
            MacroAction::EnableCrosshairProfile | MacroAction::DisableCrosshair => {
                if !trimmed_key.is_empty()
                    && let Some(profile) = self.state.profiles.iter().find(|profile| {
                        Self::trimmed_eq_ignore_ascii_case(&profile.name, trimmed_key)
                    })
                    && seen
                        .crosshair_profiles
                        .insert(profile.name.trim().to_ascii_lowercase())
                {
                    resources.crosshair_profiles.push(profile.clone());
                }
            }
            MacroAction::EnablePinPreset | MacroAction::DisablePin => {
                if let Ok(preset_id) = trimmed_key.parse::<u32>()
                    && let Some(preset) = self.pin_preset(preset_id)
                    && seen.pin_presets.insert(preset.id)
                {
                    resources.pin_presets.push(preset.clone());
                }
            }
            MacroAction::PlayMousePathPreset => {
                if let Ok(preset_id) = trimmed_key.parse::<u32>()
                    && let Some(preset) = self
                        .state
                        .mouse_path_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.mouse_path_presets.insert(preset.id)
                {
                    resources.mouse_path_presets.push(preset.clone());
                }
            }
            MacroAction::ApplyMouseSensitivityPreset => {
                if let Some(preset_id) = trimmed_key.parse::<u32>().ok()
                    && let Some(preset) = self
                        .state
                        .mouse_sensitivity_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.mouse_sensitivity_presets.insert(preset.id)
                {
                    resources.mouse_sensitivity_presets.push(preset.clone());
                }
            }
            MacroAction::EnableZoomPreset => {
                if let Ok(preset_id) = trimmed_key.parse::<u32>()
                    && let Some(preset) = self
                        .state
                        .zoom_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.zoom_presets.insert(preset.id)
                {
                    resources.zoom_presets.push(preset.clone());
                }
            }
            MacroAction::StartVisionSearch
            | MacroAction::ScanVisionOnce
            | MacroAction::StopVision => {
                if let Some(preset) = Self::find_named_item_by_spec(
                    &self.state.vision_presets,
                    trimmed_key,
                    |item| item.id,
                    |item| &item.name,
                ) && seen.vision_presets.insert(preset.id)
                {
                    resources
                        .vision_presets
                        .push(crate::macro_code::SharedVisionPreset {
                            preset: preset.clone(),
                            template_png: fs::read(self.vision_template_file_for_preset(preset.id))
                                .ok(),
                        });
                }
            }
            MacroAction::StartAudioSensePreset => {
                if let Some(preset_id) = step.audio_sense_preset_id
                    && let Some(preset) = self
                        .state
                        .audio_sense_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.audio_sense_presets.insert(preset.id)
                {
                    resources.audio_sense_presets.push(preset.clone());
                }
            }
            MacroAction::ShowHud => {
                if let Ok(preset_id) = trimmed_key.parse::<u32>()
                    && let Some(preset) = self
                        .state
                        .hud_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.hud_presets.insert(preset.id)
                {
                    resources.hud_presets.push(preset.clone());
                }
            }
            MacroAction::OcrSearch => {
                if let Ok(preset_id) = trimmed_key.parse::<u32>()
                    && preset_id != 0
                    && let Some(preset) = self
                        .state
                        .ocr_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.ocr_presets.insert(preset.id)
                {
                    resources.ocr_presets.push(preset.clone());
                }
            }
            MacroAction::ShowGeometryPreset | MacroAction::HideGeometryPreset => {
                if let Ok(preset_id) = trimmed_key.parse::<u32>()
                    && let Some(preset) = self
                        .state
                        .geometry_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.geometry_presets.insert(preset.id)
                {
                    resources.geometry_presets.push(preset.clone());
                }
            }
            MacroAction::DrawGeometry => {
                if let Some(preset_id) = step.geometry_preset_id
                    && let Some(preset) = self
                        .state
                        .geometry_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.geometry_presets.insert(preset.id)
                {
                    resources.geometry_presets.push(preset.clone());
                }
            }
            MacroAction::StartTimerPreset
            | MacroAction::PauseTimerPreset
            | MacroAction::StopTimerPreset
            | MacroAction::ReadTimerPreset => {
                if let Some(preset_id) = step.timer_preset_id
                    && let Some(preset) = self
                        .state
                        .timer_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.timer_presets.insert(preset.id)
                {
                    resources.timer_presets.push(preset.clone());
                }
            }
            _ => {}
        }

        if step.action == MacroAction::IfStart {
            if let Some(preset_id) = step.if_vision_preset_id
                && let Some(preset) = self
                    .state
                    .vision_presets
                    .iter()
                    .find(|item| item.id == preset_id)
                && seen.vision_presets.insert(preset.id)
            {
                resources
                    .vision_presets
                    .push(crate::macro_code::SharedVisionPreset {
                        preset: preset.clone(),
                        template_png: fs::read(self.vision_template_file_for_preset(preset.id))
                            .ok(),
                    });
            }

            if let Some(preset_id) = step.if_ocr_preset_id
                && let Some(preset) = self
                    .state
                    .ocr_presets
                    .iter()
                    .find(|item| item.id == preset_id)
                && seen.ocr_presets.insert(preset.id)
            {
                resources.ocr_presets.push(preset.clone());
            }

            for extra in &step.extra_conditions {
                if let Some(preset_id) = extra.vision_preset_id
                    && let Some(preset) = self
                        .state
                        .vision_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.vision_presets.insert(preset.id)
                {
                    resources
                        .vision_presets
                        .push(crate::macro_code::SharedVisionPreset {
                            preset: preset.clone(),
                            template_png: fs::read(self.vision_template_file_for_preset(preset.id))
                                .ok(),
                        });
                }
                if let Some(preset_id) = extra.ocr_preset_id
                    && let Some(preset) = self
                        .state
                        .ocr_presets
                        .iter()
                        .find(|item| item.id == preset_id)
                    && seen.ocr_presets.insert(preset.id)
                {
                    resources.ocr_presets.push(preset.clone());
                }
            }
        }
    }

    fn import_macro_share_resources(
        &mut self,
        resources: crate::macro_code::MacroShareResources,
    ) -> ImportedMacroShareMaps {
        let mut maps = ImportedMacroShareMaps::default();

        for profile in resources.crosshair_profiles {
            let mut imported = profile.clone();
            let old_name = imported.name.clone();
            if self
                .state
                .profiles
                .iter()
                .any(|item| Self::trimmed_eq_ignore_ascii_case(&item.name, &imported.name))
            {
                imported.name = self.unique_profile_name(&imported.name);
            }
            imported.collapsed = true;
            maps.crosshair_profiles
                .insert(old_name, imported.name.clone());
            self.state.profiles.push(imported);
        }

        for mut preset in resources.window_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.window_presets,
                &mut self.state.next_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.window_presets.insert(old_id, preset.id);
            self.state.window_presets.push(preset);
        }

        for mut layout in resources.window_layouts {
            let old_id = layout.id;
            layout.id = Self::allocate_next_id(
                &self.state.window_layouts,
                &mut self.state.next_window_layout_id,
                |item| item.id,
            );
            layout.collapsed = true;
            maps.window_layouts.insert(old_id, layout.id);
            self.state.window_layouts.push(layout);
        }

        for mut preset in resources.window_focus_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.window_focus_presets,
                &mut self.state.next_window_focus_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.window_focus_presets.insert(old_id, preset.id);
            self.state.window_focus_presets.push(preset);
        }

        for mut preset in resources.pin_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.pin_presets,
                &mut self.state.next_pin_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.pin_presets.insert(old_id, preset.id);
            self.state.pin_presets.push(preset);
        }

        for mut preset in resources.mouse_path_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.mouse_path_presets,
                &mut self.state.next_mouse_path_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.mouse_path_presets.insert(old_id, preset.id);
            self.state.mouse_path_presets.push(preset);
        }

        for mut preset in resources.mouse_sensitivity_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.mouse_sensitivity_presets,
                &mut self.state.next_mouse_sensitivity_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.mouse_sensitivity_presets.insert(old_id, preset.id);
            self.state.mouse_sensitivity_presets.push(preset);
        }

        for mut preset in resources.zoom_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.zoom_presets,
                &mut self.state.next_zoom_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.zoom_presets.insert(old_id, preset.id);
            self.state.zoom_presets.push(preset);
        }

        for mut preset in resources.hud_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.hud_presets,
                &mut self.state.next_hud_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.hud_presets.insert(old_id, preset.id);
            self.state.hud_presets.push(preset);
        }

        for mut preset in resources.command_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.command_presets,
                &mut self.state.next_command_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.command_presets.insert(old_id, preset.id);
            self.state.command_presets.push(preset);
        }

        for mut preset in resources.geometry_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.geometry_presets,
                &mut self.state.next_geometry_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.geometry_presets.insert(old_id, preset.id);
            self.state.geometry_presets.push(preset);
        }

        for shared in resources.vision_presets {
            let old_id = shared.preset.id;
            let mut preset = shared.preset;
            preset.id = Self::allocate_next_id(
                &self.state.vision_presets,
                &mut self.state.next_vision_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            if let Some(bytes) = shared.template_png {
                let _ = fs::write(self.vision_template_file_for_preset(preset.id), bytes);
            }
            maps.vision_presets.insert(old_id, preset.id);
            self.state.vision_presets.push(preset);
        }

        for mut preset in resources.ocr_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.ocr_presets,
                &mut self.state.next_ocr_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.ocr_presets.insert(old_id, preset.id);
            self.state.ocr_presets.push(preset);
        }

        for mut preset in resources.audio_sense_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.audio_sense_presets,
                &mut self.state.next_audio_sense_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.audio_sense_presets.insert(old_id, preset.id);
            self.state.audio_sense_presets.push(preset);
        }

        for mut preset in resources.timer_presets {
            let old_id = preset.id;
            preset.id = Self::allocate_next_id(
                &self.state.timer_presets,
                &mut self.state.next_timer_preset_id,
                |item| item.id,
            );
            preset.collapsed = true;
            maps.timer_presets.insert(old_id, preset.id);
            self.state.timer_presets.push(preset);
        }

        maps
    }

    fn remap_macro_share_refs_in_key(
        key: &mut String,
        mapping: &HashMap<u32, u32>,
        prefix: Option<&str>,
    ) {
        let trimmed = key.trim();
        let Some(raw_spec) =
            prefix.map_or_else(|| Some(trimmed), |value| trimmed.strip_prefix(value))
        else {
            return;
        };
        let raw_spec = raw_spec.trim();
        if let Ok(old_id) = raw_spec.parse::<u32>()
            && let Some(new_id) = mapping.get(&old_id)
        {
            *key = prefix
                .map(|value| format!("{value}{new_id}"))
                .unwrap_or_else(|| new_id.to_string());
        }
    }

    fn remap_macro_share_resource_refs_in_step(
        step: &mut MacroStep,
        maps: &ImportedMacroShareMaps,
    ) {
        match step.action {
            MacroAction::ApplyWindowPreset => {
                if step.key.trim().starts_with("layout:") {
                    Self::remap_macro_share_refs_in_key(
                        &mut step.key,
                        &maps.window_layouts,
                        Some("layout:"),
                    );
                } else {
                    Self::remap_macro_share_refs_in_key(&mut step.key, &maps.window_presets, None);
                }
            }
            MacroAction::FocusWindowPreset => {
                Self::remap_macro_share_refs_in_key(
                    &mut step.key,
                    &maps.window_focus_presets,
                    None,
                );
            }
            MacroAction::TriggerCommandPreset => {
                Self::remap_macro_share_refs_in_key(&mut step.key, &maps.command_presets, None);
            }
            MacroAction::EnableCrosshairProfile | MacroAction::DisableCrosshair => {
                if let Some((_, new_name)) = maps.crosshair_profiles.iter().find(|(old_name, _)| {
                    Self::trimmed_eq_ignore_ascii_case(old_name, step.key.trim())
                }) {
                    step.key = new_name.clone();
                }
            }
            MacroAction::EnablePinPreset | MacroAction::DisablePin => {
                Self::remap_macro_share_refs_in_key(&mut step.key, &maps.pin_presets, None);
            }
            MacroAction::PlayMousePathPreset => {
                Self::remap_macro_share_refs_in_key(&mut step.key, &maps.mouse_path_presets, None);
            }
            MacroAction::ApplyMouseSensitivityPreset => {
                Self::remap_macro_share_refs_in_key(
                    &mut step.key,
                    &maps.mouse_sensitivity_presets,
                    None,
                );
            }
            MacroAction::EnableZoomPreset => {
                Self::remap_macro_share_refs_in_key(&mut step.key, &maps.zoom_presets, None);
            }
            MacroAction::StartVisionSearch
            | MacroAction::ScanVisionOnce
            | MacroAction::StopVision => {
                Self::remap_macro_share_refs_in_key(&mut step.key, &maps.vision_presets, None);
            }
            MacroAction::StartAudioSensePreset => {
                if let Some(old_id) = step.audio_sense_preset_id
                    && let Some(new_id) = maps.audio_sense_presets.get(&old_id)
                {
                    step.audio_sense_preset_id = Some(*new_id);
                }
            }
            MacroAction::ShowHud => {
                Self::remap_macro_share_refs_in_key(&mut step.key, &maps.hud_presets, None);
            }
            MacroAction::OcrSearch => {
                Self::remap_macro_share_refs_in_key(&mut step.key, &maps.ocr_presets, None);
            }
            MacroAction::ShowGeometryPreset | MacroAction::HideGeometryPreset => {
                Self::remap_macro_share_refs_in_key(&mut step.key, &maps.geometry_presets, None);
            }
            MacroAction::DrawGeometry => {
                if let Some(old_id) = step.geometry_preset_id
                    && let Some(new_id) = maps.geometry_presets.get(&old_id)
                {
                    step.geometry_preset_id = Some(*new_id);
                }
            }
            MacroAction::StartTimerPreset
            | MacroAction::PauseTimerPreset
            | MacroAction::StopTimerPreset
            | MacroAction::ReadTimerPreset => {
                if let Some(old_id) = step.timer_preset_id
                    && let Some(new_id) = maps.timer_presets.get(&old_id)
                {
                    step.timer_preset_id = Some(*new_id);
                }
            }
            _ => {}
        }

        if let Some(old_id) = step.if_vision_preset_id
            && let Some(new_id) = maps.vision_presets.get(&old_id)
        {
            step.if_vision_preset_id = Some(*new_id);
        }
        if let Some(old_id) = step.if_ocr_preset_id
            && let Some(new_id) = maps.ocr_presets.get(&old_id)
        {
            step.if_ocr_preset_id = Some(*new_id);
        }
        for extra in &mut step.extra_conditions {
            if let Some(old_id) = extra.vision_preset_id
                && let Some(new_id) = maps.vision_presets.get(&old_id)
            {
                extra.vision_preset_id = Some(*new_id);
            }
            if let Some(old_id) = extra.ocr_preset_id
                && let Some(new_id) = maps.ocr_presets.get(&old_id)
            {
                extra.ocr_preset_id = Some(*new_id);
            }
        }
    }

    fn remap_macro_share_resource_refs_in_preset(
        preset: &mut MacroPreset,
        maps: &ImportedMacroShareMaps,
    ) {
        Self::remap_macro_share_resource_refs_in_step(&mut preset.hold_stop_step, maps);
        Self::remap_macro_share_resource_refs_in_step(&mut preset.press_stop_step, maps);
        for step in &mut preset.steps {
            Self::remap_macro_share_resource_refs_in_step(step, maps);
        }
    }

    fn export_macro_step(&mut self, preset_id: u32, step_index: usize, step: &MacroStep) {
        let shared = crate::macro_code::SharedMacroStep {
            step: step.clone(),
            resources: self.collect_macro_share_resources_for_step(step),
        };
        match crate::macro_code::encode_shared_step(&shared) {
            Ok(code) => self.write_macro_share_code(
                code,
                "Step code copied to clipboard.",
                MacroShareCodeKind::Step,
                Some((preset_id, step_index)),
                None,
                None,
            ),
            Err(error) => self.status = format!("Failed to export step: {error}"),
        }
    }

    fn import_macro_step_from_clipboard(
        &mut self,
        group_id: u32,
        preset_id: u32,
        insert_after_index: Option<usize>,
    ) {
        let Some(code) = self.read_clipboard_text() else {
            return;
        };
        match crate::macro_code::decode_shared_step(&code) {
            Ok(shared) => {
                let maps = self.import_macro_share_resources(shared.resources);
                let mut step = shared.step;
                Self::remap_macro_share_resource_refs_in_step(&mut step, &maps);
                Self::bind_trigger_macro_step_to_group(&mut step, group_id);
                if let Ok((group_index, preset_index)) =
                    self.macro_preset_indices(group_id, preset_id)
                {
                    let preset = &mut self.state.macro_groups[group_index].presets[preset_index];
                    if let Some(idx) = insert_after_index {
                        if idx < preset.steps.len() {
                            preset.steps.insert(idx + 1, step);
                        } else {
                            preset.steps.push(step);
                        }
                    } else {
                        preset.steps.push(step);
                    }
                    self.persist_after_syncs([
                        Self::sync_profiles,
                        Self::sync_window_presets,
                        Self::sync_window_layouts,
                        Self::sync_mouse_sensitivity_presets,
                        Self::sync_hud_presets,
                        Self::sync_command_presets,
                        Self::sync_vision_presets,
                        Self::sync_ocr_presets,
                        Self::sync_geometry_presets,
                        Self::sync_audio_sense_presets,
                        Self::sync_timer_presets,
                        Self::sync_macro_presets,
                    ]);
                    self.status = Self::tr_lang(
                        self.state.ui_language,
                        "Step imported successfully.",
                        "Step imported successfully.",
                    )
                    .to_owned();
                }
            }
            Err(error) => self.status = format!("Import failed: {error}"),
        }
    }

    fn export_macro_preset(&mut self, preset_id: u32, preset: &MacroPreset) {
        let shared = crate::macro_code::SharedMacroPreset {
            preset: preset.clone(),
            resources: self.collect_macro_share_resources_for_preset(preset),
        };
        match crate::macro_code::encode_shared_preset(&shared) {
            Ok(code) => self.write_macro_share_code(
                code,
                "Preset code copied to clipboard.",
                MacroShareCodeKind::Preset,
                None,
                Some(preset_id),
                None,
            ),
            Err(error) => self.status = format!("Failed to export preset: {error}"),
        }
    }

    fn import_macro_preset_from_clipboard(
        &mut self,
        group_id: u32,
        insert_after_preset_id: Option<u32>,
    ) {
        let Some(code) = self.read_clipboard_text() else {
            return;
        };
        match crate::macro_code::decode_shared_preset(&code) {
            Ok(shared) => {
                let maps = self.import_macro_share_resources(shared.resources);
                let mut preset = shared.preset;
                let source_preset_id = preset.id;
                let id = self.allocate_next_macro_preset_id();
                preset.id = id;
                Self::remap_macro_step_self_ref(&mut preset.hold_stop_step, source_preset_id, id);
                Self::bind_trigger_macro_step_to_group(&mut preset.hold_stop_step, group_id);
                Self::remap_macro_step_self_ref(&mut preset.press_stop_step, source_preset_id, id);
                Self::bind_trigger_macro_step_to_group(&mut preset.press_stop_step, group_id);
                for step in &mut preset.steps {
                    Self::remap_macro_step_self_ref(step, source_preset_id, id);
                    Self::bind_trigger_macro_step_to_group(step, group_id);
                }
                Self::remap_macro_share_resource_refs_in_preset(&mut preset, &maps);
                if let Some(group) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|g| g.id == group_id)
                {
                    Self::insert_after_id_or_push(
                        &mut group.presets,
                        insert_after_preset_id,
                        preset,
                        |preset| preset.id,
                    );
                    self.persist_after_syncs([
                        Self::sync_profiles,
                        Self::sync_window_presets,
                        Self::sync_window_layouts,
                        Self::sync_mouse_sensitivity_presets,
                        Self::sync_hud_presets,
                        Self::sync_command_presets,
                        Self::sync_vision_presets,
                        Self::sync_ocr_presets,
                        Self::sync_geometry_presets,
                        Self::sync_audio_sense_presets,
                        Self::sync_timer_presets,
                        Self::sync_reconciled_macro_presets,
                    ]);
                    self.status = Self::tr_lang(
                        self.state.ui_language,
                        "Preset imported successfully.",
                        "Preset imported successfully.",
                    )
                    .to_owned();
                }
            }
            Err(error) => self.status = format!("Import failed: {error}"),
        }
    }

    fn export_macro_group(&mut self, group_id: u32, group: &MacroGroup) {
        let shared = crate::macro_code::SharedMacroGroup {
            group: group.clone(),
            resources: self.collect_macro_share_resources_for_group(group),
        };
        match crate::macro_code::encode_shared_group(&shared) {
            Ok(code) => self.write_macro_share_code(
                code,
                "Group code copied to clipboard.",
                MacroShareCodeKind::Group,
                None,
                None,
                Some(group_id),
            ),
            Err(error) => self.status = format!("Failed to export group: {error}"),
        }
    }

    fn import_macro_group_from_clipboard(
        &mut self,
        folder_id: Option<u32>,
        insert_after_group_id: Option<u32>,
    ) {
        let Some(code) = self.read_clipboard_text() else {
            return;
        };
        match crate::macro_code::decode_shared_group(&code) {
            Ok(shared) => {
                let maps = self.import_macro_share_resources(shared.resources);
                let mut group = shared.group;
                let source_group_id = group.id;
                let id = Self::allocate_next_id(
                    &self.state.macro_groups,
                    &mut self.state.next_macro_group_id,
                    |group| group.id,
                );
                group.id = id;
                group.name = self.unique_macro_group_name(&group.name);
                group.folder_id = folder_id;

                let mut preset_id_map = HashMap::new();
                for preset in &mut group.presets {
                    let old_preset_id = preset.id;
                    let preset_id = self.allocate_next_macro_preset_id();
                    preset.id = preset_id;
                    preset_id_map.insert(old_preset_id, preset_id);
                }

                for preset in &mut group.presets {
                    Self::remap_macro_step_group_refs(
                        &mut preset.hold_stop_step,
                        &preset_id_map,
                        source_group_id,
                        id,
                    );
                    Self::remap_macro_step_group_refs(
                        &mut preset.press_stop_step,
                        &preset_id_map,
                        source_group_id,
                        id,
                    );
                    for step in &mut preset.steps {
                        Self::remap_macro_step_group_refs(
                            step,
                            &preset_id_map,
                            source_group_id,
                            id,
                        );
                    }
                    Self::remap_macro_share_resource_refs_in_preset(preset, &maps);
                }

                Self::insert_after_id_or_push(
                    &mut self.state.macro_groups,
                    insert_after_group_id,
                    group,
                    |group| group.id,
                );
                self.pending_macro_group_scroll_target = Some(id);
                self.persist_after_syncs([
                    Self::sync_profiles,
                    Self::sync_window_presets,
                    Self::sync_window_layouts,
                    Self::sync_mouse_sensitivity_presets,
                    Self::sync_hud_presets,
                    Self::sync_command_presets,
                    Self::sync_vision_presets,
                    Self::sync_ocr_presets,
                    Self::sync_geometry_presets,
                    Self::sync_audio_sense_presets,
                    Self::sync_timer_presets,
                    Self::sync_reconciled_macro_presets,
                ]);
                self.status = Self::tr_lang(
                    self.state.ui_language,
                    "Group imported successfully.",
                    "Group imported successfully.",
                )
                .to_owned();
            }
            Err(error) => self.status = format!("Import failed: {error}"),
        }
    }

    fn active_panel_needs_open_windows(panel: AppPanel) -> bool {
        matches!(
            panel,
            AppPanel::WindowPresets
                | AppPanel::Pin
                | AppPanel::Vision
                | AppPanel::Commands
                | AppPanel::Memory
        )
    }

    fn active_panel_needs_audio_sense_devices(panel: AppPanel) -> bool {
        matches!(panel, AppPanel::AudioSense)
    }

    fn all_panels_for_background_preload() -> &'static [AppPanel] {
        // Keep startup focused on showing the active panel quickly.
        // Other panels can warm lazily when the user actually opens them.
        &[]
    }

    fn panel_is_warmed(&self, panel: AppPanel) -> bool {
        self.warmed_panels.contains(&panel)
    }

    fn panel_loading_shell_active(&self, panel: AppPanel) -> bool {
        let _ = panel;
        self.startup_shell_frames_remaining > 0
    }

    fn finish_panel_warmup_if_ready(&mut self, panel: AppPanel) {
        if self.panel_warmup_target != Some(panel) || self.panel_warmup_frames_remaining == 0 {
            return;
        }
        self.panel_warmup_frames_remaining -= 1;
        if self.panel_warmup_frames_remaining == 0 {
            self.panel_warmup_target = None;
            if !self.panel_is_warmed(panel) {
                self.warmed_panels.push(panel);
            }
        }
    }

    fn open_audio_editor(&mut self, target: AudioEditorTarget) {
        self.active_audio_editor = Some(target);
        self.state.active_panel = AppPanel::Media;
        self.trim_timeline_zoom = 1.0;
        self.preview_cursor = None;
    }

    fn close_audio_editor(&mut self) {
        self.active_audio_editor = None;
        self.state.active_panel = AppPanel::Sound;
        audio::stop_preview();
    }

    fn allocate_next_id<T, F>(items: &[T], next_hint: &mut u32, id_of: F) -> u32
    where
        F: Fn(&T) -> u32,
    {
        let mut id = (*next_hint).max(1);
        while items.iter().any(|item| id_of(item) == id) {
            id += 1;
        }
        *next_hint = (items.iter().map(id_of).max().unwrap_or(0) + 1).max(id + 1);
        id
    }

    fn allocate_next_macro_preset_id(&mut self) -> u32 {
        let mut id = self.state.next_macro_preset_id.max(1);
        while self
            .state
            .macro_groups
            .iter()
            .flat_map(|group| group.presets.iter())
            .any(|preset| preset.id == id)
        {
            id += 1;
        }
        self.state.next_macro_preset_id = (self
            .state
            .macro_groups
            .iter()
            .flat_map(|group| group.presets.iter())
            .map(|preset| preset.id)
            .max()
            .unwrap_or(0)
            + 1)
        .max(id + 1);
        id
    }

    fn allocate_next_master_preset_id(&mut self) -> u32 {
        Self::allocate_next_id(
            &self.state.master_presets,
            &mut self.state.next_master_preset_id,
            |preset| preset.id,
        )
    }

    fn macro_group_index(&self, group_id: u32) -> Option<usize> {
        self.state
            .macro_groups
            .iter()
            .position(|group| group.id == group_id)
    }

    fn macro_preset_indices(
        &self,
        group_id: u32,
        preset_id: u32,
    ) -> Result<(usize, usize), &'static str> {
        let group_index = self
            .macro_group_index(group_id)
            .ok_or("Macro group not found.")?;
        let preset_index = self.state.macro_groups[group_index]
            .presets
            .iter()
            .position(|preset| preset.id == preset_id)
            .ok_or("Macro preset not found.")?;
        Ok((group_index, preset_index))
    }

    fn macro_preset(&self, group_id: u32, preset_id: u32) -> Option<&MacroPreset> {
        let (group_index, preset_index) = self.macro_preset_indices(group_id, preset_id).ok()?;
        self.state
            .macro_groups
            .get(group_index)
            .and_then(|group| group.presets.get(preset_index))
    }

    fn pin_preset(&self, preset_id: u32) -> Option<&crate::model::PinPreset> {
        self.state
            .pin_presets
            .iter()
            .find(|preset| preset.id == preset_id)
    }

    fn command_preset_id_from_key(command_presets: &[CommandPreset], key: &str) -> Option<u32> {
        key.trim().parse::<u32>().ok().or_else(|| {
            command_presets
                .iter()
                .find(|preset| Self::trimmed_eq_ignore_ascii_case(&preset.name, key))
                .map(|preset| preset.id)
        })
    }

    fn command_preset_selected_label(
        command_presets: &[CommandPreset],
        selected_id: Option<u32>,
        key: &str,
        language: UiLanguage,
    ) -> String {
        selected_id
            .and_then(|id| {
                command_presets
                    .iter()
                    .find(|preset| preset.id == id)
                    .map(|preset| preset.name.clone())
            })
            .unwrap_or_else(|| {
                if key.trim().is_empty() {
                    Self::tr_lang(language, "Select command", "Select command").to_owned()
                } else {
                    key.to_owned()
                }
            })
    }

    fn timer_preset_selected_label(
        timer_presets: &[TimerPreset],
        selected_id: Option<u32>,
        language: UiLanguage,
    ) -> String {
        Self::named_item_name_by_id(
            timer_presets,
            selected_id,
            |preset| preset.id,
            |preset| &preset.name,
        )
        .unwrap_or_else(|| Self::tr_lang(language, "Select timer", "Select timer").to_owned())
    }

    fn named_item_name_by_id<T>(
        items: &[T],
        selected_id: Option<u32>,
        id_of: impl Fn(&T) -> u32,
        name_of: impl Fn(&T) -> &str,
    ) -> Option<String> {
        let id = selected_id?;
        items
            .iter()
            .find(|item| id_of(item) == id)
            .map(|item| name_of(item).to_owned())
    }

    fn option_label_by_id(
        options: &[(u32, String)],
        selected_id: Option<u32>,
        fallback: &'static str,
        language: UiLanguage,
    ) -> String {
        selected_id
            .and_then(|id| {
                options
                    .iter()
                    .find(|(option_id, _)| *option_id == id)
                    .map(|(_, label)| label.clone())
            })
            .unwrap_or_else(|| {
                let fallback_vi = match fallback {
                    "Select image search preset" => "Chọn preset image search",
                    "Select geometry" => "Chọn geometry",
                    "Select OCR" => "Chọn OCR",
                    "Any Preset" => "Mọi preset",
                    _ => fallback,
                };
                Self::tr_lang(language, fallback, fallback_vi).to_owned()
            })
    }

    fn option_label_by_id_or_any(
        options: &[(u32, String)],
        selected_id: Option<u32>,
        fallback: &'static str,
        any_id: u32,
        any_label: &'static str,
        language: UiLanguage,
    ) -> String {
        selected_id
            .and_then(|id| {
                if id == any_id {
                    let any_label_vi = match any_label {
                        "Any Preset" => "Mọi preset",
                        _ => any_label,
                    };
                    Some(Self::tr_lang(language, any_label, any_label_vi).to_owned())
                } else {
                    options
                        .iter()
                        .find(|(option_id, _)| *option_id == id)
                        .map(|(_, label)| label.clone())
                }
            })
            .unwrap_or_else(|| {
                let fallback_vi = match fallback {
                    "Select image search preset" => "Chọn preset image search",
                    "Select geometry" => "Chọn geometry",
                    "Select OCR" => "Chọn OCR",
                    "Any Preset" => "Mọi preset",
                    _ => fallback,
                };
                Self::tr_lang(language, fallback, fallback_vi).to_owned()
            })
    }

    fn ocr_step_selected_label(
        ocr_preset_options: &[(u32, String)],
        is_custom: bool,
        selected_id: Option<u32>,
        language: UiLanguage,
    ) -> String {
        if is_custom {
            Self::tr_lang(language, "Custom OCR", "Custom OCR").to_owned()
        } else {
            Self::option_label_by_id(ocr_preset_options, selected_id, "Select OCR", language)
        }
    }

    fn vision_preset_selected_label(
        vision_presets: &[crate::model::VisionPreset],
        selected_id: Option<u32>,
        language: UiLanguage,
    ) -> String {
        Self::named_item_name_by_id(
            vision_presets,
            selected_id,
            |preset| preset.id,
            |preset| &preset.name,
        )
        .unwrap_or_else(|| Self::tr_lang(language, "Select preset", "Select preset").to_owned())
    }

    fn vision_preset_by_id<'a>(
        vision_presets: &'a [crate::model::VisionPreset],
        selected_id: Option<u32>,
    ) -> Option<&'a crate::model::VisionPreset> {
        let id = selected_id?;
        vision_presets.iter().find(|preset| preset.id == id)
    }

    fn item_selected_label<T>(
        items: &[T],
        selected_id: Option<u32>,
        fallback: &'static str,
        language: UiLanguage,
        id_of: impl Fn(&T) -> u32,
        name_of: impl Fn(&T) -> &str,
    ) -> String {
        selected_id
            .and_then(|id| {
                items
                    .iter()
                    .find(|item| id_of(item) == id)
                    .map(|item| name_of(item).to_owned())
            })
            .unwrap_or_else(|| Self::tr_lang(language, fallback, fallback).to_owned())
    }

    fn ordered_id_index<T, F>(items: &[T], id: u32, id_of: F) -> usize
    where
        F: Fn(&T) -> u32,
    {
        items
            .iter()
            .position(|item| id_of(item) == id)
            .unwrap_or(usize::MAX)
    }

    fn insert_after_id_or_push<T, F>(
        items: &mut Vec<T>,
        insert_after_id: Option<u32>,
        item: T,
        id_of: F,
    ) where
        F: Fn(&T) -> u32,
    {
        if let Some(target_id) = insert_after_id
            && let Some(idx) = items
                .iter()
                .position(|existing| id_of(existing) == target_id)
        {
            items.insert(idx + 1, item);
        } else {
            items.push(item);
        }
    }

    #[cfg(windows)]
    fn current_mouse_speed() -> Option<u32> {
        let mut speed = 10u32;
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                SPI_GETMOUSESPEED, SystemParametersInfoW,
            };
            SystemParametersInfoW(
                SPI_GETMOUSESPEED,
                0,
                Some((&mut speed as *mut u32).cast()),
                Default::default(),
            )
            .ok()?;
        }
        Some(speed.clamp(1, 20))
    }

    #[cfg(not(windows))]
    fn current_mouse_speed() -> Option<u32> {
        None
    }

    #[cfg(windows)]
    pub(crate) fn extract_zip_archive(
        archive_path: &std::path::Path,
        destination_dir: &std::path::Path,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        use std::fs;
        use std::io::{self, Write};
        use zip::ZipArchive;

        let file = fs::File::open(archive_path)
            .with_context(|| format!("Failed to open archive {}", archive_path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("Failed to read zip archive {}", archive_path.display()))?;

        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .with_context(|| format!("Failed to read zip entry #{index}"))?;
            let Some(relative_path) = entry.enclosed_name().map(|path| path.to_path_buf()) else {
                continue;
            };
            let output_path = destination_dir.join(relative_path);

            if entry.is_dir() {
                fs::create_dir_all(&output_path)?;
                continue;
            }

            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut output = fs::File::create(&output_path)
                .with_context(|| format!("Failed to create {}", output_path.display()))?;
            io::copy(&mut entry, &mut output)
                .with_context(|| format!("Failed to extract {}", output_path.display()))?;
            output.flush()?;
        }

        Ok(())
    }

    fn interception_install_pending_marker_path(&self) -> std::path::PathBuf {
        self.paths.bin_dir.join("interception.install.pending")
    }

    fn set_interception_install_pending_marker(&self, pending: bool) {
        let marker = self.interception_install_pending_marker_path();
        if pending {
            let _ = std::fs::write(marker, b"pending");
        } else {
            let _ = std::fs::remove_file(marker);
        }
    }

    fn poll_mouse_tool_jobs(&mut self) {
        if let Some(job) = &self.arduino_download_job {
            if job.is_finished() {
                let job = self.arduino_download_job.take().unwrap();
                match job.join() {
                    Ok(Ok(())) => {
                        self.arduino_tools_downloaded = true;
                        self.status = Self::tr_lang(
                            self.state.ui_language,
                            "Arduino tools downloaded successfully!",
                            "Arduino tools downloaded successfully!",
                        )
                        .to_owned();
                    }
                    Ok(Err(error)) => {
                        self.status = format!("Download failed: {error}");
                    }
                    Err(_) => {
                        self.status = "Download thread panicked".to_owned();
                    }
                }
            }
        }

        if let Some(job) = &self.interception_download_job {
            if job.is_finished() {
                let job = self.interception_download_job.take().unwrap();
                match job.join() {
                    Ok(Ok(())) => {
                        self.interception_package_downloaded = true;
                        self.status = Self::tr_lang(
                            self.state.ui_language,
                            "Interception package downloaded successfully.",
                            "Interception package downloaded successfully.",
                        )
                        .to_owned();
                        self.start_interception_driver_install();
                    }
                    Ok(Err(error)) => {
                        self.status = format!("Download failed: {error}");
                        let _ = fs::remove_file(&self.paths.interception_zip);
                        let _ = fs::remove_dir_all(&self.paths.interception_package_dir);
                    }
                    Err(_) => {
                        self.status = "Download thread panicked.".to_owned();
                    }
                }
            }
        }

        if let Some(job) = &self.interception_install_job {
            if job.is_finished() {
                let job = self.interception_install_job.take().unwrap();
                match job.join() {
                    Ok(Ok(())) => {
                        self.interception_driver_installed = true;
                        self.interception_driver_needs_restart = true;
                        self.set_interception_install_pending_marker(true);
                        self.status = Self::tr_lang(
                            self.state.ui_language,
                            "Interception driver installed. Restart your PC for it to take effect.",
                            "Interception driver installed. Restart your PC for it to take effect.",
                        )
                        .to_owned();
                    }
                    Ok(Err(error)) => {
                        self.status = format!("Driver install failed: {error}");
                    }
                    Err(_) => {
                        self.status = "Driver install thread panicked.".to_owned();
                    }
                }
            }
        }

        if let Some(job) = &self.interception_uninstall_job {
            if job.is_finished() {
                let job = self.interception_uninstall_job.take().unwrap();
                match job.join() {
                    Ok(Ok(())) => {
                        self.delete_interception_package();
                        self.state.vision_settings.use_interception = false;
                        self.interception_driver_installed = false;
                        self.interception_driver_needs_restart = false;
                        self.set_interception_install_pending_marker(false);
                        self.status =
                            "Interception driver removed. Package files deleted from app."
                                .to_owned();
                    }
                    Ok(Err(error)) => {
                        self.status = format!("Driver uninstall failed: {error}");
                    }
                    Err(_) => {
                        self.status = "Driver uninstall thread panicked.".to_owned();
                    }
                }
            }
        }
    }

    fn choose_audio_file_for_target(&mut self, target: AudioEditorTarget) {
        let prefix = match target {
            AudioEditorTarget::Preset(preset_id) => format!("preset-{preset_id}"),
            AudioEditorTarget::Library(item_id) => format!("library-{item_id}"),
            AudioEditorTarget::Startup => "startup".to_owned(),
            AudioEditorTarget::Exit => "exit".to_owned(),
        };
        let Some((path_str, duration)) = self.pick_and_import_audio_file(&prefix) else {
            return;
        };
        self.apply_audio_file_to_target(target, &path_str, duration);
    }

    fn pick_and_import_audio_file(&mut self, prefix: &str) -> Option<(String, Option<u64>)> {
        let path = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a"])
            .pick_file()?;
        let path_str = match self.import_audio_file_to_app_data(&path, prefix) {
            Ok(path_str) => path_str,
            Err(error) => {
                self.status = format!("Failed to import audio file: {error}");
                return None;
            }
        };
        let duration = audio::load_duration_ms(&path_str).ok();
        Some((path_str, duration))
    }

    fn apply_audio_clip_file(clip: &mut AudioClipSettings, path: &str, duration: Option<u64>) {
        clip.file_path = path.to_owned();
        clip.start_ms = 0;
        clip.end_ms = duration.unwrap_or(0);
        clip.enabled = true;
    }

    fn apply_audio_file_to_target(
        &mut self,
        target: AudioEditorTarget,
        path: &str,
        duration: Option<u64>,
    ) {
        match target {
            AudioEditorTarget::Preset(preset_id) => {
                if let Some(preset) = self
                    .state
                    .audio_settings
                    .presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    Self::apply_audio_clip_file(&mut preset.clip, path, duration);
                    self.finish_audio_file_assignment(target, path, duration);
                }
            }
            AudioEditorTarget::Library(item_id) => {
                if let Some(item) = self
                    .state
                    .audio_settings
                    .library
                    .iter_mut()
                    .find(|item| item.id == item_id)
                {
                    Self::apply_audio_clip_file(&mut item.clip, path, duration);
                    self.finish_audio_file_assignment(target, path, duration);
                }
            }
            AudioEditorTarget::Startup => {
                Self::apply_audio_clip_file(&mut self.state.audio_settings.startup, path, duration);
                self.finish_audio_file_assignment(target, path, duration);
            }
            AudioEditorTarget::Exit => {
                Self::apply_audio_clip_file(&mut self.state.audio_settings.exit, path, duration);
                self.finish_audio_file_assignment(target, path, duration);
            }
        }
    }

    fn finish_audio_file_assignment(
        &mut self,
        target: AudioEditorTarget,
        path: &str,
        duration: Option<u64>,
    ) {
        self.set_audio_editor_target_duration(target, duration);
        match target {
            AudioEditorTarget::Preset(preset_id) => {
                self.show_sound_preset_audio_editor.insert(preset_id);
            }
            AudioEditorTarget::Library(item_id) => {
                self.show_library_audio_editor.insert(item_id);
            }
            AudioEditorTarget::Startup => {
                self.show_startup_audio_editor = true;
            }
            AudioEditorTarget::Exit => {
                self.show_exit_audio_editor = true;
            }
        }
        self.refresh_audio_waveform_for_path(path);
        self.preview_cursor = None;
        self.trim_timeline_zoom = 1.0;
        self.sync_and_persist_audio_settings();
    }

    fn sync_and_persist_audio_settings(&mut self) {
        self.persist_after_sync(Self::sync_audio_settings);
    }

    fn audio_storage_dir(&self) -> PathBuf {
        self.paths.root.join("audio")
    }

    fn import_audio_file_to_app_data(&self, source_path: &Path, prefix: &str) -> Result<String> {
        let audio_dir = self.audio_storage_dir();
        fs::create_dir_all(&audio_dir)?;

        let source_stem = source_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.trim().is_empty())
            .unwrap_or("audio");
        let extension = source_path
            .extension()
            .and_then(|ext| ext.to_str())
            .filter(|ext| !ext.trim().is_empty())
            .unwrap_or("wav");
        let sanitized_stem: String = source_stem
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
                _ => '_',
            })
            .collect();
        let sanitized_prefix: String = prefix
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
                _ => '_',
            })
            .collect();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let file_name = format!(
            "{}-{}-{}.{}",
            sanitized_prefix,
            sanitized_stem.trim_matches('_').trim(),
            timestamp,
            extension
        );
        let target_path = audio_dir.join(file_name);
        fs::copy(source_path, &target_path)?;
        Ok(target_path.to_string_lossy().to_string())
    }

    fn refresh_audio_waveform_for_path(&mut self, path: &str) {
        let trimmed = path.trim().to_owned();
        if trimmed.is_empty() || self.audio_waveforms.contains_key(&trimmed) {
            return;
        }
        // Insert a placeholder to prevent spawning duplicate loading threads
        self.audio_waveforms.insert(trimmed.clone(), Vec::new());

        let ui_tx = self.ui_tx.clone();
        std::thread::spawn(move || {
            let waveform = audio::load_waveform(&trimmed, 320).unwrap_or_default();
            let duration_ms = audio::load_duration_ms(&trimmed).ok();
            let _ = ui_tx.send(UiCommand::AudioWaveformLoaded {
                path: trimmed,
                waveform,
                duration_ms,
            });
        });
    }

    fn update_audio_clip_duration_for_path(&mut self, path: &str, duration_ms: Option<u64>) {
        for preset in &mut self.state.audio_settings.presets {
            if preset.clip.file_path.trim() == path {
                self.sound_preset_clip_duration_ms
                    .insert(preset.id, duration_ms);
            }
        }
        for item in &mut self.state.audio_settings.library {
            if item.clip.file_path.trim() == path {
                self.library_clip_duration_ms.insert(item.id, duration_ms);
            }
        }
        if self.state.audio_settings.startup.file_path.trim() == path {
            self.startup_clip_duration_ms = duration_ms;
        }
        if self.state.audio_settings.exit.file_path.trim() == path {
            self.exit_clip_duration_ms = duration_ms;
        }
    }

    fn set_audio_editor_target_duration(
        &mut self,
        target: AudioEditorTarget,
        duration: Option<u64>,
    ) {
        match target {
            AudioEditorTarget::Preset(preset_id) => {
                self.sound_preset_clip_duration_ms
                    .insert(preset_id, duration);
            }
            AudioEditorTarget::Library(item_id) => {
                self.library_clip_duration_ms.insert(item_id, duration);
            }
            AudioEditorTarget::Startup => {
                self.startup_clip_duration_ms = duration;
            }
            AudioEditorTarget::Exit => {
                self.exit_clip_duration_ms = duration;
            }
        }
    }

    fn audio_path_is_referenced(&self, path: &str) -> bool {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return false;
        }
        self.state.audio_settings.startup.file_path.trim() == trimmed
            || self.state.audio_settings.exit.file_path.trim() == trimmed
            || self
                .state
                .audio_settings
                .presets
                .iter()
                .any(|preset| preset.clip.file_path.trim() == trimmed)
            || self
                .state
                .audio_settings
                .library
                .iter()
                .any(|item| item.clip.file_path.trim() == trimmed)
    }

    fn retain_referenced_audio_waveforms(&mut self) {
        let mut referenced_paths = std::collections::HashSet::new();
        for path in [
            self.state.audio_settings.startup.file_path.trim(),
            self.state.audio_settings.exit.file_path.trim(),
        ] {
            if !path.is_empty() {
                referenced_paths.insert(path.to_owned());
            }
        }
        for preset in &self.state.audio_settings.presets {
            let path = preset.clip.file_path.trim();
            if !path.is_empty() {
                referenced_paths.insert(path.to_owned());
            }
        }
        for item in &self.state.audio_settings.library {
            let path = item.clip.file_path.trim();
            if !path.is_empty() {
                referenced_paths.insert(path.to_owned());
            }
        }
        self.audio_waveforms
            .retain(|path, _| referenced_paths.contains(path));
    }

    fn preview_cursor_ms_for(
        preview_cursor: &Option<(AudioEditorTarget, u64)>,
        target: AudioEditorTarget,
        clip: &AudioClipSettings,
    ) -> u64 {
        preview_cursor
            .filter(|(cursor_target, _)| *cursor_target == target)
            .map(|(_, cursor_ms)| cursor_ms)
            .unwrap_or(clip.start_ms)
            .clamp(clip.start_ms, clip.end_ms.max(clip.start_ms + 1))
    }

    fn set_preview_cursor_ms(
        preview_cursor: &mut Option<(AudioEditorTarget, u64)>,
        target: AudioEditorTarget,
        cursor_ms: u64,
        clip: &AudioClipSettings,
    ) {
        *preview_cursor = Some((
            target,
            cursor_ms.clamp(clip.start_ms, clip.end_ms.max(clip.start_ms + 1)),
        ));
    }

    fn trim_audio_bounds(clip: &mut AudioClipSettings, total_ms: u64) {
        const MIN_TRIM_MS: u64 = 50;
        clip.start_ms = clip.start_ms.min(total_ms);
        clip.end_ms = if clip.end_ms == 0 {
            total_ms
        } else {
            clip.end_ms.min(total_ms)
        };
        if clip.end_ms <= clip.start_ms {
            clip.end_ms = (clip.start_ms + MIN_TRIM_MS).min(total_ms);
            clip.start_ms = clip.end_ms.saturating_sub(MIN_TRIM_MS);
        }
        clip.volume = clip.volume.clamp(0.0, 2.0);
        clip.speed = clip.speed.clamp(0.25, 3.0);
    }

    fn format_ms(ms: u64) -> String {
        format!("{:.2}s", ms as f64 / 1000.0)
    }

    fn preset_title_text(dark_mode: bool, name: &str, enabled: bool) -> RichText {
        let text = RichText::new(name).strong();
        text.color(Self::preset_body_text_color(dark_mode, enabled))
    }

    fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }

    fn trimmed_eq_ignore_ascii_case(left: &str, right: &str) -> bool {
        left.trim().eq_ignore_ascii_case(right.trim())
    }

    fn sort_macro_groups(groups: &mut [MacroGroup]) {
        groups.sort_by_key(|group| group.id);
    }

    fn macro_preset_matches_search_query(
        group: &MacroGroup,
        preset: &MacroPreset,
        query: &str,
    ) -> bool {
        if query.trim().is_empty() {
            return true;
        }
        let query = query.trim();
        Self::contains_case_insensitive(&group.name, query)
            || Self::contains_case_insensitive(
                &Self::format_macro_trigger_ui(UiLanguage::English, preset),
                query,
            )
    }

    fn macro_group_matches_search_query(group: &MacroGroup, query: &str) -> bool {
        if query.trim().is_empty() {
            return true;
        }
        let query = query.trim();
        Self::contains_case_insensitive(&group.name, query)
            || group
                .presets
                .iter()
                .any(|preset| Self::macro_preset_matches_search_query(group, preset, query))
    }

    fn desired_window_size() -> egui::Vec2 {
        vec2(1180.0, 780.0)
    }

    #[cfg(windows)]
    fn screen_size() -> egui::Vec2 {
        vec2(unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32, unsafe {
            GetSystemMetrics(SM_CYSCREEN)
        }
            as f32)
    }

    #[cfg(not(windows))]
    fn screen_size() -> egui::Vec2 {
        vec2(1920.0, 1080.0)
    }

    fn crosshair_position_limits(screen_size: egui::Vec2) -> (i32, i32) {
        let screen_w = screen_size.x.round().max(1.0) as i32;
        let screen_h = screen_size.y.round().max(1.0) as i32;
        (screen_w.saturating_sub(1), screen_h.saturating_sub(1))
    }

    fn square_window_size(size: egui::Vec2) -> egui::Vec2 {
        let edge = size.x.max(size.y).max(900.0);
        vec2(edge, edge)
    }

    #[cfg(windows)]
    fn centered_outer_position_for_size(size: egui::Vec2, _scale: f32) -> egui::Pos2 {
        use windows::Win32::UI::HiDpi::GetDpiForSystem;
        let dpi = unsafe { GetDpiForSystem() } as f32;
        let scale = if dpi > 0.0 { dpi / 96.0 } else { 1.0 };
        let screen_w = (unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32) / scale;
        let screen_h = (unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32) / scale;
        egui::pos2(
            ((screen_w - size.x) * 0.5).round(),
            ((screen_h - size.y) * 0.5).round().max(10.0),
        )
    }

    #[cfg(not(windows))]
    fn centered_outer_position_for_size(_size: egui::Vec2, _scale: f32) -> egui::Pos2 {
        egui::pos2(120.0, 120.0)
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.last_applied_theme == Some(self.state.ui_theme) {
            return;
        }

        configure_theme(ctx, self.state.ui_theme);

        self.last_applied_theme = Some(self.state.ui_theme);
    }

    fn cycle_language(&mut self) {
        self.state.ui_language = match self.state.ui_language {
            UiLanguage::English => UiLanguage::Vietnamese,
            UiLanguage::Vietnamese => UiLanguage::English,
            UiLanguage::Icon => UiLanguage::English,
        };
        self.persist();
    }

    fn cycle_vietnamese_input_mode(&mut self) {
        self.state.vietnamese_input_mode = match self.state.vietnamese_input_mode {
            VietnameseInputMode::Off => VietnameseInputMode::Telex,
            VietnameseInputMode::Telex => VietnameseInputMode::Vni,
            VietnameseInputMode::Vni => VietnameseInputMode::Off,
        };
        self.persist();
    }

    fn toggle_theme_mode(&mut self) {
        self.state.ui_theme = match self.state.ui_theme {
            UiThemeMode::Dark => UiThemeMode::Light,
            UiThemeMode::Light => UiThemeMode::Dark,
        };
        self.persist();
    }

    fn tr(&self, english: &'static str, vietnamese: &'static str) -> &'static str {
        Self::tr_lang(self.state.ui_language, english, vietnamese)
    }

    fn normalize_vietnamese(text: &'static str) -> &'static str {
        text
    }

    fn toggle_vietnamese_input_enabled(&mut self) {
        self.state.vietnamese_input_enabled = !self.state.vietnamese_input_enabled;
        self.sync_vietnamese_input_enabled();
        self.persist();
    }

    fn compose_vietnamese_input(raw_tail: &str, mode: VietnameseInputMode) -> String {
        let mut composed_tail = String::new();
        match mode {
            VietnameseInputMode::Off => composed_tail.push_str(raw_tail),
            VietnameseInputMode::Telex => {
                vi::transform_buffer(&TELEX, raw_tail.chars(), &mut composed_tail);
            }
            VietnameseInputMode::Vni => {
                vi::transform_buffer(&VNI, raw_tail.chars(), &mut composed_tail);
            }
        }
        composed_tail
    }

    fn apply_vietnamese_input_mode(
        response: &egui::Response,
        text: &mut String,
        enabled: bool,
        mode: VietnameseInputMode,
    ) {
        let mut session = VIETNAMESE_INPUT_SESSION.lock();
        if !enabled || mode == VietnameseInputMode::Off {
            session.mode = mode;
            session.prefix.clear();
            session.raw_tail.clear();
            session.last_output.clear();
            return;
        }

        if response.gained_focus() || session.mode != mode || session.last_output.is_empty() {
            session.mode = mode;
            session.prefix = text.clone();
            session.raw_tail.clear();
            session.last_output = text.clone();
            return;
        }

        if !response.has_focus() || !response.changed() {
            return;
        }

        if let Some(suffix) = text.strip_prefix(&session.last_output) {
            if suffix.is_empty() {
                return;
            }
            for ch in suffix.chars() {
                if ch.is_whitespace() {
                    let committed = Self::compose_vietnamese_input(&session.raw_tail, mode);
                    session.prefix.push_str(&committed);
                    session.prefix.push(ch);
                    session.raw_tail.clear();
                } else {
                    session.raw_tail.push(ch);
                }
            }
        } else if session.last_output.starts_with(text.as_str()) {
            session.mode = mode;
            session.prefix = text.clone();
            session.raw_tail.clear();
            session.last_output = text.clone();
            return;
        } else {
            session.mode = mode;
            session.prefix = text.clone();
            session.raw_tail.clear();
            session.last_output = text.clone();
            return;
        }

        let composed_tail = Self::compose_vietnamese_input(&session.raw_tail, mode);
        let mut composed = String::with_capacity(session.prefix.len() + composed_tail.len());
        composed.push_str(&session.prefix);
        composed.push_str(&composed_tail);
        *text = composed.clone();
        session.last_output = composed;
    }

    fn apply_vietnamese_input_if_changed(
        response: &egui::Response,
        enabled: bool,
        mode: VietnameseInputMode,
        text: &mut String,
    ) {
        if response.gained_focus() || response.changed() {
            Self::apply_vietnamese_input_mode(response, text, enabled, mode);
        }
    }

    pub(crate) fn apply_vietnamese_input_static(response: &egui::Response, text: &mut String) {
        let (enabled, mode) = {
            let config = VIETNAMESE_INPUT_CONFIG.lock();
            (config.enabled, config.mode)
        };
        Self::apply_vietnamese_input_if_changed(response, enabled, mode, text);
    }

    fn load_svg_texture(ctx: &egui::Context, name: &str, svg: &[u8]) -> Option<TextureHandle> {
        let opt = usvg::Options::default();
        let tree = usvg::Tree::from_data(svg, &opt).ok()?;
        let size = tree.size().to_int_size();
        let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())?;
        let mut pixmap_mut = pixmap.as_mut();
        resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap_mut);
        let image = ColorImage::from_rgba_unmultiplied(
            [pixmap.width() as usize, pixmap.height() as usize],
            pixmap.data(),
        );
        Some(ctx.load_texture(name.to_owned(), image, TextureOptions::LINEAR))
    }

    fn vietnamese_input_icon_texture(
        &mut self,
        ctx: &egui::Context,
        enabled: bool,
    ) -> Option<TextureHandle> {
        let cache = if enabled {
            &mut self.vietnamese_input_enabled_texture
        } else {
            &mut self.vietnamese_input_disabled_texture
        };
        if cache.is_none() {
            let name = if enabled {
                "vietnamese-input-enabled"
            } else {
                "vietnamese-input-disabled"
            };
            let svg = if enabled {
                include_bytes!("../../assets/unikey_v.svg").as_slice()
            } else {
                include_bytes!("../../assets/unikey_e.svg").as_slice()
            };
            *cache = Self::load_svg_texture(ctx, name, svg);
        }
        cache.clone()
    }

    fn titlebar_app_icon_texture(&mut self, ctx: &egui::Context) -> Option<TextureHandle> {
        if self.titlebar_app_icon_texture.is_none() {
            self.titlebar_app_icon_texture = Self::load_svg_texture(
                ctx,
                "titlebar-app-icon",
                include_bytes!("../../assets/app-icon.svg").as_slice(),
            );
        }
        self.titlebar_app_icon_texture.clone()
    }

    fn guides_author_logo_texture(&mut self, ctx: &egui::Context) -> Option<TextureHandle> {
        if self.guides_author_logo_texture.is_none() {
            self.guides_author_logo_texture = Self::load_svg_texture(
                ctx,
                "guides-author-logo",
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
  <rect width="512" height="512" fill="#FAF7F2"/>
  <path d="M180 90V370L400 420Z" fill="#2563EB"/>
  <path d="M250 140V300H340V330H220V140Z" fill="#FAF7F2"/>
  <path d="M180 370L400 420L340 330Z" fill="#1D4ED8"/>
</svg>"##,
            );
        }
        self.guides_author_logo_texture.clone()
    }

    fn tr_lang(
        language: UiLanguage,
        english: &'static str,
        vietnamese: &'static str,
    ) -> &'static str {
        match language {
            UiLanguage::Vietnamese => {
                // 1. Check the central JSON translation system first.
                if let Some(translated) = crate::lang::translate(language, english) {
                    return Self::normalize_vietnamese(translated);
                }
                // 2. Fall back to the custom Vietnamese string if it was provided and is distinct from English.
                if !vietnamese.is_empty() && vietnamese != english {
                    return vietnamese;
                }
                // 3. Ultimate fallback.
                english
            }
            UiLanguage::English | UiLanguage::Icon => english,
        }
    }

    fn format_binding_ui(language: UiLanguage, binding: Option<&HotkeyBinding>) -> String {
        let label = hotkey::format_binding(binding);
        if label == "Not set" {
            Self::tr_lang(language, "Not set", "Not set").to_owned()
        } else {
            label
        }
    }

    fn render_hotkey_capture_control(
        ui: &mut egui::Ui,
        language: UiLanguage,
        binding: &mut Option<HotkeyBinding>,
        capture_target: &CaptureRequest,
        active_capture_target: Option<&CaptureRequest>,
        pending_combo_keys: Option<&Vec<String>>,
        live_sync: &mut bool,
    ) -> (bool, bool) {
        let capture_active = active_capture_target == Some(capture_target);
        let preview_binding = if capture_active {
            pending_combo_keys
                .map(|keys| Self::hotkey_binding_from_combo_keys(keys.clone()))
                .or_else(|| binding.clone())
        } else {
            binding.clone()
        };
        ui.monospace(Self::format_binding_ui(language, preview_binding.as_ref()));

        let mut begin_capture = false;
        let mut cancel_capture = false;
        let capture_time = ui.ctx().input(|input| input.time) as f32;
        let pulse = if capture_active {
            0.5 + 0.5 * (capture_time * 6.0).sin().abs()
        } else {
            0.0
        };
        let capture_fill = if capture_active {
            Color32::from_rgba_premultiplied(
                (88.0 + pulse * 28.0) as u8,
                (84.0 + pulse * 28.0) as u8,
                (44.0 + pulse * 10.0) as u8,
                255,
            )
        } else {
            ui.visuals().widgets.inactive.bg_fill
        };
        let capture_stroke = if capture_active {
            Color32::from_rgb(255, 232, 96)
        } else {
            ui.visuals().widgets.inactive.bg_stroke.color
        };
        if ui
            .add(
                Button::new(Self::capture_button_text(language, capture_active))
                    .fill(capture_fill)
                    .stroke(egui::Stroke::new(1.0, capture_stroke)),
            )
            .clicked()
        {
            if capture_active {
                cancel_capture = true;
            } else {
                begin_capture = true;
            }
        }
        if binding.is_some()
            && !capture_active
            && ui
                .button(Self::tr_lang(language, "Clear", "Clear"))
                .clicked()
        {
            *binding = None;
            *live_sync = true;
        }

        (begin_capture, cancel_capture)
    }

    fn preset_trigger_bindings(
        hotkey: &Option<HotkeyBinding>,
        trigger_keys: &str,
    ) -> Vec<HotkeyBinding> {
        let mut bindings = Vec::new();
        if let Some(binding) = hotkey.as_ref() {
            bindings.push(binding.clone());
        }
        for binding in hotkey::parse_binding_list(trigger_keys) {
            if !bindings
                .iter()
                .any(|existing| hotkey::binding_matches(existing, &binding))
            {
                bindings.push(binding);
            }
        }
        bindings
    }

    fn preset_trigger_has_binding(
        hotkey: &Option<HotkeyBinding>,
        trigger_keys: &str,
        binding: &HotkeyBinding,
    ) -> bool {
        Self::preset_trigger_bindings(hotkey, trigger_keys)
            .iter()
            .any(|existing| hotkey::binding_matches(existing, binding))
    }

    fn preset_trigger_add_binding(
        hotkey: &mut Option<HotkeyBinding>,
        trigger_keys: &mut String,
        binding: HotkeyBinding,
    ) -> bool {
        if Self::preset_trigger_has_binding(hotkey, trigger_keys, &binding) {
            return false;
        }
        if hotkey.is_none() && trigger_keys.trim().is_empty() {
            *hotkey = Some(binding);
            true
        } else {
            hotkey::append_binding_to_list(trigger_keys, &binding)
        }
    }

    fn preset_trigger_remove_binding(
        hotkey: &mut Option<HotkeyBinding>,
        trigger_keys: &mut String,
        binding: &HotkeyBinding,
    ) -> bool {
        if hotkey
            .as_ref()
            .is_some_and(|existing| hotkey::binding_matches(existing, binding))
        {
            *hotkey = None;
            return true;
        }

        let mut removed = false;
        let mut remaining = Vec::new();
        for entry in hotkey::split_binding_list(trigger_keys) {
            let matches_binding = hotkey::parse_binding(&entry)
                .is_some_and(|existing| hotkey::binding_matches(&existing, binding));
            if !removed && matches_binding {
                removed = true;
                continue;
            }
            remaining.push(entry);
        }

        if removed {
            *trigger_keys = remaining.join(", ");
        }
        removed
    }

    fn render_preset_trigger_chips(
        ui: &mut egui::Ui,
        language: UiLanguage,
        hotkey: &mut Option<HotkeyBinding>,
        trigger_keys: &mut String,
        capture_target: Option<&CaptureRequest>,
        expected_capture_target: &CaptureRequest,
        capture_hotkey_combo_keys: Option<&Vec<String>>,
    ) -> bool {
        let bindings = Self::preset_trigger_bindings(hotkey, trigger_keys);
        let mut changed = false;
        if !bindings.is_empty() {
            let mut remove_binding = None;
            ui.horizontal(|ui| {
                for binding in &bindings {
                    let label = hotkey::format_binding(Some(binding));
                    if ui
                        .add(
                            Button::new(RichText::new(label).monospace()).min_size(vec2(0.0, 22.0)),
                        )
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Click to remove this trigger",
                            "Click to remove this trigger",
                        ))
                        .clicked()
                    {
                        remove_binding = Some(binding.clone());
                    }
                }
            });

            if let Some(binding) = remove_binding {
                changed = Self::preset_trigger_remove_binding(hotkey, trigger_keys, &binding);
            }
        }

        if let Some(target) = capture_target
            && target == expected_capture_target
            && let Some(pending) = capture_hotkey_combo_keys
        {
            let preview = Self::hotkey_binding_from_combo_keys(pending.clone());
            let label = hotkey::format_binding(Some(&preview));
            if label != "Not set" {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(
                        Button::new(RichText::new(label).monospace())
                            .min_size(vec2(0.0, 22.0))
                            .fill(Color32::from_rgba_premultiplied(72, 156, 116, 120))
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(126, 224, 182))),
                    )
                    .on_hover_text(Self::tr_lang(
                        language,
                        "Captured key preview",
                        "Captured key preview",
                    ));
                });
            }
        }

        changed
    }

    fn macro_trigger_bindings(preset: &MacroPreset) -> Vec<HotkeyBinding> {
        let mut bindings = Vec::new();
        if let Some(binding) = preset.hotkey.as_ref() {
            bindings.push(binding.clone());
        }
        for binding in hotkey::parse_binding_list(&preset.trigger_keys) {
            if !bindings
                .iter()
                .any(|existing| hotkey::binding_matches(existing, &binding))
            {
                bindings.push(binding);
            }
        }
        bindings
    }

    fn macro_trigger_has_binding(preset: &MacroPreset, binding: &HotkeyBinding) -> bool {
        Self::macro_trigger_bindings(preset)
            .iter()
            .any(|existing| hotkey::binding_matches(existing, binding))
    }

    fn macro_trigger_add_binding(preset: &mut MacroPreset, binding: HotkeyBinding) -> bool {
        if Self::macro_trigger_has_binding(preset, &binding) {
            return false;
        }
        if preset.hotkey.is_none() && preset.trigger_keys.trim().is_empty() {
            preset.hotkey = Some(binding);
            true
        } else {
            hotkey::append_binding_to_list(&mut preset.trigger_keys, &binding)
        }
    }

    fn macro_trigger_remove_last_binding(preset: &mut MacroPreset) -> bool {
        if !preset.trigger_keys.trim().is_empty() {
            return hotkey::pop_binding_list_entry(&mut preset.trigger_keys);
        }
        if preset.hotkey.is_some() {
            preset.hotkey = None;
            return true;
        }
        false
    }

    fn macro_trigger_remove_binding(preset: &mut MacroPreset, binding: &HotkeyBinding) -> bool {
        if preset
            .hotkey
            .as_ref()
            .is_some_and(|existing| hotkey::binding_matches(existing, binding))
        {
            preset.hotkey = None;
            return true;
        }

        let mut removed = false;
        let mut remaining = Vec::new();
        for entry in hotkey::split_binding_list(&preset.trigger_keys) {
            let matches_binding = hotkey::parse_binding(&entry)
                .is_some_and(|existing| hotkey::binding_matches(&existing, binding));
            if !removed && matches_binding {
                removed = true;
                continue;
            }
            remaining.push(entry);
        }

        if removed {
            preset.trigger_keys = remaining.join(", ");
        }
        removed
    }

    fn render_macro_trigger_chips(
        ui: &mut egui::Ui,
        language: UiLanguage,
        group_id: u32,
        preset: &mut MacroPreset,
        capture_target: Option<&CaptureRequest>,
        capture_hotkey_combo_keys: Option<&Vec<String>>,
    ) -> bool {
        if preset.trigger_mode == MacroTriggerMode::WindowFocus {
            let target = preset
                .event_target_window_title
                .as_deref()
                .map(Self::simplify_window_title)
                .unwrap_or_else(|| {
                    Self::tr_lang(language, "Any focused window", "Any focused window").to_owned()
                });
            ui.label(target);
            return false;
        }

        let bindings = Self::macro_trigger_bindings(preset);
        if bindings.is_empty() {
            ui.label(Self::tr_lang(language, "Not set", "Not set"));
        } else {
            let mut remove_binding = None;
            ui.horizontal_wrapped(|ui| {
                for binding in &bindings {
                    let label = hotkey::format_binding(Some(binding));
                    if ui
                        .add(
                            Button::new(RichText::new(label).monospace()).min_size(vec2(0.0, 22.0)),
                        )
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Click to remove this trigger",
                            "Click to remove this trigger",
                        ))
                        .clicked()
                    {
                        remove_binding = Some(binding.clone());
                    }
                }
            });

            if let Some(binding) = remove_binding {
                return Self::macro_trigger_remove_binding(preset, &binding);
            }
        }

        if let Some(CaptureRequest::MacroPresetHotkey(capture_group_id, capture_preset_id)) =
            capture_target
            && *capture_group_id == group_id
            && *capture_preset_id == preset.id
            && let Some(pending) = capture_hotkey_combo_keys
        {
            let preview = Self::hotkey_binding_from_combo_keys(pending.clone());
            let label = hotkey::format_binding(Some(&preview));
            if label != "Not set" {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        Button::new(RichText::new(label).monospace())
                            .min_size(vec2(0.0, 22.0))
                            .fill(Color32::from_rgba_premultiplied(72, 156, 116, 120))
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(126, 224, 182))),
                    )
                    .on_hover_text(Self::tr_lang(
                        language,
                        "Captured key preview",
                        "Captured key preview",
                    ));
                });
            }
        }

        false
    }

    fn collect_preset_referenced_variables(preset: &MacroPreset) -> Vec<String> {
        let mut vars = std::collections::HashSet::new();

        for step in &preset.steps {
            Self::collect_vars_from_step(step, &mut vars);
        }

        if preset.hold_stop_step_enabled {
            Self::collect_vars_from_step(&preset.hold_stop_step, &mut vars);
        }

        if preset.press_stop_step_enabled {
            Self::collect_vars_from_step(&preset.press_stop_step, &mut vars);
        }

        vars.retain(|var_name| !crate::overlay::is_builtin_property_name(var_name));

        let mut list: Vec<String> = vars.into_iter().collect();
        list.sort();
        list
    }

    fn format_macro_trigger_ui(language: UiLanguage, preset: &MacroPreset) -> String {
        if preset.trigger_mode == MacroTriggerMode::WindowFocus {
            let target = preset
                .event_target_window_title
                .as_deref()
                .map(Self::simplify_window_title)
                .unwrap_or_else(|| {
                    Self::tr_lang(language, "Any focused window", "Any focused window").to_owned()
                });
            return format!("Focus: {target}");
        }

        let bindings = Self::macro_trigger_bindings(preset);
        let label = hotkey::format_binding_list(&bindings);
        if label == "Not set" {
            Self::tr_lang(language, "Not set", "Not set").to_owned()
        } else {
            label
        }
    }

    fn pop_key_list_entry(spec: &mut String) -> bool {
        let mut keys = hotkey::split_key_list(spec);
        let Some(_) = keys.pop() else {
            return false;
        };
        *spec = keys.join(", ");
        true
    }

    fn short_key_chip_label(key: &str) -> String {
        match key.trim().to_ascii_uppercase().as_str() {
            "MOUSELEFT" => "LClick".to_owned(),
            "MOUSERIGHT" => "RClick".to_owned(),
            "MOUSEMIDDLE" => "MClick".to_owned(),
            "MOUSEX1" => "X1".to_owned(),
            "MOUSEX2" => "X2".to_owned(),
            "MOUSEWHEELUP" => "WheelUp".to_owned(),
            "MOUSEWHEELDOWN" => "WheelDn".to_owned(),
            "ESCAPE" => "Esc".to_owned(),
            "BACKSPACE" => "Bksp".to_owned(),
            "PAGEUP" => "PgUp".to_owned(),
            "PAGEDOWN" => "PgDn".to_owned(),
            "CONTROL" => "Ctrl".to_owned(),
            "WINDOWS" | "WIN" => "Win".to_owned(),
            other => other.to_owned(),
        }
    }

    fn render_key_list_chips(
        ui: &mut egui::Ui,
        language: UiLanguage,
        spec: &mut String,
        empty_text: &str,
    ) -> bool {
        let keys = hotkey::split_key_list(spec);
        if keys.is_empty() {
            ui.label(empty_text);
            return false;
        }

        let mut remove_index = None;
        ui.horizontal_wrapped(|ui| {
            for (index, key) in keys.iter().enumerate() {
                let label = Self::short_key_chip_label(key);
                if ui
                    .add(Button::new(RichText::new(label).monospace()).min_size(vec2(0.0, 22.0)))
                    .on_hover_text(Self::tr_lang(
                        language,
                        "Click to remove this key",
                        "Click to remove this key",
                    ))
                    .clicked()
                {
                    remove_index = Some(index);
                }
            }
        });

        if let Some(index) = remove_index {
            let mut next_keys = keys;
            next_keys.remove(index);
            *spec = next_keys.join(", ");
            true
        } else {
            false
        }
    }

    fn paint_titlebar_quick_action_icon(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        action_kind: TitlebarQuickActionKind,
        active: bool,
        icon_color: Color32,
    ) {
        match action_kind {
            TitlebarQuickActionKind::Taskbar => {
                let frame_rect = rect.shrink2(vec2(18.0, 18.0));
                let shelf_y = frame_rect.bottom() - 4.0;
                painter.rect_stroke(
                    frame_rect,
                    4.0,
                    egui::Stroke::new(1.9, icon_color),
                    StrokeKind::Inside,
                );
                painter.line_segment(
                    [
                        pos2(frame_rect.left() + 2.0, shelf_y),
                        pos2(frame_rect.right() - 2.0, shelf_y),
                    ],
                    egui::Stroke::new(1.9, icon_color),
                );
                if active {
                    let slash_rect = frame_rect.expand2(vec2(4.0, 3.0));
                    painter.line_segment(
                        [slash_rect.left_top(), slash_rect.right_bottom()],
                        egui::Stroke::new(2.0, icon_color),
                    );
                }
            }
            TitlebarQuickActionKind::WindowsKey => {
                let logo_rect = rect.shrink2(vec2(17.0, 17.0));
                let gap = 3.0;
                let tile_w = (logo_rect.width() - gap) * 0.5;
                let tile_h = (logo_rect.height() - gap) * 0.5;
                for row in 0..2 {
                    for col in 0..2 {
                        let min = pos2(
                            logo_rect.left() + col as f32 * (tile_w + gap),
                            logo_rect.top() + row as f32 * (tile_h + gap),
                        );
                        let max = pos2(min.x + tile_w, min.y + tile_h);
                        painter.rect_filled(egui::Rect::from_min_max(min, max), 1.2, icon_color);
                    }
                }
                if active {
                    let slash_rect = logo_rect.expand2(vec2(3.0, 3.0));
                    painter.line_segment(
                        [slash_rect.left_top(), slash_rect.right_bottom()],
                        egui::Stroke::new(2.0, icon_color),
                    );
                    let dot_rect = egui::Rect::from_center_size(
                        pos2(logo_rect.right() + 2.0, logo_rect.top() + 2.0),
                        vec2(6.0, 6.0),
                    );
                    painter.rect_filled(dot_rect, 3.0, icon_color);
                }
            }
            TitlebarQuickActionKind::WindowPin => {
                let center = rect.center();
                let head_rect = egui::Rect::from_center_size(
                    pos2(center.x, rect.top() + 18.0),
                    vec2(18.0, 7.0),
                );
                painter.rect_filled(head_rect, 3.0, icon_color);

                let collar_rect = egui::Rect::from_center_size(
                    pos2(center.x, head_rect.bottom() + 2.5),
                    vec2(7.0, 5.0),
                );
                painter.rect_filled(collar_rect, 2.0, icon_color);

                painter.line_segment(
                    [
                        pos2(center.x, collar_rect.bottom() - 1.0),
                        pos2(center.x, rect.bottom() - 19.0),
                    ],
                    egui::Stroke::new(2.0, icon_color),
                );

                painter.line_segment(
                    [
                        pos2(center.x, rect.bottom() - 19.0),
                        pos2(center.x - 5.5, rect.bottom() - 11.5),
                    ],
                    egui::Stroke::new(2.0, icon_color),
                );

                painter.line_segment(
                    [
                        pos2(center.x, rect.bottom() - 19.0),
                        pos2(center.x + 5.5, rect.bottom() - 11.5),
                    ],
                    egui::Stroke::new(2.0, icon_color),
                );

                painter.line_segment(
                    [
                        pos2(center.x, rect.bottom() - 19.0),
                        pos2(center.x, rect.bottom() - 7.0),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );
            }
            TitlebarQuickActionKind::FocusHighlight => {
                let frame_rect = rect.shrink2(vec2(16.0, 16.0));
                painter.rect_stroke(
                    frame_rect,
                    5.0,
                    egui::Stroke::new(2.0, icon_color),
                    StrokeKind::Inside,
                );

                let corner = 8.0;
                for (start, end) in [
                    (
                        frame_rect.left_top(),
                        pos2(frame_rect.left() + corner, frame_rect.top()),
                    ),
                    (
                        frame_rect.left_top(),
                        pos2(frame_rect.left(), frame_rect.top() + corner),
                    ),
                    (
                        frame_rect.right_top(),
                        pos2(frame_rect.right() - corner, frame_rect.top()),
                    ),
                    (
                        frame_rect.right_top(),
                        pos2(frame_rect.right(), frame_rect.top() + corner),
                    ),
                    (
                        frame_rect.left_bottom(),
                        pos2(frame_rect.left() + corner, frame_rect.bottom()),
                    ),
                    (
                        frame_rect.left_bottom(),
                        pos2(frame_rect.left(), frame_rect.bottom() - corner),
                    ),
                    (
                        frame_rect.right_bottom(),
                        pos2(frame_rect.right() - corner, frame_rect.bottom()),
                    ),
                    (
                        frame_rect.right_bottom(),
                        pos2(frame_rect.right(), frame_rect.bottom() - corner),
                    ),
                ] {
                    painter.line_segment([start, end], egui::Stroke::new(2.7, icon_color));
                }

                if active {
                    painter.circle_filled(rect.center(), 4.0, icon_color);
                }
            }
            TitlebarQuickActionKind::FocusMode => {
                let outer = rect.shrink2(vec2(14.0, 14.0));
                painter.rect_stroke(
                    outer,
                    3.0,
                    egui::Stroke::new(2.0, icon_color.gamma_multiply(0.55)),
                    StrokeKind::Inside,
                );
                let focus = egui::Rect::from_center_size(rect.center(), vec2(24.0, 18.0));
                painter.rect_filled(focus, 2.0, icon_color.gamma_multiply(0.18));
                painter.rect_stroke(
                    focus,
                    2.0,
                    egui::Stroke::new(2.0, icon_color),
                    StrokeKind::Inside,
                );
                if active {
                    painter.circle_filled(focus.center(), 3.0, icon_color);
                }
            }
            TitlebarQuickActionKind::WindowOpacity => {
                let outer = rect.shrink2(vec2(15.0, 15.0));
                painter.rect_stroke(
                    outer,
                    4.0,
                    egui::Stroke::new(2.0, icon_color),
                    StrokeKind::Inside,
                );
                painter.rect_filled(
                    egui::Rect::from_min_max(outer.left_top(), outer.center_bottom()),
                    3.0,
                    icon_color.gamma_multiply(if active { 0.65 } else { 0.25 }),
                );
            }
            TitlebarQuickActionKind::Protractor => {
                let center = rect.center();
                let radius = 11.0;
                painter.circle_stroke(center, radius, egui::Stroke::new(1.8, icon_color));
                painter.line_segment(
                    [
                        pos2(center.x - radius, center.y),
                        pos2(center.x + radius, center.y),
                    ],
                    egui::Stroke::new(1.2, icon_color),
                );
                let rad = (-45.0_f32).to_radians();
                painter.line_segment(
                    [
                        center,
                        pos2(center.x + radius * rad.cos(), center.y + radius * rad.sin()),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );
            }
            TitlebarQuickActionKind::Ruler => {
                let start = pos2(rect.left() + 18.0, rect.bottom() - 20.0);
                let end = pos2(rect.right() - 18.0, rect.top() + 20.0);
                painter.line_segment([start, end], egui::Stroke::new(2.2, icon_color));
                painter.circle_filled(start, 3.5, icon_color);
                painter.circle_filled(end, 3.5, icon_color);
                let tick_dir = vec2(-0.6, -0.8);
                for offset in [0.22_f32, 0.42, 0.62, 0.82] {
                    let point = start.lerp(end, offset);
                    let tick_start = point + tick_dir * 4.0;
                    let tick_end = point - tick_dir * 4.0;
                    painter
                        .line_segment([tick_start, tick_end], egui::Stroke::new(1.5, icon_color));
                }
            }
            TitlebarQuickActionKind::GetCoordinates => {
                let center = rect.center();
                let radius = 10.0;
                painter.circle_stroke(center, radius, egui::Stroke::new(1.8, icon_color));
                painter.circle_filled(center, 2.0, icon_color);
                painter.line_segment(
                    [
                        pos2(center.x - 15.0, center.y),
                        pos2(center.x - 7.0, center.y),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );
                painter.line_segment(
                    [
                        pos2(center.x + 7.0, center.y),
                        pos2(center.x + 15.0, center.y),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );
                painter.line_segment(
                    [
                        pos2(center.x, center.y - 15.0),
                        pos2(center.x, center.y - 7.0),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );
                painter.line_segment(
                    [
                        pos2(center.x, center.y + 7.0),
                        pos2(center.x, center.y + 15.0),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );
            }
            TitlebarQuickActionKind::GetColor => {
                let center = rect.center();
                let start = pos2(center.x - 7.0, center.y + 7.0);
                let end = pos2(center.x + 7.0, center.y - 7.0);
                painter.line_segment([start, end], egui::Stroke::new(3.5, icon_color));
                painter.line_segment(
                    [start, pos2(start.x - 3.0, start.y + 3.0)],
                    egui::Stroke::new(1.8, icon_color),
                );
                painter.circle_filled(pos2(end.x + 2.0, end.y - 2.0), 4.5, icon_color);
            }
            TitlebarQuickActionKind::KeyDisplay => {
                let key_shadow_rect =
                    egui::Rect::from_center_size(rect.center() + vec2(0.0, 3.0), vec2(50.0, 28.0));
                let key_rect =
                    egui::Rect::from_center_size(rect.center() + vec2(0.0, 0.5), vec2(50.0, 26.0));
                let top_glow_rect = egui::Rect::from_min_max(
                    pos2(key_rect.left() + 3.0, key_rect.top() + 3.0),
                    pos2(key_rect.right() - 3.0, key_rect.center().y + 2.0),
                );
                painter.rect_filled(key_shadow_rect, 10.0, icon_color.gamma_multiply(0.28));
                painter.rect_filled(
                    key_rect,
                    10.0,
                    Color32::from_rgba_premultiplied(255, 255, 255, 32),
                );
                painter.rect_filled(
                    top_glow_rect,
                    7.0,
                    Color32::from_rgba_premultiplied(255, 255, 255, 24),
                );
                painter.rect_stroke(
                    key_rect,
                    10.0,
                    egui::Stroke::new(1.8, icon_color),
                    StrokeKind::Inside,
                );
                painter.text(
                    key_rect.center() + vec2(0.0, -0.5),
                    egui::Align2::CENTER_CENTER,
                    "A",
                    egui::FontId::proportional(15.5),
                    icon_color,
                );
            }
            TitlebarQuickActionKind::ScreenDraw => {
                let start = pos2(rect.left() + 17.0, rect.center().y + 5.0);
                let mid = pos2(rect.center().x - 1.0, rect.center().y - 6.0);
                let end = pos2(rect.right() - 15.0, rect.center().y + 1.0);
                painter.line_segment([start, mid], egui::Stroke::new(3.0, icon_color));
                painter.line_segment([mid, end], egui::Stroke::new(3.0, icon_color));
                painter.circle_filled(end, 4.0, icon_color);
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        pos2(rect.left() + 14.0, rect.top() + 13.0),
                        vec2(12.0, 7.0),
                    ),
                    2.0,
                    icon_color,
                );
            }
            TitlebarQuickActionKind::VideoRecord => {
                let body = egui::Rect::from_center_size(rect.center(), vec2(28.0, 20.0));
                painter.rect_stroke(
                    body,
                    3.0,
                    egui::Stroke::new(2.0, icon_color),
                    StrokeKind::Inside,
                );
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        pos2(body.right() + 1.0, body.top() + 4.0),
                        pos2(body.right() + 8.0, body.center().y),
                        pos2(body.right() + 1.0, body.bottom() - 4.0),
                    ],
                    icon_color,
                    egui::Stroke::NONE,
                ));
                painter.circle_filled(body.center(), 3.5, icon_color);
            }
            TitlebarQuickActionKind::ClearOverlays => {
                let center = rect.center();

                // Bottom-right layer
                let lay1 = egui::Rect::from_center_size(center + vec2(4.0, -4.0), vec2(16.0, 16.0));
                painter.rect_stroke(
                    lay1,
                    2.0,
                    egui::Stroke::new(1.2, icon_color.gamma_multiply(0.5)),
                    StrokeKind::Inside,
                );

                // Top-left layer
                let lay2 = egui::Rect::from_center_size(center + vec2(-4.0, 4.0), vec2(16.0, 16.0));
                // Fill behind with dark background to hide overlapping lines of lay1
                painter.rect_filled(lay2, 2.0, Color32::from_rgba_premultiplied(15, 23, 42, 240));
                painter.rect_stroke(
                    lay2,
                    2.0,
                    egui::Stroke::new(1.8, icon_color),
                    StrokeKind::Inside,
                );

                // A diagonal slash line representing "clear" or "prohibit"
                let slash_start = pos2(center.x + 11.0, center.y - 11.0);
                let slash_end = pos2(center.x - 11.0, center.y + 11.0);

                // Draw slash with slightly thicker stroke
                painter.line_segment([slash_start, slash_end], egui::Stroke::new(2.4, icon_color));
            }
            TitlebarQuickActionKind::KeySound => {
                let center = rect.center();
                let body_rect =
                    egui::Rect::from_center_size(center + vec2(-4.0, 0.0), vec2(6.0, 8.0));
                painter.rect_filled(body_rect, 1.0, icon_color);

                let p1 = pos2(center.x - 2.0, center.y - 4.0);
                let p2 = pos2(center.x + 3.0, center.y - 10.0);
                let p3 = pos2(center.x + 3.0, center.y + 10.0);
                let p4 = pos2(center.x - 2.0, center.y + 4.0);
                painter.add(egui::Shape::convex_polygon(
                    vec![p1, p2, p3, p4],
                    icon_color,
                    egui::Stroke::NONE,
                ));

                let r1 = 7.0;
                painter.line_segment(
                    [
                        pos2(center.x + 6.0, center.y - r1 * 0.7),
                        pos2(center.x + 8.0, center.y),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );
                painter.line_segment(
                    [
                        pos2(center.x + 8.0, center.y),
                        pos2(center.x + 6.0, center.y + r1 * 0.7),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );

                let r2 = 12.0;
                painter.line_segment(
                    [
                        pos2(center.x + 10.0, center.y - r2 * 0.7),
                        pos2(center.x + 13.0, center.y),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );
                painter.line_segment(
                    [
                        pos2(center.x + 13.0, center.y),
                        pos2(center.x + 10.0, center.y + r2 * 0.7),
                    ],
                    egui::Stroke::new(1.8, icon_color),
                );
            }
        }
    }

    fn titlebar_quick_action_button(
        &self,
        ui: &mut egui::Ui,
        action_kind: TitlebarQuickActionKind,
        active: bool,
    ) -> egui::Response {
        let button_size = vec2(96.0, 66.0);
        let corner_radius = 14.0;
        let (frame_fill, frame_stroke, face_fill, face_bottom_fill, face_border, icon_color) =
            match (self.state.ui_theme, active) {
                (UiThemeMode::Dark, true) => (
                    Color32::from_rgb(57, 72, 96),
                    Color32::from_rgb(92, 110, 138),
                    Color32::from_rgb(117, 219, 166),
                    Color32::from_rgb(82, 180, 132),
                    Color32::from_rgb(232, 250, 240),
                    Color32::from_rgb(246, 252, 248),
                ),
                (UiThemeMode::Dark, false) => (
                    Color32::from_rgb(57, 72, 96),
                    Color32::from_rgb(92, 110, 138),
                    Color32::from_rgb(128, 151, 198),
                    Color32::from_rgb(88, 112, 160),
                    Color32::from_rgb(234, 242, 252),
                    Color32::from_rgb(244, 248, 252),
                ),
                (UiThemeMode::Light, true) => (
                    Color32::from_rgb(181, 192, 206),
                    Color32::from_rgb(116, 130, 152),
                    Color32::from_rgb(118, 214, 160),
                    Color32::from_rgb(72, 168, 118),
                    Color32::from_rgb(248, 252, 250),
                    Color32::from_rgb(248, 252, 250),
                ),
                (UiThemeMode::Light, false) => (
                    Color32::from_rgb(181, 192, 206),
                    Color32::from_rgb(116, 130, 152),
                    Color32::from_rgb(122, 164, 218),
                    Color32::from_rgb(58, 120, 188),
                    Color32::from_rgb(244, 248, 252),
                    Color32::from_rgb(248, 250, 252),
                ),
            };
        let (outer_rect, response) = ui.allocate_exact_size(button_size, Sense::click());
        let hovered = response.hovered();
        let pressed = response.is_pointer_button_down_on();
        let rest_offset = if active { 2.0 } else { 0.0 };
        let press_offset = if pressed {
            1.5
        } else if hovered {
            0.5
        } else {
            0.0
        };
        let face_offset_y = rest_offset + press_offset;
        let base_rect = outer_rect.shrink2(vec2(2.0, 3.0));
        let face_rect = egui::Rect::from_min_max(
            pos2(
                base_rect.left() + 2.0,
                base_rect.top() + 2.0 + face_offset_y,
            ),
            pos2(
                base_rect.right() - 2.0,
                base_rect.bottom() - 6.0 + face_offset_y,
            ),
        );
        let face_bottom_rect = egui::Rect::from_min_max(
            pos2(face_rect.left(), face_rect.bottom() - 9.0),
            face_rect.right_bottom(),
        );
        ui.painter()
            .rect_filled(base_rect, corner_radius, frame_fill);
        ui.painter().rect_stroke(
            base_rect,
            corner_radius,
            egui::Stroke::new(1.2, frame_stroke),
            StrokeKind::Inside,
        );
        ui.painter()
            .rect_filled(face_rect, corner_radius - 3.0, face_fill);
        ui.painter()
            .rect_filled(face_bottom_rect, corner_radius - 3.0, face_bottom_fill);
        ui.painter().rect_stroke(
            face_rect,
            corner_radius - 3.0,
            egui::Stroke::new(1.2, face_border),
            StrokeKind::Inside,
        );
        self.paint_titlebar_quick_action_icon(
            ui.painter(),
            face_rect,
            action_kind,
            active,
            icon_color,
        );
        response
    }

    fn set_quick_action_window_pinned(&mut self, selector: &str, pinned: bool) -> bool {
        let success = window_list::set_window_topmost(selector, pinned);
        if success {
            if pinned {
                self.quick_action_pinned_windows.insert(selector.to_owned());
            } else {
                self.quick_action_pinned_windows.remove(selector);
            }
        } else if !pinned {
            self.quick_action_pinned_windows.remove(selector);
        }
        success
    }

    fn unpin_all_quick_action_windows(&mut self) {
        let selectors = self.quick_action_pinned_windows.drain().collect::<Vec<_>>();
        for selector in selectors {
            let _ = window_list::set_window_topmost(&selector, false);
        }
    }

    fn render_titlebar_quick_actions_grid(
        &mut self,
        ui: &mut egui::Ui,
        taskbar_hidden: bool,
    ) -> bool {
        self.prime_open_windows_if_empty();
        self.sync_quick_action_window_selection();
        let pinned_window_active = !self.quick_action_pinned_windows.is_empty();
        let macro_visual_overlay_active = crate::overlay::has_active_macro_visual_overlay();
        let mut keep_menu_open = false;
        // Reset hover-card visibility flag each frame before render_popup calls
        let qa_hover_card_key = egui::Id::new("qa-hover-card-visible");
        ui.ctx().data_mut(|data| {
            data.remove_temp::<bool>(qa_hover_card_key);
        });
        let action_width = 104.0;
        let action_height = 100.0;
        let current_time = ui.input(|i| i.time);

        // Helper to render interactive settings popup on hover
        let render_popup =
            |ui: &mut egui::Ui,
             button_response: &egui::Response,
             action_kind: TitlebarQuickActionKind,
             draw_controls: &mut dyn FnMut(&mut egui::Ui) -> bool| {
                let popup_id = ui.make_persistent_id(format!("qa-popup-state-{:?}", action_kind));

                // Persistent state IDs
                let active_qa_id = ui.make_persistent_id("active-quick-action-popup");
                let active_qa_time_id = ui.make_persistent_id("active-quick-action-popup-time");
                // Track whether a sub-popup was open in the previous frame to give a one-frame buffer
                let popup_was_open_id =
                    ui.make_persistent_id(format!("qa-popup-was-open-{:?}", action_kind));

                let mut active_qa = ui
                    .ctx()
                    .data(|data| data.get_temp::<TitlebarQuickActionKind>(active_qa_id));
                let mut last_active_time = ui
                    .ctx()
                    .data(|data| data.get_temp::<f64>(active_qa_time_id))
                    .unwrap_or(0.0);
                let popup_was_open_prev = ui
                    .ctx()
                    .data(|data| data.get_temp::<bool>(popup_was_open_id))
                    .unwrap_or(false);

                let is_button_hovered = button_response.hovered();

                // If the button is hovered, this action kind becomes the active one immediately
                if is_button_hovered {
                    last_active_time = current_time;
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(active_qa_time_id, current_time);
                    });
                    if active_qa != Some(action_kind) {
                        active_qa = Some(action_kind);
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(active_qa_id, action_kind);
                        });
                    }
                }

                // Keep open if user is actively dragging or has a combobox/sub-popup open (current or previous frame)
                let is_dragging = ui.ctx().dragged_id().is_some();
                let is_any_popup_open = egui::Popup::is_any_open(ui.ctx());

                // Read the card rect from the previous frame to check if mouse is inside the card.
                // This ensures the card stays alive on the frame the user clicks a ComboBox inside it
                // (before the ComboBox popup gets a chance to open and make is_any_popup_open true).
                let card_rect_id = ui.make_persistent_id(format!("qa-card-rect-{:?}", action_kind));
                let prev_card_rect = ui
                    .ctx()
                    .data(|d| d.get_temp::<egui::Rect>(card_rect_id))
                    .unwrap_or(egui::Rect::NOTHING);
                let mouse_in_card_prev =
                    if let Some(mouse_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                        prev_card_rect.contains(mouse_pos)
                    } else {
                        false
                    };
                let clicked_outside = active_qa == Some(action_kind)
                    && ui
                        .ctx()
                        .input(|input| input.pointer.press_origin())
                        .is_some_and(|position| {
                            let in_child_popup = is_any_popup_open
                                && ui
                                    .ctx()
                                    .layer_id_at(position)
                                    .is_some_and(|layer| layer.order == egui::Order::Foreground);
                            !button_response.rect.contains(position)
                                && !prev_card_rect.contains(position)
                                && !in_child_popup
                        });
                if clicked_outside {
                    active_qa = None;
                    last_active_time = 0.0;
                    ui.ctx().data_mut(|data| {
                        data.remove_temp::<TitlebarQuickActionKind>(active_qa_id);
                        data.remove_temp::<f64>(active_qa_time_id);
                    });
                }
                let is_active = active_qa == Some(action_kind);

                // One-frame buffer: if popup was open last frame, treat this frame as interacting too
                let is_interacting = is_active
                    && (is_dragging
                        || is_any_popup_open
                        || popup_was_open_prev
                        || mouse_in_card_prev);

                if is_interacting {
                    last_active_time = current_time;
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(active_qa_time_id, current_time);
                    });
                }

                // Store popup open state for next frame
                ui.ctx().data_mut(|data| {
                    data.insert_temp(popup_was_open_id, is_any_popup_open);
                });

                let time_since_active = current_time - last_active_time;
                let should_show = is_active && (time_since_active < 0.25 || is_interacting);

                if should_show {
                    // Mark that a hover card is visible this frame so the outer panel stays open
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(qa_hover_card_key, true);
                    });
                    let opens_up = matches!(
                        action_kind,
                        TitlebarQuickActionKind::Taskbar
                            | TitlebarQuickActionKind::WindowsKey
                            | TitlebarQuickActionKind::WindowPin
                            | TitlebarQuickActionKind::FocusHighlight
                            | TitlebarQuickActionKind::FocusMode
                            | TitlebarQuickActionKind::WindowOpacity
                            | TitlebarQuickActionKind::Protractor
                            | TitlebarQuickActionKind::Ruler
                    );
                    let (pos, pivot) = if opens_up {
                        (
                            button_response.rect.left_top() + vec2(-42.0, -4.0),
                            egui::Align2::LEFT_BOTTOM,
                        )
                    } else {
                        (
                            button_response.rect.left_bottom() + vec2(-42.0, 4.0),
                            egui::Align2::LEFT_TOP,
                        )
                    };
                    let parent_layer = ui.layer_id();
                    let popup_layer = egui::LayerId::new(egui::Order::Foreground, popup_id);
                    let mut content_rect = egui::Rect::NOTHING;
                    let area_response = egui::Area::new(popup_id)
                        .order(egui::Order::Foreground)
                        .pivot(pivot)
                        .fixed_pos(pos)
                        .show(ui.ctx(), |ui| {
                            let frame_response = egui::Frame::popup(ui.style())
                                .rounding(8.0)
                                .inner_margin(8.0)
                                .stroke(egui::Stroke::new(1.0, ui.visuals().window_stroke.color))
                                .show(ui, |ui| {
                                    let font = egui::FontId::proportional(12.0);
                                    ui.style_mut()
                                        .text_styles
                                        .insert(egui::TextStyle::Body, font.clone());
                                    ui.style_mut()
                                        .text_styles
                                        .insert(egui::TextStyle::Button, font.clone());
                                    ui.style_mut()
                                        .text_styles
                                        .insert(egui::TextStyle::Small, font);
                                    ui.set_width(180.0);
                                    ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                                    draw_controls(ui)
                                });
                            content_rect = frame_response.response.rect;
                            frame_response.inner
                        });
                    ui.ctx().set_sublayer(parent_layer, popup_layer);
                    ui.ctx().move_to_top(popup_layer);

                    // Persist card rect for next frame's mouse-in-card check
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(card_rect_id, content_rect));

                    // Keep active if pointer is over the card, any sub-popup is open, or inner controls returned true
                    let mouse_in_card =
                        if let Some(mouse_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                            content_rect.contains(mouse_pos)
                        } else {
                            false
                        };
                    let is_popup_hovered =
                        mouse_in_card || is_any_popup_open || area_response.inner;
                    if is_popup_hovered {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(active_qa_time_id, current_time);
                        });
                    }
                } else {
                    // Clear stored card rect so it doesn't keep the popup alive after close
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(card_rect_id, egui::Rect::NOTHING));
                    // If this was the active action but the decay expired, clear it
                    if is_active {
                        ui.ctx().data_mut(|data| {
                            data.remove_temp::<TitlebarQuickActionKind>(active_qa_id);
                        });
                    }
                }
            };

        Grid::new("titlebar-quick-actions-grid")
            .num_columns(6)
            .spacing([16.0, 12.0])
            .show(ui, |ui| {
                // Taskbar Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::Taskbar,
                            taskbar_hidden,
                        );
                        if button_response.clicked() {
                            let success = if taskbar_hidden {
                                crate::platform::show_taskbar()
                            } else {
                                crate::platform::hide_taskbar()
                            };
                            self.status = if success {
                                if taskbar_hidden {
                                    Self::tr_lang(
                                        self.state.ui_language,
                                        "Windows taskbar restored.",
                                        "Windows taskbar restored.",
                                    )
                                } else {
                                    Self::tr_lang(
                                        self.state.ui_language,
                                        "Windows taskbar hidden.",
                                        "Windows taskbar hidden.",
                                    )
                                }
                            } else if taskbar_hidden {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Failed to restore the Windows taskbar.",
                                    "Failed to restore the Windows taskbar.",
                                )
                            } else {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Failed to hide the Windows taskbar.",
                                    "Failed to hide the Windows taskbar.",
                                )
                            }
                            .to_owned();
                        }

                        ui.add_space(6.0);
                        let taskbar_label = if taskbar_hidden {
                            Self::tr_lang(self.state.ui_language, "Show taskbar", "Show taskbar")
                        } else {
                            Self::tr_lang(self.state.ui_language, "Hide taskbar", "Hide taskbar")
                        };
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(taskbar_label).size(11.0).color(
                                            if button_response.hovered() {
                                                ui.visuals().strong_text_color()
                                            } else {
                                                ui.visuals().text_color()
                                            },
                                        ),
                                    )
                                    .wrap(),
                                );
                            },
                        );
                    },
                );

                // WindowsKey Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::WindowsKey,
                            self.state.windows_key_locked,
                        );
                        if button_response.clicked() {
                            self.state.windows_key_locked = !self.state.windows_key_locked;
                            self.sync_windows_key_locked();
                            self.persist();
                            self.status = if self.state.windows_key_locked {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Windows key locked.",
                                    "Windows key locked.",
                                )
                            } else {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Windows key unlocked.",
                                    "Windows key unlocked.",
                                )
                            }
                            .to_owned();
                        }

                        ui.add_space(6.0);
                        let windows_label = if self.state.windows_key_locked {
                            Self::tr_lang(
                                self.state.ui_language,
                                "Unlock Windows key",
                                "Unlock Windows key",
                            )
                        } else {
                            Self::tr_lang(
                                self.state.ui_language,
                                "Lock Windows key",
                                "Lock Windows key",
                            )
                        };
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(windows_label).size(11.0).color(
                                            if button_response.hovered() {
                                                ui.visuals().strong_text_color()
                                            } else {
                                                ui.visuals().text_color()
                                            },
                                        ),
                                    )
                                    .wrap(),
                                );
                            },
                        );
                    },
                );

                // WindowPin Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::WindowPin,
                            pinned_window_active,
                        );
                        if button_response.clicked() {
                            if pinned_window_active {
                                self.unpin_all_quick_action_windows();
                                self.status = Self::tr_lang(
                                    self.state.ui_language,
                                    "Unpinned all selected windows.",
                                    "Đã bỏ ghim tất cả cửa sổ đã chọn.",
                                )
                                .to_owned();
                            } else {
                                self.status = Self::tr_lang(
                                    self.state.ui_language,
                                    "Select windows from the dropdown to pin them.",
                                    "Chọn cửa sổ trong danh sách để ghim.",
                                )
                                .to_owned();
                            }
                        }

                        ui.add_space(6.0);
                        let pin_label = Self::truncate_window_title(
                            if pinned_window_active {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Unpin all",
                                    "Bỏ ghim tất cả",
                                )
                            } else {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Pin windows",
                                    "Ghim cửa sổ",
                                )
                            },
                            14,
                        );
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(pin_label).size(11.0).color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                ));
                            },
                        );

                        // Popup settings
                        let mut keep_open = false;
                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::WindowPin,
                            &mut |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Target window",
                                            "Cửa sổ mục tiêu",
                                        ))
                                        .size(12.0),
                                    );

                                    let selected_window_text = if pinned_window_active {
                                        format!(
                                            "{} ({})",
                                            Self::tr_lang(
                                                self.state.ui_language,
                                                "Selected",
                                                "Đã chọn",
                                            ),
                                            self.quick_action_pinned_windows.len()
                                        )
                                    } else {
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Select windows",
                                            "Chọn cửa sổ",
                                        )
                                        .to_owned()
                                    };

                                    let selector_popup_id =
                                        ui.make_persistent_id("quick-action-window-selector-popup");
                                    let mut selector_popup_open = ui
                                        .ctx()
                                        .data(|data| data.get_temp::<bool>(selector_popup_id))
                                        .unwrap_or(false);

                                    let selector_button = Button::new(
                                        RichText::new(format!("{selected_window_text}  v"))
                                            .size(12.0),
                                    )
                                    .fill(Color32::from_rgba_premultiplied(60, 60, 60, 220));
                                    let selector_response =
                                        ui.add_sized([164.0, 22.0], selector_button);
                                    if selector_response.clicked() {
                                        selector_popup_open = !selector_popup_open;
                                        if selector_popup_open {
                                            self.ensure_open_windows_ready(true);
                                        }
                                    }

                                    let selector_popup_result =
                                        egui::Popup::from_response(&selector_response)
                                            .id(selector_popup_id)
                                            .open_bool(&mut selector_popup_open)
                                            .close_behavior(
                                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                            )
                                            .align(egui::RectAlign::BOTTOM_START)
                                            .width(164.0)
                                            .show(|ui| {
                                                ui.set_min_width(164.0);
                                                ui.set_max_width(164.0);
                                                let windows = self
                                                    .open_window_infos
                                                    .iter()
                                                    .map(|window| {
                                                        (
                                                            window.selector.clone(),
                                                            window.title.clone(),
                                                        )
                                                    })
                                                    .collect::<Vec<_>>();
                                                for (selector, title) in windows {
                                                    let display_title =
                                                        Self::quick_action_window_display(
                                                            &selector,
                                                            &self.open_window_infos,
                                                        );
                                                    let truncated_title =
                                                        Self::truncate_window_title(
                                                            &display_title,
                                                            20,
                                                        );
                                                    let mut selected = self
                                                        .quick_action_pinned_windows
                                                        .contains(&selector);
                                                    let response = ui.checkbox(
                                                        &mut selected,
                                                        truncated_title,
                                                    );
                                                    if response.changed()
                                                        && !self.set_quick_action_window_pinned(
                                                            &selector,
                                                            selected,
                                                        )
                                                    {
                                                        self.status = Self::tr_lang(
                                                            self.state.ui_language,
                                                            "Could not update the selected window.",
                                                            "Không thể cập nhật cửa sổ đã chọn.",
                                                        )
                                                        .to_owned();
                                                    }
                                                    response.on_hover_text(title);
                                                }
                                            });
                                    let _ = selector_popup_result;
                                    if selector_popup_open {
                                        keep_open = true;
                                    }
                                    ui.ctx().data_mut(|data| {
                                        data.insert_temp(selector_popup_id, selector_popup_open);
                                    });
                                    selector_popup_open
                                })
                                .inner
                            },
                        );
                        if keep_open {
                            keep_menu_open = true;
                        }
                    },
                );

                // FocusHighlight Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::FocusHighlight,
                            self.state.native_focus_highlight_enabled,
                        );
                        if button_response.clicked() {
                            self.state.native_focus_highlight_enabled =
                                !self.state.native_focus_highlight_enabled;
                            self.sync_native_focus_highlight_enabled();
                            self.persist();
                            self.status = if self.state.native_focus_highlight_enabled {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Native focus highlight enabled.",
                                    "Native focus highlight enabled.",
                                )
                            } else {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Native focus highlight disabled.",
                                    "Native focus highlight disabled.",
                                )
                            }
                            .to_owned();
                        }

                        ui.add_space(6.0);
                        let focus_label = Self::tr_lang(
                            self.state.ui_language,
                            "Focus highlight",
                            "Focus highlight",
                        );
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(focus_label).size(11.0).color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                ));
                            },
                        );

                        // Popup settings
                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::FocusHighlight,
                            &mut |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Color",
                                                "Màu",
                                            ))
                                            .size(12.0),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let color_changed = Self::edit_rgba_color(
                                                    ui,
                                                    &mut self.state.focus_highlight_color,
                                                )
                                                .changed();
                                                if color_changed {
                                                    self.sync_focus_highlight_config();
                                                    self.persist_deferred(ui.ctx());
                                                }
                                            },
                                        );
                                    });

                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Decoration",
                                            "Trang trí",
                                        ))
                                        .size(12.0),
                                    );

                                    let selected_text = match self.state.focus_highlight_decoration
                                    {
                                        FocusHighlightDecoration::Plain => {
                                            Self::tr_lang(
                                                self.state.ui_language,
                                                "Plain (Native / Smooth)",
                                                "Đơn giản (Native / mượt)",
                                            )
                                        }
                                        FocusHighlightDecoration::Rainbow => Self::tr_lang(
                                            self.state.ui_language,
                                            "Rainbow Frame",
                                            "Rainbow Frame",
                                        ),
                                        FocusHighlightDecoration::FloralWood => Self::tr_lang(
                                            self.state.ui_language,
                                            "Floral Wood",
                                            "Floral Wood",
                                        ),
                                    };

                                    let decoration_changed =
                                        egui::ComboBox::from_id_salt("focus-highlight-decoration")
                                            .width(164.0)
                                            .selected_text(selected_text)
                                            .show_ui(ui, |ui| {
                                                let mut changed = false;
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut self.state.focus_highlight_decoration,
                                                        FocusHighlightDecoration::Plain,
                                                        Self::tr_lang(
                                                            self.state.ui_language,
                                                            "Plain (Native / Smooth)",
                                                            "Đơn giản (Native / mượt)",
                                                        ),
                                                    )
                                                    .clicked();
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut self.state.focus_highlight_decoration,
                                                        FocusHighlightDecoration::Rainbow,
                                                        Self::tr_lang(
                                                            self.state.ui_language,
                                                            "Rainbow Frame",
                                                            "Rainbow Frame",
                                                        ),
                                                    )
                                                    .clicked();
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut self.state.focus_highlight_decoration,
                                                        FocusHighlightDecoration::FloralWood,
                                                        Self::tr_lang(
                                                            self.state.ui_language,
                                                            "Floral Wood",
                                                            "Floral Wood",
                                                        ),
                                                    )
                                                    .clicked();
                                                changed
                                            })
                                            .inner
                                            .unwrap_or(false);
                                    if decoration_changed {
                                        self.sync_focus_highlight_config();
                                        self.persist();
                                    }
                                    false
                                })
                                .inner
                            },
                        );
                    },
                );

                // FocusMode Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::FocusMode,
                            self.state.focus_mode_enabled,
                        );
                        if button_response.clicked() {
                            self.state.focus_mode_enabled = !self.state.focus_mode_enabled;
                            self.sync_focus_mode_config();
                            self.persist();
                        }

                        ui.add_space(6.0);
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(Self::tr_lang(
                                        self.state.ui_language,
                                        "Focus mode",
                                        "Chế độ tập trung",
                                    ))
                                    .size(11.0)
                                    .color(if button_response.hovered() {
                                        ui.visuals().strong_text_color()
                                    } else {
                                        ui.visuals().text_color()
                                    }),
                                ));
                            },
                        );

                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::FocusMode,
                            &mut |ui| {
                                ui.set_min_width(190.0);
                                let follow_changed = ui
                                    .checkbox(
                                        &mut self.state.focus_mode_follow_focused_window,
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Focused window",
                                            "Cửa sổ đang focus",
                                        ),
                                    )
                                    .changed();
                                if follow_changed {
                                    self.sync_focus_mode_config();
                                    self.persist();
                                }

                                if !self.state.focus_mode_follow_focused_window {
                                    let selected_text = if self
                                        .state
                                        .focus_mode_target_window
                                        .is_empty()
                                    {
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Select window",
                                            "Chọn cửa sổ",
                                        )
                                        .to_owned()
                                    } else {
                                        Self::truncate_window_title(
                                            &Self::quick_action_window_display(
                                                &self.state.focus_mode_target_window,
                                                &self.open_window_infos,
                                            ),
                                            22,
                                        )
                                    };
                                    let response = egui::ComboBox::from_id_salt(
                                        "focus-mode-target-window",
                                    )
                                    .width(180.0)
                                    .selected_text(selected_text)
                                    .show_ui(ui, |ui| {
                                        let mut changed = false;
                                        for window in &self.open_window_infos {
                                            changed |= ui
                                                .selectable_value(
                                                    &mut self.state.focus_mode_target_window,
                                                    window.selector.clone(),
                                                    Self::truncate_window_title(
                                                        &Self::quick_action_window_display(
                                                            &window.selector,
                                                            &self.open_window_infos,
                                                        ),
                                                        24,
                                                    ),
                                                )
                                                .clicked();
                                        }
                                        changed
                                    });
                                    if response.response.clicked() {
                                        self.ensure_open_windows_ready(true);
                                    }
                                    if response.inner.unwrap_or(false) {
                                        self.sync_focus_mode_config();
                                        self.persist();
                                    }
                                }

                                ui.add_space(4.0);
                                let dim_changed = ui
                                    .add(
                                        egui::Slider::new(
                                            &mut self.state.focus_mode_dim_percent,
                                            0..=100,
                                        )
                                        .text(Self::tr_lang(
                                            self.state.ui_language,
                                            "Dim",
                                            "Độ tối",
                                        ))
                                        .suffix("%"),
                                    )
                                    .changed();
                                if dim_changed {
                                    self.sync_focus_mode_config();
                                    self.persist_deferred(ui.ctx());
                                }

                                let taskbar_changed = ui
                                    .checkbox(
                                        &mut self.state.focus_mode_include_taskbar,
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Include taskbar",
                                            "Làm tối cả taskbar",
                                        ),
                                    )
                                    .changed();
                                if taskbar_changed {
                                    self.sync_focus_mode_config();
                                    self.persist();
                                }
                                false
                            },
                        );
                    },
                );

                // Window opacity Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::WindowOpacity,
                            self.state.window_opacity_enabled,
                        );
                        if button_response.clicked() {
                            self.state.window_opacity_enabled =
                                !self.state.window_opacity_enabled;
                            self.sync_window_opacity_config();
                            self.persist();
                        }

                        ui.add_space(6.0);
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(Self::tr_lang(
                                        self.state.ui_language,
                                        "Window opacity",
                                        "Độ trong suốt",
                                    ))
                                    .size(11.0)
                                    .color(if button_response.hovered() {
                                        ui.visuals().strong_text_color()
                                    } else {
                                        ui.visuals().text_color()
                                    }),
                                ));
                            },
                        );

                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::WindowOpacity,
                            &mut |ui| {
                                ui.set_min_width(190.0);
                                if ui
                                    .checkbox(
                                        &mut self.state.window_opacity_follow_focused_window,
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Focused window",
                                            "Cửa sổ đang focus",
                                        ),
                                    )
                                    .changed()
                                {
                                    self.sync_window_opacity_config();
                                    self.persist();
                                }

                                if !self.state.window_opacity_follow_focused_window {
                                    let selected_text = if self
                                        .state
                                        .window_opacity_target_window
                                        .is_empty()
                                    {
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Select window",
                                            "Chọn cửa sổ",
                                        )
                                        .to_owned()
                                    } else {
                                        Self::truncate_window_title(
                                            &Self::quick_action_window_display(
                                                &self.state.window_opacity_target_window,
                                                &self.open_window_infos,
                                            ),
                                            22,
                                        )
                                    };
                                    let response = egui::ComboBox::from_id_salt(
                                        "window-opacity-target-window",
                                    )
                                    .width(180.0)
                                    .selected_text(selected_text)
                                    .show_ui(ui, |ui| {
                                        let mut changed = false;
                                        for window in &self.open_window_infos {
                                            changed |= ui
                                                .selectable_value(
                                                    &mut self.state.window_opacity_target_window,
                                                    window.selector.clone(),
                                                    Self::truncate_window_title(
                                                        &Self::quick_action_window_display(
                                                            &window.selector,
                                                            &self.open_window_infos,
                                                        ),
                                                        24,
                                                    ),
                                                )
                                                .clicked();
                                        }
                                        changed
                                    });
                                    if response.response.clicked() {
                                        self.ensure_open_windows_ready(true);
                                    }
                                    if response.inner.unwrap_or(false) {
                                        self.sync_window_opacity_config();
                                        self.persist();
                                    }
                                }

                                if ui
                                    .add(
                                        egui::Slider::new(
                                            &mut self.state.window_opacity_percent,
                                            0..=100,
                                        )
                                        .text(Self::tr_lang(
                                            self.state.ui_language,
                                            "Opacity",
                                            "Độ trong suốt",
                                        ))
                                        .suffix("%"),
                                    )
                                    .changed()
                                {
                                    self.sync_window_opacity_config();
                                    self.persist_deferred(ui.ctx());
                                }
                                false
                            },
                        );
                    },
                );

                // Protractor Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::Protractor,
                            self.state.protractor_enabled,
                        );
                        if button_response.clicked() {
                            self.state.protractor_enabled = !self.state.protractor_enabled;
                            if self.state.protractor_enabled {
                                let (left, top, w, h) = crate::window_list::virtual_screen_bounds();
                                self.state.protractor_center_x = left + w / 2;
                                self.state.protractor_center_y = top + h / 2;
                                self.state.protractor_scale = 1.0;
                                self.state.protractor_needle1_angle = 0.0;
                                self.state.protractor_needle2_angle = 90.0;
                            }
                            self.sync_protractor_state();
                            self.persist();
                            self.status = if self.state.protractor_enabled {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Protractor overlay enabled.",
                                    "Protractor overlay enabled.",
                                )
                            } else {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Protractor overlay disabled.",
                                    "Protractor overlay disabled.",
                                )
                            }
                            .to_owned();
                        }

                        ui.add_space(6.0);
                        let proto_label =
                            Self::tr_lang(self.state.ui_language, "Protractor", "Protractor");
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(proto_label).size(11.0).color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                ));
                            },
                        );

                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::Protractor,
                            &mut |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Hold Shift while dragging a needle to snap the angle to 15° steps.",
                                            "Giữ Shift khi kéo kim để snap góc theo từng bước 15°.",
                                        ))
                                        .size(10.0),
                                    );
                                    false
                                })
                                .inner
                            },
                        );
                    },
                );

                // Ruler Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::Ruler,
                            self.distance_measurement_active,
                        );
                        if button_response.clicked() {
                            self.begin_distance_measurement(ui.ctx(), false);
                        }

                        ui.add_space(6.0);
                        let ruler_label = Self::tr_lang(self.state.ui_language, "Ruler", "Thước");
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(ruler_label).size(11.0).color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                ));
                            },
                        );

                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::Ruler,
                            &mut |ui| {
                                ui.vertical_centered(|ui| {
                                    let changed = ui
                                        .checkbox(
                                            &mut self.state.quick_actions_copy_ruler,
                                            RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Copy result",
                                                "Chép kết quả",
                                            ))
                                            .size(10.0),
                                        )
                                        .changed();
                                    if changed {
                                        self.persist();
                                    }
                                    false
                                })
                                .inner
                            },
                        );
                    },
                );
                ui.end_row();

                // Get Coordinates Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let is_active = self.vision_capture_active
                            && self.vision_capture_target
                                == Some(VisionCaptureTarget::QuickActionsCoordinates);
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::GetCoordinates,
                            is_active,
                        );
                        if button_response.clicked() {
                            self.begin_single_pixel_capture(
                                ui.ctx(),
                                VisionCaptureTarget::QuickActionsCoordinates,
                            );
                        }

                        ui.add_space(6.0);
                        let coords_label = Self::tr_lang(
                            self.state.ui_language,
                            "Get Coordinates",
                            "Get Coordinates",
                        );
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(coords_label).size(11.0).color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                ));
                            },
                        );

                        // Popup settings
                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::GetCoordinates,
                            &mut |ui| {
                                ui.vertical_centered(|ui| {
                                    let copy_x_changed = ui
                                        .checkbox(
                                            &mut self.state.quick_actions_copy_x,
                                            RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Copy X",
                                                "Copy X",
                                            ))
                                            .size(10.0),
                                        )
                                        .changed();
                                    if copy_x_changed {
                                        self.persist();
                                    }

                                    let copy_y_changed = ui
                                        .checkbox(
                                            &mut self.state.quick_actions_copy_y,
                                            RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Copy Y",
                                                "Copy Y",
                                            ))
                                            .size(10.0),
                                        )
                                        .changed();
                                    if copy_y_changed {
                                        self.persist();
                                    }
                                    false
                                })
                                .inner
                            },
                        );
                    },
                );

                // Get Color Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let is_active = self.vision_capture_active
                            && self.vision_capture_target
                                == Some(VisionCaptureTarget::QuickActionsColor);
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::GetColor,
                            is_active,
                        );
                        if button_response.clicked() {
                            self.begin_color_pick_capture(
                                ui.ctx(),
                                VisionCaptureTarget::QuickActionsColor,
                            );
                        }

                        ui.add_space(6.0);
                        let color_label =
                            Self::tr_lang(self.state.ui_language, "Get Color", "Get Color");
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(color_label).size(11.0).color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                ));
                            },
                        );

                        // Popup settings
                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::GetColor,
                            &mut |ui| {
                                ui.vertical_centered(|ui| {
                                    let copy_color_changed = ui
                                        .checkbox(
                                            &mut self.state.quick_actions_copy_color,
                                            RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Copy hex",
                                                "Copy hex",
                                            ))
                                            .size(10.0),
                                        )
                                        .changed();
                                    if copy_color_changed {
                                        self.persist();
                                    }
                                    false
                                })
                                .inner
                            },
                        );
                    },
                );

                // KeyDisplay Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let is_pick_active = self.vision_capture_active
                            && self.vision_capture_target
                                == Some(VisionCaptureTarget::QuickActionsKeyDisplayPosition);
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::KeyDisplay,
                            self.state.quick_key_display_enabled,
                        );
                        if button_response.clicked() {
                            self.state.quick_key_display_enabled =
                                !self.state.quick_key_display_enabled;
                            self.sync_quick_key_display_config();
                            self.persist();
                            self.status = if self.state.quick_key_display_enabled {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Key display enabled.",
                                    "Key display enabled.",
                                )
                            } else {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Key display disabled.",
                                    "Key display disabled.",
                                )
                            }
                            .to_owned();
                        }

                        ui.add_space(6.0);
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(Self::tr_lang(
                                        self.state.ui_language,
                                        "Key display",
                                        "Key display",
                                    ))
                                    .size(11.0)
                                    .color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                ));
                            },
                        );

                        // Popup settings
                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::KeyDisplay,
                            &mut |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Mode",
                                            "Chế độ",
                                        ))
                                        .size(10.0),
                                    );
                                    let mode_before = self.state.quick_key_display_mode;

                                    egui::ComboBox::from_id_salt("quick-key-display-mode")
                                        .width(164.0)
                                        .selected_text(match self.state.quick_key_display_mode {
                                            QuickKeyDisplayMode::Normal => Self::tr_lang(
                                                self.state.ui_language,
                                                "Normal",
                                                "Bình thường",
                                            ),
                                            QuickKeyDisplayMode::Mascot => Self::tr_lang(
                                                self.state.ui_language,
                                                "Mascot",
                                                "Mascot",
                                            ),
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut self.state.quick_key_display_mode,
                                                QuickKeyDisplayMode::Normal,
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Normal",
                                                    "Bình thường",
                                                ),
                                            );
                                            ui.selectable_value(
                                                &mut self.state.quick_key_display_mode,
                                                QuickKeyDisplayMode::Mascot,
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Mascot",
                                                    "Mascot",
                                                ),
                                            );
                                        });
                                    if self.state.quick_key_display_mode != mode_before {
                                        self.sync_quick_key_display_config();
                                        self.persist();
                                    }

                                    if self.state.quick_key_display_mode
                                        == QuickKeyDisplayMode::Mascot
                                    {
                                        ui.add_space(2.0);
                                        ui.label(
                                            RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Preset",
                                                "Preset",
                                            ))
                                            .size(10.0),
                                        );
                                        if self.state.quick_key_display_mascot_styles.is_empty() {
                                            self.state
                                                .quick_key_display_mascot_styles
                                                .push(self.state.quick_key_display_mascot_style);
                                        }
                                        let styles_before =
                                            self.state.quick_key_display_mascot_styles.clone();
                                        self.state.quick_key_display_mascot_styles.retain(
                                            |style| *style != crate::model::MascotStyle::Chiikawa,
                                        );
                                        let mascot_options = [
                                            (crate::model::MascotStyle::Hachiware, "Hachiware"),
                                            (crate::model::MascotStyle::ChiikawaClassic, "Usagi"),
                                        ];
                                        let selected_text = mascot_options
                                            .iter()
                                            .filter(|(style, _)| {
                                                self.state
                                                    .quick_key_display_mascot_styles
                                                    .contains(style)
                                            })
                                            .map(|(_, label)| *label)
                                            .collect::<Vec<_>>()
                                            .join(", ");

                                        egui::ComboBox::from_id_salt(
                                            "quick-key-display-mascot-style",
                                        )
                                        .width(164.0)
                                        .selected_text(if selected_text.is_empty() {
                                            Self::tr_lang(
                                                self.state.ui_language,
                                                "Select presets",
                                                "Chọn preset",
                                            )
                                        } else {
                                            selected_text.as_str()
                                        })
                                        .close_behavior(
                                            egui::PopupCloseBehavior::CloseOnClickOutside,
                                        )
                                        .show_ui(
                                            ui,
                                            |ui| {
                                                for (style, label) in mascot_options {
                                                    let mut selected = self
                                                        .state
                                                        .quick_key_display_mascot_styles
                                                        .contains(&style);
                                                    if ui.checkbox(&mut selected, label).changed() {
                                                        if selected {
                                                            self.state
                                                                .quick_key_display_mascot_styles
                                                                .push(style);
                                                        } else {
                                                            self.state
                                                                .quick_key_display_mascot_styles
                                                                .retain(|entry| *entry != style);
                                                        }
                                                    }
                                                }
                                            },
                                        );
                                        if self.state.quick_key_display_mascot_styles.is_empty() {
                                            self.state
                                                .quick_key_display_mascot_styles
                                                .push(self.state.quick_key_display_mascot_style);
                                        }
                                        if let Some(first_style) = self
                                            .state
                                            .quick_key_display_mascot_styles
                                            .first()
                                            .copied()
                                        {
                                            self.state.quick_key_display_mascot_style = first_style;
                                        }
                                        if self.state.quick_key_display_mascot_styles
                                            != styles_before
                                        {
                                            self.sync_quick_key_display_config();
                                            self.persist();
                                        }
                                    }

                                    ui.add_space(2.0);
                                    egui::Grid::new("quick-key-display-xy-grid")
                                        .spacing(vec2(8.0, 4.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("X").size(10.0));
                                            let x_changed = ui
                                                .add_sized(
                                                    [146.0, 20.0],
                                                    egui::DragValue::new(
                                                        &mut self.state.quick_key_display_x,
                                                    )
                                                    .speed(1.0),
                                                )
                                                .changed();
                                            ui.end_row();
                                            ui.label(RichText::new("Y").size(10.0));
                                            let y_changed = ui
                                                .add_sized(
                                                    [146.0, 20.0],
                                                    egui::DragValue::new(
                                                        &mut self.state.quick_key_display_y,
                                                    )
                                                    .speed(1.0),
                                                )
                                                .changed();
                                            ui.end_row();
                                            if x_changed || y_changed {
                                                self.sync_quick_key_display_config();
                                                self.persist_deferred(ui.ctx());
                                            }
                                        });

                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Size",
                                                "Size",
                                            ))
                                            .size(10.0),
                                        );
                                        let size_changed = ui
                                            .add_sized(
                                                [132.0, 20.0],
                                                egui::DragValue::new(
                                                    &mut self.state.quick_key_display_size,
                                                )
                                                .range(10.0..=96.0)
                                                .speed(1.0),
                                            )
                                            .changed();
                                        if size_changed {
                                            self.sync_quick_key_display_config();
                                            self.persist_deferred(ui.ctx());
                                        }
                                    });

                                    ui.add_space(4.0);
                                    if ui
                                        .add_sized(
                                            [164.0, 20.0],
                                            Button::new(if is_pick_active {
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Picking...",
                                                    "Picking...",
                                                )
                                            } else {
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Pick point",
                                                    "Chọn điểm",
                                                )
                                            }),
                                        )
                                        .clicked()
                                    {
                                        self.begin_image_search_capture(
                                            ui.ctx(),
                                            VisionCaptureTarget::QuickActionsKeyDisplayPosition,
                                            VisionCaptureMode::SinglePixel,
                                        );
                                    }
                                    false
                                })
                                .inner
                            },
                        );
                    },
                );

                // ScreenDraw Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::ScreenDraw,
                            self.state.quick_screen_draw_enabled,
                        );

                        // Instant screenshot corner button (chọn vùng -> copy)
                        let snap_rect = egui::Rect::from_min_size(
                            pos2(button_response.rect.right() - 31.0, button_response.rect.top() + 3.0),
                            vec2(27.0, 27.0),
                        );
                        let snap_response = ui.put(
                            snap_rect,
                            Button::new(Self::material_icon_text(0xe3b0, 14.0)) // photo_camera
                                .corner_radius(7.0)
                                .fill(Color32::from_rgba_premultiplied(20, 28, 44, 230))
                                .stroke(egui::Stroke::new(1.2, Color32::from_rgb(117, 219, 166))),
                        ).on_hover_text(Self::tr_lang(
                            self.state.ui_language,
                            "Instant screenshot (select region → copy)",
                            "Chụp nhanh (chọn vùng → copy)",
                        ));

                        if snap_response.clicked() {
                            crate::overlay::screen_draw_instant_screenshot();
                        } else if button_response.clicked() {
                            self.state.quick_screen_draw_enabled =
                                !self.state.quick_screen_draw_enabled;
                            self.sync_quick_screen_draw_config();
                            self.persist();
                            self.status = if self.state.quick_screen_draw_enabled {
                                "Screen draw hotkey enabled."
                            } else {
                                "Screen draw hotkey disabled."
                            }
                            .to_owned();
                        }

                        ui.add_space(6.0);
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(Self::tr_lang(
                                        self.state.ui_language,
                                        "Draw",
                                        "Vẽ",
                                    ))
                                    .size(11.0)
                                    .color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                ));
                            },
                        );

                        // Popup settings
                        let mut keep_open = false;
                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::ScreenDraw,
                            &mut |ui| {
                                ui.vertical(|ui| {
                                    let freeze_changed = ui
                                        .checkbox(
                                            &mut self.state.quick_screen_draw_freeze,
                                            RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Freeze screen",
                                                "Đóng băng màn hình",
                                            ))
                                            .size(10.0),
                                        )
                                        .changed();
                                    if freeze_changed {
                                        self.sync_quick_screen_draw_config();
                                        self.persist();
                                    }

                                    ui.add_space(4.0);
                                    let capture_active =
                                        self.capture_target.as_ref().is_some_and(|target| {
                                            matches!(target, CaptureRequest::QuickScreenDrawHotkey)
                                        });
                                    let hotkey_label = if capture_active {
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Capturing...",
                                            "Đang bắt phím...",
                                        )
                                        .to_owned()
                                    } else {
                                        self.state
                                            .quick_screen_draw_hotkey
                                            .as_ref()
                                            .map(|binding| {
                                                Self::format_binding_ui(
                                                    self.state.ui_language,
                                                    Some(binding),
                                                )
                                            })
                                            .unwrap_or_else(|| {
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Set trigger key",
                                                    "Đặt phím trigger",
                                                )
                                                .to_owned()
                                            })
                                    };
                                    let capture_time = ui.ctx().input(|input| input.time) as f32;
                                    let pulse = if capture_active {
                                        0.5 + 0.5 * (capture_time * 6.0).sin().abs()
                                    } else {
                                        0.0
                                    };
                                    let capture_fill = if capture_active {
                                        Color32::from_rgba_premultiplied(
                                            (88.0 + pulse * 28.0) as u8,
                                            (84.0 + pulse * 28.0) as u8,
                                            (44.0 + pulse * 10.0) as u8,
                                            255,
                                        )
                                    } else {
                                        ui.visuals().widgets.inactive.bg_fill
                                    };
                                    let capture_stroke = if capture_active {
                                        Color32::from_rgb(255, 232, 96)
                                    } else {
                                        ui.visuals().widgets.inactive.bg_stroke.color
                                    };
                                    let mut capture_button = Button::new(hotkey_label);
                                    if capture_active {
                                        capture_button = capture_button
                                            .fill(capture_fill)
                                            .stroke(egui::Stroke::new(1.0, capture_stroke));
                                    }
                                    if ui
                                        .add_sized(
                                            [164.0, 22.0],
                                            capture_button,
                                        )
                                        .on_hover_text(if capture_active {
                                            Self::tr_lang(
                                                self.state.ui_language,
                                                "Cancel capture",
                                                "Hủy bắt phím",
                                            )
                                        } else {
                                            Self::tr_lang(
                                                self.state.ui_language,
                                                "Capture draw hotkey",
                                                "Bắt phím vẽ",
                                            )
                                        })
                                        .clicked()
                                    {
                                        if capture_active {
                                            self.cancel_capture();
                                        } else if self.state.quick_screen_draw_hotkey.is_some() {
                                            self.state.quick_screen_draw_hotkey = None;
                                            self.sync_quick_screen_draw_config();
                                            self.persist();
                                            self.status =
                                                "Cleared screen draw toggle key.".to_owned();
                                        } else {
                                            self.begin_capture(
                                                CaptureRequest::QuickScreenDrawHotkey,
                                                "Press the key that toggles screen drawing."
                                                    .to_owned(),
                                            );
                                        }
                                    }

                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Hold trigger to capture screen region",
                                            "Đè nút trigger để chụp vùng màn hình",
                                        ))
                                        .size(10.0)
                                        .color(ui.visuals().weak_text_color()),
                                    );

                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Hold right mouse to erase",
                                            "Đè chuột phải để xóa",
                                        ))
                                        .size(10.0)
                                        .color(ui.visuals().weak_text_color()),
                                    );

                                    if capture_active {
                                        keep_open = true;
                                    }
                                    capture_active
                                })
                                .inner
                            },
                        );
                        if keep_open {
                            keep_menu_open = true;
                        }
                    },
                );

                // Video recorder action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let recording = crate::video_recorder::is_recording();
                        let recorder_busy = crate::video_recorder::is_busy();
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::VideoRecord,
                            self.state.quick_video_record_enabled,
                        );

                        // Instant record corner button (auto full screen record)
                        let rec_rect = egui::Rect::from_min_size(
                            pos2(button_response.rect.right() - 31.0, button_response.rect.top() + 3.0),
                            vec2(27.0, 27.0),
                        );
                        let rec_fill = if recording {
                            Color32::from_rgb(220, 38, 38) // Red when recording
                        } else {
                            Color32::from_rgba_premultiplied(20, 28, 44, 230)
                        };
                        let rec_stroke = if recording {
                            egui::Stroke::new(1.5, Color32::from_rgb(254, 202, 202))
                        } else {
                            egui::Stroke::new(1.2, Color32::from_rgb(117, 219, 166))
                        };
                        let rec_tooltip = if recording {
                            Self::tr_lang(
                                self.state.ui_language,
                                "Stop recording",
                                "Dừng quay video",
                            )
                        } else {
                            Self::tr_lang(
                                self.state.ui_language,
                                "Instant record (full screen)",
                                "Quay nhanh (toàn màn hình)",
                            )
                        };

                        let rec_response = ui.put(
                            rec_rect,
                            Button::new(Self::material_icon_text(0xe04b, 14.0)) // videocam
                                .corner_radius(7.0)
                                .fill(rec_fill)
                                .stroke(rec_stroke),
                        ).on_hover_text(rec_tooltip);

                        if rec_response.clicked() {
                            if !recorder_busy && self.ffmpeg_installed {
                                if !recording {
                                    self.state.quick_video_record_mode =
                                        QuickVideoRecordMode::FullScreen;
                                    self.sync_quick_video_record_config();
                                }
                                crate::video_recorder::toggle_async();
                            }
                        } else if button_response.clicked() && !recorder_busy {
                            self.state.quick_video_record_enabled =
                                !self.state.quick_video_record_enabled;
                            self.sync_quick_video_record_config();
                            if !self.state.quick_video_record_enabled && recording {
                                crate::video_recorder::toggle_async();
                            } else if self.state.quick_video_record_enabled
                                && !self.ffmpeg_installed
                            {
                                self.start_ffmpeg_download();
                            }
                            self.persist();
                        }

                        ui.add_space(6.0);
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(Self::tr_lang(
                                        self.state.ui_language,
                                        "Record",
                                        "Quay video",
                                    ))
                                    .size(11.0)
                                    .color(if button_response.hovered() {
                                        ui.visuals().strong_text_color()
                                    } else {
                                        ui.visuals().text_color()
                                    }),
                                ));
                            },
                        );

                        let mut keep_open = false;
                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::VideoRecord,
                            &mut |ui| {
                                ui.set_min_width(194.0);
                                ui.add_enabled_ui(!recording && !recorder_busy, |ui| {
                                    let mode_before = self.state.quick_video_record_mode;
                                    egui::ComboBox::from_id_salt("quick-video-source")
                                        .width(186.0)
                                        .selected_text(match mode_before {
                                            QuickVideoRecordMode::FullScreen => Self::tr_lang(
                                                self.state.ui_language,
                                                "Full screen",
                                                "Toàn màn hình",
                                            ),
                                            QuickVideoRecordMode::FocusedWindow => Self::tr_lang(
                                                self.state.ui_language,
                                                "Focused window",
                                                "Cửa sổ đang focus",
                                            ),
                                            QuickVideoRecordMode::SelectedWindow => Self::tr_lang(
                                                self.state.ui_language,
                                                "Selected window",
                                                "Cửa sổ đã chọn",
                                            ),
                                            QuickVideoRecordMode::Region => Self::tr_lang(
                                                self.state.ui_language,
                                                "Screen region",
                                                "Vùng màn hình",
                                            ),
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut self.state.quick_video_record_mode,
                                                QuickVideoRecordMode::FullScreen,
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Full screen",
                                                    "Toàn màn hình",
                                                ),
                                            );
                                            ui.selectable_value(
                                                &mut self.state.quick_video_record_mode,
                                                QuickVideoRecordMode::FocusedWindow,
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Focused window",
                                                    "Cửa sổ đang focus",
                                                ),
                                            );
                                            ui.selectable_value(
                                                &mut self.state.quick_video_record_mode,
                                                QuickVideoRecordMode::SelectedWindow,
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Selected window",
                                                    "Cửa sổ đã chọn",
                                                ),
                                            );
                                            ui.selectable_value(
                                                &mut self.state.quick_video_record_mode,
                                                QuickVideoRecordMode::Region,
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Screen region",
                                                    "Vùng màn hình",
                                                ),
                                            );
                                        });
                                    if self.state.quick_video_record_mode != mode_before {
                                        self.sync_quick_video_record_config();
                                        self.persist();
                                    }

                                    match self.state.quick_video_record_mode {
                                        QuickVideoRecordMode::SelectedWindow => {
                                            let selected = if self
                                                .state
                                                .quick_video_record_target_window
                                                .is_empty()
                                            {
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Select window",
                                                    "Chọn cửa sổ",
                                                )
                                                .to_owned()
                                            } else {
                                                Self::truncate_window_title(
                                                    &Self::quick_action_window_display(
                                                        &self.state.quick_video_record_target_window,
                                                        &self.open_window_infos,
                                                    ),
                                                    24,
                                                )
                                            };
                                            let target_before = self
                                                .state
                                                .quick_video_record_target_window
                                                .clone();
                                            let combo = egui::ComboBox::from_id_salt(
                                                "quick-video-target-window",
                                            )
                                            .width(186.0)
                                            .selected_text(selected)
                                            .show_ui(ui, |ui| {
                                                for window in &self.open_window_infos {
                                                    ui.selectable_value(
                                                        &mut self.state.quick_video_record_target_window,
                                                        window.selector.clone(),
                                                        Self::truncate_window_title(
                                                            &Self::quick_action_window_display(
                                                                &window.selector,
                                                                &self.open_window_infos,
                                                            ),
                                                            26,
                                                        ),
                                                    );
                                                }
                                            });
                                            if combo.response.clicked() {
                                                self.ensure_open_windows_ready(true);
                                            }
                                            if self.state.quick_video_record_target_window
                                                != target_before
                                            {
                                                self.sync_quick_video_record_config();
                                                self.persist();
                                            }
                                        }
                                        QuickVideoRecordMode::Region => {
                                            let region_text = self
                                                .state
                                                .quick_video_record_region
                                                .map(|(_, _, width, height)| {
                                                    format!("{width} x {height}")
                                                })
                                                .unwrap_or_else(|| {
                                                    Self::tr_lang(
                                                        self.state.ui_language,
                                                        "Select region",
                                                        "Chọn vùng",
                                                    )
                                                    .to_owned()
                                                });
                                            if ui
                                                .add_sized(
                                                    [186.0, 22.0],
                                                    Button::new(region_text),
                                                )
                                                .clicked()
                                            {
                                                crate::overlay::screen_draw_select_video_region();
                                            }
                                        }
                                        _ => {}
                                    }

                                    ui.add_space(3.0);
                                    let capture_active = self
                                        .capture_target
                                        .as_ref()
                                        .is_some_and(|target| {
                                            matches!(
                                                target,
                                                CaptureRequest::QuickVideoRecordHotkey
                                            )
                                        });
                                    let label = if capture_active {
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Capturing...",
                                            "Đang bắt phím...",
                                        )
                                        .to_owned()
                                    } else {
                                        self.state
                                            .quick_video_record_hotkey
                                            .as_ref()
                                            .map(|binding| {
                                                Self::format_binding_ui(
                                                    self.state.ui_language,
                                                    Some(binding),
                                                )
                                            })
                                            .unwrap_or_else(|| {
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Set trigger key",
                                                    "Đặt phím trigger",
                                                )
                                                .to_owned()
                                            })
                                    };
                                    let capture_time = ui.ctx().input(|input| input.time) as f32;
                                    let pulse = if capture_active {
                                        0.5 + 0.5 * (capture_time * 6.0).sin().abs()
                                    } else {
                                        0.0
                                    };
                                    let capture_fill = if capture_active {
                                        Color32::from_rgba_premultiplied(
                                            (88.0 + pulse * 28.0) as u8,
                                            (84.0 + pulse * 28.0) as u8,
                                            (44.0 + pulse * 10.0) as u8,
                                            255,
                                        )
                                    } else {
                                        ui.visuals().widgets.inactive.bg_fill
                                    };
                                    let capture_stroke = if capture_active {
                                        Color32::from_rgb(255, 232, 96)
                                    } else {
                                        ui.visuals().widgets.inactive.bg_stroke.color
                                    };
                                    let mut capture_button = Button::new(label);
                                    if capture_active {
                                        capture_button = capture_button
                                            .fill(capture_fill)
                                            .stroke(egui::Stroke::new(1.0, capture_stroke));
                                    }
                                    if ui
                                        .add_sized(
                                            [186.0, 22.0],
                                            capture_button,
                                        )
                                        .on_hover_text(Self::tr_lang(
                                            self.state.ui_language,
                                            "Press to start or stop. Hold while idle to select a region, then release to record it.",
                                            "Nhấn để bắt đầu hoặc dừng. Khi chưa quay, giữ phím để chọn vùng rồi thả ra để quay vùng đó.",
                                        ))
                                        .clicked()
                                    {
                                        if capture_active {
                                            self.cancel_capture();
                                        } else if self.state.quick_video_record_hotkey.is_some() {
                                            self.state.quick_video_record_hotkey = None;
                                            self.sync_quick_video_record_config();
                                            self.persist();
                                        } else {
                                            self.begin_capture(
                                                CaptureRequest::QuickVideoRecordHotkey,
                                                "Press the video recording trigger key."
                                                    .to_owned(),
                                            );
                                        }
                                    }
                                    keep_open |= capture_active;

                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Hold trigger to select and record a region",
                                            "Giữ trigger để chọn và quay một vùng",
                                        ))
                                        .size(12.0)
                                        .weak(),
                                    );

                                    ui.horizontal(|ui| {
                                        ui.label(Self::tr_lang(
                                            self.state.ui_language,
                                            "FPS",
                                            "FPS",
                                        ));
                                        let fps_before = self.state.quick_video_record_fps;
                                        egui::ComboBox::from_id_salt("quick-video-fps")
                                            .width(128.0)
                                            .selected_text(format!("{} FPS", fps_before))
                                            .show_ui(ui, |ui| {
                                                for fps in [30, 60, 144] {
                                                    ui.selectable_value(
                                                        &mut self.state.quick_video_record_fps,
                                                        fps,
                                                        format!("{fps} FPS"),
                                                    );
                                                }
                                            });
                                        if self.state.quick_video_record_fps != fps_before {
                                            self.sync_quick_video_record_config();
                                            self.persist();
                                        }
                                    });

                                    ui.add_space(3.0);
                                    let folder_name = Path::new(
                                        &self.state.quick_video_record_output_dir,
                                    )
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or_else(|| {
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Choose folder",
                                            "Chọn thư mục",
                                        )
                                    });
                                    if ui
                                        .add_sized(
                                            [186.0, 22.0],
                                            Button::new(format!(
                                                "{}: {}",
                                                Self::tr_lang(
                                                    self.state.ui_language,
                                                    "Save",
                                                    "Lưu",
                                                ),
                                                folder_name
                                            )),
                                        )
                                        .on_hover_text(&self.state.quick_video_record_output_dir)
                                        .clicked()
                                        && let Some(folder) = rfd::FileDialog::new()
                                            .set_directory(
                                                &self.state.quick_video_record_output_dir,
                                            )
                                            .pick_folder()
                                    {
                                        self.state.quick_video_record_output_dir =
                                            folder.to_string_lossy().into_owned();
                                        self.sync_quick_video_record_config();
                                        self.persist();
                                    }
                                });

                                if ui
                                    .add_sized(
                                        [186.0, 22.0],
                                        Button::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Open video folder",
                                            "Mở thư mục video",
                                        )),
                                    )
                                    .clicked()
                                {
                                    let folder = Path::new(
                                        &self.state.quick_video_record_output_dir,
                                    );
                                    let _ = fs::create_dir_all(folder);
                                    if let Err(error) =
                                        crate::platform::open_folder_in_explorer(folder)
                                    {
                                        self.status = format!("Could not open video folder: {error}");
                                    }
                                }

                                if ui
                                    .add_sized(
                                        [186.0, 22.0],
                                        Button::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Video library / Trim & compress",
                                            "Thư viện video / Cắt & nén",
                                        )),
                                    )
                                    .clicked()
                                {
                                    self.video_library_open = true;
                                }

                                let copy_response = ui.add_enabled(
                                    !recording && !recorder_busy,
                                    egui::Checkbox::new(
                                        &mut self.state.quick_video_copy_after_recording,
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Copy video after recording",
                                            "Sao chép video sau khi quay",
                                        ),
                                    ),
                                );
                                if copy_response.changed() {
                                    self.sync_quick_video_record_config();
                                    self.persist();
                                }

                                ui.add_space(4.0);
                                if !self.ffmpeg_installed {
                                    if self.ffmpeg_download_job.is_some() {
                                        let progress = self
                                            .ffmpeg_download_progress
                                            .load(std::sync::atomic::Ordering::SeqCst)
                                            as f32
                                            / 1000.0;
                                        ui.add(
                                            egui::ProgressBar::new(progress)
                                                .desired_width(186.0)
                                                .show_percentage(),
                                        );
                                        ui.ctx().request_repaint();
                                        keep_open = true;
                                    } else if ui
                                        .add_sized(
                                            [186.0, 22.0],
                                            Button::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Install recorder",
                                                "Cài công cụ quay",
                                            )),
                                        )
                                        .clicked()
                                    {
                                        self.start_ffmpeg_download();
                                        keep_open = true;
                                    }
                                }
                                let recorder_status = crate::video_recorder::status();
                                ui.label(
                                    RichText::new(if recorder_status == "Ready" {
                                        Self::tr_lang(
                                            self.state.ui_language,
                                            "Ready",
                                            "Sẵn sàng",
                                        )
                                    } else {
                                        recorder_status.as_str()
                                    })
                                    .size(12.0)
                                    .weak(),
                                );
                                keep_open
                            },
                        );
                        if keep_open {
                            keep_menu_open = true;
                        }
                    },
                );

                // ClearOverlays Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::ClearOverlays,
                            macro_visual_overlay_active,
                        );
                        if button_response.clicked() {
                            self.clear_macro_visual_overlays();
                            self.status = if macro_visual_overlay_active {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Cleared geometry, HUD, and pin overlays.",
                                    "Cleared geometry, HUD, and pin overlays.",
                                )
                            } else {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "No geometry, HUD, or pin overlays were active.",
                                    "No geometry, HUD, or pin overlays were active.",
                                )
                            }
                            .to_owned();
                        }

                        ui.add_space(6.0);
                        let clear_label = Self::tr_lang(
                            self.state.ui_language,
                            "Clear overlays",
                            "Xóa overlay",
                        );
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(clear_label).size(11.0).color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ))
                                    .wrap(),
                                );
                            },
                        );
                    },
                );

                // KeySound Action
                ui.allocate_ui_with_layout(
                    vec2(action_width, action_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let button_response = self.titlebar_quick_action_button(
                            ui,
                            TitlebarQuickActionKind::KeySound,
                            self.state.quick_key_sound_enabled,
                        );
                        if button_response.clicked() {
                            self.state.quick_key_sound_enabled =
                                !self.state.quick_key_sound_enabled;
                            self.sync_quick_key_sound_config();
                            self.persist();
                            self.status = if self.state.quick_key_sound_enabled {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Key sound enabled.",
                                    "Bật âm thanh nhấn phím.",
                                )
                            } else {
                                Self::tr_lang(
                                    self.state.ui_language,
                                    "Key sound disabled.",
                                    "Tắt âm thanh nhấn phím.",
                                )
                            }
                            .to_owned();
                        }

                        ui.add_space(6.0);
                        let sound_label =
                            Self::tr_lang(self.state.ui_language, "Key Sound", "Tiếng phím cơ");
                        ui.allocate_ui_with_layout(
                            vec2(92.0, 28.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(
                                    RichText::new(sound_label).size(11.0).color(
                                        if button_response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                ));
                            },
                        );

                        // Popup settings
                        render_popup(
                            ui,
                            &button_response,
                            TitlebarQuickActionKind::KeySound,
                            &mut |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Switch Type",
                                            "Loại phím",
                                        ))
                                        .size(10.0),
                                    );
                                    let style_before = self.state.quick_key_sound_style;
                                    const SWITCH_NAMES: &[&str] = &[
                                        "Cherry MX Blue",
                                        "Cherry MX Brown",
                                        "NovelKeys Creams",
                                        "Holy Pandas",
                                        "Alpacas",
                                        "Topre",
                                        "Kailh Box Navy",
                                        "Gateron Ink Black",
                                    ];
                                    let selected_name = SWITCH_NAMES
                                        .get(self.state.quick_key_sound_style as usize)
                                        .copied()
                                        .unwrap_or(SWITCH_NAMES[0]);

                                    egui::ComboBox::from_id_salt("quick-key-sound-style")
                                        .width(164.0)
                                        .selected_text(selected_name)
                                        .show_ui(ui, |ui| {
                                            for (idx, name) in SWITCH_NAMES.iter().enumerate() {
                                                ui.selectable_value(
                                                    &mut self.state.quick_key_sound_style,
                                                    idx as u32,
                                                    *name,
                                                );
                                            }
                                        });
                                    if self.state.quick_key_sound_style != style_before {
                                        self.sync_quick_key_sound_config();
                                        self.persist();
                                    }

                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            self.state.ui_language,
                                            "Volume",
                                            "Am luong",
                                        ))
                                        .size(10.0),
                                    );
                                    let vol_before = self.state.quick_key_sound_volume;
                                    let vol_pct = (vol_before * 100.0).round() as i32;
                                    ui.horizontal(|ui| {
                                        let slider_resp = ui.add_sized(
                                            [120.0, 20.0],
                                            egui::Slider::new(
                                                &mut self.state.quick_key_sound_volume,
                                                0.0..=2.0,
                                            )
                                            .show_value(false),
                                        );
                                        ui.label(RichText::new(format!("{}%", vol_pct)).size(10.0));
                                        let _ = slider_resp;
                                    });
                                    if (self.state.quick_key_sound_volume - vol_before).abs() > 1e-4
                                    {
                                        self.sync_quick_key_sound_config();
                                        self.persist_deferred(ui.ctx());
                                    }

                                    false
                                })
                                .inner
                            },
                        );
                    },
                );
            });
        // If any hover card is shown, keep the quick-actions panel open to survive CloseOnClickOutside
        let any_hover_card_visible = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(qa_hover_card_key))
            .unwrap_or(false);
        keep_menu_open || any_hover_card_visible
    }

    fn request_video_library_preview(&mut self, video_path: PathBuf, preview_at_seconds: f64) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let ffmpeg_exe = self.paths.ffmpeg_exe.clone();
        std::thread::spawn(move || {
            let result = crate::video_recorder::inspect_recorded_video(
                &ffmpeg_exe,
                &video_path,
                preview_at_seconds,
            );
            let _ = tx.send((video_path, result));
        });
        self.video_library_preview_rx = Some(rx);
    }

    fn stop_video_library_playback(&mut self) {
        let was_playing =
            self.video_library_playback.is_some() || self.video_library_playback_path.is_some();
        self.video_library_playback = None;
        self.video_library_playback_path = None;
        self.video_library_preloaded_playback = None;
        if was_playing {
            audio::stop_video_audio_preview();
        }
    }

    #[allow(dead_code)]
    fn render_video_library_legacy(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.video_library_preview_rx {
            match rx.try_recv() {
                Ok((path, Ok(preview))) => {
                    if self.video_library_selected.as_ref() == Some(&path) {
                        self.video_library_trim_start_seconds = self
                            .video_library_trim_start_seconds
                            .clamp(0.0, preview.duration_seconds);
                        self.video_library_trim_end_seconds = if self.video_library_trim_end_seconds
                            <= self.video_library_trim_start_seconds
                        {
                            preview.duration_seconds
                        } else {
                            self.video_library_trim_end_seconds.clamp(
                                self.video_library_trim_start_seconds,
                                preview.duration_seconds,
                            )
                        };
                        self.video_library_preview_texture = preview.rgba.as_ref().map(|rgba| {
                            ctx.load_texture(
                                format!("video-library-preview-{}", path.display()),
                                ColorImage::from_rgba_unmultiplied(
                                    [preview.width as usize, preview.height as usize],
                                    rgba,
                                ),
                                TextureOptions::LINEAR,
                            )
                        });
                        self.video_library_preview = Some(preview);
                    }
                    self.video_library_preview_rx = None;
                }
                Ok((_, Err(error))) => {
                    self.status = format!("Video preview failed: {error}");
                    self.video_library_preview_rx = None;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.video_library_preview_rx = None;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
        }

        if !self.video_library_open {
            return;
        }

        let output_dir = PathBuf::from(&self.state.quick_video_record_output_dir);
        let videos = crate::video_recorder::recorded_videos(&output_dir);
        if self
            .video_library_selected
            .as_ref()
            .is_some_and(|path| !path.is_file())
        {
            self.video_library_selected = None;
            self.video_library_preview = None;
            self.video_library_preview_texture = None;
        }

        let mut open = self.video_library_open;
        let mut select_video = None;
        let mut refresh_preview = None;
        let mut reveal_video = None;
        let mut export_request = None;
        egui::Window::new("Video library")
            .open(&mut open)
            .default_width(820.0)
            .default_height(560.0)
            .min_width(680.0)
            .min_height(420.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.scope(|ui| {
                    ui.style_mut()
                        .text_styles
                        .insert(egui::TextStyle::Body, egui::FontId::proportional(13.0));
                    ui.style_mut()
                        .text_styles
                        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Recorded videos").strong().size(13.0));
                        ui.label(
                            RichText::new(format!("{} video(s)", videos.len()))
                                .size(12.0)
                                .weak(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Open folder").clicked() {
                                let _ = fs::create_dir_all(&output_dir);
                                if let Err(error) = crate::platform::open_folder_in_explorer(&output_dir) {
                                    self.status = format!("Could not open video folder: {error}");
                                }
                            }
                        });
                    });
                    ui.separator();
                    ui.columns(2, |columns| {
                        columns[0].set_min_width(255.0);
                        columns[0].set_max_width(300.0);
                        egui::ScrollArea::vertical().show(&mut columns[0], |ui| {
                            if videos.is_empty() {
                                ui.label(RichText::new("No recorded videos yet.").weak());
                            }
                            for video in &videos {
                                let selected = self.video_library_selected.as_ref() == Some(video);
                                let name = video
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("video");
                                let bytes = fs::metadata(video).map(|metadata| metadata.len()).unwrap_or(0);
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 30.0],
                                        Button::new(format!("{name}\n{:.1} MB", bytes as f64 / 1_048_576.0))
                                            .selected(selected),
                                    )
                                    .clicked()
                                {
                                    select_video = Some(video.clone());
                                }
                            }
                        });

                        columns[1].vertical(|ui| {
                            let Some(selected_path) = self.video_library_selected.as_ref() else {
                                ui.centered_and_justified(|ui| {
                                    ui.label(RichText::new("Choose a video to preview, trim, or compress.").weak());
                                });
                                return;
                            };
                            let selected_path = selected_path.clone();
                            ui.label(
                                RichText::new(
                                    selected_path
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("video"),
                                )
                                .strong(),
                            );
                            if let Some(texture) = &self.video_library_preview_texture {
                                ui.add(Image::new(texture).max_width(500.0).max_height(260.0));
                            } else if self.video_library_preview_rx.is_some() {
                                ui.add(egui::Spinner::new());
                                ui.label(RichText::new("Loading preview…").weak());
                            } else {
                                ui.label(RichText::new("Preview frame unavailable.").weak());
                            }

                            if let Some(preview) = &self.video_library_preview {
                                ui.label(
                                    RichText::new(format!(
                                        "Duration: {}  •  Size: {:.1} MB",
                                        Self::format_video_seconds(preview.duration_seconds),
                                        preview.file_size as f64 / 1_048_576.0
                                    ))
                                    .size(12.0)
                                    .weak(),
                                );
                                let duration = preview.duration_seconds.max(0.1);
                                let start_before = self.video_library_trim_start_seconds;
                                ui.add(
                                    egui::Slider::new(
                                        &mut self.video_library_trim_start_seconds,
                                        0.0..=duration,
                                    )
                                    .text("Start")
                                    .custom_formatter(|value, _| Self::format_video_seconds(value)),
                                );
                                self.video_library_trim_end_seconds = self
                                    .video_library_trim_end_seconds
                                    .clamp(self.video_library_trim_start_seconds, duration);
                                ui.add(
                                    egui::Slider::new(
                                        &mut self.video_library_trim_end_seconds,
                                        self.video_library_trim_start_seconds..=duration,
                                    )
                                    .text("End")
                                    .custom_formatter(|value, _| Self::format_video_seconds(value)),
                                );
                                ui.horizontal(|ui| {
                                    if ui.button("Preview at start").clicked()
                                        || (start_before != self.video_library_trim_start_seconds
                                            && !ui.input(|input| input.pointer.any_down()))
                                    {
                                        refresh_preview = Some((
                                            selected_path.clone(),
                                            self.video_library_trim_start_seconds,
                                        ));
                                    }
                                    if ui.button("Open file location").clicked() {
                                        reveal_video = Some(selected_path.clone());
                                    }
                                });
                                ui.separator();
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_enabled(
                                            !crate::video_recorder::is_editing(),
                                            Button::new("Export trim"),
                                        )
                                        .clicked()
                                    {
                                        export_request = Some((
                                            selected_path.clone(),
                                            self.video_library_trim_start_seconds,
                                            self.video_library_trim_end_seconds,
                                            None,
                                        ));
                                    }
                                    ui.label("Target size");
                                    ui.add_sized(
                                        [68.0, 22.0],
                                        egui::DragValue::new(&mut self.video_library_target_size_mb)
                                            .range(1..=2048)
                                            .suffix(" MB"),
                                    );
                                    if ui
                                        .add_enabled(
                                            !crate::video_recorder::is_editing(),
                                            Button::new("Compress"),
                                        )
                                        .clicked()
                                    {
                                        export_request = Some((
                                            selected_path,
                                            self.video_library_trim_start_seconds,
                                            self.video_library_trim_end_seconds,
                                            Some(self.video_library_target_size_mb),
                                        ));
                                    }
                                });
                                ui.label(
                                    RichText::new("Compress uses the selected target as an approximate final size.")
                                        .size(11.0)
                                        .weak(),
                                );
                            }
                            ui.add_space(4.0);
                            ui.label(RichText::new(crate::video_recorder::edit_status()).size(11.0).weak());
                        });
                    });
                });
            });

        self.video_library_open = open;
        if let Some(path) = select_video {
            self.video_library_selected = Some(path.clone());
            self.video_library_preview = None;
            self.video_library_preview_texture = None;
            self.video_library_trim_start_seconds = 0.0;
            self.video_library_trim_end_seconds = 0.0;
            self.request_video_library_preview(path, 0.0);
        }
        if let Some((path, at_seconds)) = refresh_preview {
            self.request_video_library_preview(path, at_seconds);
        }
        if let Some(path) = reveal_video
            && let Err(error) = crate::platform::reveal_file_in_explorer(&path)
        {
            self.status = format!("Could not reveal video: {error}");
        }
        if let Some((input, start, end, target_size_mb)) = export_request {
            crate::video_recorder::export_trim_async(
                self.paths.ffmpeg_exe.clone(),
                input,
                output_dir,
                start,
                end,
                target_size_mb,
            );
        }
        if self.video_library_preview_rx.is_some() || crate::video_recorder::is_editing() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    #[allow(dead_code)]
    fn render_video_library_v1(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.video_library_preview_rx {
            match rx.try_recv() {
                Ok((path, Ok(preview))) => {
                    if self.video_library_selected.as_ref() == Some(&path) {
                        self.video_library_trim_start_seconds = self
                            .video_library_trim_start_seconds
                            .clamp(0.0, preview.duration_seconds);
                        self.video_library_trim_end_seconds = if self.video_library_trim_end_seconds
                            <= self.video_library_trim_start_seconds
                        {
                            preview.duration_seconds
                        } else {
                            self.video_library_trim_end_seconds.clamp(
                                self.video_library_trim_start_seconds,
                                preview.duration_seconds,
                            )
                        };
                        self.video_library_preview_texture = preview.rgba.as_ref().map(|rgba| {
                            ctx.load_texture(
                                format!("video-library-preview-{}", path.display()),
                                ColorImage::from_rgba_unmultiplied(
                                    [preview.width as usize, preview.height as usize],
                                    rgba,
                                ),
                                TextureOptions::LINEAR,
                            )
                        });
                        self.video_library_preview = Some(preview);
                    }
                    self.video_library_preview_rx = None;
                }
                Ok((_, Err(error))) => {
                    self.status = format!("Video preview failed: {error}");
                    self.video_library_preview_rx = None;
                }
                Err(TryRecvError::Disconnected) => self.video_library_preview_rx = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if !self.video_library_open {
            if self.video_library_playback.is_some()
                || self.video_library_preloaded_playback.is_some()
                || self.video_library_playback_path.is_some()
            {
                self.stop_video_library_playback();
            }
            return;
        }

        let output_dir = PathBuf::from(&self.state.quick_video_record_output_dir);
        let videos = crate::video_recorder::recorded_videos(&output_dir);
        if self
            .video_library_selected
            .as_ref()
            .is_some_and(|path| !path.is_file())
        {
            self.video_library_selected = None;
            self.video_library_preview = None;
            self.video_library_preview_texture = None;
        }

        let detail_open = self.video_library_selected.is_some();
        let mut open = self.video_library_open;
        let mut select_video = None;
        let mut return_to_library = false;
        let mut refresh_preview = None;
        let mut reveal_video = None;
        let mut copy_video = None;
        let mut export_request = None;
        let mut window = egui::Window::new("Video library")
            .open(&mut open)
            .default_width(820.0)
            .default_height(560.0)
            .min_width(680.0)
            .min_height(420.0)
            .resizable(true)
            .collapsible(false);
        if detail_open {
            let rect = ctx.content_rect().shrink(14.0);
            window = window.fixed_pos(rect.min).fixed_size(rect.size());
        }
        window.show(ctx, |ui| {
            ui.scope(|ui| {
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Body, egui::FontId::proportional(13.0));
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
                ui.horizontal(|ui| {
                    if detail_open && ui.button("← Library").clicked() {
                        return_to_library = true;
                    }
                    ui.label(RichText::new(if detail_open { "Video preview" } else { "Recorded videos" }).strong());
                    ui.label(RichText::new(format!("{} video(s)", videos.len())).size(12.0).weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Open folder").clicked() {
                            let _ = fs::create_dir_all(&output_dir);
                            if let Err(error) = crate::platform::open_folder_in_explorer(&output_dir) {
                                self.status = format!("Could not open video folder: {error}");
                            }
                        }
                    });
                });
                ui.separator();

                if !detail_open {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if videos.is_empty() {
                            ui.label(RichText::new("No recorded videos yet.").weak());
                        }
                        ui.columns(3, |columns| {
                            for (index, video) in videos.iter().enumerate() {
                                let ui = &mut columns[index % 3];
                                let name = video.file_name().and_then(|name| name.to_str()).unwrap_or("video");
                                let bytes = fs::metadata(video).map(|metadata| metadata.len()).unwrap_or(0);
                                let response = ui.add_sized(
                                    [ui.available_width().max(120.0), 148.0],
                                    Button::new(
                                        RichText::new(format!("▶\n\n{name}\n{:.1} MB", bytes as f64 / 1_048_576.0))
                                            .size(13.0),
                                    ),
                                );
                                if response.clicked() {
                                    select_video = Some(video.clone());
                                }
                                ui.add_space(8.0);
                            }
                        });
                    });
                } else if let Some(selected_path) = self.video_library_selected.as_ref().cloned() {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(selected_path.file_name().and_then(|name| name.to_str()).unwrap_or("video"))
                                .strong(),
                        );
                        ui.add_space(6.0);
                        if let Some(texture) = &self.video_library_preview_texture {
                            ui.add(
                                Image::new(texture)
                                    .max_width(ui.available_width())
                                    .max_height((ui.available_height() * 0.62).max(220.0)),
                            );
                        } else if self.video_library_preview_rx.is_some() {
                            ui.add(egui::Spinner::new());
                            ui.label(RichText::new("Loading preview frame…").weak());
                        } else {
                            ui.label(RichText::new("Preview frame unavailable. Select Preview frame to try again.").weak());
                        }
                        if let Some(preview) = &self.video_library_preview {
                            let duration = preview.duration_seconds.max(0.1);
                            self.video_library_trim_end_seconds = self
                                .video_library_trim_end_seconds
                                .clamp(self.video_library_trim_start_seconds, duration);
                            ui.label(
                                RichText::new(format!(
                                    "{}  •  {:.1} MB",
                                    Self::format_video_seconds(preview.duration_seconds),
                                    preview.file_size as f64 / 1_048_576.0,
                                ))
                                .size(12.0)
                                .weak(),
                            );
                            Self::render_video_trim_timeline(
                                ui,
                                duration,
                                &mut self.video_library_trim_start_seconds,
                                &mut self.video_library_trim_end_seconds,
                                &mut self.video_library_playback_position_seconds,
                            );
                            ui.horizontal(|ui| {
                                if ui.button("Preview frame").clicked() {
                                    refresh_preview = Some((
                                        selected_path.clone(),
                                        self.video_library_trim_start_seconds,
                                    ));
                                }
                                if ui.button("Copy video").clicked() {
                                    copy_video = Some(selected_path.clone());
                                }
                                if ui.button("Open file location").clicked() {
                                    reveal_video = Some(selected_path.clone());
                                }
                                if ui.add_enabled(!crate::video_recorder::is_editing(), Button::new("Export trim")).clicked() {
                                    export_request = Some((
                                        selected_path.clone(),
                                        self.video_library_trim_start_seconds,
                                        self.video_library_trim_end_seconds,
                                        None,
                                    ));
                                }
                                ui.label("Compress to");
                                ui.add_sized(
                                    [70.0, 22.0],
                                    egui::DragValue::new(&mut self.video_library_target_size_mb)
                                        .range(1..=2048)
                                        .suffix(" MB"),
                                );
                                if ui.add_enabled(!crate::video_recorder::is_editing(), Button::new("Compress")).clicked() {
                                    export_request = Some((
                                        selected_path,
                                        self.video_library_trim_start_seconds,
                                        self.video_library_trim_end_seconds,
                                        Some(self.video_library_target_size_mb),
                                    ));
                                }
                            });
                            ui.label(
                                RichText::new("Drag either handle on the timeline to choose the clip. Compression size is approximate.")
                                    .size(11.0)
                                    .weak(),
                            );
                        }
                        ui.label(RichText::new(crate::video_recorder::edit_status()).size(11.0).weak());
                    });
                }
            });
        });

        self.video_library_open = open;
        if return_to_library {
            self.video_library_selected = None;
            self.video_library_preview = None;
            self.video_library_preview_texture = None;
        }
        if let Some(path) = select_video {
            self.video_library_selected = Some(path.clone());
            self.video_library_preview = None;
            self.video_library_preview_texture = None;
            self.video_library_trim_start_seconds = 0.0;
            self.video_library_trim_end_seconds = 0.0;
            self.request_video_library_preview(path, 0.0);
        }
        if let Some((path, at_seconds)) = refresh_preview {
            self.request_video_library_preview(path, at_seconds);
        }
        if let Some(path) = reveal_video
            && let Err(error) = crate::platform::reveal_file_in_explorer(&path)
        {
            self.status = format!("Could not reveal video: {error}");
        }
        if let Some(path) = copy_video {
            match crate::video_recorder::copy_video_to_clipboard(&path) {
                Ok(()) => self.status = "Video copied to clipboard.".to_owned(),
                Err(error) => self.status = format!("Could not copy video: {error}"),
            }
        }
        if let Some((input, start, end, target_size_mb)) = export_request {
            crate::video_recorder::export_trim_async(
                self.paths.ffmpeg_exe.clone(),
                input,
                output_dir,
                start,
                end,
                target_size_mb,
            );
        }
        if self.video_library_preview_rx.is_some() || crate::video_recorder::is_editing() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    fn render_video_library(&mut self, ctx: &egui::Context) {
        while let Ok((path, result)) = self.video_library_thumbnail_rx.try_recv() {
            self.video_library_thumbnail_jobs.remove(&path);
            let texture = match result {
                Ok(preview) if preview.rgba.is_some() => {
                    let rgba = preview.rgba.unwrap();
                    ctx.load_texture(
                        format!("video-library-thumbnail-{}", path.display()),
                        ColorImage::from_rgba_unmultiplied(
                            [preview.width as usize, preview.height as usize],
                            &rgba,
                        ),
                        TextureOptions::LINEAR,
                    )
                }
                _ => ctx.load_texture(
                    format!("video-library-thumbnail-fallback-{}", path.display()),
                    ColorImage::from_rgba_unmultiplied(
                        [320, 180],
                        &vec![40, 40, 45, 255].repeat(320 * 180),
                    ),
                    TextureOptions::LINEAR,
                ),
            };
            self.video_library_thumbnails.insert(path, texture);
        }
        let mut playback_finished = false;
        let mut playback_reached_end = false;
        let mut playback_error = None;
        if let Some(playback) = &self.video_library_playback {
            while let Some(event) = playback.try_recv() {
                match event {
                    crate::video_recorder::VideoPlaybackEvent::Frame {
                        rgba,
                        position_seconds,
                    } => {
                        if position_seconds >= self.video_library_trim_end_seconds {
                            self.video_library_playback_position_seconds =
                                self.video_library_trim_end_seconds;
                            playback_finished = true;
                            continue;
                        }
                        let image = ColorImage::from_rgba_unmultiplied([640, 360], &rgba);
                        if let Some(texture) = &mut self.video_library_preview_texture {
                            texture.set(image, TextureOptions::LINEAR);
                        } else {
                            self.video_library_preview_texture = Some(ctx.load_texture(
                                "video-library-embedded-playback",
                                image,
                                TextureOptions::LINEAR,
                            ));
                        }
                        self.video_library_playback_position_seconds = position_seconds;
                    }
                    crate::video_recorder::VideoPlaybackEvent::Finished => {
                        playback_finished = true;
                        playback_reached_end = true;
                    }
                    crate::video_recorder::VideoPlaybackEvent::Error(error) => {
                        playback_error = Some(error);
                        playback_finished = true;
                    }
                }
            }
            playback_finished |= playback.is_finished();
        }
        if playback_reached_end {
            self.video_library_playback_position_seconds = self.video_library_trim_end_seconds;
        }
        if playback_finished {
            self.stop_video_library_playback();
        }
        if let Some(error) = playback_error {
            self.status = format!("Could not play video: {error}");
        }
        let mut prepared_finished = false;
        let mut prepared_error = None;
        if let Some((_, _, _, playback)) = &self.video_library_preloaded_playback {
            while let Some(event) = playback.try_recv() {
                match event {
                    crate::video_recorder::VideoPlaybackEvent::Frame {
                        rgba,
                        position_seconds,
                    } => {
                        let image = ColorImage::from_rgba_unmultiplied([640, 360], &rgba);
                        if let Some(texture) = &mut self.video_library_preview_texture {
                            texture.set(image, TextureOptions::LINEAR);
                        } else {
                            self.video_library_preview_texture = Some(ctx.load_texture(
                                "video-library-prebuffered-frame",
                                image,
                                TextureOptions::LINEAR,
                            ));
                        }
                        self.video_library_playback_position_seconds = position_seconds;
                    }
                    crate::video_recorder::VideoPlaybackEvent::Finished => {
                        prepared_finished = true;
                    }
                    crate::video_recorder::VideoPlaybackEvent::Error(error) => {
                        prepared_error = Some(error);
                        prepared_finished = true;
                    }
                }
            }
            prepared_finished |= playback.is_finished();
        }
        if prepared_finished {
            self.video_library_preloaded_playback = None;
        }
        if let Some(error) = prepared_error {
            self.status = format!("Could not prepare video preview: {error}");
        }
        let delete_result = self
            .video_library_delete_rx
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        if let Some((path, result)) = delete_result {
            self.video_library_delete_rx = None;
            match result {
                Ok(()) => {
                    self.video_library_thumbnails.remove(&path);
                    if self.video_library_selected.as_ref() == Some(&path) {
                        self.video_library_selected = None;
                        self.video_library_preview = None;
                        self.video_library_preview_texture = None;
                    }
                    self.status =
                        Self::tr_lang(self.state.ui_language, "Video deleted.", "Đã xóa video.")
                            .to_owned();
                }
                Err(error) => self.status = format!("Could not delete video: {error}"),
            }
        }
        if let Some(rx) = &self.video_library_preview_rx {
            match rx.try_recv() {
                Ok((path, Ok(preview))) => {
                    let texture = preview.rgba.as_ref().map(|rgba| {
                        ctx.load_texture(
                            format!("video-library-thumbnail-{}", path.display()),
                            ColorImage::from_rgba_unmultiplied(
                                [preview.width as usize, preview.height as usize],
                                rgba,
                            ),
                            TextureOptions::LINEAR,
                        )
                    });
                    if let Some(texture) = &texture {
                        self.video_library_thumbnails
                            .insert(path.clone(), texture.clone());
                    }
                    if self.video_library_selected.as_ref() == Some(&path) {
                        self.video_library_trim_start_seconds = self
                            .video_library_trim_start_seconds
                            .clamp(0.0, preview.duration_seconds);
                        self.video_library_trim_end_seconds = if self.video_library_trim_end_seconds
                            <= self.video_library_trim_start_seconds
                        {
                            preview.duration_seconds
                        } else {
                            self.video_library_trim_end_seconds.clamp(
                                self.video_library_trim_start_seconds,
                                preview.duration_seconds,
                            )
                        };
                        self.video_library_preview_texture = preview.rgba.as_ref().map(|rgba| {
                            ctx.load_texture(
                                format!("video-library-preview-{}", path.display()),
                                ColorImage::from_rgba_unmultiplied(
                                    [preview.width as usize, preview.height as usize],
                                    rgba,
                                ),
                                TextureOptions::LINEAR,
                            )
                        });
                        self.video_library_preview = Some(preview);
                    }
                    self.video_library_preview_rx = None;
                }
                Ok((_, Err(error))) => {
                    self.status = format!("Video preview failed: {error}");
                    self.video_library_preview_rx = None;
                }
                Err(TryRecvError::Disconnected) => self.video_library_preview_rx = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if !self.video_library_open {
            self.stop_video_library_playback();
            return;
        }
        self.render_modal_backdrop(ctx, true);

        let output_dir = PathBuf::from(&self.state.quick_video_record_output_dir);
        let videos = crate::video_recorder::recorded_videos(&output_dir);
        if self
            .video_library_selected
            .as_ref()
            .is_some_and(|path| !path.is_file())
        {
            self.video_library_selected = None;
            self.video_library_preview = None;
            self.video_library_preview_texture = None;
        }

        let mut open = self.video_library_open;
        let mut close_library = false;
        let mut select_video = None;
        let mut toggle_playback = None;
        let mut reveal_video = None;
        let mut copy_video = None;
        let mut delete_video = None;
        let mut export_request = None;
        let language = self.state.ui_language;
        if ctx.input(|input| input.key_pressed(egui::Key::Space))
            && !ctx.wants_keyboard_input()
            && let Some(path) = self.video_library_selected.as_ref()
            && self.video_library_preview.is_some()
        {
            toggle_playback = Some(path.clone());
        }
        let library_bounds = ctx.content_rect().shrink2(vec2(24.0, 18.0));
        // Window::fixed_size is the content size; leave room for the frame margins too.
        let library_content_size = library_bounds.size() - vec2(24.0, 20.0);
        egui::Window::new("video-library")
            .order(Order::Foreground)
            .fixed_pos(library_bounds.min)
            .fixed_size(library_content_size)
            .constrain_to(library_bounds)
            .resizable(false)
            .collapsible(false)
            .title_bar(false)
            .frame(Frame::window(ctx.style().as_ref()).corner_radius(14.0))
            .show(ctx, |ui| {
                ui.scope(|ui| {
                    ui.style_mut()
                        .text_styles
                        .insert(egui::TextStyle::Body, egui::FontId::proportional(13.0));
                    ui.style_mut()
                        .text_styles
                        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(Self::tr_lang(
                                language,
                                "Video library",
                                "Thư viện video",
                            ))
                            .strong()
                            .size(18.0),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui
                                    .add_sized(
                                        [36.0, 30.0],
                                        Button::new(RichText::new("×").strong().size(20.0))
                                            .fill(Color32::from_rgb(174, 55, 76))
                                            .corner_radius(7.0),
                                    )
                                    .on_hover_text(Self::tr_lang(
                                        language,
                                        "Close video library",
                                        "Đóng thư viện video",
                                    ))
                                    .clicked()
                                {
                                    close_library = true;
                                }
                            },
                        );
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(Self::tr_lang(
                                language,
                                "Recorded videos",
                                "Video đã quay",
                            ))
                            .strong(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                videos.len(),
                                Self::tr_lang(language, "videos", "video")
                            ))
                            .size(12.0)
                            .weak(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(Self::tr_lang(language, "Open folder", "Mở thư mục"))
                                .clicked()
                            {
                                let _ = fs::create_dir_all(&output_dir);
                                if let Err(error) = crate::platform::open_folder_in_explorer(&output_dir) {
                                    self.status = format!("Could not open video folder: {error}");
                                }
                            }
                        });
                    });
                    ui.separator();
                    let content_rect = ui.available_rect_before_wrap();
                    let divider_width = 10.0;
                    let left_width = (content_rect.width() * 0.52)
                        .clamp(320.0, (content_rect.width() - 300.0).max(320.0));
                    let left_rect = egui::Rect::from_min_max(
                        content_rect.min,
                        pos2(content_rect.left() + left_width, content_rect.bottom()),
                    );
                    let right_rect = egui::Rect::from_min_max(
                        pos2(left_rect.right() + divider_width, content_rect.top()),
                        content_rect.max,
                    );
                    ui.allocate_rect(content_rect, Sense::hover());
                    let mut left_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(left_rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    let mut right_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(right_rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    ui.painter().line_segment(
                        [
                            pos2(left_rect.right() + divider_width * 0.5, content_rect.top()),
                            pos2(left_rect.right() + divider_width * 0.5, content_rect.bottom()),
                        ],
                        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                    );
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(&mut left_ui, |ui| {
                            ui.set_width(left_rect.width());
                            if videos.is_empty() {
                                ui.label(
                                    RichText::new(Self::tr_lang(
                                        language,
                                        "No recorded videos yet.",
                                        "Chưa có video nào.",
                                    ))
                                    .weak(),
                                );
                            }
                            let card_spacing = 8.0;
                            let card_width = ((left_rect.width() - card_spacing - 14.0) * 0.5)
                                .max(120.0);
                            egui::Grid::new("video-library-thumbnail-grid")
                                .num_columns(2)
                                .spacing([card_spacing, card_spacing])
                                .min_col_width(card_width)
                                .max_col_width(card_width)
                                .show(ui, |ui| {
                                for (index, video) in videos.iter().enumerate() {
                                    let thumbnail_size = vec2(
                                        (card_width - 10.0).max(108.0),
                                        ((card_width - 10.0) * 9.0 / 16.0).clamp(72.0, 132.0),
                                    );
                                    let name = video.file_name().and_then(|name| name.to_str()).unwrap_or("video");
                                    let bytes = fs::metadata(video).map(|metadata| metadata.len()).unwrap_or(0);
                                    let selected = self.video_library_selected.as_ref() == Some(video);
                                    let copied = self
                                        .video_library_copy_feedback
                                        .as_ref()
                                        .is_some_and(|(path, at)| {
                                            path == video && at.elapsed() < Duration::from_secs(2)
                                        });
                                    let card = Frame::group(ui.style())
                                        .fill(if selected { Color32::from_rgb(37, 107, 82) } else { Color32::TRANSPARENT })
                                        .show(ui, |ui| {
                                            let mut card_clicked = false;
                                            let mut copy_clicked = false;
                                            let mut delete_clicked = false;
                                            ui.set_min_width(card_width - 8.0);
                                            ui.set_max_width(card_width - 8.0);
                                            ui.with_layout(
                                                egui::Layout::top_down(egui::Align::Center),
                                                |ui| {
                                                ui.set_min_width(card_width - 8.0);
                                                ui.set_max_width(card_width - 8.0);
                                                if let Some(texture) = self.video_library_thumbnails.get(video) {
                                                    card_clicked |= ui
                                                        .add(
                                                            Image::new(texture)
                                                                .fit_to_exact_size(thumbnail_size)
                                                                .sense(Sense::click()),
                                                        )
                                                        .clicked();
                                                } else {
                                                    card_clicked |= ui
                                                        .add_sized(
                                                        thumbnail_size,
                                                        egui::Label::new(
                                                            RichText::new(Self::tr_lang(
                                                                language,
                                                                "Loading thumbnail...",
                                                                "Đang tải ảnh thu nhỏ...",
                                                            ))
                                                            .weak(),
                                                        )
                                                        .sense(Sense::click()),
                                                    )
                                                        .clicked();
                                                }
                                                card_clicked |= ui.add_sized(
                                                    [thumbnail_size.x, 18.0],
                                                    egui::Label::new(RichText::new(name).size(12.0))
                                                        .truncate()
                                                        .sense(Sense::click()),
                                                ).clicked();
                                                card_clicked |= ui.add_sized(
                                                    [thumbnail_size.x, 16.0],
                                                    egui::Label::new(
                                                        RichText::new(format!(
                                                            "{:.1} MB",
                                                            bytes as f64 / 1_048_576.0,
                                                        ))
                                                        .size(11.0)
                                                        .weak(),
                                                    )
                                                    .sense(Sense::click()),
                                                ).clicked();
                                                ui.horizontal(|ui| {
                                                    copy_clicked = ui
                                                        .add_sized(
                                                            [28.0, 26.0],
                                                            Button::new(
                                                                Self::material_icon_text(
                                                                    if copied {
                                                                        0xe5ca
                                                                    } else {
                                                                        0xe14d
                                                                    },
                                                                    15.0,
                                                                ),
                                                            ),
                                                        )
                                                        .on_hover_text(if copied {
                                                            Self::tr_lang(
                                                                language,
                                                                "Copied",
                                                                "Đã sao chép",
                                                            )
                                                        } else {
                                                            Self::tr_lang(
                                                                language,
                                                                "Copy video",
                                                                "Sao chép video",
                                                            )
                                                        })
                                                        .clicked();
                                                    delete_clicked = ui
                                                        .add_sized(
                                                            [28.0, 26.0],
                                                            Button::new(
                                                                Self::material_icon_text(
                                                                    0xe872, 15.0,
                                                                ),
                                                            ),
                                                        )
                                                        .on_hover_text(Self::tr_lang(
                                                            language,
                                                            "Delete video",
                                                            "Xóa video",
                                                        ))
                                                        .clicked();
                                                    if copied {
                                                        ui.label(
                                                            RichText::new(Self::tr_lang(
                                                                language,
                                                                "Copied",
                                                                "Đã sao chép",
                                                            ))
                                                            .size(11.0)
                                                            .weak(),
                                                        );
                                                    }
                                                });
                                            });
                                            (card_clicked, copy_clicked, delete_clicked)
                                        });
                                    if card.inner.0 {
                                        select_video = Some(video.clone());
                                    }
                                    if card.inner.1 {
                                        copy_video = Some(video.clone());
                                    }
                                    if card.inner.2 {
                                        delete_video = Some(video.clone());
                                    }
                                    if index % 2 == 1 {
                                        ui.end_row();
                                    }
                                }
                            });
                        });

                        right_ui.vertical_centered(|ui| {
                            let Some(selected_path) = self.video_library_selected.as_ref().cloned() else {
                                ui.add_space(100.0);
                                ui.label(
                                    RichText::new(Self::tr_lang(
                                        language,
                                        "Select a video on the left.",
                                        "Chọn một video ở bên trái.",
                                    ))
                                    .weak(),
                                );
                                return;
                            };
                            ui.label(
                                RichText::new(selected_path.file_name().and_then(|name| name.to_str()).unwrap_or("video"))
                                    .strong(),
                            );
                            ui.add_space(6.0);
                            if let Some(texture) = &self.video_library_preview_texture {
                                let preview_size = vec2(
                                    ui.available_width().max(1.0),
                                    (ui.available_width() * 9.0 / 16.0).min(270.0),
                                );
                                ui.add(Image::new(texture).fit_to_exact_size(preview_size));
                            } else if self.video_library_preview_rx.is_some() {
                                ui.add(egui::Spinner::new());
                                ui.label(
                                    RichText::new(Self::tr_lang(
                                        language,
                                        "Loading preview frame...",
                                        "Đang tải khung xem trước...",
                                    ))
                                    .weak(),
                                );
                            } else {
                                ui.label(
                                    RichText::new(Self::tr_lang(
                                        language,
                                        "Preview frame unavailable.",
                                        "Không thể xem trước khung hình.",
                                    ))
                                    .weak(),
                                );
                            }
                            let playback_buffering = self
                                .video_library_playback
                                .as_ref()
                                .is_some_and(|playback| !playback.is_ready());
                            let seek_buffering = self
                                .video_library_preloaded_playback
                                .as_ref()
                                .is_some_and(|(path, _, _, playback)| {
                                    path == &selected_path && !playback.is_ready()
                                });
                            let video_buffering = playback_buffering || seek_buffering;
                            let (loading_rect, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 24.0),
                                Sense::hover(),
                            );
                            if video_buffering {
                                let mut loading_ui = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(loading_rect)
                                        .layout(egui::Layout::left_to_right(
                                            egui::Align::Center,
                                        )),
                                );
                                loading_ui.add(egui::Spinner::new());
                                loading_ui.label(
                                    RichText::new(Self::tr_lang(
                                        language,
                                        "Loading preview...",
                                        "Đang tải xem trước...",
                                    ))
                                    .weak(),
                                );
                            }
                            ui.horizontal_wrapped(|ui| {
                                let playing = self.video_library_playback_path.as_ref()
                                    == Some(&selected_path);
                                if ui
                                    .button(if playing && video_buffering {
                                        Self::tr_lang(language, "Stop loading", "Dừng tải")
                                    } else if playing {
                                        Self::tr_lang(language, "Stop video", "Dừng video")
                                    } else {
                                        Self::tr_lang(language, "Play video", "Phát video")
                                    })
                                    .clicked()
                                {
                                    toggle_playback = Some(selected_path.clone());
                                }
                                if ui
                                    .button(Self::tr_lang(
                                        language,
                                        "Open file location",
                                        "Mở vị trí tệp",
                                    ))
                                    .clicked()
                                {
                                    reveal_video = Some(selected_path.clone());
                                    self.status = "Opening file location\u{2026}".to_owned();
                                }
                            });
                            if let Some(preview) = &self.video_library_preview {
                                let duration = preview.duration_seconds.max(0.1);
                                self.video_library_trim_end_seconds = self
                                    .video_library_trim_end_seconds
                                    .clamp(self.video_library_trim_start_seconds, duration);
                                ui.label(
                                    RichText::new(format!(
                                        "{}  -  {:.1} MB{}",
                                        Self::format_video_seconds(preview.duration_seconds),
                                        preview.file_size as f64 / 1_048_576.0,
                                        if self.video_library_playback_path.as_ref()
                                            == Some(&selected_path)
                                        {
                                            format!(
                                                "  -  Playing {}",
                                                Self::format_video_seconds(
                                                    self.video_library_playback_position_seconds,
                                                )
                                            )
                                        } else {
                                            String::new()
                                        },
                                    ))
                                    .size(12.0)
                                    .weak(),
                                );
                                let (playhead_changed, timeline_interacting) =
                                    Self::render_video_trim_timeline(
                                    ui,
                                    duration,
                                    &mut self.video_library_trim_start_seconds,
                                    &mut self.video_library_trim_end_seconds,
                                    &mut self.video_library_playback_position_seconds,
                                );
                                if timeline_interacting
                                    && self.video_library_playback.is_some()
                                {
                                    self.stop_video_library_playback();
                                }
                                if playhead_changed {
                                    let prepared_still_matches = self
                                        .video_library_preloaded_playback
                                        .as_ref()
                                        .is_some_and(|(path, start, _, playback)| {
                                            path == &selected_path
                                                && (*start
                                                    - self
                                                        .video_library_playback_position_seconds)
                                                    .abs()
                                                    < 0.001
                                                && !playback.is_finished()
                                        });
                                    if !prepared_still_matches {
                                        self.video_library_preloaded_playback = None;
                                    }
                                }
                                ui.horizontal_wrapped(|ui| {
                                    if ui.add_enabled(
                                        !crate::video_recorder::is_editing(),
                                        Button::new(Self::tr_lang(
                                            language,
                                            "Export trim",
                                            "Xuất đoạn cắt",
                                        )),
                                    ).clicked() {
                                        export_request = Some((
                                            selected_path.clone(),
                                            self.video_library_trim_start_seconds,
                                            self.video_library_trim_end_seconds,
                                            None,
                                        ));
                                    }
                                    ui.label(Self::tr_lang(
                                        language,
                                        "Compress to",
                                        "Nén xuống",
                                    ));
                                    ui.add_sized(
                                        [70.0, 22.0],
                                        egui::DragValue::new(&mut self.video_library_target_size_mb)
                                            .range(1..=2048)
                                            .suffix(" MB"),
                                    );
                                    if ui.add_enabled(
                                        !crate::video_recorder::is_editing(),
                                        Button::new(Self::tr_lang(
                                            language,
                                            "Compress",
                                            "Nén",
                                        )),
                                    ).clicked() {
                                        export_request = Some((
                                            selected_path,
                                            self.video_library_trim_start_seconds,
                                            self.video_library_trim_end_seconds,
                                            Some(self.video_library_target_size_mb),
                                        ));
                                    }
                                });
                                ui.label(
                                    RichText::new(Self::tr_lang(
                                        language,
                                        "Export trim is fast but starts at the nearest keyframe. Compress re-encodes and is slower.",
                                        "Xuất đoạn cắt nhanh nhưng bắt đầu ở keyframe gần nhất. Nén sẽ mã hóa lại nên chậm hơn.",
                                    ))
                                        .size(11.0)
                                        .weak(),
                                );
                            }
                            if let Some(progress) = crate::video_recorder::edit_progress() {
                                ui.add(
                                    egui::ProgressBar::new(progress)
                                        .show_percentage()
                                        .animate(true)
                                        .desired_width(ui.available_width()),
                                );
                            }
                            ui.label(RichText::new(crate::video_recorder::edit_status()).size(11.0).weak());
                        });
                });
            });

        if close_library || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        self.video_library_open = open;
        if !open {
            self.stop_video_library_playback();
            self.video_library_preloaded_playback = None;
        }
        if let Some(path) = select_video {
            self.stop_video_library_playback();
            self.video_library_preloaded_playback = None;
            audio::preload_video_audio_preview_async(path.to_string_lossy().into_owned());
            self.video_library_selected = Some(path.clone());
            self.video_library_preview = None;
            self.video_library_preview_texture = None;
            self.video_library_trim_start_seconds = 0.0;
            self.video_library_trim_end_seconds = 0.0;
            self.video_library_playback_position_seconds = 0.0;
            let source_mb = fs::metadata(&path)
                .map(|metadata| metadata.len() as f64 / 1_048_576.0)
                .unwrap_or(1.0);
            self.video_library_target_size_mb =
                (source_mb * 0.65).round().clamp(1.0, 2048.0) as u32;
            self.video_library_pending_preview = Some((path, 0.5));
        }
        if let Some(path) = toggle_playback {
            if self.video_library_playback_path.as_ref() == Some(&path) {
                self.stop_video_library_playback();
            } else {
                self.stop_video_library_playback();
                let playback_start = if self.video_library_playback_position_seconds
                    >= self.video_library_trim_end_seconds - 0.05
                {
                    self.video_library_trim_start_seconds
                } else {
                    self.video_library_playback_position_seconds.clamp(
                        self.video_library_trim_start_seconds,
                        self.video_library_trim_end_seconds,
                    )
                };
                let playback_end = self.video_library_trim_end_seconds;
                let prepared = self
                    .video_library_preloaded_playback
                    .take()
                    .filter(|(prepared_path, prepared_start, _, playback)| {
                        prepared_path == &path
                            && (*prepared_start - playback_start).abs() < 0.001
                            && !playback.is_finished()
                    })
                    .map(|(_, _, _, playback)| {
                        playback.play();
                        playback
                    });
                let playback = prepared.map(Ok).unwrap_or_else(|| {
                    crate::video_recorder::start_video_library_playback(
                        self.paths.ffmpeg_exe.clone(),
                        path.clone(),
                        playback_start,
                        playback_end,
                    )
                });
                match playback {
                    Ok(playback) => {
                        audio::play_video_audio_preview_async(
                            path.to_string_lossy().into_owned(),
                            (playback_start * 1000.0).round().max(0.0) as u64,
                            (self.video_library_trim_end_seconds * 1000.0)
                                .round()
                                .max(1.0) as u64,
                        );
                        self.video_library_playback = Some(playback);
                        self.video_library_playback_path = Some(path);
                        self.video_library_playback_position_seconds = playback_start;
                    }
                    Err(error) => self.status = format!("Could not play video: {error}"),
                }
            }
        }
        if let Some(path) = reveal_video
            && let Err(error) = crate::platform::reveal_file_in_explorer(&path)
        {
            self.status = format!("Could not reveal video: {error}");
        }
        if let Some(path) = copy_video {
            match crate::video_recorder::copy_video_to_clipboard(&path) {
                Ok(()) => {
                    self.video_library_copy_feedback = Some((path, Instant::now()));
                    self.status = "Video copied to clipboard.".to_owned();
                }
                Err(error) => self.status = format!("Could not copy video: {error}"),
            }
        }
        if let Some((input, start, end, target_size_mb)) = export_request {
            crate::video_recorder::export_trim_async(
                self.paths.ffmpeg_exe.clone(),
                input,
                output_dir,
                start,
                end,
                target_size_mb,
            );
        }
        if self.video_library_preview_rx.is_none() {
            if let Some((path, at_seconds)) = self.video_library_pending_preview.take() {
                self.request_video_library_preview(path, at_seconds);
            }
        }
        if let Some(path) = delete_video {
            self.stop_video_library_playback();
            self.video_library_preloaded_playback = None;
            let (sender, receiver) = crossbeam_channel::unbounded();
            self.video_library_delete_rx = Some(receiver);
            self.status =
                Self::tr_lang(language, "Deleting video...", "Đang xóa video...").to_owned();
            std::thread::spawn(move || {
                let mut last_error = None;
                for _ in 0..40 {
                    match fs::remove_file(&path) {
                        Ok(()) => {
                            let _ = sender.send((path, Ok(())));
                            return;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            let _ = sender.send((path, Ok(())));
                            return;
                        }
                        Err(error) => last_error = Some(error),
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                let error = last_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "The file is still in use.".to_owned());
                let _ = sender.send((path, Err(error)));
            });
        }
        if self.video_library_playback.is_none()
            && self.video_library_preloaded_playback.is_none()
            && let (Some(path), Some(preview)) = (
                self.video_library_selected.as_ref(),
                self.video_library_preview.as_ref(),
            )
        {
            let start = self.video_library_playback_position_seconds.clamp(
                self.video_library_trim_start_seconds,
                self.video_library_trim_end_seconds,
            );
            // ponytail: keep the decoder warm through the source duration; trim end is enforced
            // by the UI so resizing either trim handle does not destroy the playback buffer.
            let end = preview.duration_seconds;
            if end > start
                && let Ok(playback) = crate::video_recorder::prepare_video_library_playback(
                    self.paths.ffmpeg_exe.clone(),
                    path.clone(),
                    start,
                    end,
                )
            {
                self.video_library_preloaded_playback = Some((path.clone(), start, end, playback));
            }
        }
        while self.video_library_playback.is_none()
            && self.video_library_thumbnail_jobs.len() < 3
        {
            let Some(path) = videos.iter().find(|path| {
                !self.video_library_thumbnails.contains_key(*path)
                    && !self.video_library_thumbnail_jobs.contains(*path)
            }) else {
                break;
            };
            let path = path.clone();
            self.video_library_thumbnail_jobs.insert(path.clone());
            let sender = self.video_library_thumbnail_tx.clone();
            let ffmpeg_exe = self.paths.ffmpeg_exe.clone();
            std::thread::spawn(move || {
                let result =
                    crate::video_recorder::inspect_recorded_video_thumbnail(&ffmpeg_exe, &path);
                let _ = sender.send((path, result));
            });
        }
        if self.video_library_playback.is_some() || self.video_library_preloaded_playback.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.video_library_preview_rx.is_some()
            || self.video_library_delete_rx.is_some()
            || self
                .video_library_copy_feedback
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(2))
            || !self.video_library_thumbnail_jobs.is_empty()
            || crate::video_recorder::is_editing()
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn render_video_trim_timeline(
        ui: &mut egui::Ui,
        duration: f64,
        start: &mut f64,
        end: &mut f64,
        playhead: &mut f64,
    ) -> (bool, bool) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click_and_drag());
        let handle_id = ui.make_persistent_id("video-library-trim-handle");
        let track_rect = rect.shrink2(vec2(7.0, 0.0));
        let width = track_rect.width().max(1.0);
        let to_x = |seconds: f64| track_rect.left() + (seconds / duration) as f32 * width;
        let to_seconds =
            |x: f32| (((x - track_rect.left()) / width) as f64 * duration).clamp(0.0, duration);
        *playhead = playhead.clamp(*start, *end);
        let start_x = to_x(*start);
        let end_x = to_x(*end);
        const TRIM_HANDLE_HITBOX: f32 = 14.0;
        let mut playhead_changed = false;
        if let Some(pointer) = response.hover_pos() {
            let over_trim_handle =
                (pointer.x - start_x).abs().min((pointer.x - end_x).abs()) <= TRIM_HANDLE_HITBOX;
            ui.ctx().set_cursor_icon(if over_trim_handle {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::PointingHand
            });
            response.clone().on_hover_text(if over_trim_handle {
                "Drag this green handle to change the trim range."
            } else {
                "Click or drag to move the playhead. Press Space to play or stop."
            });
        }
        let active_handle = ui
            .ctx()
            .data(|data| data.get_temp::<VideoTrimHandle>(handle_id));
        if response.is_pointer_button_down_on() && active_handle.is_none() {
            if let Some(pointer) = ui
                .input(|input| input.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos())
            {
                let start_distance = (pointer.x - start_x).abs();
                let end_distance = (pointer.x - end_x).abs();
                let handle = if start_distance.min(end_distance) > TRIM_HANDLE_HITBOX {
                    VideoTrimHandle::Playhead
                } else if start_distance <= end_distance {
                    VideoTrimHandle::Start
                } else {
                    VideoTrimHandle::End
                };
                if handle == VideoTrimHandle::Playhead {
                    *playhead = to_seconds(pointer.x).clamp(*start, *end);
                    playhead_changed = true;
                }
                ui.ctx()
                    .data_mut(|data| data.insert_temp(handle_id, handle));
            }
        }
        if response.clicked()
            && let Some(pointer) = response.interact_pointer_pos()
            && (pointer.x - start_x).abs().min((pointer.x - end_x).abs()) > TRIM_HANDLE_HITBOX
        {
            *playhead = to_seconds(pointer.x).clamp(*start, *end);
            playhead_changed = true;
        }
        if response.dragged() {
            if let (Some(handle), Some(pointer)) = (
                ui.ctx()
                    .data(|data| data.get_temp::<VideoTrimHandle>(handle_id)),
                response.interact_pointer_pos(),
            ) {
                match handle {
                    VideoTrimHandle::Start => {
                        *start = to_seconds(pointer.x).min((*end - 0.05).max(0.0))
                    }
                    VideoTrimHandle::End => {
                        *end = to_seconds(pointer.x).max((*start + 0.05).min(duration))
                    }
                    VideoTrimHandle::Playhead => {
                        *playhead = to_seconds(pointer.x).clamp(*start, *end);
                        playhead_changed = true;
                    }
                }
            }
        }
        let timeline_interacting = response.is_pointer_button_down_on() || response.dragged();
        let timeline_committed =
            (playhead_changed && response.clicked()) || response.drag_stopped();
        *playhead = playhead.clamp(*start, *end);
        if !ui.input(|input| input.pointer.primary_down()) {
            ui.ctx()
                .data_mut(|data| data.remove::<VideoTrimHandle>(handle_id));
        }
        let start_x = to_x(*start);
        let end_x = to_x(*end);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(41, 45, 54));
        painter.rect_filled(
            egui::Rect::from_x_y_ranges(start_x..=end_x, rect.top() + 12.0..=rect.bottom() - 14.0),
            3.0,
            Color32::from_rgb(56, 153, 106),
        );
        for x in [start_x, end_x] {
            painter.line_segment(
                [pos2(x, rect.top() + 6.0), pos2(x, rect.bottom() - 8.0)],
                Stroke::new(2.0, Color32::from_rgb(113, 214, 162)),
            );
            painter.rect_filled(
                egui::Rect::from_center_size(pos2(x, rect.center().y), vec2(10.0, 22.0)),
                3.0,
                Color32::from_rgb(113, 214, 162),
            );
        }
        let playhead_x = to_x(*playhead);
        painter.line_segment(
            [
                pos2(playhead_x, rect.top() + 4.0),
                pos2(playhead_x, rect.bottom() - 6.0),
            ],
            Stroke::new(2.0, Color32::WHITE),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(playhead_x - 5.0, rect.top() + 4.0),
                pos2(playhead_x + 5.0, rect.top() + 4.0),
                pos2(playhead_x, rect.top() + 11.0),
            ],
            Color32::WHITE,
            Stroke::NONE,
        ));
        painter.text(
            pos2(rect.left() + 6.0, rect.top() + 2.0),
            egui::Align2::LEFT_TOP,
            Self::format_video_seconds(*start),
            egui::TextStyle::Small.resolve(ui.style()),
            Color32::WHITE,
        );
        painter.text(
            pos2(rect.right() - 6.0, rect.top() + 2.0),
            egui::Align2::RIGHT_TOP,
            Self::format_video_seconds(*end),
            egui::TextStyle::Small.resolve(ui.style()),
            Color32::WHITE,
        );
        (timeline_committed, timeline_interacting)
    }

    fn format_video_seconds(seconds: f64) -> String {
        let total_seconds = seconds.max(0.0).round() as u64;
        format!(
            "{:02}:{:02}:{:02}",
            total_seconds / 3600,
            (total_seconds / 60) % 60,
            total_seconds % 60
        )
    }

    fn render_multi_window_targets(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        label_when_none: &str,
        primary: &mut Option<String>,
        extras: &mut Vec<String>,
        open_windows: &[WindowInfo],
    ) -> bool {
        let mut changed = false;
        let extras_expanded_id =
            ui.make_persistent_id((id_source, "extra-target-windows-expanded"));
        let mut extras_expanded = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(extras_expanded_id))
            .unwrap_or(false);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.y = 21.0;
                let missing_primary = primary.as_ref().is_some_and(|current| {
                    !open_windows
                        .iter()
                        .any(|window| &window.selector == current)
                });
                if missing_primary {
                    *primary = None;
                    changed = true;
                }
                let display_primary = if missing_primary {
                    label_when_none.to_owned()
                } else {
                    primary
                        .as_deref()
                        .map(|current| Self::display_title_for_selector(current, open_windows))
                        .unwrap_or_else(|| label_when_none.to_owned())
                };
                let truncated_primary = Self::truncate_window_title(&display_primary, 40);
                ui.scope(|ui| {
                    if missing_primary || primary.is_none() {
                        let stroke = egui::Stroke::new(1.0, Color32::from_rgb(185, 82, 82));
                        ui.style_mut().visuals.widgets.inactive.bg_stroke = stroke;
                        ui.style_mut().visuals.widgets.hovered.bg_stroke = stroke;
                    }
                    egui::ComboBox::from_id_salt((id_source, "primary-target-window"))
                        .width(320.0)
                        .selected_text(truncated_primary)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(primary.is_none(), label_when_none)
                                .clicked()
                            {
                                *primary = None;
                                changed = true;
                            }
                            for window in open_windows {
                                let selector = &window.selector;
                                let display_title = Self::simplify_window_title(&window.title);
                                let truncated_title =
                                    Self::truncate_window_title(&display_title, 50);
                                if ui
                                    .selectable_label(
                                        primary.as_deref() == Some(selector),
                                        truncated_title,
                                    )
                                    .on_hover_text(selector)
                                    .clicked()
                                {
                                    *primary = Some(selector.clone());
                                    changed = true;
                                }
                            }
                        });
                });

                let add_btn = Button::new(Self::material_icon_text(0xe145, 12.0));
                if ui
                    .add_sized([24.0, 21.0], add_btn)
                    .on_hover_text(Self::tr_lang(language, "+ Window", "+ Window"))
                    .clicked()
                {
                    let next = open_windows
                        .iter()
                        .find(|window| {
                            primary.as_deref() != Some(window.selector.as_str())
                                && !extras.iter().any(|existing| existing == &window.selector)
                        })
                        .map(|window| window.selector.clone())
                        .or_else(|| open_windows.first().map(|window| window.selector.clone()))
                        .unwrap_or_default();
                    if !next.is_empty() {
                        extras.push(next);
                        extras_expanded = true;
                        changed = true;
                    }
                }
                if !extras.is_empty() {
                    let toggle_icon = if extras_expanded { 0xe5cf } else { 0xe5cc };
                    if ui
                        .add_sized(
                            [24.0, 21.0],
                            Button::new(Self::material_icon_text(toggle_icon, 14.0)),
                        )
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Show or hide the extra target windows list.",
                            "Show or hide the extra target windows list.",
                        ))
                        .clicked()
                    {
                        extras_expanded = !extras_expanded;
                    }
                }
            });

            if extras_expanded {
                let mut remove_index = None;
                for (index, extra) in extras.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().interact_size.y = 21.0;
                        let display_extra = Self::display_title_for_selector(extra, open_windows);
                        let truncated_extra = Self::truncate_window_title(&display_extra, 40);
                        egui::ComboBox::from_id_salt((id_source, "extra-target-window", index))
                            .width(320.0)
                            .selected_text(truncated_extra)
                            .show_ui(ui, |ui| {
                                for window in open_windows {
                                    let selector = &window.selector;
                                    let display_title = Self::simplify_window_title(&window.title);
                                    let truncated_title =
                                        Self::truncate_window_title(&display_title, 50);
                                    if ui
                                        .selectable_label(extra == selector, truncated_title)
                                        .on_hover_text(selector)
                                        .clicked()
                                    {
                                        *extra = selector.clone();
                                        changed = true;
                                    }
                                }
                            });
                        let remove_btn = Button::new(Self::material_icon_text(0xe14c, 12.0));
                        if ui.add_sized([24.0, 21.0], remove_btn).clicked() {
                            remove_index = Some(index);
                        }
                    });
                }
                if let Some(index) = remove_index {
                    extras.remove(index);
                    changed = true;
                    if extras.is_empty() {
                        extras_expanded = false;
                    }
                }
            }
        });
        ui.ctx()
            .data_mut(|data| data.insert_temp(extras_expanded_id, extras_expanded));
        changed
    }

    fn selector_base_title(target: &str) -> &str {
        crate::window_list::selector_base_title(target)
    }

    fn grouped_window_selectors(open_windows: &[WindowInfo]) -> Vec<(String, Vec<String>)> {
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for window in open_windows {
            let selector = &window.selector;
            let title = Self::simplify_window_title(&window.title);
            if let Some((_, selectors)) = groups
                .iter_mut()
                .find(|(existing_title, _)| existing_title == &title)
            {
                if !selectors.iter().any(|existing| existing == selector) {
                    selectors.push(selector.clone());
                }
            } else {
                groups.push((title, vec![selector.clone()]));
            }
        }
        groups
    }

    fn display_title_for_selector(selector: &str, open_windows: &[WindowInfo]) -> String {
        open_windows
            .iter()
            .find(|window| window.selector == selector)
            .map(|window| Self::simplify_window_title(&window.title))
            .unwrap_or_else(|| Self::simplify_window_title(selector))
    }

    fn process_icon_texture(ctx: &egui::Context, path: &str) -> Option<TextureHandle> {
        if path.is_empty() {
            return None;
        }
        if let Some(cached) = PROCESS_ICON_TEXTURES.lock().get(path).cloned() {
            return cached;
        }
        let texture = window_list::process_icon_rgba(path).map(|rgba| {
            ctx.load_texture(
                format!("process-icon:{path}"),
                ColorImage::from_rgba_unmultiplied([16, 16], &rgba),
                TextureOptions::LINEAR,
            )
        });
        PROCESS_ICON_TEXTURES
            .lock()
            .insert(path.to_owned(), texture.clone());
        texture
    }

    fn lazy_process_path(pid: u32, known_path: &str) -> String {
        if !known_path.is_empty() {
            return known_path.to_owned();
        }
        if let Some(path) = PROCESS_PATHS.lock().get(&pid).cloned() {
            return path;
        }
        let path = crate::memory_debugger::debugger::process_path(pid);
        PROCESS_PATHS.lock().insert(pid, path.clone());
        path
    }

    fn selectable_process_row(
        ui: &mut egui::Ui,
        selected: bool,
        label: impl Into<egui::WidgetText>,
        pid: u32,
        path: &str,
    ) -> egui::Response {
        let width = ui.available_width();
        let (rect, slot) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
        if selected || ui.rect_contains_pointer(rect) {
            let color = if selected {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().widgets.hovered.bg_fill
            };
            ui.painter().rect_filled(rect, 2.0, color);
        }
        let path = Self::lazy_process_path(pid, path);
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.horizontal(|ui| {
            if let Some(texture) = Self::process_icon_texture(ui.ctx(), &path) {
                ui.add(Image::new((texture.id(), vec2(16.0, 16.0))));
            } else {
                ui.label(Self::material_icon_text(0xe30a, 16.0));
            }
            ui.add(egui::Label::new(label).selectable(false));
        });
        ui.interact(rect, slot.id.with("process-row"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::Default)
    }

    fn selectable_process_detail_row(
        ui: &mut egui::Ui,
        selected: bool,
        name: &str,
        pid: u32,
        path: &str,
    ) -> egui::Response {
        let width = ui.available_width();
        let (rect, slot) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
        if selected || ui.rect_contains_pointer(rect) {
            let color = if selected {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().widgets.hovered.bg_fill
            };
            ui.painter().rect_filled(rect, 2.0, color);
        }
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.horizontal(|ui| {
            if let Some(texture) = Self::process_icon_texture(ui.ctx(), path) {
                ui.add(Image::new((texture.id(), vec2(16.0, 16.0))));
            } else {
                ui.label(Self::material_icon_text(0xe30a, 16.0));
            }
            ui.add_sized(
                [190.0, 20.0],
                egui::Label::new(name).selectable(false).truncate(),
            );
            ui.add_sized(
                [70.0, 20.0],
                egui::Label::new(pid.to_string()).selectable(false),
            );
            ui.add_sized(
                [ui.available_width(), 20.0],
                egui::Label::new(path).selectable(false).truncate(),
            );
        });
        ui.interact(rect, slot.id.with("process-detail-row"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::Default)
    }

    fn render_window_target_combo_with_duplicate_mode(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        label_when_none: &str,
        target: &mut Option<String>,
        match_duplicate_window_titles: &mut bool,
        open_windows: &[WindowInfo],
        width: f32,
        allow_none: bool,
    ) -> bool {
        let mut changed = false;
        let live_open_windows = LIVE_WINDOW_TARGET_COMBO_WINDOWS.lock().clone();
        let effective_open_windows = live_open_windows.as_deref().unwrap_or(open_windows);
        let window_groups = Self::grouped_window_selectors(effective_open_windows);
        let selected_text = target
            .as_deref()
            .map(|current| {
                let mut display = current.to_owned();
                let rules = [
                    (" [Lowest]", "[Lowest on Screen]", "[Lowest on Screen]"),
                    (" [Highest]", "[Highest on Screen]", "[Highest on Screen]"),
                    (
                        " [Leftmost]",
                        "[Leftmost on Screen]",
                        "[Leftmost on Screen]",
                    ),
                    (
                        " [Rightmost]",
                        "[Rightmost on Screen]",
                        "[Rightmost on Screen]",
                    ),
                ];
                let mut matched_rule = false;
                for (suffix, en_label, vi_label) in rules {
                    if current.ends_with(suffix) {
                        let base = current.strip_suffix(suffix).unwrap();
                        let label = Self::tr_lang(language, en_label, vi_label);
                        display = format!("{base} {label}");
                        matched_rule = true;
                        break;
                    }
                }
                if matched_rule {
                    display
                } else {
                    let base_title =
                        Self::display_title_for_selector(current, effective_open_windows);
                    let selected_specific_duplicate = !*match_duplicate_window_titles
                        && window_groups
                            .iter()
                            .any(|(title, selectors)| *title == base_title && selectors.len() > 1);
                    if selected_specific_duplicate {
                        current.to_owned()
                    } else {
                        base_title
                    }
                }
            })
            .unwrap_or(label_when_none.to_owned());
        let truncated_selected_text = Self::truncate_window_title(&selected_text, 40);
        let popup_state_id = ui.make_persistent_id((id_source, "duplicate-title-hover"));
        let mut expanded_title = ui
            .ctx()
            .data(|data| data.get_temp::<String>(popup_state_id));

        if let Some(window) = target.as_deref().and_then(|selector| {
            effective_open_windows
                .iter()
                .find(|window| window.selector == selector)
        }) {
            let path = if window.process_path.is_empty() {
                PROCESS_PATHS
                    .lock()
                    .get(&window.process_id)
                    .cloned()
                    .unwrap_or_default()
            } else {
                window.process_path.clone()
            };
            if let Some(texture) = Self::process_icon_texture(ui.ctx(), &path) {
                ui.add(Image::new((texture.id(), vec2(16.0, 16.0))));
            }
        }
        let combo_response = egui::ComboBox::from_id_salt((id_source, "target-window-combo"))
            .width(width)
            .selected_text(truncated_selected_text)
            .show_ui(ui, |ui| {
                if allow_none {
                    if ui
                        .selectable_label(target.is_none(), label_when_none)
                        .clicked()
                    {
                        *target = None;
                        *match_duplicate_window_titles = false;
                        expanded_title = None;
                        changed = true;
                    }
                }
                ui.separator();

                for (title, selectors) in window_groups {
                    let has_duplicates = selectors.len() > 1;
                    let first_selector = selectors.first().cloned().unwrap_or_default();
                    let main_selected = target.as_deref().is_some_and(|current| {
                        Self::display_title_for_selector(current, effective_open_windows) == title
                    }) && *match_duplicate_window_titles;
                    let row_label = if has_duplicates {
                        format!("{title}  >")
                    } else {
                        title.clone()
                    };
                    let truncated_row_label = Self::truncate_window_title(&row_label, 50);
                    let process_path = effective_open_windows
                        .iter()
                        .find(|window| window.selector == first_selector)
                        .map(|window| window.process_path.as_str())
                        .unwrap_or_default();
                    let process_id = effective_open_windows
                        .iter()
                        .find(|window| window.selector == first_selector)
                        .map(|window| window.process_id)
                        .unwrap_or_default();
                    let row_response = Self::selectable_process_row(
                        ui,
                        main_selected,
                        truncated_row_label,
                        process_id,
                        process_path,
                    )
                    .on_hover_text(&title);

                    if row_response.hovered() && has_duplicates {
                        expanded_title = Some(title.clone());
                    }
                    if row_response.clicked() {
                        *target = Some(Self::selector_base_title(&first_selector).to_owned());
                        *match_duplicate_window_titles = has_duplicates;
                        expanded_title = None;
                        changed = true;
                    }

                    if has_duplicates && expanded_title.as_deref() == Some(title.as_str()) {
                        ui.indent(
                            (id_source, "duplicate-title-branches", title.as_str()),
                            |ui| {
                                let mut child_hovered = false;

                                let rules = [
                                    (
                                        " [Lowest]",
                                        Self::tr_lang(
                                            language,
                                            "[Lowest on Screen]",
                                            "[Lowest on Screen]",
                                        ),
                                    ),
                                    (
                                        " [Highest]",
                                        Self::tr_lang(
                                            language,
                                            "[Highest on Screen]",
                                            "[Highest on Screen]",
                                        ),
                                    ),
                                    (
                                        " [Leftmost]",
                                        Self::tr_lang(
                                            language,
                                            "[Leftmost on Screen]",
                                            "[Leftmost on Screen]",
                                        ),
                                    ),
                                    (
                                        " [Rightmost]",
                                        Self::tr_lang(
                                            language,
                                            "[Rightmost on Screen]",
                                            "[Rightmost on Screen]",
                                        ),
                                    ),
                                ];

                                for (suffix, label) in rules {
                                    let rule_selector = format!("{title}{suffix}");
                                    let is_rule_selected =
                                        target.as_deref() == Some(&rule_selector);
                                    let rule_label = format!("{title} {label}");
                                    let response =
                                        ui.selectable_label(is_rule_selected, &rule_label);
                                    child_hovered |= response.hovered();
                                    if response.clicked() {
                                        *target = Some(rule_selector);
                                        *match_duplicate_window_titles = false;
                                        expanded_title = None;
                                        changed = true;
                                    }
                                }

                                ui.separator();

                                for selector in &selectors {
                                    let child_selected = target.as_deref()
                                        == Some(selector.as_str())
                                        && !*match_duplicate_window_titles;
                                    let truncated_selector =
                                        Self::truncate_window_title(selector, 50);
                                    let process_path = effective_open_windows
                                        .iter()
                                        .find(|window| window.selector == *selector)
                                        .map(|window| window.process_path.as_str())
                                        .unwrap_or_default();
                                    let process_id = effective_open_windows
                                        .iter()
                                        .find(|window| window.selector == *selector)
                                        .map(|window| window.process_id)
                                        .unwrap_or_default();
                                    let child_response = Self::selectable_process_row(
                                        ui,
                                        child_selected,
                                        truncated_selector,
                                        process_id,
                                        process_path,
                                    )
                                    .on_hover_text(selector);
                                    child_hovered |= child_response.hovered();
                                    if child_response.clicked() {
                                        *target = Some(selector.clone());
                                        *match_duplicate_window_titles = false;
                                        expanded_title = None;
                                        changed = true;
                                    }
                                }
                                if child_hovered {
                                    expanded_title = Some(title.clone());
                                }
                            },
                        );
                    }
                }
            });
        if combo_response.response.clicked() {
            *LIVE_WINDOW_TARGET_COMBO_WINDOWS.lock() = Some(window_list::list_open_windows());
            ui.ctx().request_repaint();
        }

        ui.ctx().data_mut(|data| {
            if let Some(title) = expanded_title {
                data.insert_temp(popup_state_id, title);
            } else {
                data.remove::<String>(popup_state_id);
            }
        });
        changed
    }

    fn render_multi_window_targets_with_duplicate_mode(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        label_when_none: &str,
        primary: &mut Option<String>,
        extras: &mut Vec<String>,
        match_duplicate_window_titles: &mut bool,
        open_windows: &[WindowInfo],
    ) -> bool {
        let mut changed = false;
        let extras_expanded_id =
            ui.make_persistent_id((id_source, "extra-target-windows-expanded"));
        let mut extras_expanded = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(extras_expanded_id))
            .unwrap_or(false);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.y = 21.0;
                changed |= Self::render_window_target_combo_with_duplicate_mode(
                    ui,
                    language,
                    (id_source, "primary"),
                    label_when_none,
                    primary,
                    match_duplicate_window_titles,
                    open_windows,
                    320.0,
                    true,
                );

                let add_btn = Button::new(Self::material_icon_text(0xe145, 12.0));
                if ui
                    .add_sized([24.0, 21.0], add_btn)
                    .on_hover_text(Self::tr_lang(language, "+ Window", "+ Window"))
                    .clicked()
                {
                    let next = open_windows
                        .iter()
                        .find(|window| {
                            primary.as_deref() != Some(window.selector.as_str())
                                && !extras.iter().any(|existing| existing == &window.selector)
                        })
                        .map(|window| window.selector.clone())
                        .or_else(|| open_windows.first().map(|window| window.selector.clone()))
                        .unwrap_or_default();
                    if !next.is_empty() {
                        extras.push(next);
                        extras_expanded = true;
                        changed = true;
                    }
                }
                if !extras.is_empty() {
                    let toggle_icon = if extras_expanded { 0xe5cf } else { 0xe5cc };
                    if ui
                        .add_sized(
                            [24.0, 21.0],
                            Button::new(Self::material_icon_text(toggle_icon, 14.0)),
                        )
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Show or hide the extra target windows list.",
                            "Show or hide the extra target windows list.",
                        ))
                        .clicked()
                    {
                        extras_expanded = !extras_expanded;
                    }
                }
            });

            if extras_expanded {
                let mut remove_index = None;
                for (index, extra) in extras.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().interact_size.y = 21.0;
                        let mut extra_target = Some(extra.clone());
                        if Self::render_window_target_combo_with_duplicate_mode(
                            ui,
                            language,
                            (id_source, "extra", index),
                            label_when_none,
                            &mut extra_target,
                            match_duplicate_window_titles,
                            open_windows,
                            320.0,
                            false,
                        ) {
                            if let Some(next) = extra_target {
                                *extra = next;
                                changed = true;
                            }
                        }
                        let remove_btn = Button::new(Self::material_icon_text(0xe14c, 12.0));
                        if ui.add_sized([24.0, 21.0], remove_btn).clicked() {
                            remove_index = Some(index);
                        }
                    });
                }
                if let Some(index) = remove_index {
                    extras.remove(index);
                    changed = true;
                    if extras.is_empty() {
                        extras_expanded = false;
                    }
                }
            }
        });
        ui.ctx()
            .data_mut(|data| data.insert_temp(extras_expanded_id, extras_expanded));
        changed
    }

    fn macro_action_label(action: MacroAction) -> &'static str {
        match action {
            MacroAction::KeyPress => "KeyPress",
            MacroAction::KeyDown => "KeyDown",
            MacroAction::KeyUp => "KeyUp",
            MacroAction::Wait => "Wait",
            MacroAction::TypeText => "TypeText",
            MacroAction::ApplyWindowPreset => "Window Control",
            MacroAction::FocusWindowPreset => "FocusWindow",
            MacroAction::TriggerMacroPreset => "TriggerMacro",
            MacroAction::TriggerMacroPresetIfEnabled => "TriggerMacroIfEnabled",
            MacroAction::StopMacroPreset => "StopMacro",
            MacroAction::TriggerCommandPreset => "TriggerCommand",
            MacroAction::DisableNetworkAdapter => "DisableNetwork",
            MacroAction::EnableNetworkAdapter => "EnableNetwork",
            MacroAction::CutInternetRoute => "CutInternet",
            MacroAction::RestoreInternetRoute => "RestoreInternet",
            MacroAction::SetWifiRadioOff => "SetWifiRadioOff",
            MacroAction::SetWifiRadioOn => "SetWifiRadioOn",
            MacroAction::EnableCrosshairProfile => "EnableCrosshair",
            MacroAction::DisableCrosshair => "DisableCrosshair",
            MacroAction::EnablePinPreset => "EnablePin",
            MacroAction::DisablePin => "DisablePin",
            MacroAction::PlayMousePathPreset => "PlayMousePath",
            MacroAction::ApplyMouseSensitivityPreset => "ApplyMouseSens",
            MacroAction::EnableZoomPreset => "EnableZoom",
            MacroAction::DisableZoom => "DisableZoom",
            MacroAction::PlaySoundPreset => "PlaySound",
            MacroAction::StartVisionSearch => "StartImageSearch",
            MacroAction::ScanVisionOnce => "ScanImageOnce",
            MacroAction::StartAudioSensePreset => "StartAudio",
            MacroAction::StopAudioSense => "StopAudioSense",

            MacroAction::StopVisionWait => "StopImageSearchWait",
            MacroAction::StopVision => "StopImageSearch",
            MacroAction::LoopStart => "LoopStart",
            MacroAction::LoopEnd => "LoopEnd",
            MacroAction::StopIfTriggerPressedAgain => "StopIfTriggerPressedAgain",
            MacroAction::StopIfKeyPressed => "Break Loop",
            MacroAction::ShowHud => "ShowHud",
            MacroAction::HideHud => "HideHud",
            MacroAction::HideTaskbar => "HideTaskbar",
            MacroAction::ShowTaskbar => "ShowTaskbar",
            MacroAction::LockKeys => "LockKeys",
            MacroAction::UnlockKeys => "UnlockKeys",
            MacroAction::LockMouse => "LockMouseMove",
            MacroAction::UnlockMouse => "UnlockMouse",
            MacroAction::EnableMacroPreset => "EnableMacro",
            MacroAction::DisableMacroPreset => "DisableMacro",
            MacroAction::StartTimerPreset => "StartTimer",
            MacroAction::PauseTimerPreset => "PauseTimer",
            MacroAction::StopTimerPreset => "StopTimer",
            MacroAction::ReadTimerPreset => "ReadTimer",
            MacroAction::EnableStep => "EnableStep",
            MacroAction::DisableStep => "DisableStep",
            MacroAction::MouseLeftClick => "LeftClick",
            MacroAction::MouseLeftDown => "LeftDown",
            MacroAction::MouseLeftUp => "LeftUp",
            MacroAction::MouseRightClick => "RightClick",
            MacroAction::MouseRightDown => "RightDown",
            MacroAction::MouseRightUp => "RightUp",
            MacroAction::MouseMiddleClick => "MiddleClick",
            MacroAction::MouseMiddleDown => "MiddleDown",
            MacroAction::MouseMiddleUp => "MiddleUp",
            MacroAction::MouseX1Click => "X1Click",
            MacroAction::MouseX1Down => "X1Down",
            MacroAction::MouseX1Up => "X1Up",
            MacroAction::MouseX2Click => "X2Click",
            MacroAction::MouseX2Down => "X2Down",
            MacroAction::MouseX2Up => "X2Up",
            MacroAction::MouseWheelUp => "WheelUp",
            MacroAction::MouseWheelDown => "WheelDown",
            MacroAction::MouseMoveAbsolute => "MoveAbs",
            MacroAction::MouseMoveRelative => "MoveRel",
            MacroAction::IfStart => "IfStart",
            MacroAction::Else => "Else",
            MacroAction::IfEnd => "IfEnd",
            MacroAction::SetVariable => "SetVariable",
            MacroAction::ReadMemory => "ReadMemory",
            MacroAction::WriteMemory => "WriteMemory",
            MacroAction::OcrSearch => "OcrSearch",
            MacroAction::DrawGeometry => "DrawGeometry",
            MacroAction::ShowGeometryPreset => "ShowGeometry",
            MacroAction::HideGeometryPreset => "HideGeometry",
            MacroAction::EnableEspPreset => "EnableESP",
            MacroAction::DisableEspPreset => "DisableESP",
            MacroAction::ReadEspTarget => "ReadESPTarget",
            MacroAction::Esp3DAimLock => "Esp3DAimLock",
            MacroAction::StartTimerPreset => "StartTimerPreset",
            MacroAction::PauseTimerPreset => "PauseTimerPreset",
            MacroAction::StopTimerPreset => "StopTimerPreset",
            MacroAction::ReadTimerPreset => "ReadTimerPreset",
            MacroAction::EnableStep => "EnableStep",
            MacroAction::DisableStep => "DisableStep",
            MacroAction::FunnyMemeReply => "MemeReply",
            MacroAction::AiResponse => "AiResponse",
            MacroAction::JumpToStep => "JumpToStep",
            _ => "Legacy (Deprecated)",
        }
    }

    fn macro_action_tooltip(action: MacroAction, language: UiLanguage) -> &'static str {
        let (translation_key, english) = match action {
            MacroAction::KeyPress => (
                "macro_action_tooltip.key_press",
                "Press and release one keyboard key.",
            ),
            MacroAction::KeyDown => ("macro_action_tooltip.key_down", "Hold a keyboard key down."),
            MacroAction::KeyUp => (
                "macro_action_tooltip.key_up",
                "Release a held keyboard key.",
            ),
            MacroAction::Wait => (
                "macro_action_tooltip.wait",
                "Wait for the number of milliseconds in Delay, then continue.",
            ),
            MacroAction::TypeText => (
                "macro_action_tooltip.type_text",
                "Type the whole text from the Input field.",
            ),
            MacroAction::ApplyWindowPreset => (
                "macro_action_tooltip.apply_window_preset",
                "Resize or apply window layout preset.",
            ),
            MacroAction::FocusWindowPreset => (
                "macro_action_tooltip.focus_window_preset",
                "Bring one window forward with the selected focus preset.",
            ),
            MacroAction::TriggerMacroPreset => (
                "macro_action_tooltip.trigger_macro_preset",
                "Run another macro preset from the same macro group.",
            ),
            MacroAction::TriggerMacroPresetIfEnabled => (
                "macro_action_tooltip.trigger_macro_preset_if_enabled",
                "Run selected macro presets only when those presets are enabled.",
            ),
            MacroAction::StopMacroPreset => (
                "macro_action_tooltip.stop_macro_preset",
                "Stop the selected macro presets if they are currently running.",
            ),
            MacroAction::TriggerCommandPreset => (
                "macro_action_tooltip.trigger_command_preset",
                "Run one custom command preset from the Custom tab.",
            ),
            MacroAction::DisableNetworkAdapter => (
                "macro_action_tooltip.disable_network_adapter",
                "Disable Wi-Fi, Ethernet, all physical adapters, or one exact adapter name.",
            ),
            MacroAction::EnableNetworkAdapter => (
                "macro_action_tooltip.enable_network_adapter",
                "Enable Wi-Fi, Ethernet, all physical adapters, or one exact adapter name.",
            ),
            MacroAction::CutInternetRoute => (
                "macro_action_tooltip.cut_internet_route",
                "Cut internet access quickly by removing the selected adapter's default route without disabling the adapter.",
            ),
            MacroAction::RestoreInternetRoute => (
                "macro_action_tooltip.restore_internet_route",
                "Restore default internet routes previously cut by MacroNest.",
            ),
            MacroAction::SetWifiRadioOff => (
                "macro_action_tooltip.set_wifi_radio_off",
                "Instantly turn off the Wi-Fi radio using the Windows Radio API — like pressing the hardware Wi-Fi button.",
            ),
            MacroAction::SetWifiRadioOn => (
                "macro_action_tooltip.set_wifi_radio_on",
                "Instantly turn on the Wi-Fi radio using the Windows Radio API — like pressing the hardware Wi-Fi button.",
            ),
            MacroAction::EnableCrosshairProfile => (
                "macro_action_tooltip.enable_crosshair_profile",
                "Enable one saved crosshair profile.",
            ),
            MacroAction::DisableCrosshair => (
                "macro_action_tooltip.disable_crosshair",
                "Turn the overlay crosshair off.",
            ),
            MacroAction::EnablePinPreset => (
                "macro_action_tooltip.enable_pin_preset",
                "Enable one saved pin preset from the Pin tab.",
            ),
            MacroAction::DisablePin => (
                "macro_action_tooltip.disable_pin",
                "Turn the pinned app overlay off.",
            ),
            MacroAction::PlayMousePathPreset => (
                "macro_action_tooltip.play_mouse_path_preset",
                "Play one recorded mouse path preset from the Mouse tab.",
            ),
            MacroAction::ApplyMouseSensitivityPreset => (
                "macro_action_tooltip.apply_mouse_sensitivity_preset",
                "Apply one mouse sensitivity preset from the Mouse tab.",
            ),
            MacroAction::EnableZoomPreset => (
                "macro_action_tooltip.enable_zoom_preset",
                "Enable one saved zoom preset.",
            ),
            MacroAction::DisableZoom => (
                "macro_action_tooltip.disable_zoom",
                "Turn the zoom overlay off.",
            ),
            MacroAction::PlaySoundPreset => (
                "macro_action_tooltip.play_sound_preset",
                "Play one sound preset from the Media tab.",
            ),
            MacroAction::StartVisionSearch => (
                "macro_action_tooltip.start_vision_search",
                "Start scanning one image-search preset in the background.",
            ),
            MacroAction::ScanVisionOnce => (
                "macro_action_tooltip.scan_vision_once",
                "Scan for the selected image, color, or pixel counter preset exactly once.",
            ),
            MacroAction::StartAudioSensePreset => (
                "macro_action_tooltip.start_audio_sense_preset",
                "Start pitch detection from an AudioSense preset or custom pitch settings.",
            ),
            MacroAction::StopAudioSense => (
                "macro_action_tooltip.stop_audio_sense",
                "Stop custom AudioSense monitoring or stop every active AudioSense monitor.",
            ),
            MacroAction::StopVisionWait => (
                "macro_action_tooltip.stop_vision_wait",
                "Stop waiting for one image-search preset to match.",
            ),
            MacroAction::StopVision => (
                "macro_action_tooltip.stop_vision",
                "Stop one image-search preset that is currently scanning.",
            ),
            MacroAction::LoopStart => (
                "macro_action_tooltip.loop_start",
                "Start looping the next adjacent steps. Input = loop count, or turn on Infinite.",
            ),
            MacroAction::LoopEnd => (
                "macro_action_tooltip.loop_end",
                "End the current loop block.",
            ),
            MacroAction::StopIfTriggerPressedAgain => (
                "macro_action_tooltip.stop_if_trigger_pressed_again",
                "Stop the current loop if you press the trigger again.",
            ),
            MacroAction::StopIfKeyPressed => (
                "macro_action_tooltip.stop_if_key_pressed",
                "Break only the current loop if the key in Input is pressed, then continue with the steps after the loop.",
            ),
            MacroAction::ShowHud => (
                "macro_action_tooltip.show_hud",
                "Show one HUD preset from the HUD tab.",
            ),
            MacroAction::HideHud => (
                "macro_action_tooltip.hide_hud",
                "Hide the currently visible HUD.",
            ),
            MacroAction::HideTaskbar => (
                "macro_action_tooltip.hide_taskbar",
                "Hide the Windows taskbar for a cleaner fullscreen layout.",
            ),
            MacroAction::ShowTaskbar => (
                "macro_action_tooltip.show_taskbar",
                "Show the Windows taskbar again if it is hidden.",
            ),
            MacroAction::LockKeys => (
                "macro_action_tooltip.lock_keys",
                "Lock the keys listed in Input.",
            ),
            MacroAction::UnlockKeys => (
                "macro_action_tooltip.unlock_keys",
                "Unlock the keys listed in Input.",
            ),
            MacroAction::LockMouse => (
                "macro_action_tooltip.lock_mouse",
                "Lock mouse movement, clicks, and wheel input until it is unlocked or the macro ends.",
            ),
            MacroAction::UnlockMouse => (
                "macro_action_tooltip.unlock_mouse",
                "Unlock mouse movement and mouse buttons again.",
            ),
            MacroAction::EnableMacroPreset => (
                "macro_action_tooltip.enable_macro_preset",
                "Enable one other macro preset from the same macro group.",
            ),
            MacroAction::DisableMacroPreset => (
                "macro_action_tooltip.disable_macro_preset",
                "Disable one other macro preset from the same macro group.",
            ),
            MacroAction::EnableStep => (
                "macro_action_tooltip.enable_step",
                "Enable one or more specific steps in this macro.",
            ),
            MacroAction::DisableStep => (
                "macro_action_tooltip.disable_step",
                "Disable one or more specific steps in this macro.",
            ),
            MacroAction::MouseLeftClick => (
                "macro_action_tooltip.mouse_left_click",
                "Press and release left mouse button.",
            ),
            MacroAction::MouseLeftDown => (
                "macro_action_tooltip.mouse_left_down",
                "Hold left mouse button down.",
            ),
            MacroAction::MouseLeftUp => (
                "macro_action_tooltip.mouse_left_up",
                "Release held left mouse button.",
            ),
            MacroAction::MouseRightClick => (
                "macro_action_tooltip.mouse_right_click",
                "Press and release right mouse button.",
            ),
            MacroAction::MouseRightDown => (
                "macro_action_tooltip.mouse_right_down",
                "Hold right mouse button down.",
            ),
            MacroAction::MouseRightUp => (
                "macro_action_tooltip.mouse_right_up",
                "Release held right mouse button.",
            ),
            MacroAction::MouseMiddleClick => (
                "macro_action_tooltip.mouse_middle_click",
                "Press and release middle mouse button.",
            ),
            MacroAction::MouseMiddleDown => (
                "macro_action_tooltip.mouse_middle_down",
                "Hold middle mouse button down.",
            ),
            MacroAction::MouseMiddleUp => (
                "macro_action_tooltip.mouse_middle_up",
                "Release held middle mouse button.",
            ),
            MacroAction::MouseX1Click => (
                "macro_action_tooltip.mouse_x1_click",
                "Press and release mouse button X1.",
            ),
            MacroAction::MouseX1Down => (
                "macro_action_tooltip.mouse_x1_down",
                "Hold mouse button X1 down.",
            ),
            MacroAction::MouseX1Up => (
                "macro_action_tooltip.mouse_x1_up",
                "Release held mouse button X1.",
            ),
            MacroAction::MouseX2Click => (
                "macro_action_tooltip.mouse_x2_click",
                "Press and release mouse button X2.",
            ),
            MacroAction::MouseX2Down => (
                "macro_action_tooltip.mouse_x2_down",
                "Hold mouse button X2 down.",
            ),
            MacroAction::MouseX2Up => (
                "macro_action_tooltip.mouse_x2_up",
                "Release held mouse button X2.",
            ),
            MacroAction::MouseWheelUp => (
                "macro_action_tooltip.mouse_wheel_up",
                "Scroll mouse wheel up.",
            ),
            MacroAction::MouseWheelDown => (
                "macro_action_tooltip.mouse_wheel_down",
                "Scroll mouse wheel down.",
            ),
            MacroAction::MouseMoveAbsolute => (
                "macro_action_tooltip.mouse_move_absolute",
                "Move mouse to absolute coordinates.",
            ),
            MacroAction::MouseMoveRelative => (
                "macro_action_tooltip.mouse_move_relative",
                "Move mouse relative to current position.",
            ),
            MacroAction::IfStart => (
                "macro_action_tooltip.if_start",
                "Start a conditional If block. Only runs steps inside if the expression comparison is met.",
            ),
            MacroAction::Else => (
                "macro_action_tooltip.else",
                "Otherwise (Else) block. Runs steps inside if the above If condition was NOT met.",
            ),
            MacroAction::IfEnd => (
                "macro_action_tooltip.if_end",
                "End the current conditional If block.",
            ),
            MacroAction::SetVariable => (
                "macro_action_tooltip.set_variable",
                "Set a variable to a numeric value or copy from another variable.",
            ),
            MacroAction::ReadMemory => (
                "macro_action_tooltip.read_memory",
                "Read one value from an address in the selected process and store it in a variable.",
            ),
            MacroAction::WriteMemory => (
                "macro_action_tooltip.write_memory",
                "Write one value to an address in the selected process.",
            ),
            MacroAction::ReadTimerPreset => (
                "macro_action_tooltip.read_timer_preset",
                "Read one running timer value and store it into a variable.",
            ),
            MacroAction::OcrSearch => (
                "macro_action_tooltip.ocr_search",
                "Scan a screen region with fast local PaddleOCR to extract text and numbers.",
            ),
            MacroAction::DrawGeometry => (
                "macro_action_tooltip.draw_geometry",
                "Draw one geometry shape on the screen overlay using coordinates or expressions.",
            ),
            MacroAction::ShowGeometryPreset => (
                "macro_action_tooltip.show_geometry_preset",
                "Show one saved geometry preset from the Geometry tab.",
            ),
            MacroAction::HideGeometryPreset => (
                "macro_action_tooltip.hide_geometry_preset",
                "Hide geometry preset (or clear all geometry overlay).",
            ),
            MacroAction::EnableEspPreset => (
                "macro_action_tooltip.enable_esp_preset",
                "Enable one shared ESP preset from the ESP tab.",
            ),
            MacroAction::DisableEspPreset => (
                "macro_action_tooltip.disable_esp_preset",
                "Disable one shared ESP preset from the ESP tab.",
            ),
            MacroAction::ReadEspTarget => (
                "macro_action_tooltip.read_esp_target",
                "Read the selected ESP preset's latest projected target data into macro variables.",
            ),
            MacroAction::Esp3DAimLock => (
                "macro_action_tooltip.esp_3d_aim_lock",
                "Move mouse towards target 3D angle calculated from ESP.",
            ),
            MacroAction::StartTimerPreset => (
                "macro_action_tooltip.start_timer_preset",
                "Start or restart a timer preset.",
            ),
            MacroAction::PauseTimerPreset => (
                "macro_action_tooltip.pause_timer_preset",
                "Pause a running timer preset.",
            ),
            MacroAction::StopTimerPreset => (
                "macro_action_tooltip.stop_timer_preset",
                "Stop a running timer preset.",
            ),
            MacroAction::ReadTimerPreset => (
                "macro_action_tooltip.read_timer_preset",
                "Read the value of a timer preset into a variable.",
            ),
            MacroAction::EnableStep => (
                "macro_action_tooltip.enable_step",
                "Enable a macro step by key.",
            ),
            MacroAction::DisableStep => (
                "macro_action_tooltip.disable_step",
                "Disable a macro step by key.",
            ),
            MacroAction::FunnyMemeReply => (
                "macro_action_tooltip.funny_meme_reply",
                "Turn one message into a meme search query, fetch the best image result, and copy it to the clipboard.",
            ),
            MacroAction::AiResponse => (
                "macro_action_tooltip.ai_response",
                "Send request to Groq AI, save the response to a variable, and resume macro when done.",
            ),
            MacroAction::JumpToStep => (
                "macro_action_tooltip.jump_to_step",
                "Jump to a specified step (1-indexed or math expression).",
            ),
            _ => ("macro_action_tooltip.legacy", "Legacy (Deprecated)"),
        };
        match language {
            UiLanguage::Vietnamese => {
                crate::lang::translate(language, translation_key).unwrap_or(english)
            }
            UiLanguage::English | UiLanguage::Icon => english,
        }
    }

    fn macro_action_icon(action: MacroAction) -> char {
        let codepoint = match action {
            MacroAction::KeyPress => 0xe312,
            MacroAction::KeyDown => 0xe313,
            MacroAction::KeyUp => 0xe316,
            MacroAction::Wait => 0xe8b5,
            MacroAction::TypeText => 0xe262,
            MacroAction::ApplyWindowPreset => 0xe8b8,
            MacroAction::FocusWindowPreset => 0xe89e,
            MacroAction::TriggerMacroPreset => 0xe037,
            MacroAction::TriggerMacroPresetIfEnabled => 0xe86c,
            MacroAction::StopMacroPreset => 0xe047,
            MacroAction::TriggerCommandPreset => 0xeb8e,
            MacroAction::DisableNetworkAdapter => 0xe648,
            MacroAction::EnableNetworkAdapter => 0xe63e,
            MacroAction::CutInternetRoute => 0xe628,
            MacroAction::RestoreInternetRoute => 0xe2bd,
            MacroAction::SetWifiRadioOff => 0xe648,
            MacroAction::SetWifiRadioOn => 0xe63e,
            MacroAction::EnableCrosshairProfile => 0xe3c5,
            MacroAction::DisableCrosshair => 0xe1b7,
            MacroAction::EnablePinPreset => 0xe0c8,
            MacroAction::DisablePin => 0xe0c7,
            MacroAction::PlayMousePathPreset => 0xe913,
            MacroAction::ApplyMouseSensitivityPreset => 0xe837,
            MacroAction::EnableZoomPreset => 0xe8ff,
            MacroAction::DisableZoom => 0xe8f4,
            MacroAction::PlaySoundPreset => 0xe050,
            MacroAction::StartVisionSearch => 0xe8b6,
            MacroAction::ScanVisionOnce => 0xe8b6,
            MacroAction::StartAudioSensePreset => 0xe050,
            MacroAction::StopAudioSense => 0xe047,

            MacroAction::StopVisionWait => 0xe047,
            MacroAction::StopVision => 0xe047,
            MacroAction::LoopStart => 0xe028,
            MacroAction::LoopEnd => 0xe040,
            MacroAction::StopIfTriggerPressedAgain => 0xe047,
            MacroAction::StopIfKeyPressed => 0xe14b,
            MacroAction::ShowHud => 0xe8f4,
            MacroAction::HideHud => 0xe8f5,
            MacroAction::HideTaskbar => 0xe8f5,
            MacroAction::ShowTaskbar => 0xe8f4,
            MacroAction::LockKeys => 0xe897,
            MacroAction::UnlockKeys => 0xe898,
            MacroAction::LockMouse => 0xe897,
            MacroAction::UnlockMouse => 0xe898,
            MacroAction::EnableMacroPreset => 0xe86c,
            MacroAction::DisableMacroPreset => 0xe14b,
            MacroAction::StartTimerPreset => 0xe037,
            MacroAction::PauseTimerPreset => 0xe034,
            MacroAction::StopTimerPreset => 0xe047,
            MacroAction::ReadTimerPreset => 0xe150,
            MacroAction::EnableStep => 0xe86c,
            MacroAction::DisableStep => 0xe14b,
            MacroAction::MouseLeftClick => 0xe323,
            MacroAction::MouseLeftDown => 0xe5c5,
            MacroAction::MouseLeftUp => 0xe5c7,
            MacroAction::MouseRightClick => 0xe323,
            MacroAction::MouseRightDown => 0xe5c5,
            MacroAction::MouseRightUp => 0xe5c7,
            MacroAction::MouseMiddleClick => 0xe323,
            MacroAction::MouseMiddleDown => 0xe5c5,
            MacroAction::MouseMiddleUp => 0xe5c7,
            MacroAction::MouseX1Click => 0xe913,
            MacroAction::MouseX1Down => 0xe5c5,
            MacroAction::MouseX1Up => 0xe5c7,
            MacroAction::MouseX2Click => 0xe913,
            MacroAction::MouseX2Down => 0xe5c5,
            MacroAction::MouseX2Up => 0xe5c7,
            MacroAction::MouseWheelUp => 0xe5d8,
            MacroAction::MouseWheelDown => 0xe5db,
            MacroAction::MouseMoveAbsolute => 0xe89f,
            MacroAction::MouseMoveRelative => 0xe3ec,
            MacroAction::IfStart => 0xe8af,
            MacroAction::Else => 0xe3ec,
            MacroAction::IfEnd => 0xe040,
            MacroAction::SetVariable => 0xe150,
            MacroAction::ReadMemory => 0xe30a,
            MacroAction::WriteMemory => 0xe3c9,
            MacroAction::OcrSearch => 0xe8b6,
            MacroAction::DrawGeometry => 0xe85b,
            MacroAction::ShowGeometryPreset => 0xe8f4,
            MacroAction::HideGeometryPreset => 0xe8f5,
            MacroAction::EnableEspPreset => 0xe8f4,
            MacroAction::DisableEspPreset => 0xe8f5,
            MacroAction::ReadEspTarget => 0xe8b6,
            MacroAction::Esp3DAimLock => 0xe876,
            MacroAction::StartTimerPreset => 0xe425,
            MacroAction::PauseTimerPreset => 0xe034,
            MacroAction::StopTimerPreset => 0xe047,
            MacroAction::ReadTimerPreset => 0xe8b6,
            MacroAction::EnableStep => 0xe8f4,
            MacroAction::DisableStep => 0xe8f5,
            MacroAction::FunnyMemeReply => 0xe420,
            MacroAction::AiResponse => 0xeb8e,
            MacroAction::JumpToStep => 0xe5c8,
            _ => 0xe8b5,
        };
        char::from_u32(codepoint).unwrap_or('?')
    }

    fn macro_action_icon_text(action: MacroAction) -> RichText {
        Self::material_icon_text(Self::macro_action_icon(action) as u32, 18.0)
    }

    fn macro_action_short_label(action: MacroAction, language: UiLanguage) -> &'static str {
        let (translation_key, english) = match action {
            MacroAction::KeyPress => ("macro_action_short_label.key_press", "Press"),
            MacroAction::KeyDown => ("macro_action_short_label.key_down", "KEY Dn"),
            MacroAction::KeyUp => ("macro_action_short_label.key_up", "KEY Up"),
            MacroAction::Wait => ("macro_action_short_label.wait", "Wait"),
            MacroAction::TypeText => ("macro_action_short_label.type_text", "Text"),
            MacroAction::ApplyWindowPreset => {
                ("macro_action_short_label.apply_window_preset", "Window")
            }
            MacroAction::FocusWindowPreset => {
                ("macro_action_short_label.focus_window_preset", "Focus")
            }
            MacroAction::TriggerMacroPreset => {
                ("macro_action_short_label.trigger_macro_preset", "Macro")
            }
            MacroAction::TriggerMacroPresetIfEnabled => (
                "macro_action_short_label.trigger_macro_preset_if_enabled",
                "Start",
            ),
            MacroAction::StopMacroPreset => ("macro_action_short_label.stop_macro_preset", "Stop"),
            MacroAction::TriggerCommandPreset => {
                ("macro_action_short_label.trigger_command_preset", "Cmd")
            }
            MacroAction::DisableNetworkAdapter => {
                ("macro_action_short_label.disable_network_adapter", "NetOff")
            }
            MacroAction::EnableNetworkAdapter => {
                ("macro_action_short_label.enable_network_adapter", "NetOn")
            }
            MacroAction::CutInternetRoute => {
                ("macro_action_short_label.cut_internet_route", "CutNet")
            }
            MacroAction::RestoreInternetRoute => {
                ("macro_action_short_label.restore_internet_route", "RestNet")
            }
            MacroAction::SetWifiRadioOff => {
                ("macro_action_short_label.set_wifi_radio_off", "RadioOff")
            }
            MacroAction::SetWifiRadioOn => {
                ("macro_action_short_label.set_wifi_radio_on", "RadioOn")
            }
            MacroAction::EnableCrosshairProfile => {
                ("macro_action_short_label.enable_crosshair_profile", "Cross")
            }
            MacroAction::DisableCrosshair => {
                ("macro_action_short_label.disable_crosshair", "NoCross")
            }
            MacroAction::EnablePinPreset => ("macro_action_short_label.enable_pin_preset", "Pin"),
            MacroAction::DisablePin => ("macro_action_short_label.disable_pin", "NoPin"),
            MacroAction::PlayMousePathPreset => {
                ("macro_action_short_label.play_mouse_path_preset", "Path")
            }
            MacroAction::ApplyMouseSensitivityPreset => (
                "macro_action_short_label.apply_mouse_sensitivity_preset",
                "Sense",
            ),
            MacroAction::EnableZoomPreset => {
                ("macro_action_short_label.enable_zoom_preset", "Zoom")
            }
            MacroAction::DisableZoom => ("macro_action_short_label.disable_zoom", "NoZoom"),
            MacroAction::PlaySoundPreset => ("macro_action_short_label.play_sound_preset", "Sound"),
            MacroAction::StartVisionSearch => {
                ("macro_action_short_label.start_vision_search", "Start")
            }
            MacroAction::ScanVisionOnce => ("macro_action_short_label.scan_vision_once", "Scan"),
            MacroAction::StartAudioSensePreset => (
                "macro_action_short_label.start_audio_sense_preset",
                "AudioOn",
            ),
            MacroAction::StopAudioSense => {
                ("macro_action_short_label.stop_audio_sense", "AudioOff")
            }
            MacroAction::StopVisionWait => ("macro_action_short_label.stop_vision_wait", "Wait"),
            MacroAction::StopVision => ("macro_action_short_label.stop_vision", "Stop"),
            MacroAction::LoopStart => ("macro_action_short_label.loop_start", "Loop"),
            MacroAction::LoopEnd => ("macro_action_short_label.loop_end", "End"),
            MacroAction::StopIfTriggerPressedAgain => (
                "macro_action_short_label.stop_if_trigger_pressed_again",
                "Stop",
            ),
            MacroAction::StopIfKeyPressed => {
                ("macro_action_short_label.stop_if_key_pressed", "Break")
            }
            MacroAction::ShowHud => ("macro_action_short_label.show_hud", "Show HUD"),
            MacroAction::HideHud => ("macro_action_short_label.hide_hud", "Hide HUD"),
            MacroAction::HideTaskbar => ("macro_action_short_label.hide_taskbar", "TB Off"),
            MacroAction::ShowTaskbar => ("macro_action_short_label.show_taskbar", "TB On"),
            MacroAction::LockKeys => ("macro_action_short_label.lock_keys", "KL On"),
            MacroAction::UnlockKeys => ("macro_action_short_label.unlock_keys", "KL Off"),
            MacroAction::LockMouse => ("macro_action_short_label.lock_mouse", "Lock M"),
            MacroAction::UnlockMouse => ("macro_action_short_label.unlock_mouse", "Unlock M"),
            MacroAction::EnableMacroPreset => {
                ("macro_action_short_label.enable_macro_preset", "PresetOn")
            }
            MacroAction::DisableMacroPreset => {
                ("macro_action_short_label.disable_macro_preset", "PresetOff")
            }
            MacroAction::StartTimerPreset => {
                ("macro_action_short_label.start_timer_preset", "TimerOn")
            }
            MacroAction::PauseTimerPreset => {
                ("macro_action_short_label.pause_timer_preset", "TimerPs")
            }
            MacroAction::StopTimerPreset => {
                ("macro_action_short_label.stop_timer_preset", "TimerOff")
            }
            MacroAction::ReadTimerPreset => {
                ("macro_action_short_label.read_timer_preset", "TimerVar")
            }
            MacroAction::EnableStep => ("macro_action_short_label.enable_step", "StepOn"),
            MacroAction::DisableStep => ("macro_action_short_label.disable_step", "StepOff"),
            MacroAction::MouseLeftClick => ("macro_action_short_label.mouse_left_click", "LClick"),
            MacroAction::MouseLeftDown => ("macro_action_short_label.mouse_left_down", "LDown"),
            MacroAction::MouseLeftUp => ("macro_action_short_label.mouse_left_up", "LUp"),
            MacroAction::MouseRightClick => {
                ("macro_action_short_label.mouse_right_click", "RClick")
            }
            MacroAction::MouseRightDown => ("macro_action_short_label.mouse_right_down", "RDown"),
            MacroAction::MouseRightUp => ("macro_action_short_label.mouse_right_up", "RUp"),
            MacroAction::MouseMiddleClick => {
                ("macro_action_short_label.mouse_middle_click", "MClick")
            }
            MacroAction::MouseMiddleDown => ("macro_action_short_label.mouse_middle_down", "MDown"),
            MacroAction::MouseMiddleUp => ("macro_action_short_label.mouse_middle_up", "MUp"),
            MacroAction::MouseX1Click => ("macro_action_short_label.mouse_x1_click", "X1"),
            MacroAction::MouseX1Down => ("macro_action_short_label.mouse_x1_down", "X1Dn"),
            MacroAction::MouseX1Up => ("macro_action_short_label.mouse_x1_up", "X1Up"),
            MacroAction::MouseX2Click => ("macro_action_short_label.mouse_x2_click", "X2"),
            MacroAction::MouseX2Down => ("macro_action_short_label.mouse_x2_down", "X2Dn"),
            MacroAction::MouseX2Up => ("macro_action_short_label.mouse_x2_up", "X2Up"),
            MacroAction::MouseWheelUp => ("macro_action_short_label.mouse_wheel_up", "WhUp"),
            MacroAction::MouseWheelDown => ("macro_action_short_label.mouse_wheel_down", "WhDn"),
            MacroAction::MouseMoveAbsolute => {
                ("macro_action_short_label.mouse_move_absolute", "MoveTo")
            }
            MacroAction::MouseMoveRelative => {
                ("macro_action_short_label.mouse_move_relative", "MoveBy")
            }
            MacroAction::IfStart => ("macro_action_short_label.if_start", "IfStart"),
            MacroAction::Else => ("macro_action_short_label.else", "Else"),
            MacroAction::IfEnd => ("macro_action_short_label.if_end", "IfEnd"),
            MacroAction::SetVariable => ("macro_action_short_label.set_variable", "SetVar"),
            MacroAction::ReadMemory => ("macro_action_short_label.read_memory", "ReadMemory"),
            MacroAction::WriteMemory => ("macro_action_short_label.write_memory", "WriteMemory"),
            MacroAction::DrawGeometry => ("macro_action_short_label.draw_geometry", "DrawGeo"),
            MacroAction::ShowGeometryPreset => {
                ("macro_action_short_label.show_geometry_preset", "ShowGeo")
            }
            MacroAction::HideGeometryPreset => {
                ("macro_action_short_label.hide_geometry_preset", "HideGeo")
            }
            MacroAction::EnableEspPreset => {
                ("macro_action_short_label.enable_esp_preset", "ESP On")
            }
            MacroAction::DisableEspPreset => {
                ("macro_action_short_label.disable_esp_preset", "ESP Off")
            }
            MacroAction::ReadEspTarget => ("macro_action_short_label.read_esp_target", "ESP Data"),
            MacroAction::Esp3DAimLock => ("macro_action_short_label.esp_3d_aim_lock", "3D Lock"),
            MacroAction::FunnyMemeReply => ("macro_action_short_label.funny_meme_reply", "Meme"),
            MacroAction::AiResponse => ("macro_action_short_label.ai_response", "AI"),
            MacroAction::OcrSearch => ("macro_action_short_label.ocr_search", "OCR"),
            MacroAction::JumpToStep => ("macro_action_short_label.jump_to_step", "Jump"),
            _ => ("macro_action_short_label.legacy", "Legacy"),
        };
        match language {
            UiLanguage::Vietnamese => Self::normalize_vietnamese(
                crate::lang::translate(language, translation_key).unwrap_or(english),
            ),
            UiLanguage::English | UiLanguage::Icon => english,
        }
    }

    fn macro_action_pair_tag(action: MacroAction) -> Option<&'static str> {
        match action {
            MacroAction::KeyDown | MacroAction::KeyUp => Some("KEY"),
            MacroAction::LockKeys | MacroAction::UnlockKeys => Some("KLOCK"),
            MacroAction::LockMouse | MacroAction::UnlockMouse => Some("MLOCK"),
            MacroAction::HideTaskbar | MacroAction::ShowTaskbar => Some("TASKBAR"),
            _ => None,
        }
    }

    fn macro_action_selected_label(action: MacroAction, language: UiLanguage) -> String {
        if matches!(
            action,
            MacroAction::DisableNetworkAdapter
                | MacroAction::EnableNetworkAdapter
                | MacroAction::CutInternetRoute
                | MacroAction::RestoreInternetRoute
                | MacroAction::SetWifiRadioOff
                | MacroAction::SetWifiRadioOn
        ) {
            return match language {
                UiLanguage::Vietnamese => "Mạng".to_owned(),
                UiLanguage::English | UiLanguage::Icon => "Network".to_owned(),
            };
        }
        match language {
            UiLanguage::Vietnamese => Self::macro_action_short_label(action, language).to_owned(),
            UiLanguage::English => Self::macro_action_label(action).to_owned(),
            UiLanguage::Icon => Self::macro_action_label(action).to_owned(),
        }
    }

    fn material_icon_text(codepoint: u32, size: f32) -> RichText {
        RichText::new(char::from_u32(codepoint).unwrap_or('?').to_string())
            .family(FontFamily::Name(MATERIAL_ICONS_FONT.into()))
            .size(size)
    }

    fn macro_action_selected_widget_text(
        action: MacroAction,
        language: UiLanguage,
        theme: UiThemeMode,
    ) -> egui::WidgetText {
        let mut job = egui::text::LayoutJob::default();
        let weak_color = if theme == UiThemeMode::Dark {
            Color32::from_gray(224)
        } else {
            Color32::from_rgb(28, 36, 48)
        };
        let icon_format = egui::TextFormat {
            font_id: egui::FontId::new(13.0, FontFamily::Name(MATERIAL_ICONS_FONT.into())),
            color: weak_color,
            valign: egui::Align::Center,
            ..Default::default()
        };
        let text_format = egui::TextFormat {
            font_id: egui::FontId::new(13.0, FontFamily::Proportional),
            color: weak_color,
            valign: egui::Align::Center,
            ..Default::default()
        };
        let icon = if matches!(
            action,
            MacroAction::DisableNetworkAdapter
                | MacroAction::EnableNetworkAdapter
                | MacroAction::CutInternetRoute
                | MacroAction::RestoreInternetRoute
                | MacroAction::SetWifiRadioOff
                | MacroAction::SetWifiRadioOn
        ) {
            char::from_u32(0xe1ba).unwrap_or('?')
        } else {
            char::from_u32(Self::macro_action_icon(action) as u32).unwrap_or('?')
        };
        job.append(&icon.to_string(), 0.0, icon_format);
        job.append(" ", 0.0, text_format.clone());
        let label = Self::macro_action_selected_label(action, language);
        job.append(&label, 0.0, text_format);
        egui::WidgetText::LayoutJob(job.into())
    }

    fn ai_badge_text(with_label: bool) -> RichText {
        let text = "AI";
        let size = if with_label { 13.0 } else { 12.0 };
        RichText::new(text)
            .strong()
            .size(size)
            .color(Color32::from_rgb(233, 247, 255))
    }

    fn ai_badge_fill() -> Color32 {
        Color32::from_rgb(27, 58, 96)
    }

    fn ai_badge_stroke() -> Stroke {
        Stroke::new(1.0, Color32::from_rgb(90, 190, 255))
    }

    pub(crate) fn pending_update_badge_count(&self) -> u32 {
        match self.update_status {
            UpdateStatus::Available(_, _, _)
            | UpdateStatus::Downloading
            | UpdateStatus::ReadyToRestart(_) => 1,
            _ => 0,
        }
    }

    fn show_update_notice(&mut self, message: impl Into<String>) {
        self.update_notice = Some(UpdateNotice {
            message: message.into(),
            expires_at: Instant::now() + Duration::from_secs(2),
        });
    }

    fn render_update_notice(&mut self, ctx: &egui::Context) {
        let Some(notice) = self.update_notice.clone() else {
            return;
        };
        if Instant::now() >= notice.expires_at {
            self.update_notice = None;
            return;
        }

        ctx.request_repaint_after(Duration::from_millis(200));

        egui::Area::new(egui::Id::new("update_notice"))
            .order(Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, vec2(-18.0, 54.0))
            .interactable(false)
            .show(ctx, |ui| {
                let fill = if self.state.ui_theme == UiThemeMode::Dark {
                    Color32::from_rgba_premultiplied(20, 24, 32, 246)
                } else {
                    Color32::from_rgba_premultiplied(250, 251, 253, 246)
                };

                Frame::new()
                    .fill(fill)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(255, 92, 92)))
                    .corner_radius(12.0)
                    .shadow(Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_rgba_premultiplied(0, 0, 0, 72),
                    })
                    .inner_margin(Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (badge_rect, _) =
                                ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            ui.painter().circle_filled(
                                badge_rect.center(),
                                8.0,
                                Color32::from_rgb(255, 60, 60),
                            );
                            ui.painter().text(
                                badge_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "1",
                                egui::FontId::proportional(9.0),
                                Color32::WHITE,
                            );
                            ui.add_space(4.0);
                            ui.label(RichText::new(notice.message).strong().color(
                                if self.state.ui_theme == UiThemeMode::Dark {
                                    Color32::WHITE
                                } else {
                                    Color32::from_rgb(16, 24, 40)
                                },
                            ));
                        });
                    });
            });
    }

    fn shell_toggle_button(
        ui: &mut egui::Ui,
        selected: bool,
        label: RichText,
        tooltip: &str,
    ) -> egui::Response {
        let (fill, stroke, text_color) = if selected {
            (
                Color32::from_rgb(36, 90, 160),
                Color32::from_rgb(102, 196, 255),
                Color32::WHITE,
            )
        } else {
            (
                ui.visuals().widgets.inactive.bg_fill,
                ui.visuals().widgets.inactive.bg_stroke.color,
                ui.visuals().weak_text_color(),
            )
        };
        ui.add(
            Button::new(label.color(text_color))
                .fill(fill)
                .stroke(Stroke::new(1.0, stroke))
                .min_size(vec2(56.0, 24.0)),
        )
        .on_hover_text(tooltip)
    }

    fn folder_icon_text(open: bool, size: f32) -> RichText {
        if open {
            Self::material_icon_text(0xe2c8, size)
        } else {
            Self::material_icon_text(0xe2c7, size)
        }
    }

    fn macro_action_uses_key(action: MacroAction) -> bool {
        matches!(
            action,
            MacroAction::KeyPress
                | MacroAction::KeyDown
                | MacroAction::KeyUp
                | MacroAction::TypeText
                | MacroAction::ApplyWindowPreset
                | MacroAction::FocusWindowPreset
                | MacroAction::TriggerMacroPreset
                | MacroAction::TriggerMacroPresetIfEnabled
                | MacroAction::StopMacroPreset
                | MacroAction::TriggerCommandPreset
                | MacroAction::DisableNetworkAdapter
                | MacroAction::EnableNetworkAdapter
                | MacroAction::CutInternetRoute
                | MacroAction::RestoreInternetRoute
                | MacroAction::SetWifiRadioOff
                | MacroAction::SetWifiRadioOn
                | MacroAction::EnableCrosshairProfile
                | MacroAction::EnablePinPreset
                | MacroAction::PlayMousePathPreset
                | MacroAction::ApplyMouseSensitivityPreset
                | MacroAction::EnableZoomPreset
                | MacroAction::PlaySoundPreset
                | MacroAction::EnableMacroPreset
                | MacroAction::DisableMacroPreset
                | MacroAction::StartTimerPreset
                | MacroAction::PauseTimerPreset
                | MacroAction::StopTimerPreset
                | MacroAction::ReadTimerPreset
                | MacroAction::EnableStep
                | MacroAction::DisableStep
                | MacroAction::LoopStart
                | MacroAction::StopIfKeyPressed
                | MacroAction::LockKeys
                | MacroAction::UnlockKeys
                | MacroAction::StartVisionSearch
                | MacroAction::ScanVisionOnce
                | MacroAction::StopVision
                | MacroAction::StopVisionWait
                | MacroAction::ShowHud
                | MacroAction::OcrSearch
                | MacroAction::IfStart
                | MacroAction::Else
                | MacroAction::IfEnd
                | MacroAction::SetVariable
                | MacroAction::ReadMemory
                | MacroAction::WriteMemory
                | MacroAction::FunnyMemeReply
                | MacroAction::AiResponse
                | MacroAction::DisableCrosshair
                | MacroAction::DisableZoom
                | MacroAction::DisablePin
                | MacroAction::HideHud
                | MacroAction::LockMouse
                | MacroAction::UnlockMouse
                | MacroAction::Wait
                | MacroAction::JumpToStep
        )
    }

    fn macro_action_supports_capture(action: MacroAction) -> bool {
        matches!(
            action,
            MacroAction::KeyPress
                | MacroAction::KeyDown
                | MacroAction::KeyUp
                | MacroAction::StopIfKeyPressed
                | MacroAction::LockKeys
                | MacroAction::UnlockKeys
        )
    }

    fn macro_trigger_mode_label(mode: MacroTriggerMode, language: UiLanguage) -> &'static str {
        let (translation_key, english) = match mode {
            MacroTriggerMode::Press => ("macro_trigger_mode_label.press", "Press"),
            MacroTriggerMode::Hold => ("macro_trigger_mode_label.hold", "Hold"),
            MacroTriggerMode::Release => ("macro_trigger_mode_label.release", "Release"),
            MacroTriggerMode::WindowFocus => ("macro_trigger_mode_label.window_focus", "Focus"),
        };
        match language {
            UiLanguage::Vietnamese => {
                crate::lang::translate(language, translation_key).unwrap_or(english)
            }
            UiLanguage::English | UiLanguage::Icon => english,
        }
    }

    fn macro_group_binding_labels(group: &MacroGroup) -> HashMap<u32, String> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for preset in &group.presets {
            let label = Self::format_macro_trigger_ui(UiLanguage::English, preset);
            *counts.entry(label).or_insert(0) += 1;
        }

        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut labels = HashMap::new();
        for preset in &group.presets {
            let label = Self::format_macro_trigger_ui(UiLanguage::English, preset);
            if counts.get(&label).copied().unwrap_or_default() > 1 && label != "Not set" {
                let entry = seen.entry(label.clone()).or_insert(0);
                *entry += 1;
                labels.insert(preset.id, format!("{label} ({})", *entry));
            } else {
                labels.insert(preset.id, label);
            }
        }
        labels
    }

    fn select_macro_step(
        &mut self,
        group_id: u32,
        preset_id: u32,
        step_index: usize,
        additive: bool,
        currently_selected: bool,
        selected_count_in_preset: usize,
    ) {
        if additive {
            let key = (group_id, preset_id, step_index);
            if !self.selected_macro_steps.insert(key) {
                self.selected_macro_steps.remove(&key);
            }
        } else if currently_selected && selected_count_in_preset <= 1 {
            self.selected_macro_steps.clear();
        } else {
            self.selected_macro_steps.clear();
            self.selected_macro_steps
                .insert((group_id, preset_id, step_index));
        }
    }

    fn clear_macro_step_selection_for_preset(&mut self, group_id: u32, preset_id: u32) {
        self.selected_macro_steps
            .retain(|(selected_group, selected_preset, _)| {
                *selected_group != group_id || *selected_preset != preset_id
            });
    }

    fn set_macro_step_range_selection(
        &mut self,
        group_id: u32,
        preset_id: u32,
        start_index: usize,
        end_index: usize,
    ) {
        self.clear_macro_step_selection_for_preset(group_id, preset_id);
        let start = start_index.min(end_index);
        let end = start_index.max(end_index);
        for step_index in start..=end {
            self.selected_macro_steps
                .insert((group_id, preset_id, step_index));
        }
    }

    fn macro_action_uses_position(action: MacroAction) -> bool {
        matches!(
            action,
            MacroAction::MouseMoveAbsolute | MacroAction::MouseMoveRelative
        )
    }

    fn format_macro_steps_for_ai_context(steps: &[MacroStep]) -> String {
        if steps.is_empty() {
            return "None".to_owned();
        }

        steps
            .iter()
            .enumerate()
            .map(|(index, step)| {
                let json = serde_json::to_string(step).unwrap_or_else(|_| format!("{:?}", step));
                format!("{}. {}", index + 1, json)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_id_name_catalog(title: &str, items: &[(u32, String)]) -> String {
        let mut output = String::new();
        output.push_str(title);
        output.push('\n');
        if items.is_empty() {
            output.push_str("- None\n");
            return output;
        }

        for (id, name) in items {
            output.push_str(&format!("- {id} | {name}\n"));
        }
        output
    }

    fn format_name_catalog(title: &str, items: &[String]) -> String {
        let mut output = String::new();
        output.push_str(title);
        output.push('\n');
        if items.is_empty() {
            output.push_str("- None\n");
            return output;
        }

        for name in items {
            output.push_str(&format!("- {name}\n"));
        }
        output
    }

    fn format_custom_preset_catalog(items: &[CommandPreset]) -> String {
        let mut output = String::new();
        output.push_str("Available custom presets:\n");
        if items.is_empty() {
            output.push_str("- None\n");
            return output;
        }

        for preset in items {
            let target = preset
                .target_window_title
                .as_deref()
                .unwrap_or("Any focused window");
            let command = preset.command.trim();
            let command_preview = if command.is_empty() {
                "no command".to_owned()
            } else if command.chars().count() > 80 {
                let mut preview = command.chars().take(77).collect::<String>();
                preview.push_str("...");
                preview
            } else {
                command.to_owned()
            };
            output.push_str(&format!(
                "- {} | {} | target: {} | command: {}\n",
                preset.id, preset.name, target, command_preview
            ));
        }
        output
    }

    fn mouse_path_event_label(event: MousePathEventKind) -> &'static str {
        match event {
            MousePathEventKind::Move => "Move",
            MousePathEventKind::LeftDown => "LDown",
            MousePathEventKind::LeftUp => "LUp",
            MousePathEventKind::RightDown => "RDown",
            MousePathEventKind::RightUp => "RUp",
            MousePathEventKind::MiddleDown => "MDown",
            MousePathEventKind::MiddleUp => "MUp",
            MousePathEventKind::WheelUp => "Wheel+",
            MousePathEventKind::WheelDown => "Wheel-",
        }
    }

    fn macro_recording_action_to_mouse_path_event(
        action: MacroAction,
        x: i32,
        y: i32,
        delay_ms: u64,
    ) -> Option<MousePathEvent> {
        let kind = match action {
            MacroAction::MouseMoveAbsolute => MousePathEventKind::Move,
            MacroAction::MouseLeftDown | MacroAction::MouseLeftClick => {
                MousePathEventKind::LeftDown
            }
            MacroAction::MouseLeftUp => MousePathEventKind::LeftUp,
            MacroAction::MouseRightDown | MacroAction::MouseRightClick => {
                MousePathEventKind::RightDown
            }
            MacroAction::MouseRightUp => MousePathEventKind::RightUp,
            MacroAction::MouseMiddleDown | MacroAction::MouseMiddleClick => {
                MousePathEventKind::MiddleDown
            }
            MacroAction::MouseMiddleUp => MousePathEventKind::MiddleUp,
            MacroAction::MouseWheelUp => MousePathEventKind::WheelUp,
            MacroAction::MouseWheelDown => MousePathEventKind::WheelDown,
            _ => return None,
        };
        Some(MousePathEvent {
            kind,
            x,
            y,
            delay_ms,
        })
    }

    fn build_macro_steps_from_recording(
        &mut self,
        preset_name: &str,
        events: &[crate::overlay::MacroRecordingEvent],
    ) -> Vec<MacroStep> {
        let mut built_steps = Vec::new();
        let mut elapsed_ms = 0u64;
        let mut last_emitted_at = 0u64;
        let mut first_mouse_at = None;
        let mut first_mouse_insert_index = None;
        let mut mouse_path_events = Vec::new();

        for event in events {
            elapsed_ms = elapsed_ms.saturating_add(event.delay_ms);
            if let Some(path_event) = Self::macro_recording_action_to_mouse_path_event(
                event.action,
                event.x,
                event.y,
                event.delay_ms,
            ) {
                if first_mouse_at.is_none() {
                    first_mouse_at = Some(elapsed_ms);
                    first_mouse_insert_index = Some(built_steps.len());
                }
                mouse_path_events.push(path_event);
                continue;
            }

            let mut step = MacroStep::default();
            step.action = event.action;
            step.delay_ms = elapsed_ms.saturating_sub(last_emitted_at);
            step.x = event.x;
            step.y = event.y;
            if let Some(key) = &event.key {
                step.key = key.clone();
            }
            built_steps.push(step);
            last_emitted_at = elapsed_ms;
        }

        if !mouse_path_events.is_empty() {
            let path_name = format!("{preset_name} Recorded Path");
            let path_preset_id =
                self.add_mouse_path_preset_with_events(path_name, mouse_path_events, false);
            let mut path_step = MacroStep::default();
            path_step.action = MacroAction::PlayMousePathPreset;
            path_step.key = path_preset_id.to_string();
            path_step.delay_ms = first_mouse_at.unwrap_or_default().saturating_sub(
                if let Some(insert_index) = first_mouse_insert_index {
                    if insert_index == 0 {
                        0
                    } else {
                        built_steps[..insert_index]
                            .iter()
                            .fold(0u64, |acc, step| acc.saturating_add(step.delay_ms))
                    }
                } else {
                    0
                },
            );
            path_step.wait_for_completion = false;
            let insert_index = first_mouse_insert_index.unwrap_or(built_steps.len());
            built_steps.insert(insert_index, path_step);
        }

        built_steps
    }

    fn is_copy_feedback_active(until: Option<Instant>) -> bool {
        until.is_some_and(|deadline| Instant::now() < deadline)
    }

    fn macro_share_code_kind_from_text(text: &str) -> MacroShareCodeKind {
        let payload = text.trim();
        if payload.starts_with("MN5_STEP:")
            || payload.starts_with("MN4_STEP:")
            || payload.starts_with("MN3_STEP:")
            || payload.starts_with("MN2_STEP:")
            || payload.starts_with("MN_STEP:")
        {
            MacroShareCodeKind::Step
        } else if payload.starts_with("MN5_PRESET:")
            || payload.starts_with("MN3_PRESET:")
            || payload.starts_with("MN2_PRESET:")
            || payload.starts_with("MN_PRESET:")
        {
            MacroShareCodeKind::Preset
        } else if payload.starts_with("MN5_GROUP:")
            || payload.starts_with("MN3_GROUP:")
            || payload.starts_with("MN2_GROUP:")
            || payload.starts_with("MN_GROUP:")
        {
            MacroShareCodeKind::Group
        } else {
            MacroShareCodeKind::None
        }
    }

    fn refresh_macro_share_clipboard_kind(&mut self, force: bool) {
        if !self.show_share_buttons {
            self.macro_share_clipboard_kind = MacroShareCodeKind::None;
            self.macro_share_clipboard_checked_at = None;
            return;
        }

        if !force
            && self
                .macro_share_clipboard_checked_at
                .is_some_and(|checked_at| checked_at.elapsed() < Duration::from_millis(250))
        {
            return;
        }

        let kind = Clipboard::new()
            .ok()
            .and_then(|mut clipboard| clipboard.get_text().ok())
            .map(|text| Self::macro_share_code_kind_from_text(&text))
            .unwrap_or(MacroShareCodeKind::None);

        self.macro_share_clipboard_kind = kind;
        self.macro_share_clipboard_checked_at = Some(Instant::now());
    }

    fn read_clipboard_text(&mut self) -> Option<String> {
        let mut clipboard = match Clipboard::new() {
            Ok(cb) => cb,
            Err(e) => {
                self.status = format!("Clipboard error: {e}");
                return None;
            }
        };
        match clipboard.get_text() {
            Ok(text) => Some(text),
            Err(e) => {
                self.status = format!("Failed to read clipboard: {e}");
                None
            }
        }
    }

    fn write_macro_share_code(
        &mut self,
        code: String,
        status_message: &'static str,
        kind: MacroShareCodeKind,
        feedback_target: Option<(u32, usize)>,
        preset_target: Option<u32>,
        group_target: Option<u32>,
    ) {
        self.status =
            Self::tr_lang(self.state.ui_language, status_message, status_message).to_owned();
        self.macro_step_export_feedback_until =
            feedback_target.map(|_| Instant::now() + Duration::from_millis(1200));
        self.macro_step_export_feedback_target = feedback_target;
        self.macro_preset_export_feedback_until =
            preset_target.map(|_| Instant::now() + Duration::from_millis(1200));
        self.macro_preset_export_feedback_target = preset_target;
        self.macro_group_export_feedback_until =
            group_target.map(|_| Instant::now() + Duration::from_millis(1200));
        self.macro_group_export_feedback_target = group_target;
        self.macro_share_clipboard_kind = kind;
        self.macro_share_clipboard_checked_at = Some(Instant::now());
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(code);
        }
    }

    fn window_anchor_label(anchor: WindowAnchor) -> &'static str {
        match anchor {
            WindowAnchor::Manual => "Manual",
            WindowAnchor::Center => "Center",
            WindowAnchor::TopLeft => "Top Left",
            WindowAnchor::Top => "Top",
            WindowAnchor::TopRight => "Top Right",
            WindowAnchor::Left => "Left",
            WindowAnchor::Right => "Right",
            WindowAnchor::BottomLeft => "Bottom Left",
            WindowAnchor::Bottom => "Bottom",
            WindowAnchor::BottomRight => "Bottom Right",
        }
    }

    fn window_anchor_icon(anchor: WindowAnchor) -> &'static str {
        match anchor {
            WindowAnchor::Manual => "XY",
            WindowAnchor::Center => "\u{25CE}",
            WindowAnchor::TopLeft => "\u{2196}",
            WindowAnchor::Top => "\u{2191}",
            WindowAnchor::TopRight => "\u{2197}",
            WindowAnchor::Left => "\u{2190}",
            WindowAnchor::Right => "\u{2192}",
            WindowAnchor::BottomLeft => "\u{2199}",
            WindowAnchor::Bottom => "\u{2193}",
            WindowAnchor::BottomRight => "\u{2198}",
        }
    }

    fn window_anchor_picker(ui: &mut egui::Ui, preset: &mut WindowPreset) -> bool {
        let mut changed = false;
        let rows = [
            [
                WindowAnchor::TopLeft,
                WindowAnchor::Top,
                WindowAnchor::TopRight,
            ],
            [
                WindowAnchor::Left,
                WindowAnchor::Center,
                WindowAnchor::Right,
            ],
            [
                WindowAnchor::BottomLeft,
                WindowAnchor::Bottom,
                WindowAnchor::BottomRight,
            ],
        ];

        let draw_anchor_btn = |ui: &mut egui::Ui,
                               anchor: WindowAnchor,
                               selected: bool,
                               hover_text: &str|
         -> egui::Response {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
            let visuals = ui.style().interact(&response);

            let bg_fill = if selected {
                ui.visuals().selection.bg_fill
            } else if response.hovered() {
                visuals.bg_fill
            } else {
                egui::Color32::from_rgb(54, 54, 54)
            };

            let rounding = egui::Rounding::same(6);
            ui.painter().rect_filled(rect, rounding, bg_fill);

            if selected {
                ui.painter().rect_stroke(
                    rect,
                    rounding,
                    egui::Stroke::new(1.0, ui.visuals().selection.stroke.color),
                    egui::StrokeKind::Inside,
                );
            }

            let fg_color = if selected {
                ui.visuals().selection.stroke.color
            } else {
                egui::Color32::from_rgb(220, 220, 220)
            };

            let center = rect.center();

            match anchor {
                WindowAnchor::Manual => {
                    ui.painter().text(
                        center + egui::vec2(0.0, -0.5),
                        egui::Align2::CENTER_CENTER,
                        "XY",
                        egui::FontId::proportional(11.0),
                        fg_color,
                    );
                }
                WindowAnchor::Center => {
                    ui.painter().circle_filled(center, 2.0, fg_color);
                    ui.painter()
                        .circle_stroke(center, 5.0, egui::Stroke::new(1.5, fg_color));
                    ui.painter()
                        .circle_stroke(center, 8.0, egui::Stroke::new(1.5, fg_color));
                }
                _ => {
                    let angle = match anchor {
                        WindowAnchor::TopLeft => 5.0 * std::f32::consts::PI / 4.0,
                        WindowAnchor::Top => 3.0 * std::f32::consts::PI / 2.0,
                        WindowAnchor::TopRight => 7.0 * std::f32::consts::PI / 4.0,
                        WindowAnchor::Left => std::f32::consts::PI,
                        WindowAnchor::Right => 0.0,
                        WindowAnchor::BottomLeft => 3.0 * std::f32::consts::PI / 4.0,
                        WindowAnchor::Bottom => std::f32::consts::PI / 2.0,
                        WindowAnchor::BottomRight => std::f32::consts::PI / 4.0,
                        _ => 0.0,
                    };

                    let dir = egui::vec2(angle.cos(), angle.sin());
                    let dir_perp = egui::vec2(-dir.y, dir.x);

                    let shaft_start = center - dir * 5.0;
                    let shaft_end = center + dir * 1.5;
                    ui.painter()
                        .line_segment([shaft_start, shaft_end], egui::Stroke::new(2.8, fg_color));

                    let tip = center + dir * 6.5;
                    let left_wing = center + dir * 1.0 + dir_perp * 3.8;
                    let right_wing = center + dir * 1.0 - dir_perp * 3.8;

                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![tip, left_wing, right_wing],
                        fg_color,
                        egui::Stroke::NONE,
                    ));
                }
            }

            if !hover_text.is_empty() {
                response.on_hover_text(hover_text)
            } else {
                response
            }
        };

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let manual_response = draw_anchor_btn(
                        ui,
                        WindowAnchor::Manual,
                        preset.anchor == WindowAnchor::Manual,
                        "Manual X/Y position",
                    );
                    if manual_response.clicked() {
                        preset.anchor = WindowAnchor::Manual;
                        changed = true;
                    }
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    for row in rows {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                            for anchor in row {
                                let selected = preset.anchor == anchor;
                                let response = draw_anchor_btn(
                                    ui,
                                    anchor,
                                    selected,
                                    Self::window_anchor_label(anchor),
                                );
                                if response.clicked() {
                                    preset.anchor = anchor;
                                    changed = true;
                                }
                            }
                        });
                    }
                });
            });
        });

        changed
    }

    fn window_anchor_summary(anchor: WindowAnchor) -> &'static str {
        match anchor {
            WindowAnchor::Manual => "Manual X/Y",
            WindowAnchor::Center => "Auto: Center",
            WindowAnchor::TopLeft => "Auto: Top Left",
            WindowAnchor::Top => "Auto: Top Edge",
            WindowAnchor::TopRight => "Auto: Top Right",
            WindowAnchor::Left => "Auto: Left Edge",
            WindowAnchor::Right => "Auto: Right Edge",
            WindowAnchor::BottomLeft => "Auto: Bottom Left",
            WindowAnchor::Bottom => "Auto: Bottom Edge",
            WindowAnchor::BottomRight => "Auto: Bottom Right",
        }
    }

    fn window_anchor_preview_position(preset: &WindowPreset) -> Option<(i32, i32)> {
        if preset.anchor == WindowAnchor::Manual {
            return None;
        }

        #[cfg(windows)]
        unsafe {
            let screen_width = GetSystemMetrics(SM_CXSCREEN);
            let screen_height = GetSystemMetrics(SM_CYSCREEN);
            let width = preset.width.max(1);
            let height = preset.height.max(1);
            let position = match preset.anchor {
                WindowAnchor::Manual => (preset.x, preset.y),
                WindowAnchor::Center => ((screen_width - width) / 2, (screen_height - height) / 2),
                WindowAnchor::TopLeft => (0, 0),
                WindowAnchor::Top => (((screen_width - width) / 2), 0),
                WindowAnchor::TopRight => ((screen_width - width), 0),
                WindowAnchor::Left => (0, ((screen_height - height) / 2)),
                WindowAnchor::Right => ((screen_width - width), ((screen_height - height) / 2)),
                WindowAnchor::BottomLeft => (0, (screen_height - height)),
                WindowAnchor::Bottom => (((screen_width - width) / 2), (screen_height - height)),
                WindowAnchor::BottomRight => ((screen_width - width), (screen_height - height)),
            };
            return Some(position);
        }

        #[allow(unreachable_code)]
        None
    }

    fn edit_rgba_color(ui: &mut egui::Ui, color: &mut RgbaColor) -> egui::Response {
        let popup_id = ui.make_persistent_id(color as *const RgbaColor as usize);
        Self::edit_rgba_color_with_id(ui, color, popup_id)
    }

    fn edit_rgba_color_with_id(
        ui: &mut egui::Ui,
        color: &mut RgbaColor,
        id: egui::Id,
    ) -> egui::Response {
        let mut changed = false;
        let mut popup_open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(id))
            .unwrap_or(false);

        // Draw a small color button with preview
        let button_size = egui::vec2(28.0, 18.0);
        let (rect, mut response) = ui.allocate_exact_size(button_size, egui::Sense::click());

        if response.clicked() {
            popup_open = !popup_open;
        }

        // Paint the preview rectangle
        let c32 = egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
        ui.painter().rect_filled(rect, 3.0, c32);

        // Paint a hover highlight or a standard border
        let stroke_color = if response.hovered() {
            ui.visuals().widgets.hovered.bg_stroke.color
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        ui.painter().rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );

        // Create the popup
        let popup_response = egui::Popup::from_response(&response)
            .id(id)
            .open_bool(&mut popup_open)
            .align(egui::RectAlign::BOTTOM_START)
            .layout(egui::Layout::top_down_justified(egui::Align::Min))
            .width(260.0)
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .show(|ui| {
                ui.set_min_width(260.0);
                if Self::render_premium_color_picker(
                    ui,
                    color,
                    egui::color_picker::Alpha::BlendOrAdditive,
                ) {
                    changed = true;
                }
            });

        // Close popup if cursor hovers away from both the button and the popup
        if popup_open && !ui.input(|input| input.pointer.any_down()) {
            if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
                let mut keep_open_rect = response.rect.expand(24.0);
                if let Some(ref popup) = popup_response {
                    keep_open_rect = keep_open_rect.union(popup.response.rect.expand(24.0));
                }
                if !keep_open_rect.contains(pointer_pos) {
                    popup_open = false;
                }
            }
        }

        ui.ctx().data_mut(|data| data.insert_temp(id, popup_open));

        if changed {
            response.mark_changed();
        }
        response
    }

    pub(crate) fn render_timer_rect_editor(
        ui: &mut egui::Ui,
        id_source: impl std::hash::Hash + Copy,
        preset: &mut TimerPreset,
    ) -> bool {
        let mut changed = false;
        let screen_size = Self::screen_size();
        let desired = vec2(ui.available_width().max(560.0), 420.0);
        let (canvas_rect, response) =
            ui.allocate_exact_size(desired, Sense::drag().union(Sense::click()));

        let mut arrow_dx = 0;
        let mut arrow_dy = 0;
        if response.hovered() || response.has_focus() {
            ui.input(|i| {
                if i.key_pressed(egui::Key::ArrowLeft) {
                    arrow_dx -= 1;
                }
                if i.key_pressed(egui::Key::ArrowRight) {
                    arrow_dx += 1;
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    arrow_dy -= 1;
                }
                if i.key_pressed(egui::Key::ArrowDown) {
                    arrow_dy += 1;
                }
            });
            if arrow_dx != 0 || arrow_dy != 0 {
                preset.x = (preset.x + arrow_dx).clamp(0, screen_size.x.round() as i32);
                preset.y = (preset.y + arrow_dy).clamp(0, screen_size.y.round() as i32);
                changed = true;
            }
        }

        let draw_rect = canvas_rect.shrink(8.0);
        let scale = (draw_rect.width() / screen_size.x)
            .min(draw_rect.height() / screen_size.y)
            .max(0.0001);
        let preview_size = vec2(screen_size.x * scale, screen_size.y * scale);
        let preview_rect = egui::Rect::from_center_size(draw_rect.center(), preview_size);
        ui.painter().rect_filled(
            preview_rect,
            8.0,
            Color32::from_rgba_premultiplied(18, 24, 22, 220),
        );
        ui.painter().rect_stroke(
            preview_rect,
            8.0,
            egui::Stroke::new(1.0, Color32::from_rgb(104, 148, 124)),
            egui::StrokeKind::Outside,
        );

        let min_size = vec2(4.0, 4.0);
        let mut rect = egui::Rect::from_min_size(
            egui::pos2(
                preview_rect.left() + (preset.x as f32 * scale),
                preview_rect.top() + (preset.y as f32 * scale),
            ),
            vec2(
                preset.width.max(1) as f32 * scale,
                preset.height.max(1) as f32 * scale,
            ),
        )
        .intersect(preview_rect);
        if rect.width() < min_size.x {
            rect.max.x = (rect.min.x + min_size.x).min(preview_rect.right());
        }
        if rect.height() < min_size.y {
            rect.max.y = (rect.min.y + min_size.y).min(preview_rect.bottom());
        }

        let rect_id = ui.make_persistent_id((id_source, "timer-rect"));
        let drag_id = ui.make_persistent_id((id_source, "timer-selection-drag-handle"));
        let offset_id = ui.make_persistent_id((id_source, "timer-selection-drag-offset"));
        let anchor_id = ui.make_persistent_id((id_source, "timer-selection-drag-anchor"));

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum SelectionDragHandle {
            None,
            Center,
            TopLeft,
            TopRight,
            BottomLeft,
            BottomRight,
            Left,
            Right,
            Top,
            Bottom,
        }

        let mut active_handle: SelectionDragHandle =
            ui.data_mut(|d| d.get_temp(drag_id).unwrap_or(SelectionDragHandle::None));
        let mut drag_offset: egui::Vec2 =
            ui.data_mut(|d| d.get_temp(offset_id).unwrap_or(egui::Vec2::ZERO));
        let mut drag_anchor: egui::Pos2 =
            ui.data_mut(|d| d.get_temp(anchor_id).unwrap_or(egui::Pos2::ZERO));

        let pick_selection_drag_handle = |pointer_pos: egui::Pos2, rect: egui::Rect| {
            let dist_tl = pointer_pos.distance(rect.left_top());
            let dist_tr = pointer_pos.distance(rect.right_top());
            let dist_bl = pointer_pos.distance(rect.left_bottom());
            let dist_br = pointer_pos.distance(rect.right_bottom());
            let edge_threshold = 10.0;
            let vertical_hit_min = rect.top() - edge_threshold;
            let vertical_hit_max = rect.bottom() + edge_threshold;
            let horizontal_hit_min = rect.left() - edge_threshold;
            let horizontal_hit_max = rect.right() + edge_threshold;

            if dist_tl < 14.0 {
                SelectionDragHandle::TopLeft
            } else if dist_tr < 14.0 {
                SelectionDragHandle::TopRight
            } else if dist_bl < 14.0 {
                SelectionDragHandle::BottomLeft
            } else if dist_br < 14.0 {
                SelectionDragHandle::BottomRight
            } else if (pointer_pos.x - rect.left()).abs() < edge_threshold
                && pointer_pos.y >= vertical_hit_min
                && pointer_pos.y <= vertical_hit_max
            {
                SelectionDragHandle::Left
            } else if (pointer_pos.x - rect.right()).abs() < edge_threshold
                && pointer_pos.y >= vertical_hit_min
                && pointer_pos.y <= vertical_hit_max
            {
                SelectionDragHandle::Right
            } else if (pointer_pos.y - rect.top()).abs() < edge_threshold
                && pointer_pos.x >= horizontal_hit_min
                && pointer_pos.x <= horizontal_hit_max
            {
                SelectionDragHandle::Top
            } else if (pointer_pos.y - rect.bottom()).abs() < edge_threshold
                && pointer_pos.x >= horizontal_hit_min
                && pointer_pos.x <= horizontal_hit_max
            {
                SelectionDragHandle::Bottom
            } else if rect.contains(pointer_pos) {
                SelectionDragHandle::Center
            } else {
                SelectionDragHandle::None
            }
        };

        if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            if let Some(pointer_pos) = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos())
            {
                active_handle = pick_selection_drag_handle(pointer_pos, rect);
                ui.data_mut(|d| d.insert_temp(drag_id, active_handle));

                drag_offset = match active_handle {
                    SelectionDragHandle::Center => pointer_pos - rect.min,
                    SelectionDragHandle::Left
                    | SelectionDragHandle::TopLeft
                    | SelectionDragHandle::BottomLeft => {
                        let ox = pointer_pos.x - rect.min.x;
                        let oy = if active_handle == SelectionDragHandle::TopLeft {
                            pointer_pos.y - rect.min.y
                        } else if active_handle == SelectionDragHandle::BottomLeft {
                            pointer_pos.y - rect.max.y
                        } else {
                            0.0
                        };
                        egui::vec2(ox, oy)
                    }
                    SelectionDragHandle::Right
                    | SelectionDragHandle::TopRight
                    | SelectionDragHandle::BottomRight => {
                        let ox = pointer_pos.x - rect.max.x;
                        let oy = if active_handle == SelectionDragHandle::TopRight {
                            pointer_pos.y - rect.min.y
                        } else if active_handle == SelectionDragHandle::BottomRight {
                            pointer_pos.y - rect.max.y
                        } else {
                            0.0
                        };
                        egui::vec2(ox, oy)
                    }
                    SelectionDragHandle::Top => egui::vec2(0.0, pointer_pos.y - rect.min.y),
                    SelectionDragHandle::Bottom => egui::vec2(0.0, pointer_pos.y - rect.max.y),
                    SelectionDragHandle::None => egui::Vec2::ZERO,
                };
                ui.data_mut(|d| d.insert_temp(offset_id, drag_offset));

                drag_anchor = match active_handle {
                    SelectionDragHandle::Left | SelectionDragHandle::TopLeft => rect.max,
                    SelectionDragHandle::BottomLeft => egui::pos2(rect.max.x, rect.min.y),
                    SelectionDragHandle::Right | SelectionDragHandle::BottomRight => rect.min,
                    SelectionDragHandle::TopRight => egui::pos2(rect.min.x, rect.max.y),
                    SelectionDragHandle::Top => rect.max,
                    SelectionDragHandle::Bottom => rect.min,
                    _ => egui::Pos2::ZERO,
                };
                ui.data_mut(|d| d.insert_temp(anchor_id, drag_anchor));
            }
        }

        let pointer_primary_down = ui.input(|i| i.pointer.primary_down());
        if pointer_primary_down && active_handle != SelectionDragHandle::None {
            if let Some(pointer_pos) = ui
                .input(|i| i.pointer.latest_pos())
                .or_else(|| ui.input(|i| i.pointer.hover_pos()))
            {
                let shift_pressed = ui.input(|i| i.modifiers.shift);
                let original_aspect = if preset.height > 0 {
                    preset.width as f32 / preset.height as f32
                } else {
                    16.0 / 9.0
                };
                let lock_aspect = if shift_pressed { original_aspect } else { 0.0 };

                changed = true;

                let mut target_pos = pointer_pos - drag_offset;

                match active_handle {
                    SelectionDragHandle::Left
                    | SelectionDragHandle::TopLeft
                    | SelectionDragHandle::BottomLeft => {
                        target_pos.x = target_pos
                            .x
                            .clamp(preview_rect.left(), drag_anchor.x - min_size.x);
                    }
                    SelectionDragHandle::Right
                    | SelectionDragHandle::TopRight
                    | SelectionDragHandle::BottomRight => {
                        target_pos.x = target_pos
                            .x
                            .clamp(drag_anchor.x + min_size.x, preview_rect.right());
                    }
                    _ => {}
                }
                match active_handle {
                    SelectionDragHandle::Top
                    | SelectionDragHandle::TopLeft
                    | SelectionDragHandle::TopRight => {
                        target_pos.y = target_pos
                            .y
                            .clamp(preview_rect.top(), drag_anchor.y - min_size.y);
                    }
                    SelectionDragHandle::Bottom
                    | SelectionDragHandle::BottomLeft
                    | SelectionDragHandle::BottomRight => {
                        target_pos.y = target_pos
                            .y
                            .clamp(drag_anchor.y + min_size.y, preview_rect.bottom());
                    }
                    _ => {}
                }
                if active_handle == SelectionDragHandle::Center {
                    target_pos.x = target_pos
                        .x
                        .clamp(preview_rect.left(), preview_rect.right() - rect.width());
                    target_pos.y = target_pos
                        .y
                        .clamp(preview_rect.top(), preview_rect.bottom() - rect.height());
                }

                match active_handle {
                    SelectionDragHandle::Center => {
                        let size = rect.size();
                        rect.min = target_pos;
                        rect.max = rect.min + size;
                    }
                    SelectionDragHandle::Left => {
                        let new_left = target_pos.x.min(drag_anchor.x - min_size.x);
                        rect.min.x = new_left;
                        rect.max.x = drag_anchor.x;
                    }
                    SelectionDragHandle::Right => {
                        let new_right = target_pos.x.max(drag_anchor.x + min_size.x);
                        rect.min.x = drag_anchor.x;
                        rect.max.x = new_right;
                    }
                    SelectionDragHandle::Top => {
                        let new_top = target_pos.y.min(drag_anchor.y - min_size.y);
                        rect.min.y = new_top;
                        rect.max.y = drag_anchor.y;
                    }
                    SelectionDragHandle::Bottom => {
                        let new_bottom = target_pos.y.max(drag_anchor.y + min_size.y);
                        rect.min.y = drag_anchor.y;
                        rect.max.y = new_bottom;
                    }
                    SelectionDragHandle::TopLeft => {
                        let new_left = target_pos.x.min(drag_anchor.x - min_size.x);
                        let new_top = target_pos.y.min(drag_anchor.y - min_size.y);
                        rect.min = egui::pos2(new_left, new_top);
                        rect.max = drag_anchor;
                    }
                    SelectionDragHandle::TopRight => {
                        let new_right = target_pos.x.max(drag_anchor.x + min_size.x);
                        let new_top = target_pos.y.min(drag_anchor.y - min_size.y);
                        rect.min = egui::pos2(drag_anchor.x, new_top);
                        rect.max = egui::pos2(new_right, drag_anchor.y);
                    }
                    SelectionDragHandle::BottomLeft => {
                        let new_left = target_pos.x.min(drag_anchor.x - min_size.x);
                        let new_bottom = target_pos.y.max(drag_anchor.y + min_size.y);
                        rect.min = egui::pos2(new_left, drag_anchor.y);
                        rect.max = egui::pos2(drag_anchor.x, new_bottom);
                    }
                    SelectionDragHandle::BottomRight => {
                        let new_right = target_pos.x.max(drag_anchor.x + min_size.x);
                        let new_bottom = target_pos.y.max(drag_anchor.y + min_size.y);
                        rect.min = drag_anchor;
                        rect.max = egui::pos2(new_right, new_bottom);
                    }
                    SelectionDragHandle::None => {}
                }

                if lock_aspect > 0.0 {
                    match active_handle {
                        SelectionDragHandle::Right
                        | SelectionDragHandle::BottomRight
                        | SelectionDragHandle::TopRight => {
                            let new_h = rect.width() / lock_aspect;
                            if active_handle == SelectionDragHandle::TopRight {
                                rect.min.y = rect.max.y - new_h;
                            } else {
                                rect.max.y = rect.min.y + new_h;
                            }
                        }
                        SelectionDragHandle::Left
                        | SelectionDragHandle::TopLeft
                        | SelectionDragHandle::BottomLeft => {
                            let new_h = rect.width() / lock_aspect;
                            if active_handle == SelectionDragHandle::TopLeft {
                                rect.min.y = rect.max.y - new_h;
                            } else {
                                rect.max.y = rect.min.y + new_h;
                            }
                        }
                        SelectionDragHandle::Bottom => {
                            let new_w = rect.height() * lock_aspect;
                            rect.max.x = rect.min.x + new_w;
                        }
                        SelectionDragHandle::Top => {
                            let new_w = rect.height() * lock_aspect;
                            rect.min.x = rect.max.x - new_w;
                        }
                        _ => {}
                    }
                }

                if active_handle == SelectionDragHandle::Center {
                    if rect.left() < preview_rect.left() {
                        rect = rect.translate(egui::vec2(preview_rect.left() - rect.left(), 0.0));
                    }
                    if rect.top() < preview_rect.top() {
                        rect = rect.translate(egui::vec2(0.0, preview_rect.top() - rect.top()));
                    }
                    if rect.right() > preview_rect.right() {
                        rect = rect.translate(egui::vec2(preview_rect.right() - rect.right(), 0.0));
                    }
                    if rect.bottom() > preview_rect.bottom() {
                        rect =
                            rect.translate(egui::vec2(0.0, preview_rect.bottom() - rect.bottom()));
                    }
                }

                rect.min.x = rect
                    .min
                    .x
                    .clamp(preview_rect.left(), preview_rect.right() - min_size.x);
                rect.min.y = rect
                    .min
                    .y
                    .clamp(preview_rect.top(), preview_rect.bottom() - min_size.y);
                rect.max.x = rect
                    .max
                    .x
                    .clamp(rect.min.x + min_size.x, preview_rect.right());
                rect.max.y = rect
                    .max
                    .y
                    .clamp(rect.min.y + min_size.y, preview_rect.bottom());
            }
        }

        if ui.input(|i| i.pointer.any_released()) {
            active_handle = SelectionDragHandle::None;
            ui.data_mut(|d| d.insert_temp(drag_id, active_handle));
        }

        if response.hovered() || active_handle != SelectionDragHandle::None {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.hover_pos()) {
                let mut handle_to_use = if active_handle != SelectionDragHandle::None {
                    active_handle
                } else {
                    pick_selection_drag_handle(pointer_pos, rect)
                };
                if active_handle == SelectionDragHandle::None
                    && handle_to_use == SelectionDragHandle::Center
                    && !rect.contains(pointer_pos)
                {
                    handle_to_use = SelectionDragHandle::None;
                }

                match handle_to_use {
                    SelectionDragHandle::TopLeft | SelectionDragHandle::BottomRight => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                    }
                    SelectionDragHandle::TopRight | SelectionDragHandle::BottomLeft => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNeSw);
                    }
                    SelectionDragHandle::Left | SelectionDragHandle::Right => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    SelectionDragHandle::Top | SelectionDragHandle::Bottom => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    }
                    SelectionDragHandle::Center => {
                        if active_handle == SelectionDragHandle::Center {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        } else {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                    }
                    _ => {}
                }
            }
        }

        let size_text = format!("{}x{}", preset.width, preset.height);
        ui.painter().text(
            rect.left_top() + egui::vec2(0.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            size_text,
            egui::FontId::proportional(10.0),
            Color32::from_rgb(124, 240, 164),
        );

        let bg_alpha = (preset.background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let background = Color32::from_rgba_premultiplied(
            ((preset.background_color.r as u32 * bg_alpha as u32) / 255) as u8,
            ((preset.background_color.g as u32 * bg_alpha as u32) / 255) as u8,
            ((preset.background_color.b as u32 * bg_alpha as u32) / 255) as u8,
            bg_alpha,
        );
        let text_color = Color32::from_rgba_premultiplied(
            preset.text_color.r,
            preset.text_color.g,
            preset.text_color.b,
            preset.text_color.a,
        );
        let rounding = if preset.rounded_background { 12.0 } else { 0.0 };
        if bg_alpha > 0 {
            ui.painter().rect_filled(rect, rounding, background);
        }
        ui.painter().rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(2.0, Color32::from_rgb(124, 240, 164)),
            egui::StrokeKind::Outside,
        );
        if preset.show_text {
            let preview_text = crate::overlay::format_stopwatch_time(
                125_432,
                preset.show_minutes,
                preset.show_seconds,
                preset.show_ms,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                preview_text,
                egui::FontId::proportional((preset.font_size * scale).clamp(2.0, 200.0)),
                text_color,
            );
        }

        if changed {
            preset.x = ((rect.left() - preview_rect.left()) / scale).round() as i32;
            preset.y = ((rect.top() - preview_rect.top()) / scale).round() as i32;
            preset.width = (rect.width() / scale).round().max(1.0) as i32;
            preset.height = (rect.height() / scale).round().max(1.0) as i32;
        }

        ui.label(
            RichText::new(format!(
                "X={} Y={} W={} H={}",
                preset.x, preset.y, preset.width, preset.height
            ))
            .small(),
        );
        changed
    }

    fn capture_button_text(language: UiLanguage, active: bool) -> RichText {
        if active {
            RichText::new(Self::tr_lang(language, "Capturing...", "Capturing..."))
                .strong()
                .color(Color32::from_rgb(255, 232, 96))
        } else {
            RichText::new(Self::tr_lang(language, "Capture", "Capture"))
        }
    }

    fn ai_generation_feedback(error: &str) -> String {
        let mut message = format!("AI generation skipped: {error}");
        if error.contains("JSON") || error.contains("script") {
            message.push_str(
                "\nHint: the model returned prose or malformed script/JSON instead of the macro format this app expects.",
            );
        }
        message
    }

    #[allow(deprecated)]
    fn show_instant_hover_tooltip(
        ui: &egui::Ui,
        response: &egui::Response,
        text: impl Into<String>,
    ) {
        let text = text.into();
        let _ = response.clone().on_hover_ui(|ui| {
            ui.set_max_width(280.0);
            ui.label(&text);
        });
    }

    fn capture_master_preset_snapshot(&self, id: u32, name: String) -> MasterPreset {
        MasterPreset {
            id,
            name,
            collapsed: true,
            macros_master_enabled: self.state.macros_master_enabled,
            window_expand_controls_enabled: self.state.window_expand_controls.enabled,
            window_presets: self
                .state
                .window_presets
                .iter()
                .map(|preset| MasterWindowPresetState {
                    id: preset.id,
                    enabled: preset.enabled,
                    animate_enabled: preset.animate_enabled,
                    restore_titlebar_enabled: preset.restore_titlebar_enabled,
                })
                .collect(),
            window_focus_presets: self
                .state
                .window_focus_presets
                .iter()
                .map(|preset| MasterWindowFocusPresetState {
                    id: preset.id,
                    enabled: preset.enabled,
                })
                .collect(),
            zoom_presets: self
                .state
                .zoom_presets
                .iter()
                .map(|preset| MasterZoomPresetState {
                    id: preset.id,
                    enabled: preset.enabled,
                })
                .collect(),
            macro_groups: self
                .state
                .macro_groups
                .iter()
                .map(|group| MasterMacroGroupState {
                    id: group.id,
                    enabled: group.enabled,
                    presets: group
                        .presets
                        .iter()
                        .map(|preset| MasterMacroPresetState {
                            id: preset.id,
                            enabled: preset.enabled,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    fn ensure_master_presets_without_persist(&mut self) -> bool {
        let before_presets = self.state.master_presets.clone();
        let before_selected = self.state.selected_master_preset_id;

        if self.state.master_presets.is_empty() {
            let id = self.allocate_next_master_preset_id();
            self.state
                .master_presets
                .push(self.capture_master_preset_snapshot(id, "Default".to_owned()));
            self.state.selected_master_preset_id = Some(id);
        } else {
            self.reconcile_master_presets();
            if self.state.selected_master_preset_id.is_none() {
                self.state.selected_master_preset_id =
                    self.state.master_presets.first().map(|preset| preset.id);
            }
        }

        self.state.master_presets != before_presets
            || self.state.selected_master_preset_id != before_selected
    }

    fn reconcile_master_presets(&mut self) {
        let window_lookup = self
            .state
            .window_presets
            .iter()
            .map(|preset| {
                (
                    preset.id,
                    MasterWindowPresetState {
                        id: preset.id,
                        enabled: preset.enabled,
                        animate_enabled: preset.animate_enabled,
                        restore_titlebar_enabled: preset.restore_titlebar_enabled,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let focus_lookup = self
            .state
            .window_focus_presets
            .iter()
            .map(|preset| {
                (
                    preset.id,
                    MasterWindowFocusPresetState {
                        id: preset.id,
                        enabled: preset.enabled,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let zoom_lookup = self
            .state
            .zoom_presets
            .iter()
            .map(|preset| {
                (
                    preset.id,
                    MasterZoomPresetState {
                        id: preset.id,
                        enabled: preset.enabled,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let macro_lookup = self
            .state
            .macro_groups
            .iter()
            .map(|group| {
                (
                    group.id,
                    MasterMacroGroupState {
                        id: group.id,
                        enabled: group.enabled,
                        presets: group
                            .presets
                            .iter()
                            .map(|preset| MasterMacroPresetState {
                                id: preset.id,
                                enabled: preset.enabled,
                            })
                            .collect(),
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        for preset in &mut self.state.master_presets {
            preset
                .window_presets
                .retain(|item| window_lookup.contains_key(&item.id));
            for window_preset in &self.state.window_presets {
                if !preset
                    .window_presets
                    .iter()
                    .any(|item| item.id == window_preset.id)
                    && let Some(item) = window_lookup.get(&window_preset.id)
                {
                    preset.window_presets.push(item.clone());
                }
            }
            preset.window_presets.sort_by_key(|item| {
                Self::ordered_id_index(&self.state.window_presets, item.id, |preset| preset.id)
            });

            preset
                .window_focus_presets
                .retain(|item| focus_lookup.contains_key(&item.id));
            for focus_preset in &self.state.window_focus_presets {
                if !preset
                    .window_focus_presets
                    .iter()
                    .any(|item| item.id == focus_preset.id)
                    && let Some(item) = focus_lookup.get(&focus_preset.id)
                {
                    preset.window_focus_presets.push(item.clone());
                }
            }
            preset.window_focus_presets.sort_by_key(|item| {
                Self::ordered_id_index(&self.state.window_focus_presets, item.id, |preset| {
                    preset.id
                })
            });

            preset
                .zoom_presets
                .retain(|item| zoom_lookup.contains_key(&item.id));
            for zoom_preset in &self.state.zoom_presets {
                if !preset
                    .zoom_presets
                    .iter()
                    .any(|item| item.id == zoom_preset.id)
                    && let Some(item) = zoom_lookup.get(&zoom_preset.id)
                {
                    preset.zoom_presets.push(item.clone());
                }
            }
            preset.zoom_presets.sort_by_key(|item| {
                Self::ordered_id_index(&self.state.zoom_presets, item.id, |preset| preset.id)
            });

            preset
                .macro_groups
                .retain(|item| macro_lookup.contains_key(&item.id));
            for macro_group in &self.state.macro_groups {
                if !preset
                    .macro_groups
                    .iter()
                    .any(|item| item.id == macro_group.id)
                    && let Some(item) = macro_lookup.get(&macro_group.id)
                {
                    preset.macro_groups.push(item.clone());
                }
            }
            for group_state in &mut preset.macro_groups {
                if let Some(group) = self
                    .state
                    .macro_groups
                    .iter()
                    .find(|group| group.id == group_state.id)
                {
                    group_state
                        .presets
                        .retain(|item| group.presets.iter().any(|preset| preset.id == item.id));
                    for preset_item in &group.presets {
                        if !group_state
                            .presets
                            .iter()
                            .any(|item| item.id == preset_item.id)
                        {
                            group_state.presets.push(MasterMacroPresetState {
                                id: preset_item.id,
                                enabled: preset_item.enabled,
                            });
                        }
                    }
                    group_state.presets.sort_by_key(|item| {
                        Self::ordered_id_index(&group.presets, item.id, |preset| preset.id)
                    });
                }
            }
            preset.macro_groups.sort_by_key(|item| {
                Self::ordered_id_index(&self.state.macro_groups, item.id, |group| group.id)
            });
        }
    }

    fn add_macro_group(&mut self) {
        let id = Self::allocate_next_id(
            &self.state.macro_groups,
            &mut self.state.next_macro_group_id,
            |group| group.id,
        );
        let mut group = MacroGroup::new(id);
        group.name = self.unique_macro_group_name(&group.name);
        let preset_id = self.allocate_next_macro_preset_id();
        group.presets = vec![MacroPreset::new(preset_id)];
        self.state.macro_groups.push(group);
        self.pending_macro_group_scroll_target = Some(id);
        self.sync_reconciled_macro_presets();
        self.status = format!("Added macro group {id}.");
    }

    fn add_macro_preset_to_group(&mut self, group_id: u32) {
        let id = self.allocate_next_macro_preset_id();
        if let Some(group) = self
            .state
            .macro_groups
            .iter_mut()
            .find(|group| group.id == group_id)
        {
            group.presets.push(MacroPreset::new(id));
            self.sync_reconciled_macro_presets();
            self.status = format!("Added macro preset {id}.");
        }
    }

    fn open_command_ai_dialog_for_preset(&mut self, preset_id: u32) {
        if self.command_ai_job.is_some() {
            self.status = "AI generation is already running.".to_owned();
            return;
        }
        let Some(preset) = self
            .state
            .command_presets
            .iter()
            .find(|preset| preset.id == preset_id)
        else {
            self.status = "Custom preset not found.".to_owned();
            return;
        };

        self.command_ai_dialog = Some(CommandAiDialog {
            preset_id,
            prompt: String::new(),
        });
        self.command_ai_feedback = None;
        self.status = format!("Ready to generate a custom command for {}.", preset.name);
    }

    fn build_custom_ai_prompt(&self, preset: &CommandPreset, user_prompt: &str) -> String {
        let current_preset = serde_json::to_string_pretty(preset)
            .unwrap_or_else(|_| serde_json::to_string(preset).unwrap_or_else(|_| "{}".to_owned()));
        let target_window = preset
            .target_window_title
            .as_deref()
            .unwrap_or("Any focused window");
        let extra_windows = if preset.extra_target_window_titles.is_empty() {
            "None".to_owned()
        } else {
            preset.extra_target_window_titles.join(", ")
        };
        let open_windows = if self.open_window_infos.is_empty() {
            "None".to_owned()
        } else {
            self.open_window_infos
                .iter()
                .map(|window| window.selector.clone())
                .collect::<Vec<_>>()
                .join("\n- ")
        };
        let shell_type = if preset.use_powershell {
            "PowerShell"
        } else {
            "CMD"
        };
        let other_shell = if preset.use_powershell {
            "CMD"
        } else {
            "PowerShell"
        };
        let power_rule = format!(
            "The target environment is configured to use {}. You MUST write the 'command' field specifically as a {} command, NOT a {} command. Do NOT change the 'use_powershell' field in the JSON (keep it as {}).",
            shell_type, shell_type, other_shell, preset.use_powershell
        );
        format!(
            "Edit the current MacroNest custom preset for one existing preset.\n\
             \n\
             Custom preset name: {}\n\
             Target window: {}\n\
             Extra target windows: {}\n\
             Current custom preset JSON:\n{}\n\
             Available open windows:\n- {}\n\
             \n\
             Rules:\n\
             - Return only a JSON object.\n\
             - Use only fields that exist in CommandPreset.\n\
             - Omit any field you do not want to change.\n\
             - Do not invent new fields or prose.\n\
             - IMPORTANT: {}\n\
             - The command field must be a shell command or PowerShell command string, not a macro step list.\n\
             - If the user asks for a simple task like shutdown, open app, launch file, or run console commands, encode that as the command string.\n\
             - If the user says center or center of the screen, that is not screen coordinate 0,0; that means the middle of the screen.\n\
             - Keep unrelated fields unchanged.\n\
             - IMPORTANT: You MUST also generate an appropriate, concise, and descriptive name for the 'name' field (in the same language as the user request, maximum 3-5 words, e.g., 'Start MsPaint' or 'Start Discord') that summarizes what the new command does. Do not leave the 'name' field unchanged if the command's behavior is changed.\n\
             - The JSON object will be treated as a patch and applied onto the current custom preset.\n\
             \n\
             User request: {}\n",
            preset.name.trim(),
            target_window,
            extra_windows,
            current_preset,
            open_windows,
            power_rule,
            user_prompt.trim()
        )
    }

    fn start_custom_ai_generation(&mut self, ctx: &egui::Context) {
        let Some(dialog_snapshot) = self
            .command_ai_dialog
            .as_ref()
            .map(|dialog| (dialog.preset_id, dialog.prompt.trim().to_owned()))
        else {
            return;
        };
        if self.command_ai_job.is_some() {
            self.command_ai_feedback = Some("AI generation is already running.".to_owned());
            self.status = "AI generation is already running.".to_owned();
            return;
        }
        let (preset_id, prompt) = dialog_snapshot;
        if prompt.is_empty() {
            self.command_ai_feedback = Some("Type what custom command you want first.".to_owned());
            self.status = "Type what custom command you want first.".to_owned();
            return;
        }
        if self.state.groq_settings.api_key.trim().is_empty() {
            self.open_groq_api_settings();
            self.command_ai_dialog = None;
            self.command_ai_feedback =
                Some("Open Settings > API and paste your Groq API key.".to_owned());
            self.status = "Open Settings > API and paste your Groq API key.".to_owned();
            return;
        }
        let Some(preset) = self
            .state
            .command_presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .cloned()
        else {
            self.command_ai_feedback = Some("Custom preset not found.".to_owned());
            self.status = "Custom preset not found.".to_owned();
            self.command_ai_dialog = None;
            return;
        };

        let groq_settings = self.state.groq_settings.clone();
        let prompt_body = self.build_custom_ai_prompt(&preset, &prompt);
        let system_instruction = "You are a deterministic MacroNest custom preset compiler. Return one JSON object only. Use only fields that exist in CommandPreset. You MUST also generate an appropriate, concise, descriptive name in the 'name' field that summarizes the command. The command field must contain a shell or PowerShell command string. Do not return markdown, arrays, or prose.";
        let (tx, rx) = crossbeam_channel::bounded(1);
        let token = self.command_ai_next_token.max(1);
        self.command_ai_next_token = token + 1;
        self.command_ai_job = Some(CommandAiJob {
            token,
            preset_id,
            receiver: rx,
        });
        self.command_ai_feedback = Some("Generating custom preset...".to_owned());
        self.status = format!(
            "Generating a custom preset for {} using Groq...",
            preset.name
        );
        let thread_ctx = ctx.clone();
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(|| {
                ai::generate_command_preset_patch_groq(
                    &groq_settings,
                    &prompt_body,
                    system_instruction,
                )
                .map_err(|error| error.to_string())
            })
            .unwrap_or_else(|_| Err("AI generation panicked.".to_owned()));
            let _ = tx.send(CommandAiJobResult {
                token,
                preset_id,
                outcome,
            });
            thread_ctx.request_repaint();
        });
        ctx.request_repaint();
    }

    fn apply_custom_ai_generated_patch(&mut self, preset_id: u32, patch: ai::CommandPresetPatch) {
        if preset_id == 999999 {
            if let Some(target) = self.command_ai_step_target.take() {
                let (group_id, preset_id, step_index) = target;
                let mut temp_preset = CommandPreset::new(999999);
                if let Some(preset) = self.macro_preset(group_id, preset_id) {
                    if let Some(step_index) = step_index {
                        if let Some(step) = preset.steps.get(step_index) {
                            temp_preset.command = step.command_preset_command.clone();
                            temp_preset.use_powershell = step.command_preset_use_powershell;
                        }
                    } else if preset.trigger_mode == crate::model::MacroTriggerMode::Hold {
                        temp_preset.command = preset.hold_stop_step.command_preset_command.clone();
                        temp_preset.use_powershell =
                            preset.hold_stop_step.command_preset_use_powershell;
                    } else {
                        temp_preset.command = preset.press_stop_step.command_preset_command.clone();
                        temp_preset.use_powershell =
                            preset.press_stop_step.command_preset_use_powershell;
                    }
                }
                let old_name = temp_preset.name.clone();
                let old_use_powershell = temp_preset.use_powershell;
                patch.apply_to(&mut temp_preset);
                temp_preset.use_powershell = old_use_powershell;

                // Robust Fallback: If the name wasn't renamed by AI, but the command changed, let's auto-generate a descriptive name!
                if temp_preset
                    .name
                    .trim()
                    .eq_ignore_ascii_case(old_name.trim())
                    && temp_preset.command.trim() != old_name.trim()
                {
                    let cmd_lower = temp_preset.command.to_ascii_lowercase();
                    let vi_name = |english: &'static str| {
                        crate::lang::translate(UiLanguage::Vietnamese, english)
                            .unwrap_or(english)
                            .to_owned()
                    };
                    let new_fallback_name = if cmd_lower.contains("shutdown") {
                        vi_name("Shutdown")
                    } else if cmd_lower.contains("mspaint") || cmd_lower.contains("pbrush") {
                        vi_name("Start Paint")
                    } else if cmd_lower.contains("calc") {
                        vi_name("Start Calculator")
                    } else if cmd_lower.contains("notepad") {
                        vi_name("Start Notepad")
                    } else if cmd_lower.contains("discord") {
                        vi_name("Start Discord")
                    } else if cmd_lower.contains("chrome") {
                        vi_name("Start Chrome")
                    } else if cmd_lower.contains("edge") || cmd_lower.contains("msedge") {
                        vi_name("Start Edge")
                    } else {
                        let mut parts = temp_preset.command.split_whitespace();
                        if let Some(first) = parts.next() {
                            let name_part = first
                                .trim_end_matches(".exe")
                                .trim_end_matches(".bat")
                                .trim_end_matches(".cmd")
                                .to_owned();
                            let mut chars = name_part.chars();
                            if let Some(first_char) = chars.next() {
                                let capitalized =
                                    first_char.to_uppercase().collect::<String>() + chars.as_str();
                                vi_name("Start {}").replace("{}", &capitalized)
                            } else {
                                temp_preset.name.clone()
                            }
                        } else {
                            temp_preset.name.clone()
                        }
                    };
                    temp_preset.name = new_fallback_name;
                }

                if let Some(group) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                {
                    if let Some(preset) = group
                        .presets
                        .iter_mut()
                        .find(|preset| preset.id == preset_id)
                    {
                        if let Some(step_index) = step_index {
                            if let Some(step) = preset.steps.get_mut(step_index) {
                                step.command_preset_command = temp_preset.command;
                                step.command_preset_use_powershell = temp_preset.use_powershell;
                                step.key = temp_preset.name.clone();
                            }
                        } else {
                            if preset.trigger_mode == crate::model::MacroTriggerMode::Hold {
                                preset.hold_stop_step.command_preset_command = temp_preset.command;
                                preset.hold_stop_step.command_preset_use_powershell =
                                    temp_preset.use_powershell;
                                preset.hold_stop_step.key = temp_preset.name.clone();
                            } else {
                                preset.press_stop_step.command_preset_command = temp_preset.command;
                                preset.press_stop_step.command_preset_use_powershell =
                                    temp_preset.use_powershell;
                                preset.press_stop_step.key = temp_preset.name.clone();
                            }
                        }
                        self.status = "Updated step command and preset name.".to_owned();
                    }
                }
                self.persist();
                self.state.command_presets.retain(|p| p.id != 999999);
            }
            return;
        }
        let preset_name = {
            let Some(preset) = self
                .state
                .command_presets
                .iter_mut()
                .find(|preset| preset.id == preset_id)
            else {
                self.command_ai_feedback = Some("Custom preset not found.".to_owned());
                self.status = "Custom preset not found.".to_owned();
                return;
            };
            let old_name = preset.name.clone();
            let old_use_powershell = preset.use_powershell;
            patch.apply_to(preset);
            preset.use_powershell = old_use_powershell;
            preset.collapsed = false;

            // Robust Fallback: If the name wasn't renamed by AI, but the command changed, let's auto-generate a descriptive name!
            if Self::trimmed_eq_ignore_ascii_case(&preset.name, &old_name)
                && preset.command.trim() != old_name.trim()
            {
                let cmd_lower = preset.command.to_ascii_lowercase();
                let is_vietnamese = old_name.chars().any(|c| c as u32 > 127);
                let vi_name = |english: &'static str| {
                    crate::lang::translate(UiLanguage::Vietnamese, english)
                        .unwrap_or(english)
                        .to_owned()
                };
                let new_fallback_name = if cmd_lower.contains("shutdown") {
                    if is_vietnamese {
                        vi_name("Shutdown")
                    } else {
                        "Shutdown".to_owned()
                    }
                } else if cmd_lower.contains("mspaint") || cmd_lower.contains("pbrush") {
                    if is_vietnamese {
                        vi_name("Start Paint")
                    } else {
                        "Start Paint".to_owned()
                    }
                } else if cmd_lower.contains("calc") {
                    if is_vietnamese {
                        vi_name("Start Calculator")
                    } else {
                        "Start Calculator".to_owned()
                    }
                } else if cmd_lower.contains("notepad") {
                    if is_vietnamese {
                        vi_name("Start Notepad")
                    } else {
                        "Start Notepad".to_owned()
                    }
                } else if cmd_lower.contains("discord") {
                    if is_vietnamese {
                        vi_name("Start Discord")
                    } else {
                        "Start Discord".to_owned()
                    }
                } else if cmd_lower.contains("chrome") {
                    if is_vietnamese {
                        vi_name("Start Chrome")
                    } else {
                        "Start Chrome".to_owned()
                    }
                } else if cmd_lower.contains("edge") || cmd_lower.contains("msedge") {
                    if is_vietnamese {
                        vi_name("Start Edge")
                    } else {
                        "Start Edge".to_owned()
                    }
                } else {
                    let mut parts = preset.command.split_whitespace();
                    if let Some(first) = parts.next() {
                        let name_part = first
                            .trim_end_matches(".exe")
                            .trim_end_matches(".bat")
                            .trim_end_matches(".cmd")
                            .to_owned();
                        let mut chars = name_part.chars();
                        if let Some(first_char) = chars.next() {
                            let capitalized =
                                first_char.to_uppercase().collect::<String>() + chars.as_str();
                            if is_vietnamese {
                                vi_name("Start {}").replace("{}", &capitalized)
                            } else {
                                format!("Start {}", capitalized)
                            }
                        } else {
                            preset.name.clone()
                        }
                    } else {
                        preset.name.clone()
                    }
                };
                preset.name = new_fallback_name;
            }

            let new_name = preset.name.clone();
            let new_command = preset.command.clone();
            let new_use_powershell = preset.use_powershell;

            // Synchronize all macro steps that reference this preset
            for group in &mut self.state.macro_groups {
                for p in &mut group.presets {
                    for step in &mut p.steps {
                        if step.action == MacroAction::TriggerCommandPreset {
                            let is_match = step.key.trim() == old_name.trim()
                                || step.key.trim() == preset_id.to_string()
                                || step.key.trim() == new_name.trim();
                            if is_match {
                                step.key = preset_id.to_string();
                                step.command_preset_command = new_command.clone();
                                step.command_preset_use_powershell = new_use_powershell;
                            }
                        }
                    }
                    if p.hold_stop_step.action == MacroAction::TriggerCommandPreset {
                        let is_match = p.hold_stop_step.key.trim() == old_name.trim()
                            || p.hold_stop_step.key.trim() == preset_id.to_string()
                            || p.hold_stop_step.key.trim() == new_name.trim();
                        if is_match {
                            p.hold_stop_step.key = preset_id.to_string();
                            p.hold_stop_step.command_preset_command = new_command.clone();
                            p.hold_stop_step.command_preset_use_powershell = new_use_powershell;
                        }
                    }
                    if p.press_stop_step.action == MacroAction::TriggerCommandPreset {
                        let is_match = p.press_stop_step.key.trim() == old_name.trim()
                            || p.press_stop_step.key.trim() == preset_id.to_string()
                            || p.press_stop_step.key.trim() == new_name.trim();
                        if is_match {
                            p.press_stop_step.key = preset_id.to_string();
                            p.press_stop_step.command_preset_command = new_command.clone();
                            p.press_stop_step.command_preset_use_powershell = new_use_powershell;
                        }
                    }
                }
            }
            new_name
        };
        self.persist_command_presets();
        self.status = format!("Updated custom preset {}.", preset_name);
    }

    fn poll_custom_ai_generation(&mut self, ctx: &egui::Context) {
        let Some(job) = self.command_ai_job.as_ref() else {
            return;
        };
        let job_token = job.token;
        let job_preset_id = job.preset_id;
        match job.receiver.try_recv() {
            Ok(result) => {
                self.command_ai_job = None;
                if result.token != job_token || result.preset_id != job_preset_id {
                    self.status = "AI result was ignored for a different custom preset.".to_owned();
                    ctx.request_repaint();
                    return;
                }
                match result.outcome {
                    Ok(patch) => {
                        self.apply_custom_ai_generated_patch(result.preset_id, patch);
                        if result.preset_id == 999999 {
                            self.command_ai_dialog = None;
                        }
                        self.command_ai_feedback =
                            Some("Custom preset updated successfully.".to_owned());
                        self.status = "Custom preset updated successfully.".to_owned();
                    }
                    Err(error) => {
                        let message = Self::ai_generation_feedback(&error);
                        self.command_ai_feedback = Some(message.clone());
                        self.status = message;
                        if result.preset_id == 999999 && self.command_ai_dialog.is_none() {
                            self.cleanup_custom_ai_dialog_state();
                        }
                    }
                }
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.command_ai_job = None;
                self.command_ai_feedback = Some("AI generation stopped unexpectedly.".to_owned());
                self.status = "AI generation stopped unexpectedly.".to_owned();
                if job_preset_id == 999999 && self.command_ai_dialog.is_none() {
                    self.cleanup_custom_ai_dialog_state();
                }
                ctx.request_repaint();
            }
        }
    }

    fn upsert_custom_preset_from_step_draft_values(
        &mut self,
        name: String,
        command: String,
        use_powershell: bool,
    ) -> Option<u32> {
        let command = ai::normalize_command_text(&command);
        if name.is_empty() || command.is_empty() {
            return None;
        }

        if let Some(existing_index) = self
            .state
            .command_presets
            .iter()
            .position(|preset| Self::trimmed_eq_ignore_ascii_case(&preset.name, &name))
        {
            let preset = &mut self.state.command_presets[existing_index];
            preset.name = name.clone();
            preset.command = command;
            preset.use_powershell = use_powershell;
            preset.collapsed = true;
            return Some(preset.id);
        }

        let id = Self::allocate_next_id(
            &self.state.command_presets,
            &mut self.state.next_command_preset_id,
            |preset| preset.id,
        );
        let mut preset = CommandPreset::new(id);
        preset.name = name;
        preset.command = command;
        preset.use_powershell = use_powershell;
        preset.collapsed = true;
        self.state.command_presets.push(preset);
        Some(id)
    }

    fn add_macro_folder(&mut self) {
        let id = Self::allocate_next_id(
            &self.state.macro_folders,
            &mut self.state.next_macro_folder_id,
            |folder| folder.id,
        );
        self.state.macro_folders.push(MacroFolder::new(id));
        self.status = format!("Added macro folder {id}.");
    }

    fn add_macro_group_to_folder(&mut self, folder_id: u32) {
        let id = Self::allocate_next_id(
            &self.state.macro_groups,
            &mut self.state.next_macro_group_id,
            |group| group.id,
        );
        let mut group = MacroGroup::new(id);
        group.name = self.unique_macro_group_name(&group.name);
        group.folder_id = Some(folder_id);
        let preset_id = self.allocate_next_macro_preset_id();
        group.presets = vec![MacroPreset::new(preset_id)];
        self.state.macro_groups.push(group);
        self.pending_macro_group_scroll_target = Some(id);
        self.sync_reconciled_macro_presets();
        self.status = format!("Added macro group {id} to folder.");
    }

    fn clone_macro_preset_with_new_id(&mut self, source: &MacroPreset) -> MacroPreset {
        let new_preset_id = self.allocate_next_macro_preset_id();
        let mut preset = source.clone();
        let old_preset_id = preset.id;
        preset.id = new_preset_id;
        preset.collapsed = true;
        Self::remap_macro_step_self_ref(&mut preset.hold_stop_step, old_preset_id, new_preset_id);
        Self::remap_macro_step_self_ref(&mut preset.press_stop_step, old_preset_id, new_preset_id);
        for step in &mut preset.steps {
            Self::remap_macro_step_self_ref(step, old_preset_id, new_preset_id);
        }
        preset
    }

    fn remap_macro_step_self_ref(step: &mut MacroStep, old_preset_id: u32, new_preset_id: u32) {
        if matches!(
            step.action,
            MacroAction::TriggerMacroPreset
                | MacroAction::TriggerMacroPresetIfEnabled
                | MacroAction::StopMacroPreset
                | MacroAction::EnableMacroPreset
                | MacroAction::DisableMacroPreset
        ) {
            let remapped = step
                .key
                .split(',')
                .filter_map(|part| part.trim().parse::<u32>().ok())
                .map(|id| {
                    if id == old_preset_id {
                        new_preset_id
                    } else {
                        id
                    }
                })
                .collect::<Vec<_>>();
            if !remapped.is_empty() {
                step.key = remapped
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
            }
        }
    }

    fn clone_macro_group_with_new_ids(
        &mut self,
        source_group: &MacroGroup,
        target_folder_id: Option<u32>,
    ) -> MacroGroup {
        let new_group_id = Self::allocate_next_id(
            &self.state.macro_groups,
            &mut self.state.next_macro_group_id,
            |group| group.id,
        );

        let mut copied_group = source_group.clone();
        copied_group.id = new_group_id;
        copied_group.name = format!("{} Copy", copied_group.name);
        copied_group.name = self.unique_macro_group_name(&copied_group.name);
        copied_group.folder_id = target_folder_id;

        let mut preset_id_map = HashMap::new();
        for preset in &mut copied_group.presets {
            let old_id = preset.id;
            let new_preset_id = self.allocate_next_macro_preset_id();
            preset.id = new_preset_id;
            preset.collapsed = true;
            preset_id_map.insert(old_id, new_preset_id);
        }

        for preset in &mut copied_group.presets {
            Self::remap_macro_step_group_refs(
                &mut preset.hold_stop_step,
                &preset_id_map,
                source_group.id,
                new_group_id,
            );
            Self::remap_macro_step_group_refs(
                &mut preset.press_stop_step,
                &preset_id_map,
                source_group.id,
                new_group_id,
            );
            for step in &mut preset.steps {
                Self::remap_macro_step_group_refs(
                    step,
                    &preset_id_map,
                    source_group.id,
                    new_group_id,
                );
            }
        }

        copied_group
    }

    fn unique_macro_group_name(&self, base_name: &str) -> String {
        let base = base_name.trim();
        let base = if base.is_empty() { "Macro Group" } else { base };
        let lower_base = base.to_ascii_lowercase();
        let names = self
            .state
            .macro_groups
            .iter()
            .map(|group| group.name.trim().to_ascii_lowercase())
            .collect::<HashSet<_>>();
        if !names.contains(&lower_base) {
            return base.to_owned();
        }
        let mut suffix = 2u32;
        loop {
            let candidate = format!("{base} {suffix}");
            if !names.contains(&candidate.to_ascii_lowercase()) {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn remap_macro_step_group_refs(
        step: &mut MacroStep,
        preset_id_map: &HashMap<u32, u32>,
        old_group_id: u32,
        new_group_id: u32,
    ) {
        if matches!(
            step.action,
            MacroAction::TriggerMacroPreset
                | MacroAction::TriggerMacroPresetIfEnabled
                | MacroAction::StopMacroPreset
                | MacroAction::EnableMacroPreset
                | MacroAction::DisableMacroPreset
        ) {
            let remapped = step
                .key
                .split(',')
                .filter_map(|part| part.trim().parse::<u32>().ok())
                .map(|id| preset_id_map.get(&id).copied().unwrap_or(id))
                .collect::<Vec<_>>();
            if !remapped.is_empty() {
                step.key = remapped
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
            }
        }
        if matches!(
            step.action,
            MacroAction::TriggerMacroPreset
                | MacroAction::TriggerMacroPresetIfEnabled
                | MacroAction::StopMacroPreset
        ) && step.trigger_macro_group_id == Some(old_group_id)
        {
            step.trigger_macro_group_id = Some(new_group_id);
        }
    }

    fn bind_trigger_macro_step_to_group(step: &mut MacroStep, group_id: u32) {
        if matches!(
            step.action,
            MacroAction::TriggerMacroPreset
                | MacroAction::TriggerMacroPresetIfEnabled
                | MacroAction::StopMacroPreset
        ) {
            step.trigger_macro_group_id = Some(group_id);
        }
    }

    fn set_active_macro_folder_view(&mut self, folder_id: Option<u32>) {
        self.active_macro_folder_view = folder_id;
        self.selected_macro_groups.clear();
        self.sync_active_macro_folder_scope();
    }

    fn open_macro_folder_mode(&mut self) {
        self.macro_folders_panel_open = true;
        self.set_active_macro_folder_view(None);
    }

    fn close_macro_folder_mode(&mut self) {
        self.macro_folders_panel_open = false;
        self.set_active_macro_folder_view(None);
    }

    fn normalize_macro_folder_view_state(&mut self) {
        if !self.macro_folders_panel_open {
            if self.active_macro_folder_view.is_some() {
                self.set_active_macro_folder_view(None);
            } else {
                self.sync_active_macro_folder_scope();
            }
            return;
        }
        if self.active_macro_folder_view.is_some()
            && self.resolved_active_macro_folder_view().is_none()
        {
            self.set_active_macro_folder_view(None);
        }
    }

    fn copy_selected_macro_groups(&mut self) {
        let mut ids = self
            .selected_macro_groups
            .iter()
            .copied()
            .collect::<Vec<_>>();
        ids.sort_unstable();
        self.macro_group_clipboard = ids;
        self.macro_group_clipboard_is_cut = false;
        self.macro_group_clipboard_feedback = Some(MacroGroupClipboardFeedback::Copy);
        self.macro_group_clipboard_feedback_until = Some(Instant::now() + Duration::from_secs(1));
        self.status = format!(
            "Copied {} macro group(s).",
            self.macro_group_clipboard.len()
        );
    }

    fn copy_macro_group_to_clipboard(&mut self, group_id: u32) {
        if self
            .state
            .macro_groups
            .iter()
            .any(|group| group.id == group_id)
        {
            self.macro_group_clipboard = vec![group_id];
            self.macro_group_clipboard_is_cut = false;
            self.macro_group_clipboard_feedback = Some(MacroGroupClipboardFeedback::Copy);
            self.macro_group_clipboard_feedback_until =
                Some(Instant::now() + Duration::from_secs(1));
            self.status = "Copied 1 macro group.".to_owned();
        } else {
            self.status = "Macro group was not found.".to_owned();
        }
    }

    fn paste_macro_groups_after(&mut self, anchor_group_id: u32) {
        if self.macro_group_clipboard.is_empty() {
            self.status = "No macro groups in clipboard.".to_owned();
            return;
        }

        let Some(anchor_index) = self
            .state
            .macro_groups
            .iter()
            .position(|group| group.id == anchor_group_id)
        else {
            self.status = "Target macro group was not found.".to_owned();
            return;
        };
        let target_folder_id = self.state.macro_groups[anchor_index].folder_id;
        let clipboard_ids = self.macro_group_clipboard.clone();

        if self.macro_group_clipboard_is_cut {
            if clipboard_ids.contains(&anchor_group_id) {
                self.status = "Cannot paste a cut macro group after itself.".to_owned();
                return;
            }

            let clipboard_id_set = clipboard_ids.iter().copied().collect::<HashSet<_>>();
            let mut moved_groups = Vec::new();
            self.state.macro_groups.retain(|group| {
                if clipboard_id_set.contains(&group.id) {
                    moved_groups.push(group.clone());
                    false
                } else {
                    true
                }
            });
            for group in &mut moved_groups {
                group.folder_id = target_folder_id;
            }

            let insert_index = self
                .state
                .macro_groups
                .iter()
                .position(|group| group.id == anchor_group_id)
                .map(|index| index + 1)
                .unwrap_or(self.state.macro_groups.len());
            self.state
                .macro_groups
                .splice(insert_index..insert_index, moved_groups);

            self.macro_group_clipboard.clear();
            self.macro_group_clipboard_is_cut = false;
            self.macro_group_clipboard_feedback = Some(MacroGroupClipboardFeedback::Paste);
            self.macro_group_clipboard_feedback_until =
                Some(Instant::now() + Duration::from_secs(1));
            self.status = "Moved macro group selection.".to_owned();
        } else {
            let sources = clipboard_ids
                .iter()
                .filter_map(|group_id| {
                    self.state
                        .macro_groups
                        .iter()
                        .find(|group| group.id == *group_id)
                        .cloned()
                })
                .collect::<Vec<_>>();
            let mut insert_index = anchor_index + 1;
            for source in &sources {
                let copied_group = self.clone_macro_group_with_new_ids(source, target_folder_id);
                self.state.macro_groups.insert(insert_index, copied_group);
                insert_index += 1;
            }
            self.macro_group_clipboard_feedback = Some(MacroGroupClipboardFeedback::Paste);
            self.macro_group_clipboard_feedback_until =
                Some(Instant::now() + Duration::from_secs(1));
            self.status = format!("Pasted {} macro group copy(s).", sources.len());
        }

        self.persist_reconciled_macro_presets();
    }

    fn open_groq_api_settings(&mut self) {
        self.settings_popup_open = true;
        self.state.groq_settings.details_open = true;
        self.focus_groq_api_key_pending = true;
    }

    fn set_macro_step_inline_feedback(
        &mut self,
        preset_id: u32,
        step_index: usize,
        message: impl Into<String>,
        open_groq_settings: bool,
    ) {
        let message = message.into();
        if message.trim().is_empty() {
            self.macro_step_inline_feedback
                .remove(&(preset_id, step_index));
            return;
        }
        self.macro_step_inline_feedback.insert(
            (preset_id, step_index),
            MacroStepInlineFeedback {
                message,
                open_groq_settings,
            },
        );
    }

    fn copy_selected_macro_steps_for_preset(&mut self, group_id: u32, preset_id: u32) {
        let mut selected_indices = self
            .selected_macro_steps
            .iter()
            .filter_map(|(selected_group, selected_preset, selected_index)| {
                (*selected_group == group_id && *selected_preset == preset_id)
                    .then_some(*selected_index)
            })
            .collect::<Vec<_>>();
        selected_indices.sort_unstable();
        selected_indices.dedup();

        let mut clipboard = Vec::new();
        let (group_index, preset_index) = match self.macro_preset_indices(group_id, preset_id) {
            Ok(indices) => indices,
            Err(message) => {
                self.status = message.to_owned();
                return;
            }
        };
        let preset = &self.state.macro_groups[group_index].presets[preset_index];
        for &index in &selected_indices {
            if let Some(step) = preset.steps.get(index) {
                clipboard.push(step.clone());
            }
        }

        self.macro_step_clipboard = clipboard;
        if self.macro_step_clipboard.is_empty() {
            self.status = "No selected steps to copy.".to_owned();
        } else {
            self.macro_selected_steps_copy_feedback_target = Some((group_id, preset_id));
            self.macro_selected_steps_copy_feedback_until =
                Some(Instant::now() + Duration::from_secs(1));
            self.status = format!("Copied {} step(s).", self.macro_step_clipboard.len());
        }
    }

    fn remove_selected_macro_steps_for_preset(&mut self, group_id: u32, preset_id: u32) {
        let mut selected_indices = self
            .selected_macro_steps
            .iter()
            .filter_map(|(selected_group, selected_preset, selected_index)| {
                (*selected_group == group_id && *selected_preset == preset_id)
                    .then_some(*selected_index)
            })
            .collect::<Vec<_>>();
        selected_indices.sort_unstable();
        selected_indices.dedup();
        selected_indices.reverse();

        if let Ok((group_index, preset_index)) = self.macro_preset_indices(group_id, preset_id) {
            let preset = &mut self.state.macro_groups[group_index].presets[preset_index];
            for index in selected_indices {
                if index < preset.steps.len() {
                    preset.steps.remove(index);
                }
            }
        }
        self.selected_macro_steps
            .retain(|(g_id, p_id, _)| *g_id != group_id || *p_id != preset_id);
    }

    fn paste_macro_steps_after(
        &mut self,
        group_id: u32,
        preset_id: u32,
        step_index: usize,
    ) -> Option<Vec<usize>> {
        if self.macro_step_clipboard.is_empty() {
            self.status = "No steps in clipboard.".to_owned();
            return None;
        }

        let clipboard_steps = self.macro_step_clipboard.clone();
        let pasted_count = clipboard_steps.len();
        let mut final_insert_at = 0;

        let (group_index, preset_index) = match self.macro_preset_indices(group_id, preset_id) {
            Ok(indices) => indices,
            Err(message) => {
                self.status = message.to_owned();
                return None;
            }
        };
        let preset = &mut self.state.macro_groups[group_index].presets[preset_index];
        let insert_at = (step_index + 1).min(preset.steps.len());
        final_insert_at = insert_at;
        for (offset, step) in clipboard_steps.into_iter().enumerate() {
            preset.steps.insert(insert_at + offset, step);
        }

        self.status = format!("Pasted {} step(s).", pasted_count);
        Some((final_insert_at..final_insert_at + pasted_count).collect::<Vec<_>>())
    }

    fn paste_macro_steps_at_start(&mut self, group_id: u32, preset_id: u32) -> Option<Vec<usize>> {
        if self.macro_step_clipboard.is_empty() {
            self.status = "No steps in clipboard.".to_owned();
            return None;
        }

        let clipboard_steps = self.macro_step_clipboard.clone();
        let pasted_count = clipboard_steps.len();

        let (group_index, preset_index) = match self.macro_preset_indices(group_id, preset_id) {
            Ok(indices) => indices,
            Err(message) => {
                self.status = message.to_owned();
                return None;
            }
        };
        let preset = &mut self.state.macro_groups[group_index].presets[preset_index];

        for (offset, step) in clipboard_steps.into_iter().enumerate() {
            preset.steps.insert(offset, step);
        }

        self.status = format!("Pasted {} step(s).", pasted_count);
        Some((0..pasted_count).collect::<Vec<_>>())
    }

    fn cut_selected_macro_groups(&mut self) {
        let mut ids = self
            .selected_macro_groups
            .iter()
            .copied()
            .collect::<Vec<_>>();
        ids.sort_unstable();
        self.macro_group_clipboard = ids;
        self.macro_group_clipboard_is_cut = true;
        self.macro_group_clipboard_feedback = Some(MacroGroupClipboardFeedback::Cut);
        self.macro_group_clipboard_feedback_until = Some(Instant::now() + Duration::from_secs(1));
        self.status = format!("Cut {} macro group(s).", self.macro_group_clipboard.len());
    }

    fn paste_macro_groups_into_folder(&mut self, target_folder_id: Option<u32>) {
        if self.macro_group_clipboard.is_empty() {
            self.status = "No macro groups in clipboard.".to_owned();
            return;
        }

        let clipboard_ids = self.macro_group_clipboard.clone();
        if self.macro_group_clipboard_is_cut {
            for group_id in clipboard_ids {
                if let Some(group) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                {
                    group.folder_id = target_folder_id;
                }
            }
            self.macro_group_clipboard.clear();
            self.macro_group_clipboard_is_cut = false;
            self.macro_group_clipboard_feedback = Some(MacroGroupClipboardFeedback::Paste);
            self.macro_group_clipboard_feedback_until =
                Some(Instant::now() + Duration::from_secs(1));
            self.status = "Moved macro group selection.".to_owned();
        } else {
            let sources = clipboard_ids
                .iter()
                .filter_map(|group_id| {
                    self.state
                        .macro_groups
                        .iter()
                        .find(|group| group.id == *group_id)
                        .cloned()
                })
                .collect::<Vec<_>>();
            for source in &sources {
                let copied_group = self.clone_macro_group_with_new_ids(source, target_folder_id);
                self.state.macro_groups.push(copied_group);
            }
            self.macro_group_clipboard_feedback = Some(MacroGroupClipboardFeedback::Paste);
            self.macro_group_clipboard_feedback_until =
                Some(Instant::now() + Duration::from_secs(1));
            self.status = format!("Pasted {} macro group copy(s).", sources.len());
        }

        self.persist_reconciled_macro_presets();
    }

    fn remove_selected_macro_groups(&mut self) {
        if self.selected_macro_groups.is_empty() {
            self.status = "No macro groups selected.".to_owned();
            return;
        }
        let selected = self.selected_macro_groups.clone();
        self.state
            .macro_groups
            .retain(|group| !selected.contains(&group.id));
        self.selected_macro_groups.clear();
        self.macro_group_clipboard
            .retain(|group_id| !selected.contains(group_id));
        self.persist_reconciled_macro_presets();
        self.status = "Removed selected macro groups.".to_owned();
    }

    fn begin_capture(&mut self, target: CaptureRequest, status: String) {
        let waits_for_mouse_release = self.capture_request_accepts_mouse(&target);
        self.capture_target = Some(target.clone());
        self.capture_ignored_keys = self.snapshot_pressed_capture_keys();
        self.capture_ignored_keys
            .extend([0x01, 0x02, 0x04, 0x05, 0x06]);
        self.capture_hotkey_combo_keys = None;
        self.capture_hotkey_combo_vks.clear();
        self.capture_suppress_next_poll = false;
        self.capture_wait_for_mouse_release = waits_for_mouse_release;
        self.capture_ignore_mouse_until_release = waits_for_mouse_release;
        self.capture_suppress_polls_remaining = 0;
        self.capture_mouse_guard_until = None;
        self.status = if self.capture_request_keeps_open(&target) {
            crate::lang::translate(
                self.state.ui_language,
                "Capturing triggers. Hold keys, then release to save. Click Capture again to cancel.",
            )
            .unwrap_or("Capturing triggers. Hold keys, then release to save. Click Capture again to cancel.")
            .to_owned()
        } else {
            status
        };
    }

    fn capture_request_keeps_open(&self, _target: &CaptureRequest) -> bool {
        false
    }

    pub(crate) fn clear_geometry_spec_preview(&mut self) {
        self.geometry_preview_target = None;
        self.geometry_preview_sent = None;
        let _ = self
            .overlay_tx
            .send(crate::overlay::OverlayCommand::PreviewGeometrySpec(None));
    }

    pub(crate) fn sync_geometry_spec_preview(&mut self, spec: Option<GeometrySpec>) {
        self.geometry_preview_sent = spec.clone();
        let _ = self
            .overlay_tx
            .send(crate::overlay::OverlayCommand::PreviewGeometrySpec(spec));
    }

    pub(crate) fn clear_geometry_preset_preview(&mut self) {
        self.geometry_preset_preview_target = None;
        let _ = self
            .overlay_tx
            .send(crate::overlay::OverlayCommand::PreviewGeometryPreset(None));
    }

    pub(crate) fn sync_geometry_preset_preview(&mut self, preset_id: Option<u32>) {
        self.geometry_preset_preview_target = preset_id;
        let _ = self
            .overlay_tx
            .send(crate::overlay::OverlayCommand::PreviewGeometryPreset(
                preset_id,
            ));
    }

    fn capture_request_accepts_mouse(&self, target: &CaptureRequest) -> bool {
        match target {
            CaptureRequest::MacroStepInput {
                group_id,
                preset_id,
                step_index,
                extra_cond_index,
            } => self.capture_macro_step_input_accepts_mouse(
                *group_id,
                *preset_id,
                *step_index,
                *extra_cond_index,
            ),
            _ => matches!(
                target,
                CaptureRequest::MacroPresetHotkey(_, _)
                    | CaptureRequest::MacroPresetRecordHotkey(_, _)
                    | CaptureRequest::MacroPresetReleaseWaitKey(_, _)
                    | CaptureRequest::MacroPresetHoldStopInput(_, _)
                    | CaptureRequest::CommandPresetHotkey(_)
                    | CaptureRequest::WindowPresetHotkey(_)
                    | CaptureRequest::WindowFocusPresetHotkey(_)
                    | CaptureRequest::WindowLayoutHotkey(_)
                    | CaptureRequest::WindowPresetAnimateHotkey(_)
                    | CaptureRequest::WindowPresetTitlebarHotkey(_)
                    | CaptureRequest::WindowExpandHotkey(_)
                    | CaptureRequest::PinPresetHotkey(_)
                    | CaptureRequest::MouseSensitivityPresetHotkey(_)
                    | CaptureRequest::ZoomPresetHotkey(_)
                    | CaptureRequest::VisionPresetHotkey(_)
                    | CaptureRequest::QuickScreenDrawHotkey
                    | CaptureRequest::QuickVideoRecordHotkey
                    | CaptureRequest::MacrosMasterHotkey
            ),
        }
    }

    fn capture_macro_step_input_accepts_mouse(
        &self,
        group_id: u32,
        preset_id: u32,
        step_index: usize,
        extra_cond_index: Option<usize>,
    ) -> bool {
        let Some(step) = self
            .state
            .macro_groups
            .iter()
            .find(|group| group.id == group_id)
            .and_then(|group| group.presets.iter().find(|preset| preset.id == preset_id))
            .and_then(|preset| preset.steps.get(step_index))
        else {
            return true;
        };

        if let Some(extra_idx) = extra_cond_index {
            return step.extra_conditions.get(extra_idx).is_some_and(|cond| {
                cond.condition_type == crate::model::IfConditionType::MouseHeld
            });
        }

        !matches!(
            step.action,
            MacroAction::KeyPress | MacroAction::KeyDown | MacroAction::KeyUp
        )
    }

    fn capture_request_registers_on_press(&self, target: &CaptureRequest) -> bool {
        !matches!(
            target,
            CaptureRequest::QuickScreenDrawHotkey | CaptureRequest::QuickVideoRecordHotkey
        )
    }

    fn split_key_list(value: &str) -> Vec<String> {
        value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn join_key_list(keys: &[String]) -> String {
        keys.join(",")
    }

    fn append_key_list_value(list: &mut String, key: &str) -> bool {
        let key = key.trim();
        if key.is_empty() {
            return false;
        }
        let existing = Self::split_key_list(list);
        if existing.iter().any(|part| part.eq_ignore_ascii_case(key)) {
            return false;
        }
        let mut updated = existing;
        updated.push(key.to_owned());
        *list = Self::join_key_list(&updated);
        true
    }

    fn remove_key_list_value(list: &mut String, key: &str) -> bool {
        let key = key.trim();
        if key.is_empty() {
            return false;
        }
        let existing = Self::split_key_list(list);
        let original_len = existing.len();
        let remaining: Vec<String> = existing
            .into_iter()
            .filter(|part| !part.eq_ignore_ascii_case(key))
            .collect();
        if remaining.len() == original_len {
            return false;
        }
        *list = Self::join_key_list(&remaining);
        true
    }

    fn cancel_capture(&mut self) {
        self.capture_target = None;
        self.capture_hotkey_combo_keys = None;
        self.capture_hotkey_combo_vks.clear();
        self.capture_suppress_next_poll = false;
        self.capture_wait_for_mouse_release = true;
        self.capture_ignore_mouse_until_release = true;
        self.capture_suppress_polls_remaining = 0;
        self.capture_mouse_guard_until = None;
        self.status = "Capture cancelled.".to_owned();
    }

    fn pick_point_button_text(language: UiLanguage, active: bool) -> RichText {
        if active {
            RichText::new(Self::tr_lang(language, "Picking...", "Picking..."))
                .strong()
                .color(Color32::from_rgb(255, 232, 96))
        } else {
            RichText::new(Self::tr_lang(language, "Pick", "Pick"))
        }
    }

    fn apply_captured_input(&mut self, target: CaptureRequest, captured: CapturedInput) -> bool {
        let target_clone = target.clone();
        let keep_capture_open = self.capture_request_keeps_open(&target);
        match (target, captured) {
            (CaptureRequest::WindowPresetHotkey(preset_id), CapturedInput::Binding(binding)) => {
                if let Some(preset) = self
                    .state
                    .window_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    let changed = Self::preset_trigger_add_binding(
                        &mut preset.hotkey,
                        &mut preset.trigger_keys,
                        binding,
                    );
                    self.status = if changed {
                        format!("Captured hotkey for {}.", preset.name)
                    } else {
                        format!("Hotkey already exists for {}.", preset.name)
                    };
                    preset.enabled =
                        preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
                }
                self.sync_window_presets();
            }
            (
                CaptureRequest::WindowFocusPresetHotkey(preset_id),
                CapturedInput::Binding(binding),
            ) => {
                if let Some(preset) = self
                    .state
                    .window_focus_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    let changed = Self::preset_trigger_add_binding(
                        &mut preset.hotkey,
                        &mut preset.trigger_keys,
                        binding,
                    );
                    self.status = if changed {
                        format!("Captured focus hotkey for {}.", preset.name)
                    } else {
                        format!("Focus hotkey already exists for {}.", preset.name)
                    };
                    preset.enabled =
                        preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
                }
                self.sync_window_presets();
            }
            (CaptureRequest::WindowLayoutHotkey(layout_id), CapturedInput::Binding(binding)) => {
                if let Some(layout) = self
                    .state
                    .window_layouts
                    .iter_mut()
                    .find(|l| l.id == layout_id)
                {
                    let changed = Self::preset_trigger_add_binding(
                        &mut layout.hotkey,
                        &mut layout.trigger_keys,
                        binding,
                    );
                    self.status = if changed {
                        format!("Captured layout hotkey for {}.", layout.name)
                    } else {
                        format!("Layout hotkey already exists for {}.", layout.name)
                    };
                    layout.enabled =
                        layout.hotkey.is_some() || !layout.trigger_keys.trim().is_empty();
                }
                self.sync_window_layouts();
            }
            (
                CaptureRequest::WindowPresetAnimateHotkey(preset_id),
                CapturedInput::Binding(binding),
            ) => {
                if let Some(preset) = self
                    .state
                    .window_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.animate_hotkey = Some(binding);
                    self.status = format!("Captured animated hotkey for {}.", preset.name);
                }
                self.sync_window_presets();
            }
            (
                CaptureRequest::WindowPresetTitlebarHotkey(preset_id),
                CapturedInput::Binding(binding),
            ) => {
                if let Some(preset) = self
                    .state
                    .window_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.titlebar_hotkey = Some(binding);
                    self.status = format!("Captured restore title bar hotkey for {}.", preset.name);
                }
                self.sync_window_presets();
            }
            (CaptureRequest::WindowExpandHotkey(direction), CapturedInput::Binding(binding)) => {
                let controls = &mut self.state.window_expand_controls;
                match direction {
                    WindowExpandDirection::Up => controls.up = Some(binding),
                    WindowExpandDirection::Down => controls.down = Some(binding),
                    WindowExpandDirection::Left => controls.left = Some(binding),
                    WindowExpandDirection::Right => controls.right = Some(binding),
                }
                self.sync_window_presets();
                self.status = "Captured window expand hotkey.".to_owned();
            }
            (CaptureRequest::ZoomPresetHotkey(preset_id), CapturedInput::Binding(binding)) => {
                if let Some(preset) = self
                    .state
                    .zoom_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.hotkey = Some(binding);
                    self.status = format!("Captured zoom hotkey for {}.", preset.name);
                }
                self.sync_window_presets();
            }
            (CaptureRequest::VisionPresetHotkey(preset_id), CapturedInput::Binding(binding)) => {
                if let Some(preset) = self
                    .state
                    .vision_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    let changed = Self::preset_trigger_add_binding(
                        &mut preset.hotkey,
                        &mut preset.trigger_keys,
                        binding,
                    );
                    self.status = if changed {
                        format!("Captured image search hotkey for {}.", preset.name)
                    } else {
                        format!("Image search hotkey already exists for {}.", preset.name)
                    };
                    preset.enabled =
                        preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
                }
                self.persist_vision_presets();
            }
            (CaptureRequest::MacrosMasterHotkey, CapturedInput::Binding(binding)) => {
                self.state.macros_master_hotkey = Some(binding);
                self.sync_macro_master_hotkey();
                self.persist();
                self.status = crate::lang::translate(
                    self.state.ui_language,
                    "Captured the macro master hotkey.",
                )
                .unwrap_or("Captured the macro master hotkey.")
                .to_owned();
            }
            (CaptureRequest::QuickScreenDrawHotkey, CapturedInput::Binding(binding)) => {
                self.state.quick_screen_draw_hotkey = Some(binding);
                self.sync_quick_screen_draw_config();
                self.persist();
                self.status = "Captured screen draw toggle key.".to_owned();
            }
            (CaptureRequest::QuickVideoRecordHotkey, CapturedInput::Binding(binding)) => {
                self.state.quick_video_record_hotkey = Some(binding);
                self.sync_quick_video_record_config();
                self.persist();
                self.status = "Captured video recording toggle key.".to_owned();
            }
            (CaptureRequest::PinPresetHotkey(preset_id), CapturedInput::Binding(binding)) => {
                if let Some(preset) = self
                    .state
                    .pin_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    let changed = Self::preset_trigger_add_binding(
                        &mut preset.hotkey,
                        &mut preset.trigger_keys,
                        binding,
                    );
                    self.status = if changed {
                        format!("Captured pin hotkey for {}.", preset.name)
                    } else {
                        format!("Pin hotkey already exists for {}.", preset.name)
                    };
                    preset.enabled =
                        preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
                }
                self.sync_window_presets();
            }
            (CaptureRequest::MousePathRecordHotkey(preset_id), CapturedInput::Binding(binding)) => {
                if let Some(preset) = self
                    .state
                    .mouse_path_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.record_hotkey = Some(binding);
                    self.status = format!("Captured record hotkey for {}.", preset.name);
                }
                self.sync_window_presets();
            }
            (
                CaptureRequest::MouseSensitivityPresetHotkey(preset_id),
                CapturedInput::Binding(binding),
            ) => {
                if let Some(preset) = self
                    .state
                    .mouse_sensitivity_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    let changed = Self::preset_trigger_add_binding(
                        &mut preset.hotkey,
                        &mut preset.trigger_keys,
                        binding,
                    );
                    self.status = if changed {
                        format!("Captured mouse sensitivity hotkey for {}.", preset.name)
                    } else {
                        format!(
                            "Mouse sensitivity hotkey already exists for {}.",
                            preset.name
                        )
                    };
                    preset.enabled =
                        preset.hotkey.is_some() || !preset.trigger_keys.trim().is_empty();
                }
                self.persist_mouse_sensitivity_presets();
            }
            (
                CaptureRequest::MacroPresetHotkey(group_id, preset_id),
                CapturedInput::Binding(binding),
            ) => {
                if let Some(preset) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .and_then(|group| {
                        group
                            .presets
                            .iter_mut()
                            .find(|preset| preset.id == preset_id)
                    })
                {
                    let changed = Self::macro_trigger_add_binding(preset, binding);
                    self.status = if changed {
                        format!("Captured trigger binding for macro {preset_id}.")
                    } else {
                        format!("Trigger binding already exists for macro {preset_id}.")
                    };
                }
                self.sync_macro_presets();
            }
            (
                CaptureRequest::MacroPresetRecordHotkey(group_id, preset_id),
                CapturedInput::Binding(binding),
            ) => {
                if let Some(preset) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .and_then(|group| {
                        group
                            .presets
                            .iter_mut()
                            .find(|preset| preset.id == preset_id)
                    })
                {
                    preset.record_hotkey = Some(binding);
                    self.status =
                        format!("Captured record trigger key for macro preset {preset_id}.");
                }
                self.sync_macro_presets();
                self.persist_macro_presets();
            }
            (CaptureRequest::CommandPresetHotkey(preset_id), CapturedInput::Binding(binding)) => {
                if let Some(preset) = self
                    .state
                    .command_presets
                    .iter_mut()
                    .find(|preset| preset.id == preset_id)
                {
                    preset.hotkey = Some(binding);
                    self.status = format!("Captured hotkey for {}.", preset.name);
                }
                self.persist_command_presets();
            }
            (
                CaptureRequest::MacroPresetReleaseWaitKey(group_id, preset_id),
                CapturedInput::Binding(binding),
            ) => {
                if let Some(preset) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .and_then(|group| {
                        group
                            .presets
                            .iter_mut()
                            .find(|preset| preset.id == preset_id)
                    })
                {
                    let key = binding.key.trim().to_owned();
                    let existing = preset
                        .release_wait_key
                        .split(',')
                        .map(str::trim)
                        .filter(|part| !part.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    if existing.iter().any(|part| part.eq_ignore_ascii_case(&key)) {
                        self.status = format!("Key {key} is already in that release wait list.");
                    } else if existing.is_empty() {
                        preset.release_wait_key = key.clone();
                        self.status = format!("Captured release wait key for macro {preset_id}.");
                    } else {
                        preset.release_wait_key =
                            format!("{},{}", preset.release_wait_key.trim(), key);
                        self.status =
                            format!("Added release wait key {key} for macro {preset_id}.");
                    }
                }
                self.sync_macro_presets();
            }
            (
                CaptureRequest::MacroPresetHoldStopInput(group_id, preset_id),
                CapturedInput::Binding(binding),
            ) => {
                if let Some(preset) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .and_then(|group| {
                        group
                            .presets
                            .iter_mut()
                            .find(|preset| preset.id == preset_id)
                    })
                {
                    if matches!(
                        preset.hold_stop_step.action,
                        MacroAction::LockKeys | MacroAction::UnlockKeys
                    ) || (preset.hold_stop_step.action == MacroAction::StopIfKeyPressed
                        && preset.hold_stop_step.get_break_loop_mode() == "StopKey")
                    {
                        let key = binding.key;
                        let existing = preset
                            .hold_stop_step
                            .key
                            .split(',')
                            .map(str::trim)
                            .filter(|part| !part.is_empty())
                            .map(str::to_owned)
                            .collect::<Vec<_>>();
                        let label = if preset.hold_stop_step.action == MacroAction::StopIfKeyPressed
                        {
                            "hold-stop stop key"
                        } else {
                            "hold-stop lock key"
                        };
                        if existing.iter().any(|part| part.eq_ignore_ascii_case(&key)) {
                            self.status = format!("Key {key} is already in that {label} list.");
                        } else if existing.is_empty() {
                            preset.hold_stop_step.key = key.clone();
                            self.status = format!("Captured {label} {key} for macro {preset_id}.");
                        } else {
                            preset.hold_stop_step.key =
                                format!("{},{}", preset.hold_stop_step.key.trim(), key);
                            self.status = format!("Added {label} {key} for macro {preset_id}.");
                        }
                    } else {
                        preset.hold_stop_step.key = binding.key.clone();
                        self.status = format!(
                            "Captured hold-stop input {} for macro {preset_id}.",
                            binding.key
                        );
                    }
                }
                self.sync_macro_presets();
            }
            (
                CaptureRequest::MacroPresetPressStopInput(group_id, preset_id),
                CapturedInput::Binding(binding),
            ) => {
                if let Some(preset) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .and_then(|group| {
                        group
                            .presets
                            .iter_mut()
                            .find(|preset| preset.id == preset_id)
                    })
                {
                    if matches!(
                        preset.press_stop_step.action,
                        MacroAction::LockKeys | MacroAction::UnlockKeys
                    ) || (preset.press_stop_step.action == MacroAction::StopIfKeyPressed
                        && preset.press_stop_step.get_break_loop_mode() == "StopKey")
                    {
                        let key = binding.key;
                        let existing = preset
                            .press_stop_step
                            .key
                            .split(',')
                            .map(str::trim)
                            .filter(|part| !part.is_empty())
                            .map(str::to_owned)
                            .collect::<Vec<_>>();
                        let label =
                            if preset.press_stop_step.action == MacroAction::StopIfKeyPressed {
                                "press-stop stop key"
                            } else {
                                "press-stop lock key"
                            };
                        if existing.iter().any(|part| part.eq_ignore_ascii_case(&key)) {
                            self.status = format!("Key {key} is already in that {label} list.");
                        } else if existing.is_empty() {
                            preset.press_stop_step.key = key.clone();
                            self.status = format!("Captured {label} {key} for macro {preset_id}.");
                        } else {
                            preset.press_stop_step.key =
                                format!("{},{}", preset.press_stop_step.key.trim(), key);
                            self.status = format!("Added {label} {key} for macro {preset_id}.");
                        }
                    } else {
                        preset.press_stop_step.key = binding.key.clone();
                        self.status = format!(
                            "Captured press-stop input {} for macro {preset_id}.",
                            binding.key
                        );
                    }
                }
                self.sync_macro_presets();
            }
            (
                CaptureRequest::MacroStepInput {
                    group_id,
                    preset_id,
                    step_index,
                    extra_cond_index,
                },
                CapturedInput::Binding(binding),
            ) => {
                if let Some(step) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .and_then(|group| {
                        group
                            .presets
                            .iter_mut()
                            .find(|preset| preset.id == preset_id)
                    })
                    .and_then(|preset| preset.steps.get_mut(step_index))
                {
                    if step.action == MacroAction::IfStart {
                        let key_to_add = binding.key.trim().to_owned();
                        if let Some(extra_idx) = extra_cond_index {
                            if let Some(cond) = step.extra_conditions.get_mut(extra_idx) {
                                if cond.condition_type == crate::model::IfConditionType::KeyHeld {
                                    let mut existing = cond
                                        .key_held_name
                                        .split(',')
                                        .map(str::trim)
                                        .filter(|p| !p.is_empty())
                                        .map(str::to_owned)
                                        .collect::<Vec<_>>();
                                    if !existing.contains(&key_to_add) {
                                        existing.push(key_to_add);
                                        cond.key_held_name = existing.join(",");
                                    }
                                } else if cond.condition_type
                                    == crate::model::IfConditionType::MouseHeld
                                {
                                    let mut existing = cond
                                        .mouse_button
                                        .split(',')
                                        .map(str::trim)
                                        .filter(|p| !p.is_empty())
                                        .map(str::to_owned)
                                        .collect::<Vec<_>>();
                                    if !existing.contains(&key_to_add) {
                                        existing.push(key_to_add);
                                        cond.mouse_button = existing.join(",");
                                    }
                                }
                            }
                        } else {
                            let mut existing = step
                                .key
                                .split(',')
                                .map(str::trim)
                                .filter(|p| !p.is_empty())
                                .map(str::to_owned)
                                .collect::<Vec<_>>();
                            if !existing.contains(&key_to_add) {
                                existing.push(key_to_add);
                                step.key = existing.join(",");
                            }
                        }
                        self.status =
                            format!("Captured Input Held condition input for preset {preset_id}.");
                    } else if matches!(step.action, MacroAction::LockKeys | MacroAction::UnlockKeys)
                        || (step.action == MacroAction::StopIfKeyPressed
                            && step.get_break_loop_mode() == "StopKey")
                    {
                        let key = binding.key;
                        let was_empty = Self::split_key_list(&step.key).is_empty();
                        if !was_empty
                            && Self::split_key_list(&step.key)
                                .iter()
                                .any(|part| part.eq_ignore_ascii_case(&key))
                        {
                            self.status = if step.action == MacroAction::StopIfKeyPressed {
                                format!("Key {key} is already in that stop key list.")
                            } else {
                                format!("Key {key} is already in that lock list.")
                            };
                        } else if Self::append_key_list_value(&mut step.key, &key) {
                            self.status = if step.action == MacroAction::StopIfKeyPressed {
                                if was_empty {
                                    format!("Captured stop key {key} for preset {preset_id}.")
                                } else {
                                    format!("Added stop key {key} for preset {preset_id}.")
                                }
                            } else {
                                if was_empty {
                                    format!("Captured lock key {key} for preset {preset_id}.")
                                } else {
                                    format!("Added lock key {key} for preset {preset_id}.")
                                }
                            };
                        }
                    } else {
                        step.key = binding.key;
                        if step.action == MacroAction::MouseMoveAbsolute
                            || step.action == MacroAction::MouseMoveRelative
                        {
                            step.action = MacroAction::KeyPress;
                        }
                        self.status = format!("Captured step input for preset {preset_id}.");
                    }
                }
                self.sync_macro_presets();
            }
            (
                CaptureRequest::MacroStepInput {
                    group_id,
                    preset_id,
                    step_index,
                    extra_cond_index: _,
                },
                CapturedInput::Step(mut captured_step),
            ) => {
                captured_step.delay_ms = 0;
                if let Some(step) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .and_then(|group| {
                        group
                            .presets
                            .iter_mut()
                            .find(|preset| preset.id == preset_id)
                    })
                    .and_then(|preset| preset.steps.get_mut(step_index))
                {
                    step.key = captured_step.key;
                    step.action = captured_step.action;
                    step.x = captured_step.x;
                    step.y = captured_step.y;
                    step.x_expr = captured_step.x_expr;
                    step.y_expr = captured_step.y_expr;
                    self.status = format!("Captured step input for preset {preset_id}.");
                }
                self.sync_macro_presets();
            }
            _ => {
                self.status = "Capture type mismatch.".to_owned();
            }
        }
        self.persist();
        if matches!(
            target_clone,
            CaptureRequest::MacroPresetRecordHotkey(_, _)
                | CaptureRequest::MacroPresetHotkey(_, _)
                | CaptureRequest::MousePathRecordHotkey(_)
                | CaptureRequest::CommandPresetHotkey(_)
                | CaptureRequest::PinPresetHotkey(_)
                | CaptureRequest::MouseSensitivityPresetHotkey(_)
                | CaptureRequest::VisionPresetHotkey(_)
        ) {
            false
        } else {
            keep_capture_open
        }
    }

    fn poll_capture_input(&mut self, ctx: &egui::Context) {
        if self.capture_target.is_some() {
            ctx.request_repaint();
        }
        if self
            .capture_mouse_guard_until
            .is_some_and(|until| Instant::now() < until)
        {
            return;
        }
        self.capture_mouse_guard_until = None;
        if self.capture_suppress_polls_remaining > 0 {
            self.capture_suppress_polls_remaining -= 1;
            return;
        }
        if self.capture_suppress_next_poll {
            self.capture_suppress_next_poll = false;
            return;
        }
        if self.capture_ignore_mouse_until_release {
            if Self::is_vk_down(0x01)
                || Self::is_vk_down(0x02)
                || Self::is_vk_down(0x04)
                || Self::is_vk_down(0x05)
                || Self::is_vk_down(0x06)
            {
                return;
            }
            self.capture_ignore_mouse_until_release = false;
            return;
        }
        let Some(target) = self.capture_target.clone() else {
            self.capture_ignored_keys.clear();
            return;
        };
        let Some(captured) = self.capture_next_input(ctx) else {
            return;
        };
        let keep_capture_open = self.apply_captured_input(target, CapturedInput::Binding(captured));
        if !keep_capture_open {
            self.capture_target = None;
            self.capture_ignored_keys.clear();
            self.capture_hotkey_combo_vks.clear();
        }
    }

    #[cfg(windows)]
    fn capture_next_input(&mut self, ctx: &egui::Context) -> Option<crate::model::HotkeyBinding> {
        let accepts_mouse = self
            .capture_target
            .as_ref()
            .is_none_or(|target| self.capture_request_accepts_mouse(target));
        if self.capture_wait_for_mouse_release {
            if Self::is_vk_down(0x01)
                || Self::is_vk_down(0x02)
                || Self::is_vk_down(0x04)
                || Self::is_vk_down(0x05)
                || Self::is_vk_down(0x06)
            {
                return None;
            }
            if self
                .capture_target
                .as_ref()
                .is_some_and(|target| self.capture_request_accepts_mouse(target))
            {
                for mouse_vk in [0x01, 0x02, 0x04, 0x05, 0x06] {
                    self.capture_ignored_keys.remove(&mouse_vk);
                }
            }
            self.capture_wait_for_mouse_release = false;
            return None;
        }
        if accepts_mouse && let Some(binding) = self.capture_scroll_binding(ctx) {
            return Some(binding);
        }
        let capture_target = self.capture_target.clone();
        let mut captured_key_down = false;
        let mut newly_pressed_keys = Vec::new();
        for vk in Self::capture_scan_keys() {
            if !accepts_mouse && Self::capture_mouse_vk(vk) {
                continue;
            }
            let pressed = unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
            if pressed {
                if self.capture_ignored_keys.contains(&vk) {
                    continue;
                }
                captured_key_down = true;
                if self.capture_hotkey_combo_vks.insert(vk)
                    && let Some(key_name) = hotkey::vk_to_key_name(vk)
                {
                    newly_pressed_keys.push(key_name.to_owned());
                }
            } else {
                self.capture_ignored_keys.remove(&vk);
            }
        }

        let first_newly_pressed = newly_pressed_keys.first().cloned();

        if let Some(pending) = self.capture_hotkey_combo_keys.as_mut() {
            for key in &newly_pressed_keys {
                if !pending
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(key))
                {
                    pending.push(key.clone());
                }
            }
        } else if !newly_pressed_keys.is_empty() {
            self.capture_hotkey_combo_keys = Some(newly_pressed_keys);
        }

        if let Some(target) = capture_target.as_ref()
            && self.capture_request_registers_on_press(target)
            && let Some(key) = first_newly_pressed
        {
            self.capture_hotkey_combo_keys = None;
            return Some(Self::hotkey_binding_from_combo_keys(vec![key]));
        }

        if let Some(target) = capture_target
            && matches!(
                target,
                CaptureRequest::MacroPresetHotkey(_, _)
                    | CaptureRequest::MacroPresetRecordHotkey(_, _)
                    | CaptureRequest::CommandPresetHotkey(_)
                    | CaptureRequest::WindowPresetHotkey(_)
                    | CaptureRequest::WindowFocusPresetHotkey(_)
                    | CaptureRequest::WindowLayoutHotkey(_)
                    | CaptureRequest::PinPresetHotkey(_)
                    | CaptureRequest::MouseSensitivityPresetHotkey(_)
            )
            && let Some(pending) = self.capture_hotkey_combo_keys.as_ref()
        {
            self.status = self.capture_combo_status_text(pending);
            ctx.request_repaint();
        }

        if self.capture_hotkey_combo_keys.is_some() && !captured_key_down {
            self.capture_hotkey_combo_vks.clear();
            return self
                .capture_hotkey_combo_keys
                .take()
                .map(Self::hotkey_binding_from_combo_keys);
        }

        None
    }

    #[cfg(not(windows))]
    fn capture_next_input(&mut self, _ctx: &egui::Context) -> Option<crate::model::HotkeyBinding> {
        None
    }

    #[cfg(windows)]
    fn capture_scroll_binding(&self, ctx: &egui::Context) -> Option<crate::model::HotkeyBinding> {
        let scroll_y = ctx.input(|input| input.raw_scroll_delta.y);
        if scroll_y.abs() < 0.01 {
            return None;
        }
        let key = if scroll_y > 0.0 {
            "MouseWheelUp".to_owned()
        } else {
            "MouseWheelDown".to_owned()
        };
        Some(crate::model::HotkeyBinding {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            key: key.clone(),
            combo_keys: vec![key],
        })
    }

    fn hotkey_binding_from_combo_keys(mut combo_keys: Vec<String>) -> crate::model::HotkeyBinding {
        combo_keys.retain(|key| !key.trim().is_empty());
        let key = combo_keys
            .iter()
            .rev()
            .find(|key| !hotkey::is_modifier_key_name(key))
            .cloned()
            .or_else(|| combo_keys.last().cloned())
            .unwrap_or_default();
        crate::model::HotkeyBinding {
            ctrl: combo_keys
                .iter()
                .any(|key| key.eq_ignore_ascii_case("Ctrl") || key.eq_ignore_ascii_case("Control")),
            alt: combo_keys.iter().any(|key| key.eq_ignore_ascii_case("Alt")),
            shift: combo_keys
                .iter()
                .any(|key| key.eq_ignore_ascii_case("Shift")),
            win: combo_keys
                .iter()
                .any(|key| key.eq_ignore_ascii_case("Win") || key.eq_ignore_ascii_case("Meta")),
            key,
            combo_keys,
        }
    }

    fn capture_combo_status_text(&self, combo_keys: &[String]) -> String {
        let preview = Self::hotkey_binding_from_combo_keys(combo_keys.to_vec());
        let label = hotkey::format_binding(Some(&preview));
        if combo_keys.len() == 1 {
            crate::lang::translate(
                self.state.ui_language,
                "Captured key: {label}. Hold another key to form a combo, or release to save.",
            )
            .unwrap_or(
                "Captured key: {label}. Hold another key to form a combo, or release to save.",
            )
            .replace("{label}", &label)
        } else {
            crate::lang::translate(
                self.state.ui_language,
                "Captured combo: {label}. Release to save.",
            )
            .unwrap_or("Captured combo: {label}. Release to save.")
            .replace("{label}", &label)
        }
    }

    #[cfg(windows)]
    fn capture_mouse_vk(vk: u32) -> bool {
        matches!(vk, 0x01 | 0x02 | 0x04 | 0x05 | 0x06)
    }

    #[cfg(not(windows))]
    fn capture_scroll_binding(&self, _ctx: &egui::Context) -> Option<crate::model::HotkeyBinding> {
        None
    }

    #[cfg(not(windows))]
    fn capture_mouse_vk(_vk: u32) -> bool {
        false
    }

    #[cfg(windows)]
    fn is_vk_down(vk: u32) -> bool {
        unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
    }

    #[cfg(windows)]
    fn snapshot_pressed_capture_keys(&self) -> HashSet<u32> {
        Self::capture_scan_keys()
            .into_iter()
            .filter(|vk| Self::is_vk_down(*vk))
            .collect()
    }

    #[cfg(not(windows))]
    fn snapshot_pressed_capture_keys(&self) -> HashSet<u32> {
        HashSet::new()
    }

    fn capture_scan_keys() -> Vec<u32> {
        let mut keys = Vec::new();
        keys.extend(0x08..=0x0D);
        keys.extend([0x01, 0x02, 0x04, 0x05, 0x06]);
        keys.extend(0x10..=0x14);
        keys.extend(0x1B..=0x28);
        keys.extend(0x2C..=0x2E);
        keys.extend(0x30..=0x39);
        keys.extend(0x41..=0x5D);
        keys.extend(0x60..=0x6F);
        keys.extend(0x70..=0x87);
        keys.extend([
            0x90, 0x91, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, 0xC0, 0xDB, 0xDC, 0xDD, 0xDE,
        ]);
        keys
    }

    fn persist_macro_presets(&mut self) {
        self.persist_after_syncs([Self::sync_macro_presets, Self::sync_macro_master_enabled]);
    }

    fn sync_reconciled_macro_presets(&mut self) {
        self.reconcile_master_presets();
        self.sync_macro_presets();
    }

    fn persist_reconciled_macro_presets(&mut self) {
        self.reconcile_master_presets();
        self.persist_macro_presets();
    }

    fn persist_timer_presets(&mut self) {
        self.persist_after_sync(Self::sync_timer_presets);
    }

    #[allow(unreachable_code)]
    fn startup_splash_progress(&mut self, ctx: &egui::Context) -> Option<f32> {
        if self.startup_splash.duration_sec <= 0.0 {
            return None;
        }
        let now = ctx.input(|input| input.time);
        let started_at = self.startup_splash.started_at.get_or_insert(now);
        let progress =
            ((now - *started_at) / self.startup_splash.duration_sec).clamp(0.0, 1.0) as f32;
        if progress >= 1.0 {
            self.startup_splash.duration_sec = 0.0;
            return None;
        }
        ctx.request_repaint();
        Some(progress)
    }

    fn render_startup_splash(&self, ctx: &egui::Context, progress: f32) {
        let time = ctx.input(|input| input.time) as f32;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(Color32::TRANSPARENT).inner_margin(0.0))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter_at(rect);
                let fade = (1.0 - progress).clamp(0.0, 1.0);
                let alpha = (fade * 255.0) as u8;
                let pulse = (time * 2.6).sin() * 0.5 + 0.5;
                let scale = 0.94 + progress * 0.06 + pulse * 0.015;
                let title_font = egui::FontId::proportional(32.0 * scale);
                let subtitle_font = egui::FontId::proportional(15.0 * scale);
                let panel_size = vec2(rect.width().min(420.0), 168.0);
                let panel_rect = egui::Rect::from_center_size(rect.center(), panel_size);
                let panel_alpha = alpha.saturating_div(3);
                painter.rect_filled(
                    rect,
                    16.0,
                    Color32::from_rgba_premultiplied(8, 10, 14, alpha.saturating_div(2)),
                );
                painter.rect_filled(
                    panel_rect,
                    16.0,
                    Color32::from_rgba_premultiplied(18, 22, 30, panel_alpha),
                );
                painter.rect_stroke(
                    panel_rect,
                    16.0,
                    Stroke::new(
                        1.0,
                        Color32::from_rgba_premultiplied(88, 132, 198, panel_alpha),
                    ),
                    StrokeKind::Outside,
                );
                let bar_w = 18.0 * scale;
                let bar_h = 36.0 * scale;
                let bar_gap = 10.0 * scale;
                let bars_total_w = bar_w * 3.0 + bar_gap * 2.0;
                let bars_left = panel_rect.center().x - bars_total_w * 0.5;
                let bars_top = panel_rect.top() + 22.0 * scale;
                for i in 0..3 {
                    let n = i as f32;
                    let bar_phase = (time * 4.0 + n * 0.7).sin() * 0.5 + 0.5;
                    let bar_alpha = (90.0 + bar_phase * 140.0) as u8;
                    let bar_rect = egui::Rect::from_min_size(
                        pos2(
                            bars_left + n * (bar_w + bar_gap),
                            bars_top + (1.0 - bar_phase) * 10.0 * scale,
                        ),
                        vec2(bar_w, bar_h - (1.0 - bar_phase) * 10.0 * scale),
                    );
                    painter.rect_filled(
                        bar_rect,
                        8.0,
                        Color32::from_rgba_premultiplied(94, 220, 176, bar_alpha),
                    );
                }
                painter.text(
                    panel_rect.center() - vec2(0.0, 8.0 * scale),
                    egui::Align2::CENTER_CENTER,
                    self.app_brand_title(),
                    title_font,
                    Color32::from_rgba_premultiplied(240, 244, 248, alpha),
                );
                painter.text(
                    panel_rect.center() + vec2(0.0, 22.0 * scale),
                    egui::Align2::CENTER_CENTER,
                    self.startup_loading_text(),
                    subtitle_font,
                    Color32::from_rgba_premultiplied(208, 220, 255, alpha),
                );
                let track_rect = egui::Rect::from_center_size(
                    pos2(panel_rect.center().x, panel_rect.bottom() - 28.0 * scale),
                    vec2(panel_rect.width() - 56.0 * scale, 6.0 * scale),
                );
                painter.rect_filled(
                    track_rect,
                    999.0,
                    Color32::from_rgba_premultiplied(44, 52, 64, panel_alpha),
                );
                let fill_w = track_rect.width() * progress.clamp(0.0, 1.0);
                let fill_rect =
                    egui::Rect::from_min_size(track_rect.min, vec2(fill_w, track_rect.height()));
                painter.rect_filled(
                    fill_rect,
                    999.0,
                    Color32::from_rgba_premultiplied(94, 220, 176, alpha),
                );
            });
    }

    fn render_custom_window_resize_handles(&self, ctx: &egui::Context) {
        if ctx.input(|input| input.viewport().maximized.unwrap_or(false)) {
            return;
        }

        let rect = ctx.content_rect();
        let edge = 8.0;
        let corner = 22.0;
        let handles = [
            (
                "resize-n",
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + edge)),
                egui::viewport::ResizeDirection::North,
                egui::CursorIcon::ResizeVertical,
            ),
            (
                "resize-s",
                egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - edge), rect.max),
                egui::viewport::ResizeDirection::South,
                egui::CursorIcon::ResizeVertical,
            ),
            (
                "resize-w",
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + edge, rect.max.y)),
                egui::viewport::ResizeDirection::West,
                egui::CursorIcon::ResizeHorizontal,
            ),
            (
                "resize-e",
                egui::Rect::from_min_max(egui::pos2(rect.max.x - edge, rect.min.y), rect.max),
                egui::viewport::ResizeDirection::East,
                egui::CursorIcon::ResizeHorizontal,
            ),
            (
                "resize-nw",
                egui::Rect::from_min_size(rect.min, vec2(corner, corner)),
                egui::viewport::ResizeDirection::NorthWest,
                egui::CursorIcon::ResizeNwSe,
            ),
            (
                "resize-ne",
                egui::Rect::from_min_max(
                    egui::pos2(rect.max.x - corner, rect.min.y),
                    egui::pos2(rect.max.x, rect.min.y + corner),
                ),
                egui::viewport::ResizeDirection::NorthEast,
                egui::CursorIcon::ResizeNeSw,
            ),
            (
                "resize-sw",
                egui::Rect::from_min_max(
                    egui::pos2(rect.min.x, rect.max.y - corner),
                    egui::pos2(rect.min.x + corner, rect.max.y),
                ),
                egui::viewport::ResizeDirection::SouthWest,
                egui::CursorIcon::ResizeNeSw,
            ),
            (
                "resize-se",
                egui::Rect::from_min_max(
                    egui::pos2(rect.max.x - corner, rect.max.y - corner),
                    rect.max,
                ),
                egui::viewport::ResizeDirection::SouthEast,
                egui::CursorIcon::ResizeNwSe,
            ),
        ];

        for (id, handle_rect, direction, cursor) in handles {
            egui::Area::new(egui::Id::new(id))
                .order(egui::Order::Foreground)
                .fixed_pos(handle_rect.min)
                .interactable(true)
                .show(ctx, |ui| {
                    let (_, response) =
                        ui.allocate_exact_size(handle_rect.size(), Sense::click_and_drag());
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(cursor);
                    }
                    if response.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                    }
                });
        }
    }

    fn render_custom_window_border(&self, ctx: &egui::Context) {
        let stroke = if self.state.ui_theme == UiThemeMode::Dark {
            egui::Stroke::new(1.4, Color32::from_rgb(64, 84, 108))
        } else {
            egui::Stroke::new(1.4, Color32::from_rgb(184, 198, 214))
        };
        let mut rect = ctx.content_rect().shrink(0.5);
        rect.max.x -= 0.5;
        rect.max.y -= 0.5;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("window-border"),
        ));
        painter.rect_stroke(rect, 16.0, stroke, egui::StrokeKind::Inside);
    }

    fn run_deferred_startup_tasks(&mut self, ctx: &egui::Context) {
        if self.startup_show_pending {
            return;
        }
        if self.startup_hide_to_tray_pending {
            self.startup_hide_to_tray_pending = false;
            self.hide_to_tray(ctx);
            return;
        }
        if self.startup_gate_release_pending {
            if self.startup_gate_frames_remaining > 0 {
                self.startup_gate_frames_remaining -= 1;
                ctx.request_repaint();
                return;
            }
            if let Some(startup_gate) = self.startup_gate.take() {
                let (gate_lock, gate_ready) = &*startup_gate;
                let mut gate_open = gate_lock.lock().expect("startup gate poisoned");
                *gate_open = true;
                gate_ready.notify_all();
            }
            self.startup_gate_release_pending = false;
        }
        if self.startup_shell_frames_remaining > 0 {
            self.startup_shell_frames_remaining -= 1;
            ctx.request_repaint();
            return;
        }
        if self.startup_overlay_sync_pending {
            self.run_all_startup_overlay_sync();
        }
        if self.startup_state_persist_pending {
            self.persist();
            self.startup_state_persist_pending = false;
        }
        if self.startup_cjk_font_check_pending {
            if self.startup_state_needs_cjk_fallback {
                configure_fonts(ctx, true);
                self.last_applied_theme = None;
                self.apply_theme(ctx);
            }
            self.startup_cjk_font_check_pending = false;
        }
        let preload_panels = Self::all_panels_for_background_preload();
        if self.background_panel_preload_index < preload_panels.len() {
            let panel = preload_panels[self.background_panel_preload_index];
            if !self.panel_is_warmed(panel) {
                self.warmed_panels.push(panel);
            }
            self.background_panel_preload_index += 1;
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        self.state.show_window = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(false));
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetTrayIconVisible(true));
        crate::overlay::wake_command_queue();
        self.persist();
    }

    fn begin_protractor_calibration(&mut self, ctx: &egui::Context, was_minimized: bool) {
        if self.protractor_picking_active || self.native_capture_in_progress {
            return;
        }

        self.protractor_picking_active = true;
        self.native_capture_in_progress = true;
        self.protractor_calibration_points = Some(Vec::new());

        // Hide main app window natively
        #[cfg(windows)]
        unsafe {
            if let Some(hwnd) = crate::overlay::find_app_ui_window_for_ui_thread() {
                use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(false));
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetProtractorEnabled(false));
        crate::overlay::wake_command_queue();

        let ui_tx = self.ui_tx.clone();
        let egui_ctx = ctx.clone();
        let ui_lang = self.state.ui_language;

        std::thread::spawn(move || {
            // Sleep to let OS process window hide
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Capture virtual screen bounds
            let (left, top, width, height) = crate::window_list::virtual_screen_bounds();
            let result = if let Some(capture) =
                crate::window_list::capture_virtual_screen_region(left, top, width, height)
            {
                let mode =
                    crate::overlay::native_capture::NativeCaptureMode::ProtractorCalibration {
                        ui_language: ui_lang,
                    };
                crate::overlay::native_capture::run_capture_overlay(
                    capture, left, top, width, height, mode,
                )
            } else {
                crate::overlay::native_capture::NativeCaptureResult::Cancelled
            };

            // Restore main app window natively
            #[cfg(windows)]
            unsafe {
                if let Some(hwnd) = crate::overlay::find_app_ui_window_for_ui_thread() {
                    if was_minimized {
                        use windows::Win32::UI::WindowsAndMessaging::{
                            SW_SHOWMINNOACTIVE, ShowWindow,
                        };
                        let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
                    } else {
                        use windows::Win32::UI::WindowsAndMessaging::{
                            SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
                        };
                        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
                        let _ = SetForegroundWindow(hwnd);
                    }
                }
            }

            // Sleep a tiny bit to let OS display the window so winit event loop is active
            std::thread::sleep(std::time::Duration::from_millis(50));

            let _ = ui_tx.send(UiCommand::NativeProtractorCalibrationFinished {
                result,
                was_minimized,
            });
            egui_ctx.request_repaint();
        });
    }

    fn begin_distance_measurement(&mut self, ctx: &egui::Context, was_minimized: bool) {
        if self.distance_measurement_active || self.native_capture_in_progress {
            return;
        }

        self.distance_measurement_active = true;
        self.native_capture_in_progress = true;

        #[cfg(windows)]
        unsafe {
            if let Some(hwnd) = crate::overlay::find_app_ui_window_for_ui_thread() {
                use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
                let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
            }
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(false));
        crate::overlay::wake_command_queue();

        let ui_tx = self.ui_tx.clone();
        let egui_ctx = ctx.clone();
        let ui_lang = self.state.ui_language;

        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));

            let (left, top, width, height) = crate::window_list::virtual_screen_bounds();
            let result = if let Some(capture) =
                crate::window_list::capture_virtual_screen_region(left, top, width, height)
            {
                let mode = crate::overlay::native_capture::NativeCaptureMode::DistanceMeasure {
                    ui_language: ui_lang,
                };
                crate::overlay::native_capture::run_capture_overlay(
                    capture, left, top, width, height, mode,
                )
            } else {
                crate::overlay::native_capture::NativeCaptureResult::Cancelled
            };

            #[cfg(windows)]
            unsafe {
                if let Some(hwnd) = crate::overlay::find_app_ui_window_for_ui_thread() {
                    if was_minimized {
                        use windows::Win32::UI::WindowsAndMessaging::{
                            SW_SHOWMINNOACTIVE, ShowWindow,
                        };
                        let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
                    } else {
                        use windows::Win32::UI::WindowsAndMessaging::{
                            SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
                        };
                        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
                        let _ = SetForegroundWindow(hwnd);
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(50));

            let _ = ui_tx.send(UiCommand::NativeDistanceMeasurementFinished {
                result,
                was_minimized,
            });
            egui_ctx.request_repaint();
        });
    }

    pub(crate) fn finish_protractor_calibration_freeze(
        &mut self,
        ctx: &egui::Context,
        points: Vec<(i32, i32)>,
    ) {
        self.protractor_picking_active = false;
        self.protractor_calibration_points = None;
        self.captured_freeze_texture = None;
        self.captured_freeze_frame = None;
        self.restore_mouse_move_absolute_capture_window(ctx);
        self.mouse_move_absolute_capture_raise_window = true;

        if points.len() == 3 {
            if let Some(((cx, cy), radius)) =
                crate::protractor::circle_from_3_points(points[0], points[1], points[2])
            {
                if radius < crate::protractor::PROTRACTOR_MIN_CALIBRATION_RADIUS {
                    self.status = Self::tr_lang(
                        self.state.ui_language,
                        "Selected circle is too small. Pick three points farther apart.",
                        "Selected circle is too small. Pick three points farther apart.",
                    )
                    .to_owned();
                } else {
                    let scale = crate::protractor::calibrated_protractor_scale(radius);
                    self.state.protractor_center_x = cx;
                    self.state.protractor_center_y = cy;
                    self.state.protractor_scale = scale;
                    self.state.protractor_enabled = true;
                    self.status = Self::tr_lang(
                        self.state.ui_language,
                        "Protractor calibrated successfully!",
                        "Protractor calibrated successfully!",
                    )
                    .to_owned();
                }
            } else {
                self.status = Self::tr_lang(
                    self.state.ui_language,
                    "Points are collinear. Cannot form a circle.",
                    "Points are collinear. Cannot form a circle.",
                )
                .to_owned();
            }
        }
        self.sync_protractor_state();
        self.persist();
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }

    pub(crate) fn render_protractor_calibration_overlay(&mut self, _ctx: &egui::Context) -> bool {
        false
    }
}

impl eframe::App for CrosshairApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        crate::overlay::UI_WANTS_KEYBOARD_INPUT.store(
            ctx.wants_keyboard_input(),
            std::sync::atomic::Ordering::Relaxed,
        );
        crate::overlay::UI_CAPTURING_INPUT.store(
            self.capture_target.is_some(),
            std::sync::atomic::Ordering::Relaxed,
        );
        {
            let mut config = VIETNAMESE_INPUT_CONFIG.lock();
            config.enabled = self.state.vietnamese_input_enabled;
            config.mode = self.state.vietnamese_input_mode;
        }
        if self.startup_shell_frames_remaining > 0 {
            self.startup_shell_frames_remaining -= 1;
            if self.startup_shell_frames_remaining == 0 {
                crate::platform::trim_working_set();
            }
            ctx.request_repaint();
        }
        if self.state.active_panel == AppPanel::Zoom {
            self.state.active_panel = AppPanel::Pin;
        } else if self.state.active_panel == AppPanel::Modes {
            self.state.active_panel = AppPanel::Macros;
        }
        crate::overlay::set_ui_context(ctx.clone());
        self.apply_theme(ctx);
        let wants_native_shadow = false;
        if self.native_shadow_applied != wants_native_shadow {
            if crate::platform::set_native_window_shadow(frame, wants_native_shadow) {
                self.native_shadow_applied = wants_native_shadow;
            }
        }
        if !self.native_transitions_disabled_applied
            && crate::platform::set_native_window_transitions_disabled(frame, true)
        {
            self.native_transitions_disabled_applied = true;
        }
        self.run_deferred_startup_tasks(ctx);
        if self.startup_update_check_pending {
            self.startup_update_check_pending = false;
            self.check_for_update_with_origin(ctx, true);
        }

        // ponytail: bound background work per frame so a noisy producer cannot starve painting.
        // If 256 becomes measurable backlog, coalesce high-frequency command variants at source.
        for _ in 0..MAX_UI_COMMANDS_PER_FRAME {
            let Ok(command) = self.ui_rx.try_recv() else {
                break;
            };
            match command {
                UiCommand::ShowWindow => {
                    if self.state.show_window {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        ctx.request_repaint();
                        continue;
                    }
                    let target_size = Self::desired_window_size();
                    let target_pos =
                        Self::centered_outer_position_for_size(target_size, ctx.pixels_per_point());
                    if crate::platform::set_native_window_shadow(frame, false) {
                        self.native_shadow_applied = false;
                    } else {
                        self.native_shadow_applied = true;
                    }
                    self.state.show_window = true;
                    self.enforce_square_window_frames = 0;
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target_size));
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(target_pos));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(true));
                    let _ = self
                        .overlay_tx
                        .send(OverlayCommand::SetTrayIconVisible(false));
                    crate::overlay::wake_command_queue();
                    ctx.request_repaint();
                }
                UiCommand::Exit => {
                    self.quit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                UiCommand::MascotDragged { style, x, y } => {
                    let active_mascot_count =
                        if self.state.quick_key_display_mascot_styles.is_empty() {
                            1
                        } else {
                            self.state.quick_key_display_mascot_styles.len()
                        };
                    if active_mascot_count <= 1 {
                        self.state.quick_key_display_x = x;
                        self.state.quick_key_display_y = y;
                    }
                    if let Some((_, pos_x, pos_y)) = self
                        .state
                        .quick_key_display_mascot_positions
                        .iter_mut()
                        .find(|(entry_style, _, _)| *entry_style == style)
                    {
                        *pos_x = x;
                        *pos_y = y;
                    } else {
                        self.state
                            .quick_key_display_mascot_positions
                            .push((style, x, y));
                    }
                    self.persist();
                    ctx.request_repaint();
                }
                UiCommand::StartupIconLoaded(icon) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(icon)));
                }
                UiCommand::StartupStateLoaded {
                    state,
                    startup_state_dirty,
                    startup_state_needs_cjk_fallback,
                } => {
                    self.apply_loaded_startup_state(
                        ctx,
                        state,
                        startup_state_dirty,
                        startup_state_needs_cjk_fallback,
                    );
                }
                UiCommand::StartupStateLoadFailed(error) => {
                    self.status = format!("Failed to load app state: {error}");
                    self.startup_state_persist_pending = false;
                    self.startup_overlay_sync_pending = true;
                    self.startup_cjk_font_check_pending = true;
                    self.startup_shell_frames_remaining =
                        self.startup_shell_frames_remaining.max(3);
                    ctx.request_repaint();
                }
                UiCommand::SyncMacroGroups(groups, status) => {
                    self.state.macro_groups = groups;
                    self.persist();
                    self.status = status;
                }
                UiCommand::SyncCrosshairProfiles(profiles, status) => {
                    self.state.profiles = profiles;
                    if self.state.profiles.is_empty() {
                        self.state.selected_profile = None;
                        self.state.active_style = CrosshairStyle {
                            enabled: false,
                            ..CrosshairStyle::default()
                        };
                        self.save_name.clear();
                    }
                    self.persist();
                    self.status = status;
                }
                UiCommand::MemoryTrackedCodeResolved {
                    pid,
                    alias_name,
                    captured_address,
                    ..
                } => {
                    let mut resolved = 0;
                    for entry in &mut self.state.memory_pointer_list {
                        if entry.name.eq_ignore_ascii_case(&alias_name) {
                            entry.runtime_address =
                                captured_address.checked_add_signed(entry.code_address_offset);
                            entry.runtime_process_id = Some(pid);
                            resolved += usize::from(entry.runtime_address.is_some());
                        }
                    }
                    if resolved != 0 {
                        crate::overlay::set_memory_pointer_entries(&self.state.memory_pointer_list);
                        self.persist();
                        self.status = format!("Rebound {resolved} tracked memory address(es)");
                    }
                }
                UiCommand::MemoryTrackedCodeInvalidated {
                    pid,
                    alias_name,
                    runtime_address,
                } => {
                    let mut invalidated = false;
                    for entry in &mut self.state.memory_pointer_list {
                        if entry.name.eq_ignore_ascii_case(&alias_name)
                            && entry.runtime_process_id == Some(pid)
                            && entry.runtime_address == Some(runtime_address)
                        {
                            entry.runtime_address = None;
                            entry.runtime_process_id = None;
                            invalidated = true;
                        }
                    }
                    if invalidated {
                        crate::overlay::set_memory_pointer_entries(&self.state.memory_pointer_list);
                        self.persist();
                    }
                }
                UiCommand::EspPresetEnabled { preset_id, enabled } => {
                    if let Some(preset) = self
                        .state
                        .esp_presets
                        .iter_mut()
                        .find(|preset| preset.id == preset_id)
                    {
                        preset.enabled = enabled;
                        self.persist_esp_presets();
                    }
                }
                UiCommand::EspCalibrationUpdated {
                    preset_id,
                    sample_count: _,
                    result,
                    status,
                } => {
                    if let Some(result) = result
                        && let Some(preset) = self
                            .state
                            .esp_presets
                            .iter_mut()
                            .find(|preset| preset.id == preset_id)
                    {
                        preset.invert_camera_yaw = result.invert_camera_yaw;
                        preset.invert_yaw = result.invert_yaw;
                        preset.yaw_offset_degrees = result.yaw_offset_degrees;
                        preset.invert_pitch = result.invert_pitch;
                        preset.pitch_offset_degrees = result.pitch_offset_degrees;
                        self.persist_esp_presets();
                    }
                    self.esp_calibration_feedback
                        .insert(preset_id, status.clone());
                    self.status = status;
                    ctx.request_repaint();
                }
                UiCommand::SetMacrosMasterEnabled(enabled, status) => {
                    self.state.macros_master_enabled = enabled;
                    self.persist();
                    self.status = status;
                    ctx.request_repaint();
                }
                UiCommand::SetVietnameseInputEnabled(enabled, status) => {
                    self.state.vietnamese_input_enabled = enabled;
                    self.sync_vietnamese_input_enabled();
                    self.persist();
                    self.status = status;
                    ctx.request_repaint();
                }
                UiCommand::MousePathRecordingStarted(preset_id, status) => {
                    self.active_mouse_record_preset_id = Some(preset_id);
                    self.status = status;
                }
                UiCommand::MacroRecordingStarted(preset_id, status) => {
                    self.active_macro_record_preset_id = Some(preset_id);
                    self.status = status;
                }
                UiCommand::MacroRealtimeStepAdded(group_id, preset_id, step) => {
                    if let Ok((group_index, preset_index)) =
                        self.macro_preset_indices(group_id, preset_id)
                    {
                        let preset =
                            &mut self.state.macro_groups[group_index].presets[preset_index];
                        if preset.steps.len() == 1
                            && preset.steps[0].action == MacroAction::KeyPress
                            && preset.steps[0].key.is_empty()
                            && preset.steps[0].delay_ms == 100
                        {
                            preset.steps.clear();
                        }
                        preset.steps.push(step);
                    }
                    ctx.request_repaint();
                }
                UiCommand::MacroRealtimeStepRemoved(group_id, preset_id) => {
                    if let Ok((group_index, preset_index)) =
                        self.macro_preset_indices(group_id, preset_id)
                    {
                        self.state.macro_groups[group_index].presets[preset_index]
                            .steps
                            .pop();
                    }
                    ctx.request_repaint();
                }
                UiCommand::MacroRecordingFinished(group_id, preset_id, events, status) => {
                    if let Ok((group_index, preset_index)) =
                        self.macro_preset_indices(group_id, preset_id)
                    {
                        let record_hotkey = self.state.macro_groups[group_index].presets
                            [preset_index]
                            .record_hotkey
                            .clone();
                        let mut filtered_events = events;
                        if let Some(record_hotkey) = &record_hotkey {
                            let hotkey_keys: Vec<String> =
                                crate::hotkey::binding_key_names(record_hotkey)
                                    .into_iter()
                                    .map(|k| k.trim().to_ascii_lowercase())
                                    .collect();
                            while let Some(last) = filtered_events.last() {
                                if last.action == MacroAction::KeyPress
                                    && last.key.as_ref().is_some_and(|k| {
                                        hotkey_keys.contains(&k.trim().to_ascii_lowercase())
                                    })
                                {
                                    filtered_events.pop();
                                    continue;
                                }
                                break;
                            }
                        }
                        let path_name = format!("Macro {}-{} Path", group_id, preset_id);
                        let rebuilt_steps =
                            self.build_macro_steps_from_recording(&path_name, &filtered_events);
                        self.state.macro_groups[group_index].presets[preset_index].steps =
                            if rebuilt_steps.is_empty() {
                                vec![MacroStep::default()]
                            } else {
                                rebuilt_steps
                            };
                    }
                    self.active_macro_record_preset_id = None;
                    self.persist();
                    self.status = status;
                    ctx.request_repaint();
                }
                UiCommand::MousePathRecordingFinished(preset_id, events, status) => {
                    let mut updated_events = None;
                    if let Some(preset) = self
                        .state
                        .mouse_path_presets
                        .iter_mut()
                        .find(|preset| preset.id == preset_id)
                    {
                        preset.events = events;
                        updated_events = Some(preset.events.clone());
                    }
                    if let Some(events) = updated_events.as_deref() {
                        self.mouse_path_timeline_initialized.insert(preset_id);
                        self.mouse_path_merge_selection.remove(&preset_id);
                        Self::reset_mouse_path_timeline_state(ctx, preset_id, events);
                    }
                    self.active_mouse_record_preset_id = None;
                    self.persist_mouse_path_presets();
                    self.status = status;
                    if self.mouse_path_draw_capture_preset_id == Some(preset_id) {
                        self.mouse_path_draw_capture_preset_id = None;
                    }
                    self.restore_mouse_path_draw_capture_window(ctx);
                    ctx.request_repaint();
                }
                UiCommand::MousePathDrawCaptureCancelled(status) => {
                    self.active_mouse_record_preset_id = None;
                    if self.mouse_path_draw_capture_preset_id.is_some() {
                        self.mouse_path_draw_capture_preset_id = None;
                        self.restore_mouse_path_draw_capture_window(ctx);
                    }
                    self.status = status;
                    ctx.request_repaint();
                }
                UiCommand::VisionFinished(status) => {
                    self.status = status;
                }
                UiCommand::MacroStepInlineFeedback {
                    preset_id,
                    step_index,
                    message,
                    open_groq_settings,
                } => {
                    self.set_macro_step_inline_feedback(
                        preset_id,
                        step_index,
                        message,
                        open_groq_settings,
                    );
                    ctx.request_repaint();
                }
                UiCommand::VisionCaptureMouseDown { screen_x, screen_y } => {
                    if self.vision_capture_active {
                        self.handle_image_search_capture_mouse_down(ctx, screen_x, screen_y);
                    }
                }
                UiCommand::VisionCaptureMouseMove { screen_x, screen_y } => {
                    if self.vision_capture_active {
                        let (screen_x, screen_y) =
                            crate::overlay::take_latest_vision_capture_mouse_move()
                                .unwrap_or((screen_x, screen_y));
                        self.handle_image_search_capture_mouse_move(ctx, screen_x, screen_y);
                    }
                }
                UiCommand::VisionCaptureMouseUp { screen_x, screen_y } => {
                    if self.vision_capture_active {
                        self.handle_image_search_capture_mouse_up(ctx, screen_x, screen_y);
                    } else if let Some(target) = self.mouse_move_absolute_capture_target
                        && Self::mouse_move_absolute_capture_uses_blocked_click(target)
                    {
                        self.finish_mouse_move_absolute_capture(
                            ctx, target, screen_x, screen_y, None,
                        );
                    }
                }
                UiCommand::VisionPointCaptured {
                    preset_id,
                    priority_anchor,
                    screen_x,
                    screen_y,
                    color,
                } => {
                    self.finish_image_search_point_capture_command(
                        ctx,
                        preset_id,
                        priority_anchor,
                        screen_x,
                        screen_y,
                        color,
                    );
                }
                UiCommand::VisionRegionPreview {
                    screen_x,
                    screen_y,
                    width,
                    height,
                } => {
                    self.vision_capture_screen_region_preview =
                        Some((screen_x, screen_y, width, height));
                    self.status =
                        format!("Selecting area {width}x{height} at {screen_x}, {screen_y}.");
                    ctx.request_repaint();
                }
                UiCommand::VisionRegionCaptured {
                    preset_id,
                    template_mode,
                    screen_x,
                    screen_y,
                    width,
                    height,
                } => {
                    self.finish_image_search_region_capture_command(
                        ctx,
                        preset_id,
                        template_mode,
                        screen_x,
                        screen_y,
                        width,
                        height,
                    );
                }
                UiCommand::VisionPointCaptureCancelled(status) => {
                    self.clear_image_search_capture_state();
                    self.restore_image_search_capture_window(ctx);
                    self.status = status;
                    ctx.request_repaint();
                }
                UiCommand::ScreenDrawCaptureStatus(status) => {
                    self.status = status;
                    ctx.request_repaint();
                }
                UiCommand::VideoRecordRegionSelected {
                    x,
                    y,
                    width,
                    height,
                } => {
                    self.state.quick_video_record_region = Some((x, y, width, height));
                    self.state.quick_video_record_mode = QuickVideoRecordMode::Region;
                    self.sync_quick_video_record_config();
                    self.persist();
                    self.status = format!(
                        "Video recording region set to {}x{} at {}, {}.",
                        width, height, x, y
                    );
                    ctx.request_repaint();
                }
                UiCommand::CrosshairDrawFinished {
                    profile_name,
                    asset_name,
                    asset_scale,
                    status,
                } => {
                    if asset_name.is_some() {
                        self.apply_drawn_crosshair_asset(&profile_name, asset_name, asset_scale);
                    } else {
                        self.sync_crosshair();
                    }
                    self.status = status;
                    ctx.request_repaint();
                }
                UiCommand::UpdateScreenDrawConfig {
                    color,
                    brush_size,
                    smoothing,
                    smoothing_amount,
                    fill,
                    freeze,
                    tool,
                    text_border,
                } => {
                    self.state.quick_screen_draw_color = color;
                    self.state.quick_screen_draw_brush_size = brush_size;
                    self.state.quick_screen_draw_smoothing = smoothing;
                    self.state.quick_screen_draw_smoothing_amount = smoothing_amount;
                    self.state.quick_screen_draw_fill = fill;
                    self.state.quick_screen_draw_freeze = freeze;
                    self.state.quick_screen_draw_tool = tool;
                    self.state.quick_screen_draw_text_border = text_border;
                    self.persist();
                }

                UiCommand::MouseMoveAbsolutePointCaptured { .. } => {}
                UiCommand::MouseMoveAbsoluteCaptureCancelled => {}
                UiCommand::NativeVisionCaptureFinished {
                    target,
                    mode,
                    result,
                    capture_frame,
                } => {
                    // Show main window natively
                    #[cfg(windows)]
                    unsafe {
                        if let Some(hwnd) = crate::overlay::find_app_ui_window_for_ui_thread() {
                            use windows::Win32::UI::WindowsAndMessaging::{
                                SW_SHOWNORMAL, ShowWindow,
                            };
                            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
                        }
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);

                    // Clear native capture flag
                    self.native_capture_in_progress = false;
                    self.captured_freeze_frame = capture_frame;

                    match result {
                        crate::overlay::NativeCaptureResult::Cancelled => {
                            self.clear_image_search_capture_state();
                            self.status = match mode {
                                VisionCaptureMode::Template | VisionCaptureMode::SearchRegion => {
                                    "Image area capture cancelled.".to_owned()
                                }
                                _ => "Image point capture cancelled.".to_owned(),
                            };
                        }
                        crate::overlay::NativeCaptureResult::SelectedRegion {
                            x,
                            y,
                            width,
                            height,
                        } => {
                            if !Self::native_selected_region_target_handles_cleanup(target) {
                                self.clear_image_search_capture_state();
                            }
                            // Process selected region
                            match target {
                                VisionCaptureTarget::Preset(preset_id) => {
                                    let template_mode = matches!(mode, VisionCaptureMode::Template);
                                    self.finish_image_search_region_capture_command(
                                        ctx,
                                        preset_id,
                                        template_mode,
                                        x,
                                        y,
                                        width,
                                        height,
                                    );
                                }
                                VisionCaptureTarget::OcrPreset(preset_id) => {
                                    self.finish_ocr_region_capture_command(
                                        ctx, preset_id, x, y, width, height,
                                    );
                                }
                                VisionCaptureTarget::OcrStepRegion {
                                    group_id,
                                    preset_id,
                                    step_index,
                                } => {
                                    self.finish_ocr_step_region_capture_command(
                                        ctx, group_id, preset_id, step_index, x, y, width, height,
                                    );
                                }
                                VisionCaptureTarget::PinPresetRegion(preset_id) => {
                                    if let Some(preset) = self
                                        .state
                                        .pin_presets
                                        .iter_mut()
                                        .find(|p| p.id == preset_id)
                                    {
                                        preset.x = x;
                                        preset.y = y;
                                        preset.width = width;
                                        preset.height = height;
                                        preset.use_custom_bounds = true;
                                        self.persist_window_presets();
                                        self.status = format!(
                                            "Saved pinned region {}x{} at {}, {} for preset #{}.",
                                            width, height, x, y, preset_id
                                        );
                                    }
                                }
                                VisionCaptureTarget::PinPresetSourceCrop(preset_id) => {
                                    if let Some(preset) = self
                                        .state
                                        .pin_presets
                                        .iter_mut()
                                        .find(|p| p.id == preset_id)
                                    {
                                        let mut sx = x;
                                        let mut sy = y;
                                        if let Some(cache) =
                                            self.zoom_preview_cache.get(&(preset_id + 100_000))
                                        {
                                            sx -= cache.view.screen_x;
                                            sy -= cache.view.screen_y;
                                        }
                                        preset.source_x = sx;
                                        preset.source_y = sy;
                                        preset.source_width = width;
                                        preset.source_height = height;
                                        preset.source_crop_initialized = true;
                                        preset.source_crop_fit_version = 2;
                                        self.persist_window_presets();
                                        self.status = format!(
                                            "Saved source crop {}x{} at {}, {} for preset #{}.",
                                            width, height, sx, sy, preset_id
                                        );
                                    }
                                }
                                VisionCaptureTarget::HudPresetRegion(preset_id) => {
                                    if let Some(preset) = self
                                        .state
                                        .hud_presets
                                        .iter_mut()
                                        .find(|p| p.id == preset_id)
                                    {
                                        preset.x = x;
                                        preset.y = y;
                                        preset.width = width;
                                        preset.height = height;
                                        self.persist_hud_presets();
                                        self.status = format!(
                                            "Saved HUD region {}x{} at {}, {} for preset #{}.",
                                            width, height, x, y, preset_id
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                        crate::overlay::NativeCaptureResult::AdjustedRegion {
                            x,
                            y,
                            width,
                            height,
                        } => match target {
                            VisionCaptureTarget::Preset(preset_id) => {
                                self.finish_image_search_region_capture_command(
                                    ctx, preset_id, false, x, y, width, height,
                                );
                            }
                            _ => {}
                        },
                        crate::overlay::NativeCaptureResult::ClickedPoint { x, y, color } => {
                            // Process clicked point
                            match target {
                                VisionCaptureTarget::Preset(preset_id) => {
                                    if matches!(mode, VisionCaptureMode::SinglePixel) {
                                        self.finish_image_search_single_pixel_capture_from_screen(
                                            ctx, preset_id, x, y,
                                        );
                                    } else {
                                        let priority_anchor =
                                            matches!(mode, VisionCaptureMode::ColorPriorityAnchor);
                                        self.finish_image_search_point_capture_command(
                                            ctx,
                                            preset_id,
                                            priority_anchor,
                                            x,
                                            y,
                                            color,
                                        );
                                    }
                                }
                                VisionCaptureTarget::GeometryColor
                                | VisionCaptureTarget::CrosshairProfileColor { .. }
                                | VisionCaptureTarget::MacroStepGeometryColor { .. }
                                | VisionCaptureTarget::PinPresetColor(_) => {
                                    if let Some(col) = color {
                                        let status =
                                            self.apply_image_search_color_pick(target, col);
                                        self.status = status;
                                    }
                                    self.clear_image_search_capture_state();
                                }
                                VisionCaptureTarget::QuickActionsCoordinates => {
                                    self.clear_image_search_capture_state();
                                    let copy_x = self.state.quick_actions_copy_x;
                                    let copy_y = self.state.quick_actions_copy_y;
                                    let mut parts = Vec::new();
                                    if copy_x {
                                        parts.push(x.to_string());
                                    }
                                    if copy_y {
                                        parts.push(y.to_string());
                                    }
                                    let formatted = parts.join(", ");
                                    if !formatted.is_empty() {
                                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                            let _ = clipboard.set_text(formatted.clone());
                                        }
                                        self.status = match self.state.ui_language {
                                            crate::model::UiLanguage::Vietnamese => format!(
                                                "Da sao chep toa do vao clipboard: {}",
                                                formatted
                                            ),
                                            _ => format!(
                                                "Coordinates copied to clipboard: {}",
                                                formatted
                                            ),
                                        };
                                    } else {
                                        self.status = match self.state.ui_language {
                                            crate::model::UiLanguage::Vietnamese => {
                                                format!("Toa do da chon: X={}, Y={}", x, y)
                                            }
                                            _ => format!("Coordinates captured: X={}, Y={}", x, y),
                                        };
                                    }
                                }
                                VisionCaptureTarget::QuickActionsColor => {
                                    self.clear_image_search_capture_state();
                                    if let Some(col) = color {
                                        let hex_str =
                                            format!("#{:02X}{:02X}{:02X}", col.r, col.g, col.b);
                                        if self.state.quick_actions_copy_color {
                                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                                let _ = clipboard.set_text(hex_str.clone());
                                            }
                                            self.status = match self.state.ui_language {
                                                crate::model::UiLanguage::Vietnamese => format!(
                                                    "Da sao chep ma mau vao clipboard: {}",
                                                    hex_str
                                                ),
                                                _ => format!(
                                                    "Color code copied to clipboard: {}",
                                                    hex_str
                                                ),
                                            };
                                        } else {
                                            self.status = match self.state.ui_language {
                                                crate::model::UiLanguage::Vietnamese => {
                                                    format!("Mau da chon: {}", hex_str)
                                                }
                                                _ => format!("Color captured: {}", hex_str),
                                            };
                                        }
                                    } else {
                                        self.status = Self::tr_lang(
                                            self.state.ui_language,
                                            "Failed to capture screen color.",
                                            "Failed to capture screen color.",
                                        )
                                        .to_owned();
                                    }
                                }
                                VisionCaptureTarget::QuickActionsKeyDisplayPosition => {
                                    self.clear_image_search_capture_state();
                                    self.state.quick_key_display_x = x;
                                    self.state.quick_key_display_y = y;
                                    self.sync_quick_key_display_config();
                                    self.persist();
                                    self.status = match self.state.ui_language {
                                        crate::model::UiLanguage::Vietnamese => {
                                            format!("Da dat vi tri hien thi phim: X={}, Y={}", x, y)
                                        }
                                        _ => {
                                            format!("Key display position set: X={}, Y={}", x, y)
                                        }
                                    };
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                    ctx.request_repaint();
                }
                UiCommand::NativeProtractorCalibrationFinished {
                    result,
                    was_minimized,
                } => {
                    self.native_capture_in_progress = false;

                    match result {
                        crate::overlay::NativeCaptureResult::ProtractorPoints(points) => {
                            self.finish_protractor_calibration_freeze(ctx, points);
                        }
                        _ => {
                            self.protractor_picking_active = false;
                            self.protractor_calibration_points = None;
                            self.status = Self::tr_lang(
                                self.state.ui_language,
                                "Protractor calibration cancelled.",
                                "Protractor calibration cancelled.",
                            )
                            .to_owned();
                            self.sync_protractor_state();
                        }
                    }

                    // Conditionally minimize or restore the main window
                    self.state.show_window = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(was_minimized));
                    if !was_minimized {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }

                    // Restore overlay visibility (was hidden before capture)
                    let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(true));
                    crate::overlay::wake_command_queue();
                    ctx.request_repaint();
                }
                UiCommand::NativeDistanceMeasurementFinished {
                    result,
                    was_minimized,
                } => {
                    self.distance_measurement_active = false;
                    self.native_capture_in_progress = false;

                    match result {
                        crate::overlay::NativeCaptureResult::DistancePoints(points)
                            if points.len() >= 2 =>
                        {
                            let (ax, ay) = points[0];
                            let (bx, by) = points[1];
                            let dx = (bx - ax) as f64;
                            let dy = (by - ay) as f64;
                            let distance = dx.hypot(dy);
                            crate::overlay::set_variable_value("RulerDistance", distance);
                            crate::overlay::set_variable_value("RulerStartX", ax as f64);
                            crate::overlay::set_variable_value("RulerStartY", ay as f64);
                            crate::overlay::set_variable_value("RulerEndX", bx as f64);
                            crate::overlay::set_variable_value("RulerEndY", by as f64);
                            let formatted = format!("{distance:.2}");
                            if self.state.quick_actions_copy_ruler {
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    let _ = clipboard.set_text(formatted.clone());
                                }
                            }
                            self.status = match self.state.ui_language {
                                crate::model::UiLanguage::Vietnamese => {
                                    if self.state.quick_actions_copy_ruler {
                                        format!(
                                            "Da do khoang cach A->B: {:.2}px. Da copy vao clipboard. Bien: RulerDistance",
                                            distance
                                        )
                                    } else {
                                        format!(
                                            "Da do khoang cach A->B: {:.2}px. Bien: RulerDistance",
                                            distance
                                        )
                                    }
                                }
                                _ => {
                                    if self.state.quick_actions_copy_ruler {
                                        format!(
                                            "Measured A->B distance: {:.2}px. Copied to clipboard. Variable: RulerDistance",
                                            distance
                                        )
                                    } else {
                                        format!(
                                            "Measured A->B distance: {:.2}px. Variable: RulerDistance",
                                            distance
                                        )
                                    }
                                }
                            };
                        }
                        _ => {
                            self.status = Self::tr_lang(
                                self.state.ui_language,
                                "Ruler capture cancelled.",
                                "Ruler capture cancelled.",
                            )
                            .to_owned();
                        }
                    }

                    self.state.show_window = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(was_minimized));
                    if !was_minimized {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }

                    let _ = self.overlay_tx.send(OverlayCommand::SetUiVisible(true));
                    crate::overlay::wake_command_queue();
                    ctx.request_repaint();
                }
                UiCommand::NativeMouseMoveAbsoluteCaptureFinished {
                    target,
                    result,
                    capture_frame,
                } => {
                    // Show main window natively
                    #[cfg(windows)]
                    unsafe {
                        if let Some(hwnd) = crate::overlay::find_app_ui_window_for_ui_thread() {
                            use windows::Win32::UI::WindowsAndMessaging::{
                                SW_SHOWNORMAL, ShowWindow,
                            };
                            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
                        }
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);

                    self.native_capture_in_progress = false;
                    self.captured_freeze_frame = capture_frame;

                    match result {
                        crate::overlay::NativeCaptureResult::ClickedPoint { x, y, color } => {
                            self.finish_mouse_move_absolute_capture(ctx, target, x, y, color);
                        }
                        _ => {
                            self.mouse_move_absolute_capture_target = None;
                            self.status = Self::tr_lang(
                                self.state.ui_language,
                                "Absolute coordinate capture cancelled.",
                                "Absolute coordinate capture cancelled.",
                            )
                            .to_owned();
                        }
                    }
                    ctx.request_repaint();
                }
                UiCommand::UpdateCheckStarted => {
                    self.update_status = UpdateStatus::Checking;
                }
                UiCommand::UpdateAvailable(version, body, url) => {
                    if self.update_check_was_automatic {
                        let message = match self.state.ui_language {
                            UiLanguage::Vietnamese => format!("Đã có phiên bản mới v{version}."),
                            UiLanguage::English | UiLanguage::Icon => {
                                format!("New version v{version} is available.")
                            }
                        };
                        self.show_update_notice(message);
                    }
                    self.update_status = UpdateStatus::Available(version, body, url);
                    self.update_check_was_automatic = false;
                }
                UiCommand::UpdateDownloadStarted => {
                    self.update_status = UpdateStatus::Downloading;
                }
                UiCommand::UpdateDownloadFinished(new_exe_path) => {
                    self.update_status = UpdateStatus::ReadyToRestart(new_exe_path);
                    self.update_check_was_automatic = false;
                }
                UiCommand::UpdateError(e) => {
                    self.update_status = UpdateStatus::Error(e);
                    self.update_check_was_automatic = false;
                }
                UiCommand::UpdateUpToDate => {
                    self.update_status = UpdateStatus::UpToDate;
                    self.update_check_was_automatic = false;
                }
                UiCommand::SetInterceptionStatus(status) => {
                    self.interception_status = status;
                }
                UiCommand::CustomCommandResult { preset_id, output } => {
                    if let Some(preset) = self
                        .state
                        .command_presets
                        .iter_mut()
                        .find(|p| p.id == preset_id)
                    {
                        preset.run_output = Some(output);
                    } else {
                        self.status = output;
                    }
                    ctx.request_repaint();
                }
                UiCommand::WindowPreviewLoaded {
                    cache_id,
                    source_window_key,
                    source_window_extra_keys,
                    match_duplicate_window_titles,
                    frame,
                } => {
                    self.window_preview_loading.remove(&cache_id);
                    let Some(frame) = frame else {
                        continue;
                    };
                    let filtered_image = if cache_id >= 100_000 {
                        let preset_id = cache_id - 100_000;
                        if let Some(preset) = self.pin_preset(preset_id) {
                            if preset.binary_filter {
                                let mut filtered_rgba = frame.rgba.clone();
                                let threshold = preset.binary_threshold;
                                let threshold_sq = (threshold as i32).pow(2);
                                let binary_mode = preset.binary_mode;
                                let transparent_black = preset.binary_transparent_black;
                                let transparent_white = preset.binary_transparent_white;
                                let target_colors = preset.binary_target_colors();
                                let single_target_color = preset.binary_target_color;

                                for chunk in filtered_rgba.chunks_exact_mut(4) {
                                    let r = chunk[0];
                                    let g = chunk[1];
                                    let b = chunk[2];
                                    let a = chunk[3];

                                    let val = match binary_mode {
                                        crate::model::PinBinaryMode::Grayscale => {
                                            let gray =
                                                ((r as u32 * 299 + g as u32 * 587 + b as u32 * 114)
                                                    / 1000)
                                                    as u8;
                                            if gray >= threshold { 255 } else { 0 }
                                        }
                                        crate::model::PinBinaryMode::ColorSimilarity => {
                                            let matched = if target_colors.is_empty() {
                                                single_target_color.is_some_and(|target_color| {
                                                    let dist_sq = (r as i32
                                                        - target_color.r as i32)
                                                        .pow(2)
                                                        + (g as i32 - target_color.g as i32).pow(2)
                                                        + (b as i32 - target_color.b as i32).pow(2);
                                                    dist_sq <= threshold_sq
                                                })
                                            } else {
                                                target_colors.iter().any(|target_color| {
                                                    let dist_sq = (r as i32
                                                        - target_color.r as i32)
                                                        .pow(2)
                                                        + (g as i32 - target_color.g as i32).pow(2)
                                                        + (b as i32 - target_color.b as i32).pow(2);
                                                    dist_sq <= threshold_sq
                                                })
                                            };
                                            if matched { 255 } else { 0 }
                                        }
                                    };

                                    chunk[0] = val;
                                    chunk[1] = val;
                                    chunk[2] = val;
                                    chunk[3] = if transparent_black && !transparent_white {
                                        if val == 0 { 0 } else { 255 }
                                    } else if transparent_white && !transparent_black {
                                        if val == 255 { 0 } else { 255 }
                                    } else {
                                        a
                                    };
                                }
                                Some(ColorImage::from_rgba_unmultiplied(
                                    [frame.width, frame.height],
                                    &filtered_rgba,
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let image = ColorImage::from_rgba_unmultiplied(
                        [frame.width, frame.height],
                        &frame.rgba,
                    );
                    if let Some(cache) = self.zoom_preview_cache.get_mut(&cache_id) {
                        cache.view.texture.set(image, TextureOptions::LINEAR);
                        match filtered_image {
                            Some(filtered_image) => {
                                if let Some(texture) = cache.view.filtered_texture.as_mut() {
                                    texture.set(filtered_image, TextureOptions::LINEAR);
                                } else {
                                    cache.view.filtered_texture = Some(ctx.load_texture(
                                        format!("window-preview-{cache_id}-filtered"),
                                        filtered_image,
                                        TextureOptions::LINEAR,
                                    ));
                                }
                            }
                            None => {
                                cache.view.filtered_texture = None;
                            }
                        }
                        cache.updated_at = Instant::now();
                        cache.source_window_key = source_window_key;
                        cache.source_window_extra_keys = source_window_extra_keys;
                        cache.match_duplicate_window_titles = match_duplicate_window_titles;
                        cache.view.title = frame.title;
                        cache.view.screen_x = frame.screen_x;
                        cache.view.screen_y = frame.screen_y;
                        cache.view.logical_width = frame.logical_width;
                        cache.view.logical_height = frame.logical_height;
                    } else {
                        let texture = ctx.load_texture(
                            format!("window-preview-{cache_id}"),
                            image,
                            TextureOptions::LINEAR,
                        );
                        let view = ZoomPreviewView {
                            texture,
                            filtered_texture: filtered_image.map(|image| {
                                ctx.load_texture(
                                    format!("window-preview-{cache_id}-filtered"),
                                    image,
                                    TextureOptions::LINEAR,
                                )
                            }),
                            title: frame.title,
                            screen_x: frame.screen_x,
                            screen_y: frame.screen_y,
                            logical_width: frame.logical_width,
                            logical_height: frame.logical_height,
                        };
                        self.zoom_preview_cache.insert(
                            cache_id,
                            ZoomPreviewCache {
                                updated_at: Instant::now(),
                                source_window_key,
                                source_window_extra_keys,
                                match_duplicate_window_titles,
                                view,
                            },
                        );
                    }
                    ctx.request_repaint();
                }
                UiCommand::AudioWaveformLoaded {
                    path,
                    waveform,
                    duration_ms,
                } => {
                    if !self.audio_path_is_referenced(&path) {
                        continue;
                    }
                    self.audio_waveforms.insert(path.clone(), waveform);
                    self.update_audio_clip_duration_for_path(&path, duration_ms);
                    ctx.request_repaint();
                }
                UiCommand::OpenWindowsLoaded { windows, status } => {
                    self.open_window_infos = windows;
                    self.sync_quick_action_window_selection();
                    self.open_windows_loaded_once = true;
                    self.open_windows_loading = false;
                    self.last_window_refresh_at = Instant::now();
                    if let Some(status) = status {
                        self.status = status;
                    }
                    ctx.request_repaint();
                }
                UiCommand::AudioSenseDevicesLoaded { devices } => {
                    self.audio_sense_devices = devices;
                    self.audio_sense_devices_loaded_once = true;
                    self.audio_sense_devices_loading = false;
                    self.last_audio_sense_devices_refresh_at = Instant::now();
                    ctx.request_repaint();
                }
                UiCommand::PersistFailed(error) => {
                    self.status = error;
                    ctx.request_repaint();
                }

                UiCommand::SetProtractorEnabled(enabled) => {
                    self.state.protractor_enabled = enabled;
                    if enabled {
                        let (left, top, w, h) = crate::window_list::virtual_screen_bounds();
                        self.state.protractor_center_x = left + w / 2;
                        self.state.protractor_center_y = top + h / 2;
                        self.state.protractor_scale = 1.0;
                        self.state.protractor_needle1_angle = 0.0;
                        self.state.protractor_needle2_angle = 90.0;
                    }
                    self.sync_protractor_state();
                    self.persist();
                    ctx.request_repaint();
                }
                UiCommand::RequestProtractorCalibration { was_minimized } => {
                    if !self.protractor_picking_active {
                        self.begin_protractor_calibration(ctx, was_minimized);
                    }
                }
                UiCommand::UpdateProtractorConfig {
                    scale,
                    needle1_angle,
                    needle2_angle,
                    center_x,
                    center_y,
                    thickness,
                } => {
                    self.state.protractor_scale = scale;
                    self.state.protractor_needle1_angle = needle1_angle;
                    self.state.protractor_needle2_angle = needle2_angle;
                    self.state.protractor_center_x = center_x;
                    self.state.protractor_center_y = center_y;
                    self.state.protractor_thickness = thickness;
                    self.persist();
                }
            }
        }
        if !self.ui_rx.is_empty() {
            ctx.request_repaint();
        }

        if let Some(job) = &self.opencv_download_job {
            if job.is_finished() {
                let job = self.opencv_download_job.take().unwrap();
                match job.join() {
                    Ok(Ok(())) => {
                        self.opencv_installed = true;
                        self.status = Self::tr_lang(
                            self.state.ui_language,
                            "OpenCV installed successfully.",
                            "OpenCV installed successfully.",
                        )
                        .to_owned();
                    }
                    Ok(Err(error)) => {
                        self.status = format!("Download failed: {error}");
                        let _ = fs::remove_file(&self.paths.opencv_dll);
                    }
                    Err(_) => {
                        self.status = "Download thread panicked.".to_owned();
                    }
                }
            }
        }

        if let Some(job) = &self.ffmpeg_download_job
            && job.is_finished()
        {
            let job = self.ffmpeg_download_job.take().unwrap();
            match job.join() {
                Ok(Ok(())) => {
                    self.ffmpeg_installed = true;
                    self.sync_quick_video_record_config();
                    self.status = Self::tr_lang(
                        self.state.ui_language,
                        "Screen recorder installed successfully.",
                        "Đã cài công cụ quay màn hình.",
                    )
                    .to_owned();
                }
                Ok(Err(error)) => {
                    self.status = format!("Recorder download failed: {error}");
                    let _ = fs::remove_file(&self.paths.ffmpeg_exe);
                    let _ = fs::remove_file(&self.paths.ffmpeg_zip);
                }
                Err(_) => {
                    self.status = "Recorder download thread panicked.".to_owned();
                }
            }
        }

        if let Some(job) = &self.frida_download_job
            && job.is_finished()
        {
            let job = self.frida_download_job.take().unwrap();
            match job.join() {
                Ok(Ok(())) => {
                    self.frida_installed = true;
                    self.status = Self::tr_lang(
                        self.state.ui_language,
                        "Frida tool installed successfully.",
                        "Đã cài đặt công cụ Frida.",
                    )
                    .to_owned();
                }
                Ok(Err(error)) => {
                    self.status = format!("Frida download failed: {error}");
                    let _ = fs::remove_file(&self.paths.frida_helper_exe);
                    let _ = fs::remove_file(&self.paths.frida_helper_zip);
                }
                Err(_) => {
                    self.status = "Frida download thread panicked.".to_owned();
                }
            }
        }

        if let Some(job) = &self.ocr_download_job {
            if job.is_finished() {
                let job = self.ocr_download_job.take().unwrap();
                match job.join() {
                    Ok(Ok(())) => {
                        self.status = "OCR packs installed.".to_owned();
                    }
                    Ok(Err(error)) => {
                        self.status = format!("OCR install failed: {error}");
                    }
                    Err(_) => {
                        self.status = "OCR download thread panicked.".to_owned();
                    }
                }
            }
        }

        self.poll_mouse_tool_jobs();

        if self.ffmpeg_download_job.is_some()
            || self.frida_download_job.is_some()
            || self.arduino_download_job.is_some()
            || self.interception_download_job.is_some()
            || self.interception_install_job.is_some()
            || self.interception_uninstall_job.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(33));
        }

        self.poll_custom_ai_generation(ctx);

        if self.command_ai_job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }

        if self.state.active_panel != self.last_active_panel {
            if Self::active_panel_needs_open_windows(self.state.active_panel) {
                self.ensure_open_windows_ready(false);
            }
            if Self::active_panel_needs_audio_sense_devices(self.state.active_panel) {
                self.ensure_audio_sense_devices_ready(false);
            }

            self.last_active_panel = self.state.active_panel;
        }

        let viewport_focused = ctx.input(|input| input.viewport().focused != Some(false));
        let keep_pin_preview = viewport_focused && self.state.active_panel == AppPanel::Pin;
        if !keep_pin_preview && self.disable_pin_preview_modes() {
            self.persist();
        }
        let keep_toolbox_preview = viewport_focused
            && (self.state.active_panel == AppPanel::Hud
                || self.state.active_panel == AppPanel::Timer);
        let mut hud_changed = false;
        if !keep_toolbox_preview {
            hud_changed |= self.disable_hud_preview_modes();
            hud_changed |= self.disable_timer_preview_modes();
        }
        if hud_changed {
            self.persist();
        }
        let keep_window_preset_preview =
            viewport_focused && self.state.active_panel == AppPanel::WindowPresets;
        if !keep_window_preset_preview && self.disable_window_presets_preview_modes() {
            self.persist();
        }
        let keep_ocr_preview = viewport_focused && self.state.active_panel == AppPanel::Ocr;
        if !keep_ocr_preview && self.disable_ocr_preview_modes() {
            self.persist();
        }
        let keep_vision_preview = viewport_focused && self.state.active_panel == AppPanel::Vision;
        if !keep_vision_preview {
            let mut changed = false;
            for preset in &mut self.state.vision_presets {
                if preset.show_search_region_overlay {
                    preset.show_search_region_overlay = false;
                    changed = true;
                }
            }
            if changed {
                self.persist_vision_presets();
            }
        }
        let keep_geometry_preview =
            viewport_focused && self.state.active_panel == AppPanel::Geometry;
        if !keep_geometry_preview
            && (self.geometry_preview_target.is_some()
                || self.geometry_preset_preview_target.is_some())
        {
            self.clear_geometry_spec_preview();
            self.clear_geometry_preset_preview();
        }

        let keep_macro_geometry_preview =
            viewport_focused && self.state.active_panel == AppPanel::Macros;
        if !keep_macro_geometry_preview
            && (self.draw_geometry_step_preview_target.is_some()
                || self.show_geometry_preset_preview_target.is_some())
        {
            self.draw_geometry_step_preview_target = None;
            self.draw_geometry_step_preview_sent = None;
            self.show_geometry_preset_preview_target = None;
            self.show_geometry_preset_preview_sent = None;
            self.clear_geometry_spec_preview();
            self.clear_geometry_preset_preview();
        } else if let Some((group_id, preset_id, step_index, is_hold_stop)) =
            self.draw_geometry_step_preview_target
        {
            let preview_spec = self
                .macro_preset(group_id, preset_id)
                .and_then(|p| {
                    if is_hold_stop {
                        Some(&*p.hold_stop_step)
                    } else {
                        p.steps.get(step_index)
                    }
                })
                .and_then(|step| {
                    if step.action == crate::model::MacroAction::DrawGeometry {
                        Some(step.geometry_spec.clone())
                    } else {
                        None
                    }
                });

            if preview_spec.is_none() {
                self.draw_geometry_step_preview_target = None;
                self.draw_geometry_step_preview_sent = None;
            }

            if self.draw_geometry_step_preview_sent != preview_spec {
                self.draw_geometry_step_preview_sent = preview_spec.clone();
                self.sync_geometry_spec_preview(preview_spec);
            }
        }
        if keep_macro_geometry_preview {
            if let Some((group_id, preset_id, step_index, is_hold_stop)) =
                self.show_geometry_preset_preview_target
            {
                let preview_preset_id = self
                    .macro_preset(group_id, preset_id)
                    .and_then(|p| {
                        if is_hold_stop {
                            Some(&*p.hold_stop_step)
                        } else {
                            p.steps.get(step_index)
                        }
                    })
                    .and_then(|step| {
                        if step.action == crate::model::MacroAction::ShowGeometryPreset {
                            Self::resolve_geometry_preset_preview_id(
                                step,
                                &self.state.geometry_presets,
                            )
                        } else {
                            None
                        }
                    });

                if self.show_geometry_preset_preview_sent != Some(preview_preset_id) {
                    self.show_geometry_preset_preview_sent = Some(preview_preset_id);
                    self.sync_geometry_preset_preview(preview_preset_id);
                }
            }
        }

        let drawing_active = crate::overlay::screen_draw_active();
        let capturing_region = crate::overlay::screen_draw_get_capturing_region();
        let trigger_pending_from_inactive =
            crate::overlay::screen_draw_trigger_pending_from_inactive();
        let color_pick_mode = crate::overlay::screen_draw_get_color_pick_mode();
        let crosshair_draw_mode = crate::overlay::screen_draw_is_crosshair_draw();
        if drawing_active {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        let mut color_pick_pending = self.screen_draw_color_pick_pending_at.is_some();
        if drawing_active {
            if color_pick_mode {
                self.screen_draw_color_pick_pending_at = None;
                color_pick_pending = false;
            } else if let Some(pending_at) = self.screen_draw_color_pick_pending_at {
                const SCREEN_DRAW_COLOR_PICK_PENDING_RESET_MS: u64 = 450;
                if pending_at.elapsed()
                    >= Duration::from_millis(SCREEN_DRAW_COLOR_PICK_PENDING_RESET_MS)
                {
                    self.screen_draw_color_pick_pending_at = None;
                    color_pick_pending = false;
                } else {
                    ctx.send_viewport_cmd_to(
                        egui::ViewportId::from_hash_of("screen_draw_toolbar"),
                        egui::ViewportCommand::Visible(false),
                    );
                    ctx.request_repaint_after(Duration::from_millis(16));
                }
            }
        } else {
            self.screen_draw_color_pick_pending_at = None;
            color_pick_pending = false;
        }
        let toolbar_visible = !capturing_region
            && !trigger_pending_from_inactive
            && (!color_pick_mode || crosshair_draw_mode)
            && !(color_pick_pending && !crosshair_draw_mode);
        let was_active = ctx.data(|d| {
            d.get_temp::<bool>(egui::Id::new("screen_draw_active"))
                .unwrap_or(false)
        });
        if drawing_active != was_active {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("screen_draw_active"), drawing_active));
            if drawing_active {
                if !self.state.show_window {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                        -10000.0, -10000.0,
                    )));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::new("main_window_hidden_for_drawing"), true)
                    });
                }
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("screen_draw_capturing"), capturing_region);
                    d.insert_temp(
                        egui::Id::new("screen_draw_color_pick_mode"),
                        color_pick_mode,
                    );
                });
            } else {
                let hidden_for_drawing = ctx.data(|d| {
                    d.get_temp::<bool>(egui::Id::new("main_window_hidden_for_drawing"))
                        .unwrap_or(false)
                });
                if hidden_for_drawing {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::new("main_window_hidden_for_drawing"), false)
                    });
                }
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::from_hash_of("screen_draw_toolbar"),
                    egui::ViewportCommand::Visible(false),
                );
                // Reset toolbar positioning for the next draw session.
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("toolbar_inited"), false);
                    d.insert_temp(egui::Id::new("screen_draw_toolbar_ready"), false);
                });
            }
        } else if drawing_active {
            let was_capturing = ctx.data(|d| {
                d.get_temp::<bool>(egui::Id::new("screen_draw_capturing"))
                    .unwrap_or(false)
            });
            let was_color_pick_mode = ctx.data(|d| {
                d.get_temp::<bool>(egui::Id::new("screen_draw_color_pick_mode"))
                    .unwrap_or(false)
            });
            if capturing_region != was_capturing || color_pick_mode != was_color_pick_mode {
                if toolbar_visible {
                    let toolbar_pos =
                        ctx.data(|d| d.get_temp::<egui::Pos2>(egui::Id::new("toolbar_pos")));
                    if let Some(toolbar_pos) = toolbar_pos {
                        ctx.send_viewport_cmd_to(
                            egui::ViewportId::from_hash_of("screen_draw_toolbar"),
                            egui::ViewportCommand::OuterPosition(toolbar_pos),
                        );
                    }
                }
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("screen_draw_capturing"), capturing_region);
                    d.insert_temp(
                        egui::Id::new("screen_draw_color_pick_mode"),
                        color_pick_mode,
                    );
                });
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::from_hash_of("screen_draw_toolbar"),
                    egui::ViewportCommand::Visible(toolbar_visible),
                );
                if toolbar_visible {
                    ctx.request_repaint();
                }
            }
        }

        if drawing_active {
            let (screen_x, screen_y, screen_w, screen_h) =
                crate::window_list::virtual_screen_bounds();
            const TOOLBAR_ESTIMATED_WIDTH: f32 = 780.0;
            const TOOLBAR_HEIGHT: f32 = 44.0;
            let toolbar_width = ctx.data(|d| {
                d.get_temp::<f32>(egui::Id::new("toolbar_width"))
                    .unwrap_or(TOOLBAR_ESTIMATED_WIDTH)
            });
            let default_x = screen_x as f32 + (screen_w as f32 - toolbar_width) / 2.0;
            let default_y = screen_y as f32 + 60.0;
            let default_pos = egui::pos2(default_x, default_y);

            // Only set position on first init. Use a persistent flag so we don't reset every frame.
            let toolbar_inited = ctx.data(|d| {
                d.get_temp::<bool>(egui::Id::new("toolbar_inited"))
                    .unwrap_or(false)
            });
            if !toolbar_inited {
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("toolbar_inited"), true);
                    d.insert_temp(egui::Id::new("toolbar_pos"), default_pos);
                });
            }

            let toolbar_pos = ctx.data(|d| {
                d.get_temp::<egui::Pos2>(egui::Id::new("toolbar_pos"))
                    .unwrap_or(default_pos)
            });
            let toolbar_ready = ctx.data(|d| {
                d.get_temp::<bool>(egui::Id::new("screen_draw_toolbar_ready"))
                    .unwrap_or(false)
            });

            let toolbar_width_px = toolbar_width.round() as i32;
            let toolbar_height = TOOLBAR_HEIGHT as i32;

            crate::overlay::screen_draw_set_toolbar_rect(
                (toolbar_pos.x - screen_x as f32) as i32,
                (toolbar_pos.y - screen_y as f32) as i32,
                toolbar_width_px,
                toolbar_height,
            );

            // Build without position every frame – only set on first init
            let mut builder = egui::ViewportBuilder::default()
                .with_title("Drawing Toolbar")
                .with_inner_size(egui::vec2(toolbar_width, TOOLBAR_HEIGHT))
                .with_visible(toolbar_visible && toolbar_ready)
                .with_active(false)
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_resizable(false);
            if !toolbar_inited {
                builder = builder.with_position(toolbar_pos);
            }

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("screen_draw_toolbar"),
                builder,
                |ctx, class| {
                    #[cfg(windows)]
                    {
                        // Apply WS_EX_NOACTIVATE to the toolbar viewport window (found by title)
                        // so clicking toolbar buttons doesn't steal focus from the drawing canvas.
                        // eframe can refresh native styles after viewport updates, so keep
                        // NOACTIVATE asserted while the toolbar exists.
                        if !crate::platform::make_window_title_no_activate("Drawing Toolbar") {
                            ctx.request_repaint_after(Duration::from_millis(16));
                        }
                    }
                    if class == egui::ViewportClass::Immediate {
                        let measured_width = egui::CentralPanel::default()
                            .frame(egui::Frame::none()
                                .fill(egui::Color32::from_rgba_unmultiplied(24, 28, 36, 255))
                                .corner_radius(8.0)
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(220, 232, 248, 40)))
                                .inner_margin(egui::Margin::symmetric(12, 8))
                            )
                            .show(ctx, |ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.horizontal(|ui| {
                                    // 1. Drag Handle - uses custom smooth manual dragging on Windows
                                    let drag_btn = ui.add(
                                        egui::Button::new(":::")
                                            .frame(false)
                                            .min_size(egui::vec2(18.0, 22.0))
                                            .sense(egui::Sense::drag())
                                    );
                                    if drag_btn.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                    }
                                    #[cfg(windows)]
                                    {
                                        if drag_btn.drag_started() {
                                            ui.ctx().memory_mut(|memory| memory.stop_text_input());
                                            crate::overlay::screen_draw_toolbar_interacted_from("drag_handle");
                                            let mut pt = POINT::default();
                                            if unsafe { GetCursorPos(&mut pt).is_ok() } {
                                                ui.ctx().data_mut(|d| {
                                                    d.insert_temp(egui::Id::new("toolbar_drag_start_mouse"), egui::pos2(pt.x as f32, pt.y as f32));
                                                    let current_pos = d.get_temp::<egui::Pos2>(egui::Id::new("toolbar_pos")).unwrap_or(default_pos);
                                                    d.insert_temp(egui::Id::new("toolbar_drag_start_window"), current_pos);
                                                    d.insert_temp(egui::Id::new("toolbar_dragging"), true);
                                                });
                                            }
                                        }
                                        if drag_btn.dragged() {
                                            let mut pt = POINT::default();
                                            if unsafe { GetCursorPos(&mut pt).is_ok() } {
                                                let (start_mouse, start_window) = ui.ctx().data(|d| {
                                                    (
                                                        d.get_temp::<egui::Pos2>(egui::Id::new("toolbar_drag_start_mouse")),
                                                        d.get_temp::<egui::Pos2>(egui::Id::new("toolbar_drag_start_window")),
                                                    )
                                                });
                                                if let (Some(start_m), Some(start_w)) = (start_mouse, start_window) {
                                                    let delta = egui::vec2(pt.x as f32 - start_m.x, pt.y as f32 - start_m.y);
                                                    let new_pos = start_w + delta;
                                                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::OuterPosition(new_pos));
                                                    ui.ctx().data_mut(|d| {
                                                        d.insert_temp(egui::Id::new("toolbar_pos"), new_pos);
                                                    });
                                                    crate::overlay::screen_draw_set_toolbar_rect(
                                                        (new_pos.x - screen_x as f32) as i32,
                                                        (new_pos.y - screen_y as f32) as i32,
                                                        toolbar_width_px,
                                                        toolbar_height,
                                                    );
                                                }
                                            }
                                        }
                                        if drag_btn.drag_stopped() {
                                            ui.ctx().data_mut(|d| {
                                                d.insert_temp(egui::Id::new("toolbar_dragging"), false);
                                            });
                                        }
                                    }
                                    #[cfg(not(windows))]
                                    {
                                        if drag_btn.drag_started() {
                                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                                        }
                                    }

                                    ui.add_space(4.0);

                                    // Helper function for rendering toolbar icons
                                    fn draw_toolbar_icon(painter: &egui::Painter, rect: egui::Rect, icon_type: &str, color: egui::Color32) {
                                        let pad = 5.0;
                                        let stroke = egui::Stroke::new(1.8, color);
                                        let center = rect.center();
                                        match icon_type {
                                            "brush" => {
                                                // Simple filled circle
                                                painter.circle_filled(center, 4.5, color);
                                            }
                                            "line" => {
                                                let start = rect.left_bottom() + egui::vec2(pad, -pad);
                                                let end = rect.right_top() + egui::vec2(-pad, pad);
                                                painter.line_segment([start, end], stroke);
                                            }
                                            "arrow" => {
                                                let start = rect.left_bottom() + egui::vec2(pad, -pad);
                                                let end = rect.right_top() + egui::vec2(-pad - 1.0, pad + 1.0);
                                                painter.line_segment([start, end], stroke);
                                                let dir = (end - start).normalized();
                                                let rot = egui::vec2(-dir.y, dir.x);
                                                painter.line_segment([end, end - dir * 4.5 + rot * 2.5], stroke);
                                                painter.line_segment([end, end - dir * 4.5 - rot * 2.5], stroke);
                                            }
                                            "rect" => {
                                                painter.rect_stroke(rect.shrink(pad), 2.0, stroke, egui::StrokeKind::Inside);
                                            }
                                            "oval" => {
                                                painter.rect_stroke(rect.shrink2(egui::vec2(pad, pad + 2.0)), 5.0, stroke, egui::StrokeKind::Inside);
                                            }
                                            "circle" => {
                                                painter.circle_stroke(center, rect.width() / 2.0 - pad, stroke);
                                            }
                                            "poly" => {
                                                let r = rect.width() / 2.0 - pad;
                                                let pts: Vec<egui::Pos2> = (0..5).map(|i| {
                                                    let angle = (i as f32) * std::f32::consts::TAU / 5.0 - std::f32::consts::FRAC_PI_2;
                                                    center + egui::vec2(angle.cos() * r, angle.sin() * r)
                                                }).collect();
                                                for i in 0..5 {
                                                    painter.line_segment([pts[i], pts[(i + 1) % 5]], stroke);
                                                }
                                            }
                                            "text" => {
                                                let top_left = rect.left_top() + egui::vec2(pad, pad);
                                                let top_right = rect.right_top() + egui::vec2(-pad, pad);
                                                let top_mid = rect.center_top() + egui::vec2(0.0, pad);
                                                let bottom_mid = rect.center_bottom() + egui::vec2(0.0, -pad);
                                                painter.line_segment([top_left, top_right], stroke);
                                                painter.line_segment([top_mid, bottom_mid], stroke);
                                            }
                                            "smooth" => {
                                                let left = rect.left() + pad;
                                                let right = rect.right() - pad;
                                                let width = right - left;
                                                let points = (0..=12).map(|i| {
                                                    let t = i as f32 / 12.0;
                                                    egui::pos2(
                                                        left + width * t,
                                                        center.y + (t * std::f32::consts::TAU).sin() * 3.0,
                                                    )
                                                });
                                                painter.add(egui::Shape::line(points.collect(), stroke));
                                            }
                                            "effect_highlight" => {
                                                let bar = egui::Rect::from_center_size(
                                                    center + egui::vec2(0.0, 3.0),
                                                    egui::vec2(rect.width() - pad * 2.0, 5.0),
                                                );
                                                painter.rect_filled(
                                                    bar,
                                                    1.0,
                                                    egui::Color32::from_rgba_premultiplied(255, 220, 50, 150),
                                                );
                                                painter.line_segment(
                                                    [center + egui::vec2(0.0, -7.0), center + egui::vec2(0.0, -2.0)],
                                                    stroke,
                                                );
                                                painter.line_segment(
                                                    [center + egui::vec2(-5.0, -5.0), center + egui::vec2(-2.0, -2.0)],
                                                    stroke,
                                                );
                                                painter.line_segment(
                                                    [center + egui::vec2(5.0, -5.0), center + egui::vec2(2.0, -2.0)],
                                                    stroke,
                                                );
                                            }
                                            "blur" => {
                                                let body = rect.shrink2(egui::vec2(pad, pad));
                                                let cols = [0.22, 0.5, 0.78];
                                                let rows = [0.28, 0.5, 0.72];
                                                for (ry, row) in rows.iter().enumerate() {
                                                    for (cx, col) in cols.iter().enumerate() {
                                                        let alpha = match (ry, cx) {
                                                            (1, 1) => 0.86,
                                                            (1, _) | (_, 1) => 0.58,
                                                            _ => 0.33,
                                                        };
                                                        painter.circle_filled(
                                                            egui::pos2(
                                                                egui::lerp(body.left()..=body.right(), *col),
                                                                egui::lerp(body.top()..=body.bottom(), *row),
                                                            ),
                                                            if ry == 1 && cx == 1 { 2.0 } else { 1.4 },
                                                            color.linear_multiply(alpha),
                                                        );
                                                    }
                                                }
                                                painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);
                                            }
                                            "eraser" => {
                                                let body = rect.shrink2(egui::vec2(pad + 1.0, pad));
                                                painter.rect_filled(body, 2.0, color.linear_multiply(0.25));
                                                painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);
                                                painter.line_segment([body.left_center(), body.right_center()], stroke);
                                            }
                                            "undo" => {
                                                let arrow_end = rect.left_center() + egui::vec2(pad, 0.0);
                                                let arrow_start = rect.right_center() + egui::vec2(-pad, 0.0);
                                                painter.line_segment([arrow_end, arrow_start], stroke);
                                                painter.line_segment([arrow_end, arrow_end + egui::vec2(4.0, -3.0)], stroke);
                                                painter.line_segment([arrow_end, arrow_end + egui::vec2(4.0, 3.0)], stroke);
                                            }
                                            "redo" => {
                                                let arrow_start = rect.left_center() + egui::vec2(pad, 0.0);
                                                let arrow_end = rect.right_center() + egui::vec2(-pad, 0.0);
                                                painter.line_segment([arrow_start, arrow_end], stroke);
                                                painter.line_segment([arrow_end, arrow_end + egui::vec2(-4.0, -3.0)], stroke);
                                                painter.line_segment([arrow_end, arrow_end + egui::vec2(-4.0, 3.0)], stroke);
                                            }
                                            "clear" => {
                                                // Draw a simple trash bin
                                                let top_y = rect.top() + pad + 2.0;
                                                let bot_y = rect.bottom() - pad;
                                                // lid
                                                painter.line_segment(
                                                    [egui::pos2(rect.left() + pad - 1.0, top_y), egui::pos2(rect.right() - pad + 1.0, top_y)],
                                                    stroke,
                                                );
                                                // lid handle
                                                painter.line_segment(
                                                    [egui::pos2(center.x - 2.0, top_y - 2.0), egui::pos2(center.x + 2.0, top_y - 2.0)],
                                                    stroke,
                                                );
                                                painter.line_segment(
                                                    [egui::pos2(center.x - 2.0, top_y - 2.0), egui::pos2(center.x - 2.0, top_y)],
                                                    stroke,
                                                );
                                                painter.line_segment(
                                                    [egui::pos2(center.x + 2.0, top_y - 2.0), egui::pos2(center.x + 2.0, top_y)],
                                                    stroke,
                                                );
                                                // bucket
                                                let left_x = rect.left() + pad + 1.0;
                                                let right_x = rect.right() - pad - 1.0;
                                                painter.line_segment([egui::pos2(left_x, top_y), egui::pos2(left_x + 1.0, bot_y)], stroke);
                                                painter.line_segment([egui::pos2(right_x, top_y), egui::pos2(right_x - 1.0, bot_y)], stroke);
                                                painter.line_segment([egui::pos2(left_x + 1.0, bot_y), egui::pos2(right_x - 1.0, bot_y)], stroke);
                                            }
                                            "exit" => {
                                                let pad_x = pad + 1.0;
                                                painter.line_segment([rect.left_top() + egui::vec2(pad_x, pad_x), rect.right_bottom() + egui::vec2(-pad_x, -pad_x)], stroke);
                                                painter.line_segment([rect.right_top() + egui::vec2(-pad_x, pad_x), rect.left_bottom() + egui::vec2(pad_x, -pad_x)], stroke);
                                            }
                                            "capture" => {
                                                let body = egui::Rect::from_min_max(
                                                    rect.left_top() + egui::vec2(pad - 0.5, pad + 3.2),
                                                    rect.right_bottom() + egui::vec2(-pad + 0.5, -pad + 0.6),
                                                );
                                                painter.rect_stroke(body, 2.8, stroke, egui::StrokeKind::Inside);
                                                let top = egui::Rect::from_min_max(
                                                    egui::pos2(body.left() + 3.2, body.top() - 2.4),
                                                    egui::pos2(body.left() + 8.4, body.top() + 0.2),
                                                );
                                                painter.rect_filled(top, 1.4, color);
                                                painter.circle_stroke(body.center() + egui::vec2(0.0, 0.2), 3.7, stroke);
                                            }
                                            "dropper" => {
                                                let body_start =
                                                    rect.left_bottom() + egui::vec2(pad + 2.8, -pad - 1.2);
                                                let body_end =
                                                    rect.right_top() + egui::vec2(-pad - 5.2, pad + 5.4);
                                                painter.line_segment(
                                                    [body_start, body_end],
                                                    egui::Stroke::new(3.2, color),
                                                );
                                                painter.line_segment(
                                                    [
                                                        body_start + egui::vec2(1.1, 1.1),
                                                        body_end + egui::vec2(1.1, 1.1),
                                                    ],
                                                    egui::Stroke::new(
                                                        1.0,
                                                        color.linear_multiply(0.55),
                                                    ),
                                                );

                                                let bulb = egui::Rect::from_center_size(
                                                    body_start + egui::vec2(1.2, -0.8),
                                                    egui::vec2(6.0, 6.0),
                                                );
                                                painter.rect_filled(bulb, 1.8, color);

                                                let head_center = body_end + egui::vec2(1.8, -1.8);
                                                let tip_top = head_center + egui::vec2(-1.4, -3.0);
                                                let tip_right = head_center + egui::vec2(3.0, 1.4);
                                                let tip_bottom = head_center + egui::vec2(1.4, 3.0);
                                                let tip_left = head_center + egui::vec2(-3.0, -1.4);
                                                painter.add(egui::Shape::convex_polygon(
                                                    vec![tip_top, tip_right, tip_bottom, tip_left],
                                                    egui::Color32::TRANSPARENT,
                                                    egui::Stroke::new(1.6, color),
                                                ));

                                                painter.circle_filled(
                                                    head_center + egui::vec2(2.2, 2.2),
                                                    1.5,
                                                    egui::Color32::from_rgb(255, 208, 96),
                                                );
                                            }
                                            _ => {}
                                        }
                                    }

                                    // Helper closure for custom icon button instantiation
                                    let mut icon_btn = |ui: &mut egui::Ui, selected: bool, icon_type: &str, tooltip: &str| -> (egui::Response, bool) {
                                        let button_size = egui::vec2(22.0, 22.0);
                                        let (rect, response) = ui.allocate_exact_size(button_size, egui::Sense::click());
                                        let visuals = if selected {
                                            &ui.visuals().widgets.active
                                        } else if response.hovered() {
                                            &ui.visuals().widgets.hovered
                                        } else {
                                            &ui.visuals().widgets.inactive
                                        };
                                        if selected {
                                            ui.painter().rect_filled(rect, 4.0, ui.visuals().selection.bg_fill);
                                        } else if response.hovered() {
                                            ui.painter().rect_filled(rect, 4.0, visuals.bg_fill);
                                        }
                                        let color = if selected {
                                            egui::Color32::WHITE
                                        } else if response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        };
                                        draw_toolbar_icon(ui.painter(), rect, icon_type, color);
                                        let response = response.on_hover_text(tooltip);
                                        let pressed_now = response.is_pointer_button_down_on();
                                        let press_id = response.id.with("press_edge");
                                        let was_pressed = ui.ctx().data(|d| {
                                            d.get_temp::<bool>(press_id).unwrap_or(false)
                                        });
                                        ui.ctx().data_mut(|d| {
                                            if pressed_now {
                                                d.insert_temp(press_id, true);
                                            } else {
                                                d.remove::<bool>(press_id);
                                            }
                                        });
                                        let activated = !was_pressed && (pressed_now || response.clicked());
                                        if activated {
                                            #[cfg(windows)]
                                            {
                                                let _ = crate::platform::make_window_title_no_activate(
                                                    "Drawing Toolbar",
                                                );
                                            }
                                            response.surrender_focus();
                                            ui.ctx().memory_mut(|memory| memory.stop_text_input());
                                            crate::overlay::screen_draw_toolbar_interacted();
                                        }
                                        (response, activated)
                                    };

                                    // 2. Undo / Redo
                                    if icon_btn(ui, false, "undo", "Undo (Ctrl+Z)").1 {
                                        crate::overlay::screen_draw_undo();
                                    }
                                    if icon_btn(ui, false, "redo", "Redo (Ctrl+Shift+Z / Ctrl+Y)").1 {
                                        crate::overlay::screen_draw_redo();
                                    }

                                    ui.separator();

                                    // 3. Tools
                                    let current_tool = crate::overlay::screen_draw_get_tool();
                                    let eraser_active = crate::overlay::screen_draw_get_eraser();

                                    let mut tool_btn = |ui: &mut egui::Ui, tool: crate::model::QuickScreenDrawTool, icon_type: &str, name: &str| {
                                        let selected = current_tool == tool && !eraser_active;
                                        if icon_btn(ui, selected, icon_type, name).1 {
                                            crate::overlay::screen_draw_set_tool(tool);
                                            crate::overlay::screen_draw_set_eraser(false);
                                        }
                                    };

                                    tool_btn(ui, crate::model::QuickScreenDrawTool::Brush, "brush", self.tr("Brush", "Cọ"));
                                    tool_btn(ui, crate::model::QuickScreenDrawTool::Line, "line", self.tr("Line", "Đường"));
                                    tool_btn(ui, crate::model::QuickScreenDrawTool::Arrow, "arrow", self.tr("Arrow", "Mũi tên"));
                                    tool_btn(ui, crate::model::QuickScreenDrawTool::Rectangle, "rect", self.tr("Rectangle", "Hình chữ nhật"));
                                    tool_btn(ui, crate::model::QuickScreenDrawTool::Ellipse, "oval", self.tr("Ellipse", "Elip"));
                                    tool_btn(ui, crate::model::QuickScreenDrawTool::Circle, "circle", self.tr("Circle", "Hình tròn"));
                                    tool_btn(ui, crate::model::QuickScreenDrawTool::Polygon, "poly", self.tr("Polygon", "Đa giác"));
                                    tool_btn(ui, crate::model::QuickScreenDrawTool::Text, "text", self.tr("Text", "Chữ"));
                                    // Eraser
                                    if icon_btn(ui, eraser_active, "eraser", self.tr("Eraser", "Tẩy")).1 {
                                        crate::overlay::screen_draw_set_eraser(!eraser_active);
                                    }
                                    let smoothing = crate::overlay::screen_draw_get_smoothing();
                                    let smoothing_clicked = icon_btn(
                                        ui,
                                        smoothing,
                                        "smooth",
                                        self.tr("Smooth line", "Làm mượt nét"),
                                    )
                                    .1;
                                    if smoothing_clicked {
                                        crate::overlay::screen_draw_set_smoothing(!smoothing);
                                        crate::overlay::screen_draw_toolbar_interacted();
                                    }
                                    let smoothing_enabled =
                                        if smoothing_clicked { !smoothing } else { smoothing };
                                    if smoothing_enabled {
                                        let mut amount =
                                            crate::overlay::screen_draw_get_smoothing_amount();
                                        let amount_response = ui
                                            .add_sized(
                                                [44.0, 20.0],
                                                egui::DragValue::new(&mut amount)
                                                    .range(0.0..=1.0)
                                                    .speed(0.01)
                                                    .fixed_decimals(2),
                                            )
                                            .on_hover_text(self.tr(
                                                "Smoothing amount",
                                                "Mức làm mượt",
                                            ));
                                        if amount_response.changed() {
                                            crate::overlay::screen_draw_set_smoothing_amount(amount);
                                        }
                                    }

                                    ui.separator();

                                    // 4. Color Presets (No clipping popups)
                                    let color_presets = [
                                        (self.tr("Red", "Đỏ"), egui::Color32::from_rgb(255, 80, 80), crate::model::RgbaColor { r: 255, g: 80, b: 80, a: 255 }),
                                        (self.tr("Green", "Xanh lá"), egui::Color32::from_rgb(80, 220, 100), crate::model::RgbaColor { r: 80, g: 220, b: 100, a: 255 }),
                                        (self.tr("Blue", "Xanh dương"), egui::Color32::from_rgb(80, 150, 255), crate::model::RgbaColor { r: 80, g: 150, b: 255, a: 255 }),
                                        (self.tr("Yellow", "Vàng"), egui::Color32::from_rgb(255, 220, 50), crate::model::RgbaColor { r: 255, g: 220, b: 50, a: 255 }),
                                        (self.tr("White", "Trắng"), egui::Color32::WHITE, crate::model::RgbaColor { r: 255, g: 255, b: 255, a: 255 }),
                                    ];
                                    let active_color = crate::overlay::screen_draw_get_color();
                                    for (name, c32, rgba) in color_presets.iter() {
                                        let is_selected = active_color == *rgba;
                                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                                        ui.painter().circle_filled(rect.center(), 7.0, *c32);
                                        if is_selected {
                                            ui.painter().circle_stroke(rect.center(), 9.0, egui::Stroke::new(1.5, egui::Color32::WHITE));
                                        } else if resp.hovered() {
                                            ui.painter().circle_stroke(rect.center(), 9.0, egui::Stroke::new(1.0, egui::Color32::LIGHT_GRAY));
                                        }
                                        if resp.clicked() {
                                            #[cfg(windows)]
                                            {
                                                let _ = crate::platform::make_window_title_no_activate(
                                                    "Drawing Toolbar",
                                                );
                                            }
                                            resp.surrender_focus();
                                            ui.ctx().memory_mut(|memory| memory.stop_text_input());
                                            crate::overlay::screen_draw_set_color(*rgba);
                                            crate::overlay::screen_draw_toolbar_interacted();
                                        }
                                        resp.on_hover_text(*name);
                                    }
                                    let (pick_color_resp, pick_color_activated) = icon_btn(
                                        ui,
                                        crate::overlay::screen_draw_get_color_pick_mode(),
                                        "dropper",
                                        self.tr("Pick color from screen", "Lấy màu từ màn hình"),
                                    );
                                    if (crosshair_draw_mode && pick_color_resp.clicked())
                                        || (!crosshair_draw_mode && pick_color_activated)
                                    {
                                        if crosshair_draw_mode {
                                            crate::overlay::screen_draw_toggle_color_pick_mode();
                                        } else {
                                            const SCREEN_DRAW_COLOR_PICK_HIDE_DELAY_MS: u64 = 160;
                                            self.screen_draw_color_pick_pending_at = Some(Instant::now());
                                            crate::overlay::screen_draw_set_color_pick_cursor();
                                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                                egui::pos2(-10000.0, -10000.0),
                                            ));
                                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
                                            std::thread::spawn(|| {
                                                #[cfg(windows)]
                                                unsafe {
                                                    let _ = DwmFlush();
                                                }
                                                std::thread::sleep(Duration::from_millis(32));
                                                #[cfg(windows)]
                                                unsafe {
                                                    let _ = DwmFlush();
                                                }
                                                std::thread::sleep(Duration::from_millis(
                                                    SCREEN_DRAW_COLOR_PICK_HIDE_DELAY_MS,
                                                ));
                                                if crate::overlay::screen_draw_active()
                                                    && !crate::overlay::screen_draw_get_color_pick_mode()
                                                {
                                                    crate::overlay::screen_draw_toggle_color_pick_mode();
                                                }
                                            });
                                        }
                                    }

                                    if !crosshair_draw_mode {
                                        ui.separator();
                                        let effect = crate::overlay::screen_draw_get_effect();
                                        if icon_btn(
                                            ui,
                                            effect == 1,
                                            "effect_highlight",
                                            self.tr("Highlight effect", "Hiệu ứng tô sáng"),
                                        ).1 {
                                            crate::overlay::screen_draw_toggle_effect(1);
                                            crate::overlay::screen_draw_toolbar_interacted();
                                        }
                                        if icon_btn(
                                            ui,
                                            effect == 2,
                                            "blur",
                                            self.tr("Blur effect", "Hiệu ứng làm mờ"),
                                        ).1 {
                                            crate::overlay::screen_draw_toggle_effect(2);
                                            crate::overlay::screen_draw_toolbar_interacted();
                                        }
                                    }

                                    ui.separator();

                                    // 5. Brush Size Slider
                                    let mut brush_size = crate::overlay::screen_draw_get_brush_size();
                                    ui.add_space(4.0);
                                    ui.add(egui::Label::new(self.tr("Size:", "Cỡ:")));
                                    let slider_resp = ui.add_sized(
                                        [56.0, 20.0],
                                        egui::DragValue::new(&mut brush_size)
                                            .range(2.0..=80.0)
                                            .speed(0.15)
                                            .fixed_decimals(0),
                                    );
                                    crate::overlay::screen_draw_set_brush_size_preview_active(
                                        slider_resp.dragged()
                                            || slider_resp.is_pointer_button_down_on(),
                                    );
                                    if slider_resp.changed() {
                                        brush_size = brush_size.round().clamp(2.0, 80.0);
                                        crate::overlay::screen_draw_set_brush_size(brush_size);
                                    }

                                    ui.separator();

                                    // 6. Capture Region
                                    if !crosshair_draw_mode {
                                        if icon_btn(ui, false, "capture", self.tr("Capture Region", "Chụp vùng")).1 {
                                            crate::overlay::screen_draw_trigger_capture_region_from_toolbar();
                                        }
                                    }
                                    // 7. Clear Canvas
                                    if icon_btn(ui, false, "clear", self.tr("Clear Canvas", "Xóa nét vẽ")).1 {
                                        crate::overlay::screen_draw_clear();
                                    }

                                    // 8. Exit Drawing Mode
                                    let (exit_resp, exit_activated) =
                                        icon_btn(ui, false, "exit", self.tr("Exit Drawing Mode", "Thoát chế độ vẽ"));
                                    if exit_resp.is_pointer_button_down_on() {
                                        crate::overlay::screen_draw_deactivate_from_toolbar();
                                    }
                                    if exit_activated {
                                        crate::overlay::screen_draw_deactivate_from_toolbar();
                                    }

                                    ui.add_space(2.0);
                                });
                                ui.min_rect().width() + 24.0
                            }).inner;
                        let measured_width = measured_width.ceil().max(1.0);
                        if (measured_width - toolbar_width).abs() > 1.0 {
                            let recentered_pos = egui::pos2(
                                toolbar_pos.x - (measured_width - toolbar_width) * 0.5,
                                toolbar_pos.y,
                            );
                            ctx.data_mut(|d| {
                                d.insert_temp(egui::Id::new("toolbar_width"), measured_width);
                                d.insert_temp(egui::Id::new("toolbar_pos"), recentered_pos);
                            });
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                                measured_width,
                                TOOLBAR_HEIGHT,
                            )));
                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                recentered_pos,
                            ));
                            crate::overlay::screen_draw_set_toolbar_rect(
                                (recentered_pos.x - screen_x as f32) as i32,
                                (recentered_pos.y - screen_y as f32) as i32,
                                measured_width.round() as i32,
                                toolbar_height,
                            );
                            ctx.request_repaint();
                        }
                        if !toolbar_ready {
                            ctx.data_mut(|d| {
                                d.insert_temp(
                                    egui::Id::new("screen_draw_toolbar_ready"),
                                    true,
                                )
                            });
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(toolbar_visible));
                            ctx.request_repaint();
                        }
                    }
                }
            );
            return;
        }

        if !self.state.show_window {
            return;
        }

        if self.enforce_square_window_frames > 0 && self.state.show_window {
            let current_size = ctx
                .input(|input| input.viewport().inner_rect.map(|rect| rect.size()))
                .unwrap_or_else(Self::desired_window_size);
            let squared = Self::square_window_size(current_size);
            if (current_size.x - squared.x).abs() > 1.0 || (current_size.y - squared.y).abs() > 1.0
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(squared));
                ctx.request_repaint();
            }
            self.enforce_square_window_frames = self.enforce_square_window_frames.saturating_sub(1);
        }

        if let Some(target) = self.capture_target.as_ref() {
            if ctx.input(|input| input.key_pressed(egui::Key::Escape))
                && !matches!(target, CaptureRequest::MacroStepInput { .. })
                && !self.capture_request_keeps_open(target)
            {
                self.cancel_capture();
            } else if ctx.input(|input| input.viewport().focused == Some(false)) {
                self.cancel_capture();
            }
        }
        if self.mouse_move_absolute_capture_raise_window {
            self.mouse_move_absolute_capture_raise_window = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
            crate::platform::bring_native_window_to_front(frame);
        }

        if let Some(progress) = self.startup_splash_progress(ctx) {
            self.render_startup_splash(ctx, progress);
            return;
        }

        if self.render_image_search_capture_overlay(ctx) {
            return;
        }
        if self.render_protractor_calibration_overlay(ctx) {
            return;
        }

        egui::TopBottomPanel::top("top")
            .frame(
                Frame::new()
                    .fill(if self.state.ui_theme == UiThemeMode::Dark {
                        Color32::from_rgb(16, 20, 26)
                    } else {
                        Color32::from_rgb(246, 248, 251)
                    })
                    .stroke(egui::Stroke::new(
                        1.0,
                        if self.state.ui_theme == UiThemeMode::Dark {
                            Color32::from_rgb(34, 42, 56)
                        } else {
                            Color32::from_rgb(210, 219, 230)
                        },
                    ))
                    .corner_radius(egui::CornerRadius {
                        nw: 16,
                        ne: 16,
                        se: 0,
                        sw: 0,
                    })
                    .inner_margin(egui::Margin::symmetric(4, 4)),
            )
            .show(ctx, |ui| {
                let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                let show_icon_tooltips = true;
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 34.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let button_fill = if self.state.ui_theme == UiThemeMode::Dark {
                            Color32::from_rgba_premultiplied(54, 67, 88, 78)
                        } else {
                            Color32::from_rgba_premultiplied(214, 223, 235, 110)
                        };

                        let exit_response = Self::hover_if(
                            Self::add_sized_with_show_hover_radius(
                                ui,
                                [38.0, 30.0],
                                8,
                                self.titlebar_button(
                                    Self::material_icon_text(0xe5cd, 18.0),
                                    false,
                                    true,
                                ),
                            ),
                            show_icon_tooltips,
                            self.tr("Exit", "Exit"),
                        );
                        if exit_response.clicked() {
                            self.network_panel.shutdown();
                            self.quit_requested = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        let hide_response = Self::hover_if(
                            Self::add_sized_with_show_hover_radius(
                                ui,
                                [38.0, 30.0],
                                8,
                                self.titlebar_button(
                                    Self::material_icon_text(0xe8a4, 18.0),
                                    false,
                                    false,
                                ),
                            ),
                            show_icon_tooltips,
                            self.tr("Hide to tray", "Hide to tray"),
                        );
                        if hide_response.clicked() {
                            self.hide_to_tray(ctx);
                        }
                        let maximize_response = Self::hover_if(
                            Self::add_sized_with_show_hover_radius(
                                ui,
                                [38.0, 30.0],
                                8,
                                self.titlebar_button(
                                    if maximized {
                                        Self::material_icon_text(0xe5cf, 18.0)
                                    } else {
                                        Self::material_icon_text(0xe5d0, 18.0)
                                    },
                                    maximized,
                                    false,
                                ),
                            ),
                            show_icon_tooltips,
                            self.titlebar_maximize_tooltip(maximized),
                        );
                        if maximize_response.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                        }
                        let minimize_response = Self::hover_if(
                            Self::add_sized_with_show_hover_radius(
                                ui,
                                [38.0, 30.0],
                                8,
                                self.titlebar_button(
                                    Self::material_icon_text(0xe15b, 18.0),
                                    false,
                                    false,
                                ),
                            ),
                            show_icon_tooltips,
                            self.titlebar_minimize_tooltip(),
                        );
                        if minimize_response.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        let theme_response = Self::hover_if(
                            Self::add_sized_with_show_hover_radius(
                                ui,
                                [38.0, 30.0],
                                8,
                                self.titlebar_button(self.theme_button_text(), false, false),
                            ),
                            show_icon_tooltips,
                            self.titlebar_theme_tooltip(),
                        );
                        if theme_response.clicked() {
                            self.toggle_theme_mode();
                        }
                        let language_response = Self::hover_if(
                            Self::add_sized_with_show_hover_radius(
                                ui,
                                [38.0, 30.0],
                                8,
                                self.titlebar_button(self.language_button_text(), false, false),
                            ),
                            show_icon_tooltips,
                            self.titlebar_language_tooltip(),
                        );
                        if language_response.clicked() {
                            self.cycle_language();
                        }
                        let vietnamese_input_texture = self.vietnamese_input_icon_texture(
                            ctx,
                            self.state.vietnamese_input_enabled,
                        );
                        let vietnamese_input_response = Self::hover_if(
                            Self::add_sized_with_show_hover_radius(
                                ui,
                                [38.0, 30.0],
                                8,
                                if let Some(texture) = vietnamese_input_texture.as_ref() {
                                    let image = Image::new((texture.id(), vec2(20.0, 20.0)));
                                    let (fill, stroke) = if self.state.ui_theme == UiThemeMode::Dark
                                    {
                                        (
                                            Color32::from_rgba_premultiplied(54, 67, 88, 88),
                                            Color32::from_rgb(74, 92, 118),
                                        )
                                    } else {
                                        (
                                            Color32::from_rgba_premultiplied(220, 228, 238, 165),
                                            Color32::from_rgb(188, 198, 214),
                                        )
                                    };
                                    Button::image(image)
                                        .fill(fill)
                                        .stroke(egui::Stroke::new(1.0, stroke))
                                        .corner_radius(8.0)
                                } else {
                                    self.titlebar_button(
                                        self.vietnamese_input_button_text(),
                                        false,
                                        false,
                                    )
                                },
                            ),
                            show_icon_tooltips,
                            self.titlebar_vietnamese_input_tooltip(),
                        );
                        if vietnamese_input_response.clicked() {
                            self.toggle_vietnamese_input_enabled();
                        }
                        let guides_button_response = Self::add_sized_with_show_hover_radius(
                            ui,
                            [38.0, 30.0],
                            8,
                            self.titlebar_button(
                                RichText::new("!").size(18.0).strong(),
                                self.titlebar_guides_open,
                                false,
                            ),
                        );
                        let guides_response = Self::hover_if(
                            guides_button_response,
                            show_icon_tooltips,
                            Self::tr_lang(self.state.ui_language, "Guides", "Guides"),
                        );
                        if guides_response.clicked() {
                            self.titlebar_guides_open = !self.titlebar_guides_open;
                        }
                        let taskbar_hidden = crate::platform::is_taskbar_hidden();
                        let quick_actions_popup_id =
                            ui.make_persistent_id("titlebar-quick-actions-popup");
                        let mut quick_actions_open = ui
                            .ctx()
                            .data(|data| data.get_temp::<bool>(quick_actions_popup_id))
                            .unwrap_or(false);
                        let quick_actions_button_response = Self::add_sized_with_show_hover_radius(
                            ui,
                            [38.0, 30.0],
                            8,
                            self.titlebar_button(
                                Self::material_icon_text(0xf86e, 18.0),
                                quick_actions_open,
                                false,
                            ),
                        );
                        let pinned_window_active = !self.quick_action_pinned_windows.is_empty();
                        let mut active_count = 0;
                        if taskbar_hidden {
                            active_count += 1;
                        }
                        if self.state.windows_key_locked {
                            active_count += 1;
                        }
                        if pinned_window_active {
                            active_count += 1;
                        }
                        if self.state.native_focus_highlight_enabled {
                            active_count += 1;
                        }
                        if self.state.focus_mode_enabled {
                            active_count += 1;
                        }
                        if self.state.window_opacity_enabled {
                            active_count += 1;
                        }
                        if self.state.protractor_enabled {
                            active_count += 1;
                        }
                        if self.state.quick_key_display_enabled {
                            active_count += 1;
                        }
                        if self.state.quick_screen_draw_enabled {
                            active_count += 1;
                        }
                        if self.state.quick_video_record_enabled {
                            active_count += 1;
                        }
                        if self.state.quick_key_sound_enabled {
                            active_count += 1;
                        }
                        if active_count > 0 {
                            let badge_center =
                                quick_actions_button_response.rect.right_top() + vec2(-8.0, 8.0);
                            ui.painter().circle_filled(
                                badge_center,
                                7.5,
                                Color32::from_rgb(255, 60, 60),
                            );
                            ui.painter().circle_stroke(
                                badge_center,
                                7.5,
                                egui::Stroke::new(1.0, Color32::WHITE),
                            );
                            ui.painter().text(
                                badge_center,
                                egui::Align2::CENTER_CENTER,
                                active_count.to_string(),
                                egui::FontId::proportional(9.0),
                                Color32::WHITE,
                            );
                        }
                        let quick_actions_response = Self::hover_if(
                            quick_actions_button_response,
                            show_icon_tooltips,
                            Self::tr_lang(self.state.ui_language, "Quick actions", "Quick actions"),
                        );
                        if quick_actions_response.clicked() {
                            quick_actions_open = !quick_actions_open;
                        }
                        self.render_modal_backdrop(ui.ctx(), quick_actions_open);
                        let mut keep_quick_actions_open = false;
                        let popup_result = egui::Popup::from_response(&quick_actions_response)
                            .id(quick_actions_popup_id)
                            .open_bool(&mut quick_actions_open)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .at_position(ui.ctx().content_rect().center())
                            .align(egui::RectAlign::from_align2(
                                egui::Align2::CENTER_CENTER,
                            ))
                            .gap(0.0)
                            .layout(egui::Layout::top_down(egui::Align::Min))
                            .width(616.0)
                            .show(|ui| {
                                ui.set_min_width(616.0);
                                Frame::new()
                                    .fill(button_fill)
                                    .stroke(egui::Stroke::new(
                                        1.0,
                                        if self.state.ui_theme == UiThemeMode::Dark {
                                            Color32::from_rgba_premultiplied(96, 118, 148, 196)
                                        } else {
                                            Color32::from_rgba_premultiplied(170, 182, 198, 180)
                                        },
                                    ))
                                    .corner_radius(14.0)
                                    .inner_margin(egui::Margin::symmetric(12, 12))
                                    .show(ui, |ui| {
                                        keep_quick_actions_open = self
                                            .render_titlebar_quick_actions_grid(ui, taskbar_hidden);
                                    });
                            });
                        let _ = popup_result;
                        if self.video_library_open {
                            quick_actions_open = false;
                        } else if keep_quick_actions_open {
                            quick_actions_open = true;
                        }
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(quick_actions_popup_id, quick_actions_open);
                        });
                        let settings_response = Self::hover_if(
                            Self::add_sized_with_show_hover_radius(
                                ui,
                                [38.0, 30.0],
                                8,
                                self.titlebar_button(
                                    Self::material_icon_text(0xe8b8, 18.0),
                                    false,
                                    false,
                                ),
                            ),
                            show_icon_tooltips,
                            Self::tr_lang(self.state.ui_language, "Settings", "Settings"),
                        );
                        let update_badge_count = self.pending_update_badge_count();
                        if update_badge_count > 0 {
                            let badge_center = settings_response.rect.right_top() + vec2(-8.0, 8.0);
                            ui.painter().circle_filled(
                                badge_center,
                                7.5,
                                Color32::from_rgb(255, 60, 60),
                            );
                            ui.painter().circle_stroke(
                                badge_center,
                                7.5,
                                egui::Stroke::new(1.0, Color32::WHITE),
                            );
                            ui.painter().text(
                                badge_center,
                                egui::Align2::CENTER_CENTER,
                                update_badge_count.to_string(),
                                egui::FontId::proportional(9.0),
                                Color32::WHITE,
                            );
                        }
                        if settings_response.clicked() {
                            self.settings_popup_open = !self.settings_popup_open;
                        }

                        ui.add_space(4.0);

                        let drag_width = ui.available_width().max(120.0);
                        let drag_response = ui
                            .allocate_ui_with_layout(
                                vec2(drag_width, 30.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let accent = if self.state.ui_theme == UiThemeMode::Dark {
                                        Color32::from_rgb(126, 214, 178)
                                    } else {
                                        Color32::from_rgb(34, 122, 88)
                                    };
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            if let Some(texture) =
                                                self.titlebar_app_icon_texture(ctx)
                                            {
                                                ui.add(
                                                    Image::new((texture.id(), vec2(28.0, 28.0)))
                                                        .sense(Sense::hover()),
                                                );
                                            }
                                            ui.add_space(8.0);
                                            ui.label(
                                                RichText::new(self.app_brand_title())
                                                    .strong()
                                                    .size(17.0)
                                                    .color(
                                                        if self.state.ui_theme == UiThemeMode::Dark
                                                        {
                                                            Color32::WHITE
                                                        } else {
                                                            Color32::from_rgb(28, 36, 48)
                                                        },
                                                    ),
                                            );
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(format!(
                                                    "v{}",
                                                    self.app_version_label()
                                                ))
                                                .size(8.5)
                                                .color(accent.gamma_multiply(0.95)),
                                            );
                                        },
                                    );
                                    ui.interact(
                                        ui.max_rect(),
                                        ui.id().with("titlebar-drag"),
                                        Sense::click_and_drag(),
                                    )
                                },
                            )
                            .inner;

                        if drag_response.double_clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                        } else if drag_response.drag_started() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                    },
                );

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let panels = [
                        AppPanel::Macros,
                        AppPanel::Commands,
                        AppPanel::Crosshair,
                        AppPanel::WindowPresets,
                        AppPanel::Pin,
                        AppPanel::Mouse,
                        AppPanel::Vision,
                        AppPanel::AudioSense,
                        AppPanel::Ocr,
                        AppPanel::Geometry,
                        AppPanel::Sound,
                    ];
                    for panel in panels {
                        let selected = self.state.active_panel == panel;
                        let emphasized = panel == AppPanel::Macros;
                        let text = RichText::new(self.panel_label(panel));
                        let response = Self::add_with_show_hover_radius(
                            ui,
                            10,
                            self.top_tab_button(text, selected, emphasized),
                        );
                        if response.clicked() {
                            self.state.active_panel = panel;
                        }
                    }
                    if self.active_audio_editor.is_some() {
                        let text = RichText::new(self.panel_label(AppPanel::Media));
                        let response = Self::add_with_show_hover_radius(
                            ui,
                            10,
                            self.top_tab_button(
                                text,
                                self.state.active_panel == AppPanel::Media,
                                false,
                            ),
                        );
                        if response.clicked() {
                            self.state.active_panel = AppPanel::Media;
                        }
                    }
                    let text = RichText::new(self.panel_label(AppPanel::Hud));
                    let response = Self::add_with_show_hover_radius(
                        ui,
                        10,
                        self.top_tab_button(text, self.state.active_panel == AppPanel::Hud, false),
                    );
                    if response.clicked() {
                        self.state.active_panel = AppPanel::Hud;
                    }
                    let text = RichText::new(self.panel_label(AppPanel::Timer));
                    let response = Self::add_with_show_hover_radius(
                        ui,
                        10,
                        self.top_tab_button(
                            text,
                            self.state.active_panel == AppPanel::Timer,
                            false,
                        ),
                    );
                    if response.clicked() {
                        self.state.active_panel = AppPanel::Timer;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let personal_warning = self.tr(
                            "This feature was created by the author for personal use, so it may be difficult to use and does not have detailed instructions.",
                            "Chức năng này do tác giả làm để phục vụ nhu cầu cá nhân, có thể khó sử dụng và không được hướng dẫn chi tiết.",
                        );

                        let selected = self.state.active_panel == AppPanel::Network;
                        let text = RichText::new(self.panel_label(AppPanel::Network)).strong();
                        let response = Self::add_with_show_hover_radius(
                            ui,
                            10,
                            self.top_tab_button_danger(text, selected),
                        )
                        .on_hover_text(personal_warning);
                        if response.clicked() {
                            self.state.active_panel = AppPanel::Network;
                        }

                        let selected = self.state.active_panel == AppPanel::Memory;
                        let text = RichText::new(self.panel_label(AppPanel::Memory)).strong();
                        let response = Self::add_with_show_hover_radius(
                            ui,
                            10,
                            self.top_tab_button_danger(text, selected),
                        )
                        .on_hover_text(personal_warning);
                        if response.clicked() {
                            self.state.active_panel = AppPanel::Memory;
                        }

                        let selected = self.state.active_panel == AppPanel::Esp;
                        let enabled_count = self
                            .state
                            .esp_presets
                            .iter()
                            .filter(|preset| preset.enabled)
                            .count();
                        let label = self.panel_label(AppPanel::Esp);
                        let text = if enabled_count == 0 {
                            RichText::new(label).strong()
                        } else {
                            RichText::new(format!("{label} ({enabled_count})")).strong()
                        };
                        let response = Self::add_with_show_hover_radius(
                            ui,
                            10,
                            self.top_tab_button_danger(text, selected),
                        )
                        .on_hover_text(personal_warning);
                        if response.clicked() {
                            self.state.active_panel = AppPanel::Esp;
                        }
                    });
                });
            });

        if !self.vision_capture_active {
            self.render_custom_window_resize_handles(ctx);
            self.render_custom_window_border(ctx);
        }

        if self.state.active_panel != AppPanel::Pin
            || ctx.input(|input| input.viewport().focused == Some(false))
        {
            self.clear_pin_preview_cache();
        }

        if self.state.active_panel != AppPanel::AudioSense {
            if self.active_pitch_preview_preset_id.take().is_some() || self.audio_sense_test_active
            {
                self.pitch_monitor.stop();
                self.audio_sense_test_active = false;
            }
        }

        let app_focused = ctx.input(|input| input.viewport().focused != Some(false));
        let audio_panel_active =
            matches!(self.state.active_panel, AppPanel::Sound | AppPanel::Media);
        if !app_focused || !audio_panel_active {
            audio::stop_preview();
            if self.video_library_playback.is_none() {
                audio::stop_video_audio_preview();
            }
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .corner_radius(egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        se: 16,
                        sw: 16,
                    })
                    .inner_margin(egui::Margin {
                        left: ctx.style().spacing.window_margin.left,
                        right: ctx.style().spacing.window_margin.right,
                        top: ctx.style().spacing.window_margin.top,
                        bottom: ctx.style().spacing.window_margin.bottom,
                    }),
            )
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());
                let active_panel = self.state.active_panel;
                let panel_shell_active = self.panel_loading_shell_active(active_panel)
                    && active_panel != AppPanel::Macros
                    && active_panel != AppPanel::Modes;
                if panel_shell_active {
                    self.render_panel_loading_shell(ui, active_panel);
                } else if active_panel == AppPanel::Macros
                    || active_panel == AppPanel::Modes
                    || active_panel == AppPanel::Mouse
                    || active_panel == AppPanel::Memory
                    || active_panel == AppPanel::Network
                {
                    if active_panel == AppPanel::Mouse {
                        self.render_mouse_panel(ui);
                    } else if active_panel == AppPanel::Memory {
                        self.render_memory_panel(ui);
                    } else if active_panel == AppPanel::Network {
                        self.render_network_panel(ui);
                    } else {
                        self.render_macro_panel(ui);
                    }
                    if self.capture_target.is_some() {
                        ctx.request_repaint_after(Duration::from_millis(16));
                    }
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            match active_panel {
                                AppPanel::Crosshair => self.render_crosshair_panel(ui),
                                AppPanel::WindowPresets => self.render_window_presets_panel(ui),
                                AppPanel::Pin => self.render_pin_panel(ui),
                                AppPanel::Mouse => unreachable!(),
                                AppPanel::Vision => self.render_vision_panel(ui, ctx),
                                AppPanel::AudioSense => self.render_audiosense_panel(ui),
                                AppPanel::Ocr => self.render_ocr_panel(ui),
                                AppPanel::Geometry => self.render_geometry_panel(ui),
                                AppPanel::Esp => self.render_esp_panel(ui),
                                AppPanel::Zoom => self.render_pin_panel(ui),
                                AppPanel::Modes => self.render_macro_panel(ui),
                                AppPanel::Macros => unreachable!(),
                                AppPanel::Commands => self.render_commands_panel(ui),
                                AppPanel::Sound => self.render_sound_panel(ui),
                                AppPanel::Hud => self.render_hud_panel(ui),
                                AppPanel::Timer => self.render_timer_panel(ui),
                                AppPanel::Media => self.render_media_panel(ui),
                                AppPanel::Memory => unreachable!(),
                                AppPanel::Network => unreachable!(),
                            };
                            if self.capture_target.is_some() {
                                ctx.request_repaint_after(Duration::from_millis(16));
                            }
                        });
                }
                self.finish_panel_warmup_if_ready(active_panel);
                if panel_shell_active {
                    ctx.request_repaint();
                }
            });

        self.render_memory_pinned_viewport(ctx);
        self.render_network_pinned_viewport(ctx);

        if self.settings_popup_open {
            if self.capture_target.is_none()
                && ctx.input(|input| input.key_pressed(egui::Key::Escape))
            {
                self.settings_popup_open = false;
            } else {
                self.render_modal_backdrop(ctx, true);
                let (panel_size, panel_pos) =
                    Self::centered_modal_placement(ctx, vec2(600.0, 620.0), vec2(500.0, 500.0));
                let settings_inner_margin = 20.0;
                let settings_content_size = vec2(
                    (panel_size.x - settings_inner_margin * 2.0).max(0.0),
                    (panel_size.y - settings_inner_margin * 2.0).max(0.0),
                );
                let mut close_request = false;
                egui::Area::new(egui::Id::new("settings_popup_modal"))
                    .order(Order::Foreground)
                    .fixed_pos(panel_pos)
                    .interactable(true)
                    .show(ctx, |ui| {
                        Frame::new()
                            .fill(if self.state.ui_theme == UiThemeMode::Dark {
                                Color32::from_rgba_premultiplied(24, 26, 32, 248)
                            } else {
                                Color32::from_rgba_premultiplied(248, 248, 250, 248)
                            })
                            .stroke(Stroke::new(
                                1.0,
                                Color32::from_rgba_premultiplied(90, 94, 108, 180),
                            ))
                            .shadow(Shadow {
                                offset: [0, 14],
                                blur: 32,
                                spread: 0,
                                color: Color32::from_rgba_premultiplied(12, 12, 16, 72),
                            })
                            .corner_radius(24.0)
                            .inner_margin(Margin::same(settings_inner_margin as i8))
                            .show(ui, |ui| {
                                ui.set_min_size(settings_content_size);
                                ui.set_width(settings_content_size.x);
                                ui.set_max_width(settings_content_size.x);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(Self::tr_lang(
                                                self.state.ui_language,
                                                "Settings",
                                                "Settings",
                                            ))
                                            .strong(),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .add_sized(
                                                        [34.0, 28.0],
                                                        Button::new(Self::material_icon_text(
                                                            0xe5cd, 18.0,
                                                        )),
                                                    )
                                                    .clicked()
                                                {
                                                    close_request = true;
                                                }
                                            },
                                        );
                                    });
                                    ui.separator();
                                    self.render_settings_popup(ui);
                                });
                            });
                    });
                if close_request {
                    self.settings_popup_open = false;
                }
            }
        }

        self.render_custom_ai_modal(ctx);
        self.render_update_notice(ctx);
        self.render_video_library(ctx);

        if self.variable_inspector_open {
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.variable_inspector_open = false;
            } else {
                self.render_modal_backdrop(ctx, true);
                let (panel_size, panel_pos) =
                    Self::centered_modal_placement(ctx, vec2(840.0, 460.0), vec2(620.0, 360.0));
                let mut close_request = false;
                let title = Self::tr_lang(self.state.ui_language, "Variables", "Variables");
                egui::Area::new(egui::Id::new("variable-inspector-modal"))
                    .order(Order::Foreground)
                    .fixed_pos(panel_pos)
                    .interactable(true)
                    .show(ctx, |ui| {
                        Frame::new()
                            .fill(if self.state.ui_theme == UiThemeMode::Dark {
                                Color32::from_rgba_premultiplied(24, 26, 32, 248)
                            } else {
                                Color32::from_rgba_premultiplied(248, 248, 250, 248)
                            })
                            .stroke(Stroke::new(
                                1.0,
                                Color32::from_rgba_premultiplied(90, 94, 108, 180),
                            ))
                            .shadow(Shadow {
                                offset: [0, 14],
                                blur: 32,
                                spread: 0,
                                color: Color32::from_rgba_premultiplied(12, 12, 16, 72),
                            })
                            .corner_radius(24.0)
                            .inner_margin(Margin::same(16))
                            .show(ui, |ui| {
                                ui.set_min_size(panel_size);
                                ui.set_max_size(panel_size);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(title).strong().size(16.0));
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .add_sized(
                                                        [34.0, 28.0],
                                                        Button::new(Self::material_icon_text(
                                                            0xe5cd, 18.0,
                                                        )),
                                                    )
                                                    .clicked()
                                                {
                                                    close_request = true;
                                                }
                                            },
                                        );
                                    });
                                    ui.separator();
                                    self.render_variable_inspector(ui);
                                });
                            });
                    });
                if close_request {
                    self.variable_inspector_open = false;
                }
            }
        }

        if self.titlebar_guides_open {
            self.render_modal_backdrop(ctx, true);
            let (panel_size, panel_pos) =
                Self::centered_modal_placement(ctx, vec2(800.0, 800.0), vec2(500.0, 500.0));
            let inner_margin = 16.0;
            let content_size = vec2(
                (panel_size.x - inner_margin * 2.0).max(0.0),
                (panel_size.y - inner_margin * 2.0).max(0.0),
            );
            egui::Area::new(egui::Id::new("expression_guides_modal"))
                .order(Order::Foreground)
                .fixed_pos(panel_pos)
                .interactable(true)
                .show(ctx, |ui| {
                    Frame::new()
                        .fill(if self.state.ui_theme == UiThemeMode::Dark {
                            Color32::from_rgba_premultiplied(24, 26, 32, 248)
                        } else {
                            Color32::from_rgba_premultiplied(248, 248, 250, 248)
                        })
                        .stroke(Stroke::new(
                            1.0,
                            Color32::from_rgba_premultiplied(90, 94, 108, 180),
                        ))
                        .shadow(Shadow {
                            offset: [0, 14],
                            blur: 32,
                            spread: 0,
                            color: Color32::from_rgba_premultiplied(12, 12, 16, 72),
                        })
                        .corner_radius(16.0)
                        .inner_margin(Margin::same(inner_margin as i8))
                        .show(ui, |ui| {
                            ui.set_min_size(content_size);
                            ui.set_max_size(content_size);
                            ui.set_width(content_size.x);
                            ui.set_max_width(content_size.x);
                            ui.set_height(content_size.y);
                            ui.set_max_height(content_size.y);
                            self.render_expression_guides_content(ui, ctx);
                        });
                });
        }

        if self.startup_show_pending && self.startup_shell_frames_remaining == 0 {
            self.startup_show_pending = false;
            if !self.startup_hide_to_tray_pending {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            self.startup_gate_release_pending = true;
            self.startup_gate_frames_remaining = 1;
            ctx.request_repaint();
        }

        if self.persist_dirty {
            let pointer_down = ctx.input(|i| i.pointer.any_down());
            let ready = self
                .persist_requested_at
                .is_some_and(|requested_at| requested_at.elapsed() >= PERSIST_DEBOUNCE);
            if ready && !pointer_down {
                let snapshot = PersistSnapshot {
                    profiles: self.state.profiles.clone(),
                    state: self.state.clone(),
                };
                self.persist_dirty = false;
                self.persist_requested_at = None;
                if self.persist_tx.send(snapshot).is_err() {
                    self.status = "Failed to queue app state save.".to_owned();
                }
            } else {
                ctx.request_repaint_after(PERSIST_DEBOUNCE);
            }
        }

        self.poll_capture_input(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.network_panel.shutdown();
        crate::video_recorder::stop_blocking();
        let _ = crate::platform::show_taskbar();
        self.unpin_all_quick_action_windows();
        self.state.reset_session_preset_visibility();
        self.sync_window_presets();
        self.sync_macro_presets();
        self.sync_macro_master_enabled();
        self.sync_audio_settings();
        self.sync_hud_presets();
        self.sync_timer_presets();
        self.sync_command_presets();
        self.sync_macro_master_hotkey();
        self.sync_vietnamese_input_enabled();
        let _ = self.overlay_tx.send(OverlayCommand::Exit);
        self.persist_blocking();
    }
}
