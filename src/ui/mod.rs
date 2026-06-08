use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU32},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::Result;
use arboard::Clipboard;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use eframe::egui::{
    self, Button, Color32, ColorImage, FontData, FontDefinitions, FontFamily, Frame, Image, Margin,
    Order, RichText, Sense, Shadow, Stroke, StrokeKind, TextureHandle, TextureOptions, pos2, vec2,
};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use resvg::usvg;

use crate::{
    ai, audio, audiosense, hotkey,
    model::{
        AppPanel, AppState, AudioClipSettings, AudioSensePreset, AudioSensePresetKind, CaptureRequest,
        CapturedInput, CommandPreset, CrosshairStyle, GeometrySpec, HotkeyBinding, MacroAction,
        MacroFolder, MacroGroup, MacroPreset, MacroStep, MacroTriggerMode,
        MasterMacroGroupState, MasterMacroPresetState, MasterPreset,
        MasterWindowFocusPresetState, MasterWindowPresetState, MasterZoomPresetState,
        MousePathEventKind, ProfileRecord, RgbaColor, SoundLibraryItem, TimerPreset, UiLanguage,
        UiThemeMode, VideoClipSettings, VietnameseInputMode, WindowAnchor, WindowExpandDirection,
        WindowPreset,
    },
    overlay::{OverlayCommand, UiCommand},
    profile_code,
    storage::AppPaths,
    window_list,
};
use vi::{self, TELEX, VNI};

mod command_panel;
mod crosshair_panel;
mod audiosense_panel;
mod geometry_panel;
mod hud_panel;
mod macro_panel;
mod macro_panel_ocr;
mod mouse_panel;
mod ocr_panel;
mod settings_panel;
mod sound_panel;
mod vision_panel;
mod window_panel;

#[cfg(windows)]
pub(crate) use windows::Win32::{
    Foundation::POINT,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AudioEditorTarget {
    Startup,
    Exit,
    Library(u32),
    Preset(u32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisionCaptureMode {
    Template,
    SearchRegion,
    ColorSample,
    ColorPriorityAnchor,
    SinglePixel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisionCaptureTarget {
    Preset(u32),
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
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
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

#[derive(Clone, Copy, PartialEq, Eq)]
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

struct StartupSplashState {
    started_at: Option<f64>,
    duration_sec: f64,
}

#[derive(Clone)]
pub(crate) struct ZoomPreviewView {
    texture: TextureHandle,
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

#[derive(Clone)]
pub(crate) struct VideoPreviewView {
    texture: TextureHandle,
    width: usize,
    height: usize,
}

pub(crate) struct VideoPreviewCache {
    updated_at: Instant,
    source_path: String,
    start_ms: u64,
    max_width: i32,
    max_height: i32,
    view: VideoPreviewView,
}

pub(crate) const MATERIAL_ICONS_FONT: &str = "material_icons";
const UI_SANS_FONT: &str = "ui_sans";
const UI_SANS_SEMIBOLD_FONT: &str = "ui_sans_semibold";
const OPEN_WINDOWS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const AUDIO_SENSE_DEVICES_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

fn text_has_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x2E80..=0x2FDF
                | 0x3040..=0x30FF
                | 0x31F0..=0x31FF
                | 0x3400..=0x4DBF
                | 0x4E00..=0x9FFF
                | 0xAC00..=0xD7AF
                | 0xF900..=0xFAFF
                | 0xFF66..=0xFF9F
        )
    })
}

pub fn app_state_needs_cjk_fallback(state: &AppState) -> bool {
    serde_json::to_string(state)
        .map(|json| text_has_cjk(&json))
        .unwrap_or(false)
}

pub fn configure_fonts(ctx: &egui::Context, load_cjk_fallback: bool) {
    let mut fonts = FontDefinitions {
        font_data: Default::default(),
        families: Default::default(),
    };
    fonts.font_data.insert(
        UI_SANS_FONT.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/SegoeUI.ttf"
        ))),
    );
    fonts.font_data.insert(
        UI_SANS_SEMIBOLD_FONT.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/SegoeUI-Semibold.ttf"
        ))),
    );
    fonts.font_data.insert(
        MATERIAL_ICONS_FONT.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/MaterialIcons-Regular.ttf"
        ))),
    );
    #[cfg(windows)]
    {
        if load_cjk_fallback
            && let Ok(font_bytes) = std::fs::read("C:\\Windows\\Fonts\\msyh.ttc")
        {
            fonts.font_data.insert(
                "cjk_fallback".to_owned(),
                Arc::new(FontData::from_owned(font_bytes)),
            );
        }
    }
    let ui_family = FontFamily::Name(UI_SANS_FONT.into());
    fonts
        .families
        .entry(ui_family.clone())
        .or_default()
        .insert(0, UI_SANS_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, UI_SANS_SEMIBOLD_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push(UI_SANS_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push(UI_SANS_FONT.to_owned());
    let material_family = FontFamily::Name(MATERIAL_ICONS_FONT.into());
    fonts
        .families
        .entry(material_family.clone())
        .or_default()
        .insert(0, MATERIAL_ICONS_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push(MATERIAL_ICONS_FONT.to_owned());
    #[cfg(windows)]
    {
        if fonts.font_data.contains_key("cjk_fallback") {
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .push("cjk_fallback".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .push("cjk_fallback".to_owned());
        }
    }
    ctx.set_fonts(fonts);
    ctx.style_mut(|style| {
        style.interaction.show_tooltips_only_when_still = false;
        style.interaction.tooltip_delay = 0.0;
        style.interaction.tooltip_grace_time = 0.0;

        use egui::{FontId, TextStyle};
        let text_styles = &mut style.text_styles;
        text_styles.insert(
            TextStyle::Small,
            FontId::new(13.5, FontFamily::Proportional),
        );
        text_styles.insert(TextStyle::Body, FontId::new(16.0, FontFamily::Proportional));
        text_styles.insert(
            TextStyle::Button,
            FontId::new(15.5, FontFamily::Proportional),
        );
        text_styles.insert(
            TextStyle::Heading,
            FontId::new(21.0, FontFamily::Proportional),
        );
        text_styles.insert(
            TextStyle::Monospace,
            FontId::new(15.5, FontFamily::Monospace),
        );
    });
}

fn visuals_for_theme(theme: UiThemeMode) -> egui::Visuals {
    match theme {
        UiThemeMode::Dark => {
            let mut visuals = egui::Visuals::dark();
            visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(220, 228, 238);
            visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(228, 234, 242);
            visuals.widgets.hovered.fg_stroke.color = Color32::from_rgb(240, 246, 252);
            visuals.widgets.active.fg_stroke.color = Color32::from_rgb(248, 250, 252);
            visuals.widgets.open.fg_stroke.color = Color32::from_rgb(240, 246, 252);
            visuals
        }
        UiThemeMode::Light => {
            let mut visuals = egui::Visuals::light();
            visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(32, 40, 54);
            visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(28, 36, 48);
            visuals.widgets.hovered.fg_stroke.color = Color32::from_rgb(18, 26, 40);
            visuals.widgets.active.fg_stroke.color = Color32::from_rgb(16, 24, 38);
            visuals.widgets.open.fg_stroke.color = Color32::from_rgb(18, 26, 40);
            visuals.hyperlink_color = Color32::from_rgb(26, 92, 164);
            visuals.panel_fill = Color32::from_rgb(248, 248, 248);
            visuals.window_fill = Color32::from_rgb(248, 248, 248);
            visuals
        }
    }
}

pub(crate) fn configure_theme(ctx: &egui::Context, theme: UiThemeMode) {
    ctx.set_visuals(visuals_for_theme(theme));
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(match theme {
        UiThemeMode::Dark => egui::SystemTheme::Dark,
        UiThemeMode::Light => egui::SystemTheme::Light,
    }));
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
    Mouse,
    ImageSearch,
    Timer,
    If,
    Geometry,
    AudioSense,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OcrLanguageOperationKind {
    Install,
    Remove,
}

pub struct CrosshairApp {
    pub paths: AppPaths,
    pub state: AppState,
    overlay_tx: Sender<OverlayCommand>,
    ui_tx: Sender<UiCommand>,
    ui_rx: Receiver<UiCommand>,
    status: String,
    save_name: String,
    import_code_buffer: String,
    export_code_buffer: String,
    custom_assets: Vec<String>,
    open_windows: Vec<String>,
    open_windows_loaded_once: bool,
    open_windows_loading: bool,
    quit_requested: bool,
    capture_target: Option<CaptureRequest>,
    startup_clip_duration_ms: Option<u64>,
    exit_clip_duration_ms: Option<u64>,
    startup_sound_played: bool,
    show_startup_audio_editor: bool,
    show_exit_audio_editor: bool,
    startup_sound_collapsed: bool,
    exit_sound_collapsed: bool,
    audio_waveforms: HashMap<String, Vec<f32>>,
    sound_preset_clip_duration_ms: HashMap<u32, Option<u64>>,
    video_preset_clip_duration_ms: HashMap<u32, Option<u64>>,
    video_preview_cursor_ms: HashMap<u32, u64>,
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
    mouse_move_absolute_capture_wait_for_mouse_release: bool,
    mouse_move_absolute_capture_raise_window: bool,
    mouse_move_absolute_restore_inner_size: Option<egui::Vec2>,
    mouse_move_absolute_restore_outer_pos: Option<egui::Pos2>,
    mouse_path_draw_capture_preset_id: Option<u32>,
    mouse_path_draw_capture_restore_inner_size: Option<egui::Vec2>,
    mouse_path_draw_capture_restore_outer_pos: Option<egui::Pos2>,
    mouse_path_step_preview_preset_id: Option<u32>,
    mouse_path_add_feedback_target: Option<(u32, u32, usize)>,
    mouse_path_add_feedback_until: Option<Instant>,
    macro_step_copy_feedback_target: Option<(u32, u32, usize)>,
    macro_step_copy_feedback_until: Option<Instant>,
    macro_selected_steps_copy_feedback_target: Option<(u32, u32)>,
    macro_selected_steps_copy_feedback_until: Option<Instant>,
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
    macro_preset_clipboard: Option<MacroPreset>,
    macro_step_clipboard: Vec<MacroStep>,
    pending_macro_group_scroll_target: Option<u32>,
    crosshair_profile_clipboard: Option<ProfileRecord>,
    crosshair_editor_dirty: bool,
    crosshair_preview_last_sync_at: Option<Instant>,
    crosshair_preview_dirty_index: Option<usize>,
    crosshair_preview_dirty_generation: u64,
    crosshair_preview_applied_generation: u64,
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
    macro_panel_render_limit: usize,
    crosshair_panel_collapsed: bool,
    startup_splash: StartupSplashState,
    settings_popup_open: bool,
    advanced_settings_open: bool,
    downloaded_tools_open: bool,
    zoom_preview_cache: HashMap<u32, ZoomPreviewCache>,
    vision_preview_cache: HashMap<u32, VisionPreviewCache>,
    video_preview_cache: HashMap<u32, VideoPreviewCache>,
    video_frame_tx: Option<crossbeam_channel::Sender<crate::media::VideoFrameRequest>>,
    video_preview_requested: HashMap<u32, (String, u64)>,
    window_preview_requested: HashMap<u32, Instant>,
    video_chroma_pick_preset_id: Option<u32>,
    active_video_preview_preset_id: Option<u32>,
    active_video_preview_started_at: Option<Instant>,
    active_video_preview_start_ms: u64,
    active_video_overlay_preset_id: Option<u32>,
    vision_color_pick_texture: Option<TextureHandle>,
    vision_color_pick_preview_color: Option<RgbaColor>,
    vietnamese_input_enabled_texture: Option<TextureHandle>,
    vietnamese_input_disabled_texture: Option<TextureHandle>,
    active_mouse_record_preset_id: Option<u32>,
    active_macro_record_preset_id: Option<u32>,
    active_hud_preview_preset_id: Option<u32>,
    active_timer_preview_preset_id: Option<u32>,
    command_ai_dialog: Option<CommandAiDialog>,
    command_ai_job: Option<CommandAiJob>,
    command_ai_next_token: u64,
    command_ai_feedback: Option<String>,
    command_ai_step_target: Option<(u32, u32, Option<usize>)>,
    last_applied_theme: Option<UiThemeMode>,
    native_shadow_applied: bool,
    native_transitions_disabled_applied: bool,
    startup_show_pending: bool,
    startup_gate_release_pending: bool,
    startup_gate_frames_remaining: u8,
    startup_shell_frames_remaining: u8,
    startup_overlay_sync_pending: bool,
    startup_state_persist_pending: bool,
    startup_cjk_font_check_pending: bool,
    background_panel_preload_index: usize,
    startup_gate: Option<std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>>,
    panel_warmup_target: Option<AppPanel>,
    panel_warmup_frames_remaining: u8,
    warmed_panels: Vec<AppPanel>,
    update_status: UpdateStatus,
    interception_status: String,
    opencv_download_job: Option<JoinHandle<Result<()>>>,
    opencv_download_progress: Arc<AtomicU32>,
    opencv_installed: bool,
    interception_download_job: Option<JoinHandle<Result<()>>>,
    interception_download_progress: Arc<AtomicU32>,
    interception_package_downloaded: bool,
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
    audio_sense_devices: Vec<String>,
    audio_sense_devices_loaded_once: bool,
    audio_sense_devices_loading: bool,
    pitch_monitor: audiosense::PitchMonitor,
    audio_sense_test_settings: crate::model::AudioSenseMonitorSettings,
    audio_sense_test_active: bool,
    active_pitch_preview_preset_id: Option<u32>,
    /// Target for color picking a DrawGeometry macro step spec (group_id, preset_id, step_index, is_fill, is_hold_stop)
    macro_step_geometry_color_pick_target: Option<(u32, u32, usize, bool, bool)>,
    /// Which DrawGeometry macro step is currently being previewed on overlay (group_id, preset_id, step_index, is_hold_stop)
    draw_geometry_step_preview_target: Option<(u32, u32, usize, bool)>,
    draw_geometry_step_preview_sent: Option<crate::model::GeometrySpec>,
    variable_inspector_open: bool,
    ocr_lang_pack_open: bool,
    ocr_lang_settings_focus: Option<String>,
    ocr_lang_operation: Option<(String, OcrLanguageOperationKind, Instant)>,
    pub show_share_buttons: bool,
    arduino_available_ports: Vec<String>,
    arduino_ports_last_refresh: Option<Instant>,
    mouse_input_normal_open: bool,
    mouse_input_arduino_open: bool,
    mouse_input_interception_open: bool,
}

impl CrosshairApp {
    fn groq_model_catalog() -> &'static [(&'static str, &'static str)] {
        &[("GPT OSS 120B", "openai/gpt-oss-120b")]
    }

    pub fn new(
        paths: AppPaths,
        state: AppState,
        overlay_tx: Sender<OverlayCommand>,
        ui_tx: Sender<UiCommand>,
        ui_rx: Receiver<UiCommand>,
        startup_state_dirty: bool,
        startup_gate: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    ) -> Self {
        let save_name = state
            .selected_profile
            .clone()
            .unwrap_or_else(|| "Default".to_owned());
        let initial_active_panel = state.active_panel;

        let opencv_installed = paths.opencv_dll.exists();
        let mut app = Self {
            paths: paths.clone(),
            state,
            overlay_tx,
            ui_tx,
            ui_rx,
            status: String::new(),
            save_name,
            import_code_buffer: String::new(),
            export_code_buffer: String::new(),
            custom_assets: Vec::new(),
            open_windows: Vec::new(),
            open_windows_loaded_once: false,
            open_windows_loading: false,
            quit_requested: false,
            capture_target: None,
            startup_clip_duration_ms: None,
            exit_clip_duration_ms: None,
            startup_sound_played: false,
            show_startup_audio_editor: false,
            show_exit_audio_editor: false,
            startup_sound_collapsed: true,
            exit_sound_collapsed: true,
            audio_waveforms: HashMap::new(),
            sound_preset_clip_duration_ms: HashMap::new(),
            video_preset_clip_duration_ms: HashMap::new(),
            video_preview_cursor_ms: HashMap::new(),
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
            mouse_move_absolute_capture_wait_for_mouse_release: false,
            mouse_move_absolute_capture_raise_window: false,
            mouse_move_absolute_restore_inner_size: None,
            mouse_move_absolute_restore_outer_pos: None,
            mouse_path_draw_capture_preset_id: None,
            mouse_path_draw_capture_restore_inner_size: None,
            mouse_path_draw_capture_restore_outer_pos: None,
            mouse_path_step_preview_preset_id: None,
            mouse_path_add_feedback_target: None,
            mouse_path_add_feedback_until: None,
            macro_step_copy_feedback_target: None,
            macro_step_copy_feedback_until: None,
            macro_selected_steps_copy_feedback_target: None,
            macro_selected_steps_copy_feedback_until: None,
            vision_capture_active: false,
            vision_capture_target: None,
            vision_capture_mode: None,
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
            macro_preset_clipboard: None,
            macro_step_clipboard: Vec::new(),
            pending_macro_group_scroll_target: None,
            crosshair_profile_clipboard: None,
            crosshair_editor_dirty: false,
            crosshair_preview_last_sync_at: None,
            crosshair_preview_dirty_index: None,
            crosshair_preview_dirty_generation: 0,
            crosshair_preview_applied_generation: 0,
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
            macro_panel_render_limit: if initial_active_panel == AppPanel::Macros {
                8
            } else {
                usize::MAX
            },
            crosshair_panel_collapsed: true,
            startup_splash: StartupSplashState {
                started_at: None,
                duration_sec: 0.0,
            },
            settings_popup_open: false,
            advanced_settings_open: false,
            downloaded_tools_open: false,
            zoom_preview_cache: HashMap::new(),
            vision_preview_cache: HashMap::new(),
            video_preview_cache: HashMap::new(),
            video_frame_tx: None,
            video_preview_requested: HashMap::new(),
            window_preview_requested: HashMap::new(),
            video_chroma_pick_preset_id: None,
            active_video_preview_preset_id: None,
            active_video_preview_started_at: None,
            active_video_preview_start_ms: 0,
            active_video_overlay_preset_id: None,
            vision_color_pick_texture: None,
            vision_color_pick_preview_color: None,
            vietnamese_input_enabled_texture: None,
            vietnamese_input_disabled_texture: None,
            active_mouse_record_preset_id: None,
            active_macro_record_preset_id: None,
            active_hud_preview_preset_id: None,
            active_timer_preview_preset_id: None,
            command_ai_dialog: None,
            command_ai_job: None,
            command_ai_next_token: 1,
            command_ai_feedback: None,
            command_ai_step_target: None,
            last_applied_theme: None,
            native_shadow_applied: false,
            native_transitions_disabled_applied: false,
            startup_show_pending: true,
            startup_gate_release_pending: false,
            startup_gate_frames_remaining: 1,
            startup_shell_frames_remaining: 0,
            startup_overlay_sync_pending: true,
            startup_state_persist_pending: false,
            startup_cjk_font_check_pending: true,
            background_panel_preload_index: 0,
            startup_gate: Some(startup_gate),
            update_status: UpdateStatus::Idle,
            interception_status: "Interception: Unavailable".to_owned(),
            opencv_download_job: None,
            opencv_download_progress: Arc::new(AtomicU32::new(0)),
            opencv_installed,
            interception_download_job: None,
            interception_download_progress: Arc::new(AtomicU32::new(0)),
            interception_package_downloaded: paths.interception_zip.exists()
                || paths.interception_package_dir.exists()
                || paths.interception_installer_exe.exists(),
            interception_driver_installed: crate::platform::is_interception_driver_installed(),
            interception_driver_needs_restart: false,
            interception_install_job: None,
            interception_uninstall_job: None,
            arduino_download_job: None,
            arduino_download_progress: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            arduino_tools_downloaded: paths.avrdude_exe.exists() && paths.avrdude_conf.exists() && paths.arduino_firmware_hex.exists(),
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
            audio_sense_devices: Vec::new(),
            audio_sense_devices_loaded_once: false,
            audio_sense_devices_loading: false,
            pitch_monitor: audiosense::PitchMonitor::new(),
            audio_sense_test_settings: crate::model::AudioSenseMonitorSettings::default(),
            audio_sense_test_active: false,
            active_pitch_preview_preset_id: None,
            macro_step_geometry_color_pick_target: None,
            draw_geometry_step_preview_target: None,
            draw_geometry_step_preview_sent: None,
            variable_inspector_open: false,
            ocr_lang_pack_open: false,
            ocr_lang_settings_focus: None,
            ocr_lang_operation: None,
            show_share_buttons: false,
            arduino_available_ports: Vec::new(),
            arduino_ports_last_refresh: None,
            mouse_input_normal_open: false,
            mouse_input_arduino_open: false,
            mouse_input_interception_open: false,
            panel_warmup_target: Some(initial_active_panel),
            panel_warmup_frames_remaining: 1,
            warmed_panels: Vec::new(),
        };
        app.interception_installed = app.paths.interception_dll.exists();
        let mut pending_startup_persist = startup_state_dirty;
        if app.apply_startup_state_adjustments() {
            pending_startup_persist = true;
        }
        app.startup_state_persist_pending = pending_startup_persist;
        for preset in &app.state.audio_settings.video_presets {
            app.video_preview_cursor_ms
                .insert(preset.id, preset.clip.start_ms);
        }
        {
            let mut vars = crate::overlay::RUNTIME_VARIABLES.lock();
            for (name, val) in &app.state.global_constants {
                vars.insert(name.clone(), *val);
            }
        }
        app.preload_primary_sound_preset_audio();
        app
    }

    fn sync_crosshair(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateProfiles(self.state.profiles.clone()));
    }



    fn run_all_startup_overlay_sync(&mut self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::Update(self.state.active_style.clone()));
        self.sync_profiles();
        self.sync_window_presets();
        self.sync_mouse_sensitivity_presets();
        self.sync_mouse_sensitivity_settings();
        self.sync_mouse_driver_settings();
        self.sync_keyboard_arrow_mouse_settings();
        self.sync_macro_delay_settings();
        self.sync_macro_presets();
        self.sync_audio_settings();
        self.sync_vision_presets();
        self.sync_ocr_presets();
        self.sync_vision_settings();
        self.sync_hud_presets();
        self.sync_timer_presets();
        self.sync_command_presets();
        self.sync_audio_sense_presets();
        self.sync_geometry_presets();
        self.preload_primary_sound_preset_audio();
        self.sync_macro_master_enabled();
        self.sync_vietnamese_input_enabled();
        self.sync_macro_master_hotkey();
        self.startup_overlay_sync_pending = false;
    }

    fn sync_macro_delay_settings(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateMacroDelays {
            mouse_click_delay_ms: self.state.macro_mouse_click_delay_ms,
            keyboard_key_press_delay_ms: self.state.macro_keyboard_key_press_delay_ms,
        });
    }

    fn sync_profiles(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateProfiles(self.state.profiles.clone()));
    }

    fn sync_crosshair_profile(&self, index: usize, profile: &ProfileRecord) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateCrosshairProfile {
                index,
                profile: profile.clone(),
            });
    }

    fn sync_macro_presets(&self) {
        let macro_groups = build_runtime_macro_groups(&self.state);
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateMacroPresets(macro_groups));
    }

    fn sync_macro_master_enabled(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::SetMacrosMasterEnabled(
            self.state.macros_master_enabled,
        ));
    }

    fn sync_vietnamese_input_enabled(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::SetVietnameseInputEnabled(
                self.state.vietnamese_input_enabled,
            ));
    }

    fn sync_macro_master_hotkey(&self) {
        let _ = self
            .overlay_tx
            .send(OverlayCommand::UpdateMacrosMasterHotkey(
                self.state.macros_master_hotkey.clone(),
            ));
    }

    fn sync_audio_settings(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateAudioSettings(
            self.state.audio_settings.clone(),
        ));
    }

    fn preload_primary_sound_preset_audio(&self) {
        let Some(path) = self
            .state
            .audio_settings
            .presets
            .iter()
            .find_map(|preset| {
                let path = preset.clip.file_path.trim();
                (!path.is_empty()).then(|| path.to_owned())
            })
        else {
            return;
        };
        let _ = audio::preload_preview_audio(&path);
    }

    fn sync_vision_settings(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateVisionSettings(
            self.state.vision_settings.clone(),
        ));
    }

    fn sync_timer_presets(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateTimerPresets(
            self.state.timer_presets.clone(),
        ));
    }

    fn sync_geometry_presets(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateGeometryPresets(
            self.state.geometry_presets.clone(),
        ));
    }

    fn migrate_legacy_audio_sense_state(&mut self) -> bool {
        let mut changed = false;

        for group in &mut self.state.macro_groups {
            for preset in &mut group.presets {
                for step in &mut preset.steps {
                    if step.audio_sense_spec.kind != AudioSensePresetKind::Pitch {
                        step.audio_sense_spec.kind = AudioSensePresetKind::Pitch;
                        changed = true;
                    }
                }
            }
        }

        if self.state.audio_sense_presets.is_empty() {
            self.state.audio_sense_presets.push(AudioSensePreset::new_pitch(1));
            self.state.next_audio_sense_preset_id = 2;
            changed = true;
        } else {
            let next_id = self
                .state
                .audio_sense_presets
                .iter()
                .map(|preset| preset.id)
                .max()
                .unwrap_or(0)
                + 1;
            if self.state.next_audio_sense_preset_id != next_id {
                self.state.next_audio_sense_preset_id = next_id;
                changed = true;
            }
        }

        changed
    }

    fn apply_startup_state_adjustments(&mut self) -> bool {
        let mut changed = false;
        if self.ensure_master_presets_without_persist() {
            changed = true;
        }
        if self.state.groq_settings.details_open {
            self.state.groq_settings.details_open = false;
            changed = true;
        }
        for preset in &mut self.state.command_presets {
            if !preset.collapsed {
                preset.collapsed = true;
                changed = true;
            }
        }
        if self.migrate_legacy_audio_sense_state() {
            changed = true;
        }
        if self.state.vision_settings.use_arduino_mouse
            && self.state.vision_settings.use_interception
        {
            self.state.vision_settings.use_interception = false;
            changed = true;
        }
        changed
    }

    fn sync_audio_sense_presets(&self) {
        let _ = self.overlay_tx.send(OverlayCommand::UpdateAudioSensePresets(
            self.state.audio_sense_presets.clone(),
        ));
    }

    fn sync_timer_preview(&mut self, preset: Option<&TimerPreset>) {
        let next_id = preset.map(|preset| preset.id);
        if self.active_timer_preview_preset_id == next_id {
            if let Some(preset) = preset {
                let _ = self
                    .overlay_tx
                    .send(OverlayCommand::PreviewTimerPreset(Some(preset.clone())));
            }
            return;
        }
        self.active_timer_preview_preset_id = next_id;
        let _ = self
            .overlay_tx
            .send(OverlayCommand::PreviewTimerPreset(preset.cloned()));
    }

    fn clear_timer_preview(&mut self) {
        if self.active_timer_preview_preset_id.take().is_some() {
            let _ = self
                .overlay_tx
                .send(OverlayCommand::PreviewTimerPreset(None));
        }
    }

    fn disable_timer_preview_modes(&mut self) -> bool {
        let mut changed = false;
        for preset in &mut self.state.timer_presets {
            if preset.preview_enabled {
                preset.preview_enabled = false;
                changed = true;
            }
        }
        if changed {
            self.clear_timer_preview();
        }
        changed
    }

    fn persist(&mut self) {
        if let Err(error) = self.paths.save_profiles(&self.state.profiles) {
            self.status = format!("Failed to save profiles: {error}");
            return;
        }
        if let Err(error) = self.paths.save_state(&self.state) {
            self.status = format!("Failed to save app state: {error}");
        }
    }

    fn save_profile(&mut self) {
        let name = self.save_name.trim().to_owned();
        if name.is_empty() {
            self.status = "Enter a profile name before saving.".to_owned();
            return;
        }
        if let Some(existing) = self
            .state
            .profiles
            .iter_mut()
            .find(|profile| profile.name == name)
        {
            existing.style = self.state.active_style.clone();
        } else {
            self.state.profiles.push(ProfileRecord {
                name: name.clone(),
                enabled: self.state.active_style.enabled,
                collapsed: true,
                style: self.state.active_style.clone(),
                target_window_title: None,
                extra_target_window_titles: Vec::new(),
            });
        }
        self.state
            .profiles
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.state.selected_profile = Some(name.clone());
        self.sync_profiles();
        self.persist();
        self.status = format!("Saved profile: {name}");
    }

    fn add_profile(&mut self) {
        let name = self.unique_profile_name("Profile");
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
        let Some(index) = self.crosshair_preview_dirty_index else {
            return;
        };
        if !force {
            if self.crosshair_preview_applied_generation == self.crosshair_preview_dirty_generation
            {
                return;
            }
            if let Some(last_sync_at) = self.crosshair_preview_last_sync_at {
                if last_sync_at.elapsed() < Duration::from_millis(70) {
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
        self.sync_profiles();
        self.persist();
        self.status = format!("Pasted crosshair preset: {}.", name);
    }

    fn delete_profile(&mut self) {
        let Some(selected) = self.state.selected_profile.clone() else {
            self.status = "No profile is selected.".to_owned();
            return;
        };

        let mut index_to_remove = None;
        for (i, p) in self.state.profiles.iter().enumerate() {
            if p.name == selected {
                index_to_remove = Some(i);
                break;
            }
        }

        if let Some(i) = index_to_remove {
            self.state.profiles.remove(i);
            self.status = format!("Deleted profile: {selected}");
        } else {
            self.status = format!("Could not find profile: {selected}");
        }

        if self.state.profiles.is_empty() {
            self.state.selected_profile = None;
            self.state.active_style = CrosshairStyle::default();
            self.state.active_style.enabled = false;
            self.save_name = String::new();
        } else {
            let next = self.state.profiles[0].clone();
            self.state.selected_profile = Some(next.name.clone());
            self.state.active_style = next.style;
            self.save_name = next.name;
        }
        self.sync_crosshair();
        self.sync_profiles();
        self.persist();
    }

    fn export_code(&mut self) {
        match profile_code::encode_style(&self.state.active_style) {
            Ok(code) => {
                self.export_code_buffer = code.clone();
                self.status = "Crosshair code copied to clipboard.".to_owned();
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(code);
                }
            }
            Err(error) => self.status = format!("Failed to export code: {error}"),
        }
    }

    fn import_code(&mut self) {
        match profile_code::decode_style(&self.import_code_buffer) {
            Ok(style) => {
                self.state.active_style = style;
                self.sync_crosshair();
                self.persist();
                self.status = "Imported crosshair code.".to_owned();
            }
            Err(error) => self.status = format!("Failed to import code: {error}"),
        }
    }

    fn export_macro_step(&mut self, preset_id: u32, step_index: usize, step: &MacroStep) {
        match crate::macro_code::encode_step(step) {
            Ok(code) => {
                self.status = Self::tr_lang(
                    self.state.ui_language,
                    "Step code copied to clipboard.",
                    "Đã sao chép mã bước vào clipboard.",
                )
                .to_owned();
                self.macro_step_export_feedback_until =
                    Some(Instant::now() + Duration::from_millis(1200));
                self.macro_step_export_feedback_target = Some((preset_id, step_index));
                self.macro_share_clipboard_kind = MacroShareCodeKind::Step;
                self.macro_share_clipboard_checked_at = Some(Instant::now());
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(code);
                }
            }
            Err(error) => self.status = format!("Failed to export step: {error}"),
        }
    }

    fn import_macro_step_from_clipboard(
        &mut self,
        group_id: u32,
        preset_id: u32,
        insert_after_index: Option<usize>,
    ) {
        let mut clipboard = match Clipboard::new() {
            Ok(cb) => cb,
            Err(e) => {
                self.status = format!("Clipboard error: {e}");
                return;
            }
        };
        let code = match clipboard.get_text() {
            Ok(text) => text,
            Err(e) => {
                self.status = format!("Failed to read clipboard: {e}");
                return;
            }
        };
        match crate::macro_code::decode_step(&code) {
            Ok(step) => {
                if let Some(group) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|g| g.id == group_id)
                {
                    if let Some(preset) = group.presets.iter_mut().find(|p| p.id == preset_id) {
                        if let Some(idx) = insert_after_index {
                            if idx < preset.steps.len() {
                                preset.steps.insert(idx + 1, step);
                            } else {
                                preset.steps.push(step);
                            }
                        } else {
                            preset.steps.push(step);
                        }
                        self.sync_macro_presets();
                        self.persist();
                        self.status = Self::tr_lang(
                            self.state.ui_language,
                            "Step imported successfully.",
                            "Đã nhập bước thành công.",
                        )
                        .to_owned();
                    }
                }
            }
            Err(error) => self.status = format!("Import failed: {error}"),
        }
    }

    fn export_macro_preset(&mut self, preset_id: u32, preset: &MacroPreset) {
        match crate::macro_code::encode_preset(preset) {
            Ok(code) => {
                self.status = Self::tr_lang(
                    self.state.ui_language,
                    "Preset code copied to clipboard.",
                    "Đã sao chép mã preset vào clipboard.",
                )
                .to_owned();
                self.macro_preset_export_feedback_until =
                    Some(Instant::now() + Duration::from_millis(1200));
                self.macro_preset_export_feedback_target = Some(preset_id);
                self.macro_share_clipboard_kind = MacroShareCodeKind::Preset;
                self.macro_share_clipboard_checked_at = Some(Instant::now());
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(code);
                }
            }
            Err(error) => self.status = format!("Failed to export preset: {error}"),
        }
    }

    fn import_macro_preset_from_clipboard(
        &mut self,
        group_id: u32,
        insert_after_preset_id: Option<u32>,
    ) {
        let mut clipboard = match Clipboard::new() {
            Ok(cb) => cb,
            Err(e) => {
                self.status = format!("Clipboard error: {e}");
                return;
            }
        };
        let code = match clipboard.get_text() {
            Ok(text) => text,
            Err(e) => {
                self.status = format!("Failed to read clipboard: {e}");
                return;
            }
        };
        match crate::macro_code::decode_preset(&code) {
            Ok(mut preset) => {
                let id = self.state.next_macro_preset_id.max(1);
                self.state.next_macro_preset_id = id + 1;
                preset.id = id;
                if let Some(group) = self
                    .state
                    .macro_groups
                    .iter_mut()
                    .find(|g| g.id == group_id)
                {
                    if let Some(target_id) = insert_after_preset_id {
                        if let Some(idx) = group.presets.iter().position(|p| p.id == target_id) {
                            group.presets.insert(idx + 1, preset);
                        } else {
                            group.presets.push(preset);
                        }
                    } else {
                        group.presets.push(preset);
                    }
                    self.reconcile_master_presets();
                    self.sync_macro_presets();
                    self.persist();
                    self.status = Self::tr_lang(
                        self.state.ui_language,
                        "Preset imported successfully.",
                        "Đã nhập preset thành công.",
                    )
                    .to_owned();
                }
            }
            Err(error) => self.status = format!("Import failed: {error}"),
        }
    }

    fn export_macro_group(&mut self, group_id: u32, group: &MacroGroup) {
        match crate::macro_code::encode_group(group) {
            Ok(code) => {
                self.status = Self::tr_lang(
                    self.state.ui_language,
                    "Group code copied to clipboard.",
                    "Đã sao chép mã nhóm vào clipboard.",
                )
                .to_owned();
                self.macro_group_export_feedback_until =
                    Some(Instant::now() + Duration::from_millis(1200));
                self.macro_group_export_feedback_target = Some(group_id);
                self.macro_share_clipboard_kind = MacroShareCodeKind::Group;
                self.macro_share_clipboard_checked_at = Some(Instant::now());
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(code);
                }
            }
            Err(error) => self.status = format!("Failed to export group: {error}"),
        }
    }

    fn import_macro_group_from_clipboard(
        &mut self,
        folder_id: Option<u32>,
        insert_after_group_id: Option<u32>,
    ) {
        let mut clipboard = match Clipboard::new() {
            Ok(cb) => cb,
            Err(e) => {
                self.status = format!("Clipboard error: {e}");
                return;
            }
        };
        let code = match clipboard.get_text() {
            Ok(text) => text,
            Err(e) => {
                self.status = format!("Failed to read clipboard: {e}");
                return;
            }
        };
        match crate::macro_code::decode_group(&code) {
            Ok(mut group) => {
                let id = self.state.next_macro_group_id.max(1);
                self.state.next_macro_group_id = id + 1;
                group.id = id;
                group.name = self.unique_macro_group_name(&group.name);
                group.folder_id = folder_id;

                for preset in &mut group.presets {
                    let preset_id = self.state.next_macro_preset_id.max(1);
                    self.state.next_macro_preset_id = preset_id + 1;
                    preset.id = preset_id;
                }

                if let Some(target_id) = insert_after_group_id {
                    if let Some(idx) = self
                        .state
                        .macro_groups
                        .iter()
                        .position(|g| g.id == target_id)
                    {
                        self.state.macro_groups.insert(idx + 1, group);
                    } else {
                        self.state.macro_groups.push(group);
                    }
                } else {
                    self.state.macro_groups.push(group);
                }
                self.pending_macro_group_scroll_target = Some(id);
                self.reconcile_master_presets();
                self.sync_macro_presets();
                self.persist();
                self.status = Self::tr_lang(
                    self.state.ui_language,
                    "Group imported successfully.",
                    "Đã nhập nhóm thành công.",
                )
                .to_owned();
            }
            Err(error) => self.status = format!("Import failed: {error}"),
        }
    }

    fn reload_custom_assets(&mut self) {
        match self.paths.list_crosshair_assets() {
            Ok(assets) => {
                self.custom_assets = assets;
                self.status = "Reloaded custom crosshair folder.".to_owned();
            }
            Err(error) => self.status = format!("Failed to scan custom folder: {error}"),
        }
    }

    fn reload_open_windows(&mut self) {
        self.schedule_open_windows_refresh(Some("Reloaded open window list.".to_owned()));
    }

    fn refresh_open_windows_now(&mut self) {
        self.schedule_open_windows_refresh(None);
    }

    fn active_panel_needs_open_windows(panel: AppPanel) -> bool {
        matches!(
            panel,
            AppPanel::WindowPresets
                | AppPanel::Pin
                | AppPanel::Macros
                | AppPanel::Vision
                | AppPanel::Commands
        )
    }

    fn active_panel_needs_audio_sense_devices(panel: AppPanel) -> bool {
        matches!(panel, AppPanel::AudioSense | AppPanel::Macros)
    }

    fn all_panels_for_background_preload() -> &'static [AppPanel] {
        &[
            AppPanel::Crosshair,
            AppPanel::WindowPresets,
            AppPanel::Pin,
            AppPanel::Mouse,
            AppPanel::Vision,
            AppPanel::AudioSense,
            AppPanel::Zoom,
            AppPanel::Modes,
            AppPanel::Macros,
            AppPanel::Commands,
            AppPanel::Sound,
            AppPanel::Media,
            AppPanel::Hud,
            AppPanel::Ocr,
            AppPanel::Geometry,
        ]
    }

    fn panel_is_warmed(&self, panel: AppPanel) -> bool {
        self.warmed_panels.contains(&panel)
    }

    fn begin_panel_warmup(&mut self, panel: AppPanel) {
        if self.panel_is_warmed(panel) {
            return;
        }
        if self.panel_warmup_target == Some(panel) && self.panel_warmup_frames_remaining > 0 {
            return;
        }
        self.panel_warmup_target = Some(panel);
        self.panel_warmup_frames_remaining = 1;
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

    fn render_panel_loading_shell(&self, ui: &mut egui::Ui, panel: AppPanel) {
        let title = self.panel_label(panel);
        let subtitle = self.tr(
            "Preparing this panel...",
            "Dang chuan bi panel nay...",
        );
        let detail = self.tr(
            "The window is ready. Content will finish loading in the next moments.",
            "Cua so da hien. Noi dung se tiep tuc hoan thien ngay sau do.",
        );
        ui.with_layout(
            egui::Layout::top_down_justified(egui::Align::Center),
            |ui| {
                ui.add_space((ui.available_height() * 0.18).max(24.0));
                ui.spinner();
                ui.add_space(14.0);
                ui.label(RichText::new(title).heading().strong());
                ui.add_space(6.0);
                ui.label(RichText::new(subtitle).strong());
                ui.add_space(4.0);
                ui.label(RichText::new(detail).small().weak());
            },
        );
    }

    fn schedule_open_windows_refresh(&mut self, status: Option<String>) {
        if self.open_windows_loading {
            return;
        }
        self.open_windows_loading = true;
        let ui_tx = self.ui_tx.clone();
        std::thread::spawn(move || {
            let windows = window_list::list_open_windows()
                .into_iter()
                .map(|item| item.selector)
                .collect();
            let _ = ui_tx.send(UiCommand::OpenWindowsLoaded { windows, status });
        });
    }

    fn ensure_open_windows_ready(&mut self, force: bool) {
        if self.open_windows_loading {
            return;
        }
        if !force
            && self.open_windows_loaded_once
            && self.last_window_refresh_at.elapsed() < OPEN_WINDOWS_REFRESH_INTERVAL
        {
            return;
        }
        self.schedule_open_windows_refresh(None);
    }

    fn schedule_audio_sense_devices_refresh(&mut self) {
        if self.audio_sense_devices_loading {
            return;
        }
        self.audio_sense_devices_loading = true;
        let ui_tx = self.ui_tx.clone();
        std::thread::spawn(move || {
            let devices = audiosense::list_capture_devices().unwrap_or_default();
            let _ = ui_tx.send(UiCommand::AudioSenseDevicesLoaded { devices });
        });
    }

    fn ensure_audio_sense_devices_ready(&mut self, force: bool) {
        if self.audio_sense_devices_loading {
            return;
        }
        if !force
            && self.audio_sense_devices_loaded_once
            && self.last_audio_sense_devices_refresh_at.elapsed()
                < AUDIO_SENSE_DEVICES_REFRESH_INTERVAL
        {
            return;
        }
        self.schedule_audio_sense_devices_refresh();
    }

    #[cfg(windows)]
    #[cfg(not(windows))]
    fn precise_image_search_capture_pointer(&self, _ctx: &egui::Context) -> Option<egui::Pos2> {
        None
    }

    #[cfg(windows)]
    #[cfg(not(windows))]
    fn current_screen_cursor_pos() -> Option<(i32, i32)> {
        None
    }

    fn play_startup_sound_once(&mut self) {
        if self.startup_sound_played {
            return;
        }
        self.startup_sound_played = true;
        if self.state.audio_settings.startup.enabled {
            audio::play_clip_async(self.state.audio_settings.startup.clone());
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

    fn modal_safe_rect(ctx: &egui::Context) -> egui::Rect {
        ctx.screen_rect().shrink(18.0)
    }

    fn centered_modal_placement(
        ctx: &egui::Context,
        desired_size: egui::Vec2,
        min_size: egui::Vec2,
    ) -> (egui::Vec2, egui::Pos2) {
        let safe_rect = Self::modal_safe_rect(ctx);
        let panel_size = vec2(
            desired_size
                .x
                .min(safe_rect.width())
                .max(min_size.x.min(safe_rect.width())),
            desired_size
                .y
                .min(safe_rect.height())
                .max(min_size.y.min(safe_rect.height())),
        );
        let center = safe_rect.center();
        let panel_pos = egui::Pos2::new(
            (center.x - panel_size.x * 0.5)
                .round()
                .clamp(safe_rect.left(), safe_rect.right() - panel_size.x),
            (center.y - panel_size.y * 0.5)
                .round()
                .clamp(safe_rect.top(), safe_rect.bottom() - panel_size.y),
        );
        (panel_size, panel_pos)
    }

    fn render_blocking_confirmation_modal(
        &self,
        ctx: &egui::Context,
        modal_key: impl std::hash::Hash,
        title: &str,
        message: &str,
        confirm_label: &str,
        cancel_label: &str,
    ) -> Option<bool> {
        self.render_modal_backdrop(ctx, true);
        let (panel_size, panel_pos) =
            Self::centered_modal_placement(ctx, vec2(380.0, 160.0), vec2(320.0, 140.0));
        let mut outcome = None;
        egui::Area::new(egui::Id::new((modal_key, "blocking-confirmation-modal")))
            .order(Order::Foreground)
            .fixed_pos(panel_pos)
            .interactable(true)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(if self.state.ui_theme == UiThemeMode::Dark {
                        Color32::from_rgba_premultiplied(24, 26, 32, 250)
                    } else {
                        Color32::from_rgba_premultiplied(248, 248, 250, 250)
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
                    .inner_margin(Margin::same(20))
                    .show(ui, |ui| {
                        ui.set_min_size(panel_size);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).strong());
                            ui.add_space(10.0);
                            ui.label(message);
                            ui.add_space(18.0);
                            ui.horizontal(|ui| {
                                if ui
                                    .add_sized(
                                        [120.0, 26.0],
                                        Button::new(confirm_label)
                                            .fill(Color32::from_rgb(176, 72, 72)),
                                    )
                                    .clicked()
                                {
                                    outcome = Some(true);
                                }
                                if ui
                                    .add_sized([100.0, 26.0], Button::new(cancel_label))
                                    .clicked()
                                {
                                    outcome = Some(false);
                                }
                            });
                        });
                    });
            });
        if outcome.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            outcome = Some(false);
        }
        outcome
    }

    fn render_modal_backdrop(&self, ctx: &egui::Context, open: bool) {
        if !open {
            return;
        }

        let rect = ctx.content_rect();
        egui::Area::new(egui::Id::new("settings-modal-backdrop"))
            .order(Order::Middle)
            .fixed_pos(rect.min)
            .interactable(true)
            .show(ctx, |ui| {
                let (backdrop_rect, _) =
                    ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
                ui.painter().rect_filled(
                    backdrop_rect,
                    egui::CornerRadius::ZERO,
                    Color32::from_rgba_premultiplied(18, 18, 24, 150),
                );
            });
    }

    fn capture_is_active(&self, target: &CaptureRequest) -> bool {
        self.capture_target.as_ref() == Some(target)
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

    fn powershell_quote(value: &str) -> String {
        value.replace('\'', "''")
    }

    fn choose_audio_file(&mut self, startup: bool) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a"])
            .pick_file()
        else {
            return;
        };
        let path_str = path.to_string_lossy().to_string();
        let duration = audio::load_duration_ms(&path_str).ok();
        let clip = if startup {
            &mut self.state.audio_settings.startup
        } else {
            &mut self.state.audio_settings.exit
        };
        clip.file_path = path_str;
        clip.start_ms = 0;
        clip.end_ms = duration.unwrap_or(0);
        if startup {
            self.startup_clip_duration_ms = duration;
            self.show_startup_audio_editor = true;
        } else {
            self.exit_clip_duration_ms = duration;
            self.show_exit_audio_editor = true;
        }
        self.refresh_audio_waveform(startup);
        self.sync_audio_settings();
        self.persist();
    }

    fn choose_audio_file_for_sound_preset(&mut self, preset_id: u32) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a"])
            .pick_file()
        else {
            return;
        };
        let path_str = path.to_string_lossy().to_string();
        let duration = audio::load_duration_ms(&path_str).ok();
        if let Some(preset) = self
            .state
            .audio_settings
            .presets
            .iter_mut()
            .find(|preset| preset.id == preset_id)
        {
            preset.clip.file_path = path_str.clone();
            preset.clip.start_ms = 0;
            preset.clip.end_ms = duration.unwrap_or(0);
            preset.clip.enabled = true;
            self.sound_preset_clip_duration_ms
                .insert(preset_id, duration);
            self.show_sound_preset_audio_editor.insert(preset_id);
            self.refresh_audio_waveform_for_path(&path_str);
            self.preview_cursor = None;
            self.trim_timeline_zoom = 1.0;
            self.sync_audio_settings();
            self.persist();
        }
    }

    fn replace_sound_preset_audio_file(&mut self, preset_id: u32, path: PathBuf) {
        let path_str = path.to_string_lossy().to_string();
        let duration = audio::load_duration_ms(&path_str).ok();
        if let Some(preset) = self
            .state
            .audio_settings
            .presets
            .iter_mut()
            .find(|preset| preset.id == preset_id)
        {
            preset.clip.file_path = path_str.clone();
            preset.clip.start_ms = 0;
            preset.clip.end_ms = duration.unwrap_or(0);
            preset.clip.enabled = true;
            self.sound_preset_clip_duration_ms
                .insert(preset_id, duration);
            self.show_sound_preset_audio_editor.insert(preset_id);
            self.refresh_audio_waveform_for_path(&path_str);
            self.preview_cursor = None;
            self.trim_timeline_zoom = 1.0;
            self.sync_audio_settings();
            self.persist();
        }
    }

    fn choose_audio_file_for_library_item(&mut self, item_id: u32) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a"])
            .pick_file()
        else {
            return;
        };
        let path_str = path.to_string_lossy().to_string();
        let duration = audio::load_duration_ms(&path_str).ok();
        if let Some(item) = self
            .state
            .audio_settings
            .library
            .iter_mut()
            .find(|item| item.id == item_id)
        {
            item.clip.file_path = path_str.clone();
            item.clip.start_ms = 0;
            item.clip.end_ms = duration.unwrap_or(0);
            item.clip.enabled = true;
            self.library_clip_duration_ms.insert(item_id, duration);
            self.show_library_audio_editor.insert(item_id);
            self.refresh_audio_waveform_for_path(&path_str);
            self.preview_cursor = None;
            self.trim_timeline_zoom = 1.0;
            self.sync_audio_settings();
            self.persist();
        }
    }

    fn choose_video_file_for_preset(&mut self, preset_id: u32) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Video", &["webm", "mp4", "mov", "mkv", "avi"])
            .pick_file()
        else {
            return;
        };
        let path_str = path.to_string_lossy().to_string();
        let duration = crate::media::load_video_metadata(&path_str)
            .ok()
            .map(|meta| meta.duration_ms);
        self.stop_active_video_preview();
        self.stop_active_video_overlay_preview();
        self.clear_video_preview_for_preset(preset_id);
        if let Some(preset) = self
            .state
            .audio_settings
            .video_presets
            .iter_mut()
            .find(|preset| preset.id == preset_id)
        {
            preset.clip.file_path = path_str.clone();
            preset.clip.start_ms = 0;
            preset.clip.end_ms = duration.unwrap_or(0);
            preset.clip.enabled = true;
            self.video_preset_clip_duration_ms
                .insert(preset_id, duration);
            self.video_preview_cursor_ms.insert(preset_id, 0);
            self.refresh_audio_waveform_for_path(&path_str);
            self.sync_audio_settings();
            self.persist();
        }
    }

    fn save_clip_to_library(&mut self, name_prefix: &str, clip: &AudioClipSettings) {
        if clip.file_path.trim().is_empty() {
            self.status = "Choose a sound file before saving it to the library.".to_owned();
            return;
        }
        let id = self.state.audio_settings.next_library_item_id.max(1);
        self.state.audio_settings.next_library_item_id = id + 1;
        let mut item = SoundLibraryItem::new(id);
        item.name = format!("{name_prefix} {id}");
        item.clip = clip.clone();
        item.clip.enabled = true;
        self.state.audio_settings.library.push(item);
        self.library_clip_duration_ms
            .insert(id, audio_duration(clip));
        self.show_library_audio_editor.insert(id);
        self.sync_audio_settings();
        self.persist();
        self.status = format!("Saved sound into library item {id}.");
    }

    fn refresh_audio_waveform(&mut self, startup: bool) {
        let clip = if startup {
            &self.state.audio_settings.startup
        } else {
            &self.state.audio_settings.exit
        };
        let path = clip.file_path.trim();
        if path.is_empty() {
            return;
        }
        let _ = audio::preload_preview_audio(path);
        if self.audio_waveforms.contains_key(path) {
            return;
        }
        if let Ok(waveform) = audio::load_waveform(path, 320) {
            self.audio_waveforms.insert(path.to_owned(), waveform);
        }
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
            let _ = audio::preload_preview_audio(&trimmed);
            let waveform = audio::load_waveform(&trimmed, 320).unwrap_or_default();
            let duration_ms = audio::load_duration_ms(&trimmed).ok();
            let _ = ui_tx.send(UiCommand::AudioWaveformLoaded {
                path: trimmed,
                waveform,
                duration_ms,
            });
        });
    }

    fn ensure_video_preview_worker_ready(
        &mut self,
    ) -> crossbeam_channel::Sender<crate::media::VideoFrameRequest> {
        if let Some(tx) = &self.video_frame_tx {
            return tx.clone();
        }
        let tx = crate::media::start_video_preview_worker(self.ui_tx.clone());
        self.video_frame_tx = Some(tx.clone());
        tx
    }

    fn ensure_video_preview_frame(
        &mut self,
        ctx: &egui::Context,
        preset_id: u32,
        path: &str,
        start_ms: u64,
        max_width: i32,
        max_height: i32,
    ) -> Option<VideoPreviewView> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return None;
        }
        let rounded_start_ms = (start_ms / 15) * 15;
        if let Some(cache) = self.video_preview_cache.get(&preset_id)
            && cache.source_path == trimmed
            && cache.start_ms == rounded_start_ms
            && cache.max_width == max_width
            && cache.max_height == max_height
        {
            return Some(cache.view.clone());
        }

        let already_requested = self.video_preview_requested.get(&preset_id)
            .map(|(p, ms)| p == trimmed && *ms == rounded_start_ms)
            .unwrap_or(false);

        if !already_requested {
            self.video_preview_requested.insert(preset_id, (trimmed.to_owned(), rounded_start_ms));
            let is_playing = self.active_video_preview_preset_id == Some(preset_id);
            let video_frame_tx = self.ensure_video_preview_worker_ready();
            let _ = video_frame_tx.send(crate::media::VideoFrameRequest {
                preset_id,
                path: trimmed.to_owned(),
                start_ms: rounded_start_ms,
                max_width,
                max_height,
                is_playing,
            });
        }

        self.video_preview_cache.get(&preset_id).map(|cache| cache.view.clone())
    }

    fn start_active_video_overlay_preview(&mut self, preset_id: u32, start_ms: u64) {
        let _ = self.overlay_tx.send(OverlayCommand::StopVideoPlayback);
        let _ = self.overlay_tx.send(OverlayCommand::PlayVideoPresetFrom(preset_id, start_ms));
        self.active_video_overlay_preset_id = Some(preset_id);
    }

    fn stop_active_video_overlay_preview(&mut self) {
        let _ = self.overlay_tx.send(OverlayCommand::StopVideoPlayback);
        self.active_video_overlay_preset_id = None;
    }

    fn start_video_preview(
        &mut self,
        preset_id: u32,
        clip: &VideoClipSettings,
        start_ms: u64,
    ) -> anyhow::Result<()> {
        let trimmed = clip.file_path.trim();
        if trimmed.is_empty() {
            anyhow::bail!("Choose a video file first");
        }

        let clip_end_ms = if clip.end_ms > clip.start_ms {
            clip.end_ms
        } else {
            crate::media::load_video_metadata(trimmed)
                .ok()
                .map(|meta| meta.duration_ms)
                .filter(|duration_ms| *duration_ms > start_ms)
                .unwrap_or(start_ms.saturating_add(1))
        };
        let next_start_ms = start_ms.min(clip_end_ms.saturating_sub(1));

        crate::audio::play_video_audio_preview(trimmed, next_start_ms, clip_end_ms)?;
        self.active_video_preview_preset_id = Some(preset_id);
        self.active_video_preview_started_at = Some(Instant::now());
        self.active_video_preview_start_ms = next_start_ms;
        self.video_preview_cursor_ms.insert(preset_id, next_start_ms);
        Ok(())
    }

    fn stop_active_video_preview(&mut self) {
        crate::audio::stop_video_audio_preview();
        self.active_video_preview_preset_id = None;
        self.active_video_preview_started_at = None;
        self.active_video_preview_start_ms = 0;
    }

    fn clear_video_preview_for_preset(&mut self, preset_id: u32) {
        self.video_preview_cache.remove(&preset_id);
    }

    fn clear_video_preview_cache(&mut self) {
        self.video_preview_cache.clear();
        self.video_chroma_pick_preset_id = None;
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

    fn preset_frame(ui: &egui::Ui, enabled: bool) -> egui::Frame {
        let fill = if enabled {
            Color32::from_rgba_premultiplied(32, 92, 52, 120)
        } else {
            ui.visuals().faint_bg_color
        };
        let stroke_color = if enabled {
            Color32::from_rgb(108, 224, 148)
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        egui::Frame::group(ui.style())
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke_color))
    }

    fn folder_frame(ui: &egui::Ui, active: bool, hovered: bool) -> egui::Frame {
        let (fill, stroke_color) = if active {
            let border = if hovered {
                Color32::from_rgb(255, 170, 75)
            } else {
                Color32::from_rgb(220, 130, 45)
            };
            (Color32::from_rgba_premultiplied(100, 60, 20, 100), border)
        } else {
            let border = if hovered {
                Color32::from_rgb(190, 135, 75)
            } else {
                Color32::from_rgb(140, 90, 45)
            };
            (Color32::from_rgba_premultiplied(45, 30, 15, 60), border)
        };
        egui::Frame::group(ui.style())
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke_color))
    }

    fn show_folder_card<R>(
        ui: &mut egui::Ui,
        active: bool,
        hovered: bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> (R, egui::Response) {
        let dark_mode = ui.visuals().dark_mode;
        let res = Self::folder_frame(ui, active, hovered).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let previous = ui.visuals().override_text_color;
            if dark_mode {
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(255, 240, 220));
            }
            let output = add_contents(ui);
            ui.visuals_mut().override_text_color = previous;
            output
        });
        (res.inner, res.response)
    }

    fn preset_body_text_color(dark_mode: bool, enabled: bool) -> Color32 {
        match (dark_mode, enabled) {
            (true, true) => Color32::from_rgb(248, 250, 252),
            (true, false) => Color32::from_rgb(214, 222, 232),
            (false, true) => Color32::from_rgb(250, 250, 250),
            (false, false) => Color32::from_rgb(32, 32, 32),
        }
    }

    fn preset_header_name_width(_ui: &egui::Ui) -> f32 {
        160.0
    }

    fn show_preset_card<R>(
        ui: &mut egui::Ui,
        enabled: bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let dark_mode = ui.visuals().dark_mode;
        Self::preset_frame(ui, enabled)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let previous = ui.visuals().override_text_color;
                if dark_mode {
                    ui.visuals_mut().override_text_color =
                        Some(Self::preset_body_text_color(dark_mode, enabled));
                }
                let output = add_contents(ui);
                ui.visuals_mut().override_text_color = previous;
                output
            })
            .inner
    }

    fn show_macro_preset_card<R>(
        ui: &mut egui::Ui,
        group_enabled: bool,
        preset_enabled: bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let dark_mode = ui.visuals().dark_mode;
        let (fill, stroke_color) = if group_enabled {
            if preset_enabled {
                // Combination 1: Group Active + Preset Active (Bright glowing green)
                (
                    Color32::from_rgba_premultiplied(32, 92, 52, 120),
                    Color32::from_rgb(108, 224, 148),
                )
            } else {
                // Combination 2: Group Active + Preset Inactive (Restore user's desired old behavior!)
                (
                    ui.visuals().faint_bg_color,
                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                )
            }
        } else {
            if preset_enabled {
                // Combination 3: Group Inactive + Preset Active (Armed but dormant - show sleep green tint!)
                (
                    Color32::from_rgba_premultiplied(25, 65, 40, 60),
                    Color32::from_rgb(60, 120, 85),
                )
            } else {
                // Combination 4: Group Inactive + Preset Inactive (Fully dark/dormant)
                (
                    ui.visuals().faint_bg_color,
                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                )
            }
        };
        let frame = egui::Frame::group(ui.style())
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke_color));
        frame
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let previous = ui.visuals().override_text_color;
                if dark_mode {
                    ui.visuals_mut().override_text_color = Some(Self::preset_body_text_color(
                        dark_mode,
                        preset_enabled, // Align text colors with preset armed state
                    ));
                }
                let output = add_contents(ui);
                ui.visuals_mut().override_text_color = previous;
                output
            })
            .inner
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
        if response.changed() {
            Self::apply_vietnamese_input_mode(response, text, enabled, mode);
        }
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
            Self::tr_lang(language, "Not set", "Chưa gán").to_owned()
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
                            "Bấm để xóa phím tắt này",
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
                        "Xem trước trigger đang bấm",
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
                        "Xem truoc trigger dang bat",
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

        let mut list: Vec<String> = vars.into_iter().collect();
        list.sort();
        list
    }

    fn format_macro_trigger_ui(language: UiLanguage, preset: &MacroPreset) -> String {
        let bindings = Self::macro_trigger_bindings(preset);
        let label = hotkey::format_binding_list(&bindings);
        if label == "Not set" {
            Self::tr_lang(language, "Not set", "Chưa gán").to_owned()
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

    fn app_brand_title(&self) -> &'static str {
        "MacroNest"
    }

    fn versions_are_equal(v1: &str, v2: &str) -> bool {
        let mut parts1: Vec<u32> = v1
            .split('.')
            .map(|s| s.parse::<u32>().unwrap_or(0))
            .collect();
        let mut parts2: Vec<u32> = v2
            .split('.')
            .map(|s| s.parse::<u32>().unwrap_or(0))
            .collect();
        while parts1.last() == Some(&0) {
            parts1.pop();
        }
        while parts2.last() == Some(&0) {
            parts2.pop();
        }
        parts1 == parts2
    }

    fn app_version_label(&self) -> &'static str {
        option_env!("MACRONEST_BUILD_TAG").unwrap_or("1.0")
    }

    fn app_brand_subtitle(&self) -> &'static str {
        match self.state.ui_language {
            UiLanguage::English => "Macro control, pin, settings, sound, and window tools",
            UiLanguage::Vietnamese => self.tr(
                "Macro control, pin, settings, sound, and window tools",
                "Macro control, pin, settings, sound, and window tools",
            ),
            UiLanguage::Icon => "Macro control, pin, settings, sound, and window tools",
        }
    }

    fn panel_icon(panel: AppPanel) -> u32 {
        match panel {
            AppPanel::Crosshair => 0xe3dc,
            AppPanel::WindowPresets => 0xe8f0,
            AppPanel::Pin | AppPanel::Zoom => 0xe55f,
            AppPanel::Mouse => 0xe323,
            AppPanel::Vision => 0xe8b6,
            AppPanel::AudioSense => 0xe050,
            AppPanel::Macros | AppPanel::Modes => 0xe312,
            AppPanel::Commands => 0xe32a,
            AppPanel::Sound | AppPanel::Media => 0xe050,
            AppPanel::Hud => 0xe8b8,
            AppPanel::Ocr => 0xe8b6,
            AppPanel::Geometry => 0xe158,
        }
    }

    fn panel_label(&self, panel: AppPanel) -> &'static str {
        let english = match panel {
            AppPanel::Crosshair => "Crosshair",
            AppPanel::WindowPresets => "Window Control",
            AppPanel::Pin | AppPanel::Zoom => "Pin",
            AppPanel::Mouse => "Mouse",
            AppPanel::Vision => "Vision",
            AppPanel::AudioSense => "AudioSense",
            AppPanel::Macros | AppPanel::Modes => "Macro",
            AppPanel::Commands => "Commands",
            AppPanel::Sound => "Media",
            AppPanel::Media => "Editor",
            AppPanel::Hud => "HUD",
            AppPanel::Ocr => "OCR",
            AppPanel::Geometry => "Geometry",
        };
        if panel == AppPanel::Ocr {
            Self::tr_lang(self.state.ui_language, "OCR", "Nhận dạng chữ (OCR)")
        } else {
            Self::tr_lang(self.state.ui_language, english, english)
        }
    }

    fn language_button_text(&self) -> RichText {
        match self.state.ui_language {
            UiLanguage::English => RichText::new("EN").strong(),
            UiLanguage::Vietnamese => RichText::new("VI").strong(),
            UiLanguage::Icon => RichText::new("EN").strong(),
        }
    }

    fn theme_button_text(&self) -> RichText {
        match self.state.ui_theme {
            UiThemeMode::Dark => Self::material_icon_text(0xe51c, 18.0),
            UiThemeMode::Light => Self::material_icon_text(0xe518, 18.0),
        }
    }

    fn startup_loading_text(&self) -> &'static str {
        match self.state.ui_language {
            UiLanguage::English => "loading macro tools, overlays, and UI",
            UiLanguage::Vietnamese => self.tr(
                "loading macro tools, overlays, and UI",
                "loading macro tools, overlays, and UI",
            ),
            UiLanguage::Icon => "loading macro tools, overlays, and UI",
        }
    }

    fn titlebar_language_tooltip(&self) -> &'static str {
        self.tr("Switch language", "Đổi ngôn ngữ")
    }

    fn vietnamese_input_button_text(&self) -> RichText {
        if self.state.vietnamese_input_enabled {
            RichText::new("V")
                .strong()
                .color(Color32::from_rgb(235, 76, 80))
        } else {
            RichText::new("E")
                .strong()
                .color(Color32::from_rgb(76, 135, 235))
        }
    }

    fn titlebar_vietnamese_input_tooltip(&self) -> &'static str {
        if !self.state.vietnamese_input_enabled {
            self.tr("Vietnamese input: off", "Gõ tiếng Việt: tắt")
        } else {
            match self.state.vietnamese_input_mode {
                VietnameseInputMode::Telex => {
                    self.tr("Vietnamese input: Telex", "Gõ tiếng Việt: Telex")
                }
                VietnameseInputMode::Vni => self.tr("Vietnamese input: VNI", "Gõ tiếng Việt: VNI"),
                VietnameseInputMode::Off => {
                    self.tr("Vietnamese input: Telex", "Gõ tiếng Việt: Telex")
                }
            }
        }
    }

    fn titlebar_theme_tooltip(&self) -> &'static str {
        self.tr("Toggle dark / light theme", "Đổi giao diện sáng / tối")
    }

    fn titlebar_minimize_tooltip(&self) -> &'static str {
        self.tr("Minimize", "Thu nhỏ")
    }

    fn titlebar_maximize_tooltip(&self, maximized: bool) -> &'static str {
        if maximized {
            self.tr("Restore", "Khôi phục")
        } else {
            self.tr("Maximize", "Phóng to")
        }
    }

    fn capture_hint_text(&self) -> String {
        if matches!(
            self.capture_target,
            Some(CaptureRequest::MacroPresetHotkey(_, _))
        ) && let Some(pending) = self.capture_hotkey_combo_keys.as_ref()
        {
            return self.capture_combo_status_text(pending);
        }
        self.tr(
            "Capture mode is active. Hold your combo, then release to save. Press Esc to cancel.",
            "Đang ở chế độ bắt phím. Giữ combo rồi thả tay để lưu. Nhấn Esc để hủy.",
        )
        .to_owned()
    }

    fn titlebar_button(&self, text: RichText, active: bool, danger: bool) -> Button<'static> {
        let (fill, stroke) = match (self.state.ui_theme, active, danger) {
            (_, _, true) => (
                Color32::from_rgba_premultiplied(160, 48, 64, if active { 138 } else { 72 }),
                Color32::from_rgb(222, 106, 126),
            ),
            (UiThemeMode::Dark, true, false) => (
                Color32::from_rgba_premultiplied(74, 146, 118, 166),
                Color32::from_rgb(126, 224, 182),
            ),
            (UiThemeMode::Dark, false, false) => (
                Color32::from_rgba_premultiplied(54, 67, 88, 88),
                Color32::from_rgb(74, 92, 118),
            ),
            (UiThemeMode::Light, true, false) => (
                Color32::from_rgba_premultiplied(72, 156, 116, 120),
                Color32::from_rgb(34, 122, 88),
            ),
            (UiThemeMode::Light, false, false) => (
                Color32::from_rgba_premultiplied(220, 228, 238, 165),
                Color32::from_rgb(188, 198, 214),
            ),
        };
        Button::new(text)
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(8.0)
    }

    fn top_tab_button(&self, text: RichText, selected: bool, emphasized: bool) -> Button<'static> {
        let (fill, stroke) = match (self.state.ui_theme, selected, emphasized) {
            (UiThemeMode::Dark, true, _) => (
                Color32::from_rgba_premultiplied(58, 120, 96, 164),
                Color32::from_rgb(126, 224, 182),
            ),
            (UiThemeMode::Dark, false, true) => (
                Color32::from_rgba_premultiplied(42, 58, 46, 118),
                Color32::from_rgb(92, 180, 148),
            ),
            (UiThemeMode::Dark, false, false) => (
                Color32::from_rgba_premultiplied(34, 42, 56, 72),
                Color32::from_rgb(56, 68, 88),
            ),
            (UiThemeMode::Light, true, _) => (
                Color32::from_rgba_premultiplied(90, 180, 132, 98),
                Color32::from_rgb(34, 122, 88),
            ),
            (UiThemeMode::Light, false, true) => (
                Color32::from_rgba_premultiplied(214, 238, 226, 208),
                Color32::from_rgb(58, 146, 110),
            ),
            (UiThemeMode::Light, false, false) => (
                Color32::from_rgba_premultiplied(230, 236, 242, 165),
                Color32::from_rgb(202, 212, 224),
            ),
        };
        Button::new(text)
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(10.0)
    }

    fn hover_if(response: egui::Response, enabled: bool, text: &str) -> egui::Response {
        if enabled && !text.is_empty() {
            response.on_hover_text(text)
        } else {
            response
        }
    }

    fn truncate_window_title(title: &str, max_chars: usize) -> String {
        let chars: Vec<char> = title.chars().collect();
        if chars.len() > max_chars {
            let mut truncated: String = chars[..max_chars].iter().collect();
            truncated.push_str("...");
            truncated
        } else {
            title.to_owned()
        }
    }

    fn clean_invisible_chars(s: &str) -> String {
        s.chars()
            .filter(|&c| c != '\u{200B}' && c != '\u{200C}' && c != '\u{200D}' && c != '\u{FEFF}')
            .collect()
    }

    fn simplify_window_title(title: &str) -> String {
        let clean = Self::clean_invisible_chars(title);
        let base = Self::selector_base_title(&clean);

        const BROWSER_SUFFIXES: &[&str] = &[
            " - Microsoft Edge",
            " - Google Chrome",
            " - Brave",
            " - Firefox",
            " - Opera GX",
            " - Opera",
            " - Vivaldi",
            " - Chromium",
            " - Tor Browser",
            " - Arc",
            " - Visual Studio Code",
            " - VS Code",
            " - Discord",
            " - Slack",
            " - Spotify",
        ];

        for suffix in BROWSER_SUFFIXES {
            if base.ends_with(suffix) {
                return suffix.trim_start_matches(" - ").to_owned();
            }
        }

        if let Some((_, last)) = base.rsplit_once(" - ") {
            let trimmed = last.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }

        base.to_owned()
    }

    fn render_multi_window_targets(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        label_when_none: &str,
        primary: &mut Option<String>,
        extras: &mut Vec<String>,
        open_windows: &[String],
    ) -> bool {
        let mut changed = false;
        let extras_expanded_id = ui.make_persistent_id((id_source, "extra-target-windows-expanded"));
        let mut extras_expanded = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(extras_expanded_id))
            .unwrap_or(false);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.y = 21.0;
                let display_primary = primary
                    .as_deref()
                    .map(|current| Self::simplify_window_title(current))
                    .unwrap_or_else(|| label_when_none.to_owned());
                let truncated_primary = Self::truncate_window_title(&display_primary, 40);
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
                        for title in open_windows {
                            let display_title = Self::simplify_window_title(title);
                            let truncated_title = Self::truncate_window_title(&display_title, 50);
                            if ui
                                .selectable_label(
                                    primary.as_deref() == Some(title),
                                    truncated_title,
                                )
                                .on_hover_text(title)
                                .clicked()
                            {
                                *primary = Some(title.clone());
                                changed = true;
                            }
                        }
                    });

                let add_btn = Button::new(Self::material_icon_text(0xe145, 12.0));
                if ui
                    .add_sized([24.0, 21.0], add_btn)
                    .on_hover_text(Self::tr_lang(language, "+ Window", "+ Cửa sổ"))
                    .clicked()
                {
                    let next = open_windows
                        .iter()
                        .find(|title| {
                            primary.as_deref() != Some(title.as_str())
                                && !extras.iter().any(|existing| existing == *title)
                        })
                        .cloned()
                        .or_else(|| open_windows.first().cloned())
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
                        let display_extra = Self::simplify_window_title(extra);
                        let truncated_extra = Self::truncate_window_title(&display_extra, 40);
                        egui::ComboBox::from_id_salt((id_source, "extra-target-window", index))
                            .width(320.0)
                            .selected_text(truncated_extra)
                            .show_ui(ui, |ui| {
                                for title in open_windows {
                                    let display_title = Self::simplify_window_title(title);
                                    let truncated_title =
                                        Self::truncate_window_title(&display_title, 50);
                                    if ui
                                        .selectable_label(extra == title, truncated_title)
                                        .on_hover_text(title)
                                        .clicked()
                                    {
                                        *extra = title.clone();
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
        ui.ctx().data_mut(|data| data.insert_temp(extras_expanded_id, extras_expanded));
        changed
    }

    fn selector_base_title(target: &str) -> &str {
        if let Some(prefix) = target.strip_suffix(')')
            && let Some((base, _)) = prefix.rsplit_once(" (0x")
        {
            return base;
        }
        target
    }

    fn grouped_window_selectors(open_windows: &[String]) -> Vec<(String, Vec<String>)> {
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for selector in open_windows {
            let title = Self::simplify_window_title(selector);
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

    fn render_window_target_combo_with_duplicate_mode(
        ui: &mut egui::Ui,
        id_source: impl std::hash::Hash + Copy,
        label_when_none: &str,
        target: &mut Option<String>,
        match_duplicate_window_titles: &mut bool,
        open_windows: &[String],
        width: f32,
        allow_none: bool,
    ) -> bool {
        let mut changed = false;
        let window_groups = Self::grouped_window_selectors(open_windows);
        let selected_text = target
            .as_deref()
            .map(|current| {
                let base_title = Self::simplify_window_title(current);
                let selected_specific_duplicate = !*match_duplicate_window_titles
                    && window_groups
                        .iter()
                        .any(|(title, selectors)| *title == base_title && selectors.len() > 1);
                if selected_specific_duplicate {
                    current.to_owned()
                } else {
                    base_title
                }
            })
            .unwrap_or(label_when_none.to_owned());
        let truncated_selected_text = Self::truncate_window_title(&selected_text, 40);
        let popup_state_id = ui.make_persistent_id((id_source, "duplicate-title-hover"));
        let mut expanded_title = ui
            .ctx()
            .data(|data| data.get_temp::<String>(popup_state_id));

        egui::ComboBox::from_id_salt((id_source, "target-window-combo"))
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

                for (title, selectors) in window_groups {
                    let has_duplicates = selectors.len() > 1;
                    let first_selector = selectors.first().cloned().unwrap_or_default();
                    let main_selected = target
                        .as_deref()
                        .is_some_and(|current| Self::simplify_window_title(current) == title)
                        && *match_duplicate_window_titles;
                    let row_label = if has_duplicates {
                        format!("{title}  >")
                    } else {
                        title.clone()
                    };
                    let truncated_row_label = Self::truncate_window_title(&row_label, 50);
                    let row_response = ui
                        .selectable_label(main_selected, truncated_row_label)
                        .on_hover_text(&title);

                    if row_response.hovered() && has_duplicates {
                        expanded_title = Some(title.clone());
                    }
                    if row_response.clicked() {
                        *target = Some(first_selector.clone());
                        *match_duplicate_window_titles = has_duplicates;
                        expanded_title = None;
                        changed = true;
                    }

                    if has_duplicates && expanded_title.as_deref() == Some(title.as_str()) {
                        ui.indent(
                            (id_source, "duplicate-title-branches", title.as_str()),
                            |ui| {
                                let mut child_hovered = false;
                                for selector in &selectors {
                                    let child_selected = target.as_deref()
                                        == Some(selector.as_str())
                                        && !*match_duplicate_window_titles;
                                    let truncated_selector =
                                        Self::truncate_window_title(selector, 50);
                                    let child_response = ui
                                        .selectable_label(child_selected, truncated_selector)
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
        open_windows: &[String],
    ) -> bool {
        let mut changed = false;
        let extras_expanded_id = ui.make_persistent_id((id_source, "extra-target-windows-expanded"));
        let mut extras_expanded = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(extras_expanded_id))
            .unwrap_or(false);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.y = 21.0;
                changed |= Self::render_window_target_combo_with_duplicate_mode(
                    ui,
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
                    .on_hover_text(Self::tr_lang(language, "+ Window", "+ Cửa sổ"))
                    .clicked()
                {
                    let next = open_windows
                        .iter()
                        .find(|title| {
                            primary.as_deref() != Some(title.as_str())
                                && !extras.iter().any(|existing| existing == *title)
                        })
                        .cloned()
                        .or_else(|| open_windows.first().cloned())
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
        ui.ctx().data_mut(|data| data.insert_temp(extras_expanded_id, extras_expanded));
        changed
    }

    fn macro_action_label(action: MacroAction) -> &'static str {
        match action {
            MacroAction::KeyPress => "KeyPress",
            MacroAction::KeyDown => "KeyDown",
            MacroAction::KeyUp => "KeyUp",
            MacroAction::Wait => "Wait",
            MacroAction::TypeText => "TypeText",
            MacroAction::ApplyWindowPreset => "ResizeWindow",
            MacroAction::FocusWindowPreset => "FocusWindow",
            MacroAction::TriggerMacroPreset => "TriggerMacro",
            MacroAction::TriggerCommandPreset => "TriggerCommand",
            MacroAction::EnableCrosshairProfile => "EnableCrosshair",
            MacroAction::DisableCrosshair => "DisableCrosshair",
            MacroAction::EnablePinPreset => "EnablePin",
            MacroAction::DisablePin => "DisablePin",
            MacroAction::PlayMousePathPreset => "PlayMousePath",
            MacroAction::ApplyMouseSensitivityPreset => "ApplyMouseSens",
            MacroAction::EnableZoomPreset => "EnableZoom",
            MacroAction::DisableZoom => "DisableZoom",
            MacroAction::PlaySoundPreset => "PlaySound",
            MacroAction::PlayVideoPreset => "PlayVideo",
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
            MacroAction::LockKeys => "LockKeys",
            MacroAction::UnlockKeys => "UnlockKeys",
            MacroAction::LockMouse => "LockMouseMove",
            MacroAction::UnlockMouse => "UnlockMouse",
            MacroAction::EnableMacroPreset => "EnableMacro",
            MacroAction::DisableMacroPreset => "DisableMacro",
            MacroAction::StartTimerPreset => "StartTimer",
            MacroAction::PauseTimerPreset => "PauseTimer",
            MacroAction::StopTimerPreset => "StopTimer",
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
            MacroAction::OcrSearch => "OcrSearch",
            MacroAction::DrawGeometry => "DrawGeometry",
            MacroAction::ShowGeometryPreset => "ShowGeometry",
            MacroAction::HideGeometryPreset => "HideGeometry",
            _ => "Legacy (Deprecated)",
        }
    }

    fn macro_action_tooltip(action: MacroAction, language: UiLanguage) -> &'static str {
        match language {
            UiLanguage::Vietnamese => match action {
                MacroAction::KeyPress => "Nhấn và nhả một phím trên bàn phím.",
                MacroAction::KeyDown => "Nhấn giữ một phím bàn phím.",
                MacroAction::KeyUp => "Nhả một phím đang giữ trên bàn phím.",
                MacroAction::Wait => {
                    "Chờ trong khoảng thời gian (Delay - Mili giây), sau đó tiếp tục."
                }
                MacroAction::TypeText => "Nhập chuỗi văn bản từ ô nhập liệu.",
                MacroAction::ApplyWindowPreset => {
                    "Thay đổi kích thước và vị trí cửa sổ bằng preset đã chọn."
                }
                MacroAction::FocusWindowPreset => {
                    "Đưa cửa sổ lên phía trước bằng preset focus đã chọn."
                }
                MacroAction::TriggerMacroPreset => {
                    "Chạy một preset macro khác trong cùng nhóm macro."
                }
                MacroAction::TriggerCommandPreset => {
                    "Chạy một preset câu lệnh tùy chỉnh từ tab Dòng lệnh."
                }
                MacroAction::EnableCrosshairProfile => "Bật một cấu hình tâm ngắm đã lưu.",
                MacroAction::DisableCrosshair => "Tắt hiển thị tâm ngắm overlay.",
                MacroAction::EnablePinPreset => "Bật một preset ghim cửa sổ đã lưu từ tab Ghim.",
                MacroAction::DisablePin => "Tắt hiển thị cửa sổ overlay đang ghim.",
                MacroAction::PlayMousePathPreset => {
                    "Chạy một preset đường di chuyển chuột đã ghi từ tab Chuột."
                }
                MacroAction::ApplyMouseSensitivityPreset => {
                    "Áp dụng một preset độ nhạy chuột từ tab Chuột."
                }
                MacroAction::EnableZoomPreset => "Bật một preset phóng to đã lưu.",
                MacroAction::DisableZoom => "Tắt hiển thị phóng to overlay.",
                MacroAction::PlaySoundPreset => "Phát một preset âm thanh đã chọn từ tab Media.",
                MacroAction::PlayVideoPreset => {
                    "Phát một preset video fullscreen đã chọn từ tab Media."
                }
                MacroAction::StartVisionSearch => {
                    "Bắt đầu quét tìm hình ảnh trong nền bằng preset tìm ảnh đã chọn."
                }
                MacroAction::ScanVisionOnce => {
                    "Quét tìm hình ảnh hoặc màu hoặc đếm pixel một lần duy nhất bằng preset đã chọn."
                }
                MacroAction::StartAudioSensePreset => {
                    "Start pitch detection from an AudioSense preset or custom pitch settings."
                }
                MacroAction::StopAudioSense => {
                    "Dừng AudioSense tùy chỉnh đang chạy hoặc dừng toàn bộ AudioSense."
                }

                MacroAction::StopVisionWait => "Dừng chờ kết quả tìm kiếm hình ảnh.",
                MacroAction::StopVision => "Dừng quét tìm hình ảnh đang chạy trong nền.",
                MacroAction::LoopStart => {
                    "Bắt đầu vòng lặp cho các bước kế tiếp. Nhập số lần lặp, hoặc bật Vô tận (Infinite)."
                }
                MacroAction::LoopEnd => "Kết thúc khối vòng lặp hiện tại.",
                MacroAction::StopIfTriggerPressedAgain => {
                    "Dừng vòng lặp hiện tại nếu bạn nhấn lại phím kích hoạt macro một lần nữa."
                }
                MacroAction::StopIfKeyPressed => {
                    "Thoát vòng lặp hiện tại nếu phím chỉ định trong ô Nhập được nhấn, sau đó tiếp tục các bước sau vòng lặp."
                }
                MacroAction::ShowHud => "Hiển thị HUD từ tab HUD.",
                MacroAction::HideHud => "Ẩn HUD (Menu công cụ) đang hiển thị.",
                MacroAction::LockKeys => "Khóa các phím được liệt kê trong ô Nhập.",
                MacroAction::UnlockKeys => "Mở khóa các phím được liệt kê trong ô Nhập.",
                MacroAction::LockMouse => {
                    "Khóa di chuyển chuột, các cú nhấp chuột và cuộn chuột cho đến khi được mở khóa hoặc dừng macro."
                }
                MacroAction::UnlockMouse => "Mở khóa lại di chuyển chuột và các nút chuột.",
                MacroAction::EnableMacroPreset => {
                    "Bật một preset macro khác trong cùng nhóm macro."
                }
                MacroAction::DisableMacroPreset => {
                    "Tắt một preset macro khác trong cùng nhóm macro."
                }
                MacroAction::EnableStep => {
                    "Bật một hoặc nhiều bước (step) cụ thể trong macro này."
                }
                MacroAction::DisableStep => {
                    "Tắt một hoặc nhiều bước (step) cụ thể trong macro này."
                }
                MacroAction::MouseLeftClick => "Click chuột trái.",
                MacroAction::MouseLeftDown => "Nhấn giữ chuột trái.",
                MacroAction::MouseLeftUp => "Nhả chuột trái.",
                MacroAction::MouseRightClick => "Click chuột phải.",
                MacroAction::MouseRightDown => "Nhấn giữ chuột phải.",
                MacroAction::MouseRightUp => "Nhả chuột phải.",
                MacroAction::MouseMiddleClick => "Click chuột giữa.",
                MacroAction::MouseMiddleDown => "Nhấn giữ chuột giữa.",
                MacroAction::MouseMiddleUp => "Nhả chuột giữa.",
                MacroAction::MouseX1Click => "Click nút chuột X1.",
                MacroAction::MouseX1Down => "Nhấn giữ nút chuột X1.",
                MacroAction::MouseX1Up => "Nhả nút chuột X1.",
                MacroAction::MouseX2Click => "Click nút chuột X2.",
                MacroAction::MouseX2Down => "Nhấn giữ nút chuột X2.",
                MacroAction::MouseX2Up => "Nhả nút chuột X2.",
                MacroAction::MouseWheelUp => "Cuộn chuột lên.",
                MacroAction::MouseWheelDown => "Cuộn chuột xuống.",
                MacroAction::MouseMoveAbsolute => "Di chuyển chuột tới tọa độ tuyệt đối.",
                MacroAction::MouseMoveRelative => "Di chuyển chuột tương đối (theo lượng pixel).",
                MacroAction::IfStart => {
                    "Bắt đầu khối điều kiện Nếu (If). Chỉ chạy các bước bên trong nếu điều kiện biến được thỏa mãn."
                }
                MacroAction::Else => {
                    "Khối Ngược lại (Else). Chạy các bước bên trong nếu điều kiện Nếu (If) bên trên KHÔNG được thỏa mãn."
                }
                MacroAction::IfEnd => "Kết thúc khối điều kiện Hiện tại.",
                MacroAction::SetVariable => {
                    "Đặt giá trị cho một biến (số nguyên hoặc sao chép từ biến khác)."
                }
                MacroAction::OcrSearch => {
                    "Quét vùng màn hình qua Windows OCR Native để nhận diện chữ/số."
                }
                MacroAction::DrawGeometry => {
                    "Vẽ một hình hình học lên overlay màn hình bằng tọa độ hoặc biểu thức."
                }
                MacroAction::ShowGeometryPreset => {
                    "Hiện một preset hình học đã lưu từ tab Geometry."
                }
                MacroAction::HideGeometryPreset => {
                    "Ẩn preset hình học (hoặc xóa toàn bộ hình học)."
                }
                _ => "Tính năng cũ (Không dùng)",
            },
            _ => match action {
                MacroAction::KeyPress => "Press and release one keyboard key.",
                MacroAction::KeyDown => "Hold a keyboard key down.",
                MacroAction::KeyUp => "Release a held keyboard key.",
                MacroAction::Wait => "Wait for the number of milliseconds in Delay, then continue.",
                MacroAction::TypeText => "Type the whole text from the Input field.",
                MacroAction::ApplyWindowPreset => {
                    "Resize and reposition window using the selected preset."
                }
                MacroAction::FocusWindowPreset => {
                    "Bring one window forward with the selected focus preset."
                }
                MacroAction::TriggerMacroPreset => {
                    "Run another macro preset from the same macro group."
                }
                MacroAction::TriggerCommandPreset => {
                    "Run one custom command preset from the Custom tab."
                }
                MacroAction::EnableCrosshairProfile => "Enable one saved crosshair profile.",
                MacroAction::DisableCrosshair => "Turn the overlay crosshair off.",
                MacroAction::EnablePinPreset => "Enable one saved pin preset from the Pin tab.",
                MacroAction::DisablePin => "Turn the pinned app overlay off.",
                MacroAction::PlayMousePathPreset => {
                    "Play one recorded mouse path preset from the Mouse tab."
                }
                MacroAction::ApplyMouseSensitivityPreset => {
                    "Apply one mouse sensitivity preset from the Mouse tab."
                }
                MacroAction::EnableZoomPreset => "Enable one saved zoom preset.",
                MacroAction::DisableZoom => "Turn the zoom overlay off.",
                MacroAction::PlaySoundPreset => "Play one sound preset from the Media tab.",
                MacroAction::PlayVideoPreset => {
                    "Play one fullscreen video preset from the Media tab."
                }
                MacroAction::StartVisionSearch => {
                    "Start scanning one image-search preset in the background."
                }
                MacroAction::ScanVisionOnce => {
                    "Scan for the selected image, color, or pixel counter preset exactly once."
                }
                MacroAction::StartAudioSensePreset => {
                    "Start pitch detection from an AudioSense preset or custom pitch settings."
                }
                MacroAction::StopAudioSense => {
                    "Stop custom AudioSense monitoring or stop every active AudioSense monitor."
                }

                MacroAction::StopVisionWait => "Stop waiting for one image-search preset to match.",
                MacroAction::StopVision => {
                    "Stop one image-search preset that is currently scanning."
                }
                MacroAction::LoopStart => {
                    "Start looping the next adjacent steps. Input = loop count, or turn on Infinite."
                }
                MacroAction::LoopEnd => "End the current loop block.",
                MacroAction::StopIfTriggerPressedAgain => {
                    "Stop the current loop if you press the trigger again."
                }
                MacroAction::StopIfKeyPressed => {
                    "Break only the current loop if the key in Input is pressed, then continue with the steps after the loop."
                }
                MacroAction::ShowHud => "Show one HUD preset from the HUD tab.",
                MacroAction::HideHud => "Hide the currently visible HUD.",
                MacroAction::LockKeys => "Lock the keys listed in Input.",
                MacroAction::UnlockKeys => "Unlock the keys listed in Input.",
                MacroAction::LockMouse => {
                    "Lock mouse movement, clicks, and wheel input until it is unlocked or the macro ends."
                }
                MacroAction::UnlockMouse => "Unlock mouse movement and mouse buttons again.",
                MacroAction::EnableMacroPreset => {
                    "Enable one other macro preset from the same macro group."
                }
                MacroAction::DisableMacroPreset => {
                    "Disable one other macro preset from the same macro group."
                }
                MacroAction::EnableStep => {
                    "Enable one or more specific steps in this macro."
                }
                MacroAction::DisableStep => {
                    "Disable one or more specific steps in this macro."
                }
                MacroAction::MouseLeftClick => "Press and release left mouse button.",
                MacroAction::MouseLeftDown => "Hold left mouse button down.",
                MacroAction::MouseLeftUp => "Release held left mouse button.",
                MacroAction::MouseRightClick => "Press and release right mouse button.",
                MacroAction::MouseRightDown => "Hold right mouse button down.",
                MacroAction::MouseRightUp => "Release held right mouse button.",
                MacroAction::MouseMiddleClick => "Press and release middle mouse button.",
                MacroAction::MouseMiddleDown => "Hold middle mouse button down.",
                MacroAction::MouseMiddleUp => "Release held middle mouse button.",
                MacroAction::MouseX1Click => "Press and release mouse button X1.",
                MacroAction::MouseX1Down => "Hold mouse button X1 down.",
                MacroAction::MouseX1Up => "Release held mouse button X1.",
                MacroAction::MouseX2Click => "Press and release mouse button X2.",
                MacroAction::MouseX2Down => "Hold mouse button X2 down.",
                MacroAction::MouseX2Up => "Release held mouse button X2.",
                MacroAction::MouseWheelUp => "Scroll mouse wheel up.",
                MacroAction::MouseWheelDown => "Scroll mouse wheel down.",
                MacroAction::MouseMoveAbsolute => "Move mouse to absolute coordinates.",
                MacroAction::MouseMoveRelative => "Move mouse relative to current position.",
                MacroAction::IfStart => {
                    "Start a conditional If block. Only runs steps inside if the expression comparison is met."
                }
                MacroAction::Else => {
                    "Otherwise (Else) block. Runs steps inside if the above If condition was NOT met."
                }
                MacroAction::IfEnd => "End the current conditional If block.",
                MacroAction::SetVariable => {
                    "Set a variable to a numeric value or copy from another variable."
                }
                MacroAction::OcrSearch => {
                    "Scan screen region via Windows OCR Native to extract text and numbers."
                }
                MacroAction::DrawGeometry => {
                    "Draw one geometry shape on the screen overlay using coordinates or expressions."
                }
                MacroAction::ShowGeometryPreset => {
                    "Show one saved geometry preset from the Geometry tab."
                }
                MacroAction::HideGeometryPreset => {
                    "Hide geometry preset (or clear all geometry overlay)."
                }
                _ => "Legacy (Deprecated)",
            },
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
            MacroAction::TriggerCommandPreset => 0xeb8e,
            MacroAction::EnableCrosshairProfile => 0xe3c5,
            MacroAction::DisableCrosshair => 0xe1b7,
            MacroAction::EnablePinPreset => 0xe0c8,
            MacroAction::DisablePin => 0xe0c7,
            MacroAction::PlayMousePathPreset => 0xe913,
            MacroAction::ApplyMouseSensitivityPreset => 0xe837,
            MacroAction::EnableZoomPreset => 0xe8ff,
            MacroAction::DisableZoom => 0xe8f4,
            MacroAction::PlaySoundPreset => 0xe050,
            MacroAction::PlayVideoPreset => 0xe04b,
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
            MacroAction::LockKeys => 0xe897,
            MacroAction::UnlockKeys => 0xe898,
            MacroAction::LockMouse => 0xe897,
            MacroAction::UnlockMouse => 0xe898,
            MacroAction::EnableMacroPreset => 0xe86c,
            MacroAction::DisableMacroPreset => 0xe14b,
            MacroAction::StartTimerPreset => 0xe037,
            MacroAction::PauseTimerPreset => 0xe034,
            MacroAction::StopTimerPreset => 0xe047,
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
            MacroAction::OcrSearch => 0xe8b6,
            MacroAction::DrawGeometry => 0xe85b,
            MacroAction::ShowGeometryPreset => 0xe8f4,
            MacroAction::HideGeometryPreset => 0xe8f5,
            _ => 0xe8b5,
        };
        char::from_u32(codepoint).unwrap_or('?')
    }

    fn macro_action_icon_text(action: MacroAction) -> RichText {
        Self::material_icon_text(Self::macro_action_icon(action) as u32, 18.0)
    }

    fn macro_action_short_label(action: MacroAction, language: UiLanguage) -> &'static str {
        match language {
            UiLanguage::Vietnamese => Self::normalize_vietnamese(match action {
                MacroAction::KeyPress => "Nhấn",
                MacroAction::KeyDown => "Giữ",
                MacroAction::KeyUp => "Nhả",
                MacroAction::Wait => "Chờ",
                MacroAction::TypeText => "Chữ",
                MacroAction::ApplyWindowPreset => "Resize cửa sổ",
                MacroAction::FocusWindowPreset => "Cửa sổ",
                MacroAction::TriggerMacroPreset => "Macro",
                MacroAction::TriggerCommandPreset => "Câu lệnh",
                MacroAction::EnableCrosshairProfile => "Tâm ngắm",
                MacroAction::DisableCrosshair => "Tắt tâm ngắm",
                MacroAction::EnablePinPreset => "Ghim",
                MacroAction::DisablePin => "Bỏ ghim",
                MacroAction::PlayMousePathPreset => "Đường chuột",
                MacroAction::ApplyMouseSensitivityPreset => "Độ nhạy",
                MacroAction::EnableZoomPreset => "Phóng",
                MacroAction::DisableZoom => "Tắt phóng",
                MacroAction::PlaySoundPreset => "Âm thanh",
                MacroAction::PlayVideoPreset => "Video",
                MacroAction::StartVisionSearch => "Tìm ảnh",
                MacroAction::ScanVisionOnce => "Quét 1 lần",
                MacroAction::StartAudioSensePreset => "AudioSense",
                MacroAction::StopAudioSense => "Tắt Audio",

                MacroAction::StopVisionWait => "Chờ",
                MacroAction::StartVisionSearch => "T??m ???nh",
                MacroAction::ScanVisionOnce => "Qu??t 1 l???n",
                MacroAction::StartAudioSensePreset => "AudioSense",
                MacroAction::StopAudioSense => "T???t Audio",
                MacroAction::LockKeys => "Khóa phím",
                MacroAction::UnlockKeys => "Mở phím",
                MacroAction::LockMouse => "Khóa chuột",
                MacroAction::UnlockMouse => "Mở chuột",
                MacroAction::EnableMacroPreset => "Bật preset",
                MacroAction::DisableMacroPreset => "Tắt preset",
                MacroAction::StartTimerPreset => "Bắt đầu Hẹn giờ",
                MacroAction::PauseTimerPreset => "Dừng Hẹn giờ",
                MacroAction::StopTimerPreset => "Tắt Hẹn giờ",
                MacroAction::EnableStep => "Bật Step",
                MacroAction::DisableStep => "Tắt Step",
                MacroAction::MouseLeftClick => "Trái",
                MacroAction::MouseLeftDown => "Trái↓",
                MacroAction::MouseLeftUp => "Trái↑",
                MacroAction::MouseRightClick => "Phải",
                MacroAction::MouseRightDown => "Phải↓",
                MacroAction::MouseRightUp => "Phải↑",
                MacroAction::MouseMiddleClick => "Giữa",
                MacroAction::MouseMiddleDown => "Giữa↓",
                MacroAction::MouseMiddleUp => "Giữa↑",
                MacroAction::MouseX1Click => "X1",
                MacroAction::MouseX1Down => "X1G",
                MacroAction::MouseX1Up => "X1N",
                MacroAction::MouseX2Click => "X2",
                MacroAction::MouseX2Down => "X2G",
                MacroAction::MouseX2Up => "X2N",
                MacroAction::MouseWheelUp => "Lên",
                MacroAction::MouseWheelDown => "Xuống",
                MacroAction::MouseMoveAbsolute => "Tuyệt đối",
                MacroAction::MouseMoveRelative => "Tương đối",
                MacroAction::IfStart => "Nếu (If)",
                MacroAction::Else => "Ngược lại",
                MacroAction::IfEnd => "Hết Nếu",
                MacroAction::SetVariable => "Gán biến",
                MacroAction::DrawGeometry => "Vẽ hình",
                MacroAction::ShowGeometryPreset => "Hiện hình",
                MacroAction::HideGeometryPreset => "Ẩn hình",
                MacroAction::OcrSearch => "Quét OCR",
                _ => "Cũ (Bỏ)",
            }),
            UiLanguage::English => match action {
                MacroAction::KeyPress => "Press",
                MacroAction::KeyDown => "KEY Dn",
                MacroAction::KeyUp => "KEY Up",
                MacroAction::Wait => "Wait",
                MacroAction::TypeText => "Text",
                MacroAction::ApplyWindowPreset => "Resize",
                MacroAction::FocusWindowPreset => "Focus",
                MacroAction::TriggerMacroPreset => "Macro",
                MacroAction::TriggerCommandPreset => "Cmd",
                MacroAction::EnableCrosshairProfile => "Cross",
                MacroAction::DisableCrosshair => "NoCross",
                MacroAction::EnablePinPreset => "Pin",
                MacroAction::DisablePin => "NoPin",
                MacroAction::PlayMousePathPreset => "Path",
                MacroAction::ApplyMouseSensitivityPreset => "Sense",
                MacroAction::EnableZoomPreset => "Zoom",
                MacroAction::DisableZoom => "NoZoom",
                MacroAction::PlaySoundPreset => "Sound",
                MacroAction::PlayVideoPreset => "Video",
                MacroAction::StartVisionSearch => "Start",
                MacroAction::ScanVisionOnce => "Scan Once",
                MacroAction::StartAudioSensePreset => "StartAudio",
                MacroAction::StopAudioSense => "StopAudio",

                MacroAction::StopVisionWait => "Wait",
                MacroAction::StopVision => "Stop",
                MacroAction::LoopStart => "Loop",
                MacroAction::LoopEnd => "End",
                MacroAction::StopIfTriggerPressedAgain => "Stop",
                MacroAction::StopIfKeyPressed => "Break",
                MacroAction::ShowHud => "Show HUD",
                MacroAction::HideHud => "Hide HUD",
                MacroAction::LockKeys => "KL On",
                MacroAction::UnlockKeys => "KL Off",
                MacroAction::LockMouse => "Lock Move",
                MacroAction::UnlockMouse => "Unlock Move",
                MacroAction::EnableMacroPreset => "PresetOn",
                MacroAction::DisableMacroPreset => "PresetOff",
                MacroAction::StartTimerPreset => "Start Timer",
                MacroAction::PauseTimerPreset => "Pause Timer",
                MacroAction::StopTimerPreset => "Stop Timer",
                MacroAction::EnableStep => "StepOn",
                MacroAction::DisableStep => "StepOff",
                MacroAction::MouseLeftClick => "LClick",
                MacroAction::MouseLeftDown => "LDown",
                MacroAction::MouseLeftUp => "LUp",
                MacroAction::MouseRightClick => "RClick",
                MacroAction::MouseRightDown => "RDown",
                MacroAction::MouseRightUp => "RUp",
                MacroAction::MouseMiddleClick => "MClick",
                MacroAction::MouseMiddleDown => "MDown",
                MacroAction::MouseMiddleUp => "MUp",
                MacroAction::MouseX1Click => "X1",
                MacroAction::MouseX1Down => "X1Dn",
                MacroAction::MouseX1Up => "X1Up",
                MacroAction::MouseX2Click => "X2",
                MacroAction::MouseX2Down => "X2Dn",
                MacroAction::MouseX2Up => "X2Up",
                MacroAction::MouseWheelUp => "WhUp",
                MacroAction::MouseWheelDown => "WhDn",
                MacroAction::MouseMoveAbsolute => "MoveTo",
                MacroAction::MouseMoveRelative => "MoveBy",
                MacroAction::IfStart => "IfStart",
                MacroAction::Else => "Else",
                MacroAction::IfEnd => "IfEnd",
                MacroAction::SetVariable => "SetVar",
                MacroAction::DrawGeometry => "DrawGeo",
                MacroAction::ShowGeometryPreset => "ShowGeo",
                MacroAction::HideGeometryPreset => "HideGeo",
                MacroAction::OcrSearch => "OcrSearch",
                _ => "Legacy",
            },
            UiLanguage::Icon => match action {
                MacroAction::KeyPress => "Press",
                MacroAction::KeyDown => "KEY Dn",
                MacroAction::KeyUp => "KEY Up",
                MacroAction::Wait => "Wait",
                MacroAction::TypeText => "Text",
                MacroAction::ApplyWindowPreset => "Resize",
                MacroAction::FocusWindowPreset => "Focus",
                MacroAction::TriggerMacroPreset => "Macro",
                MacroAction::TriggerCommandPreset => "Cmd",
                MacroAction::EnableCrosshairProfile => "Cross",
                MacroAction::DisableCrosshair => "NoCross",
                MacroAction::EnablePinPreset => "Pin",
                MacroAction::DisablePin => "NoPin",
                MacroAction::PlayMousePathPreset => "Path",
                MacroAction::ApplyMouseSensitivityPreset => "Sense",
                MacroAction::EnableZoomPreset => "Zoom",
                MacroAction::DisableZoom => "NoZoom",
                MacroAction::PlaySoundPreset => "Sound",
                MacroAction::StartVisionSearch => "Start",
                MacroAction::ScanVisionOnce => "Scan Once",
                MacroAction::StartAudioSensePreset => "StartAudio",
                MacroAction::StopAudioSense => "StopAudio",

                MacroAction::StopVisionWait => "Wait",
                MacroAction::StopVision => "Stop",
                MacroAction::LoopStart => "Loop",
                MacroAction::LoopEnd => "End",
                MacroAction::StopIfTriggerPressedAgain => "Stop",
                MacroAction::StopIfKeyPressed => "Break",
                MacroAction::ShowHud => "Show HUD",
                MacroAction::HideHud => "Hide HUD",
                MacroAction::LockKeys => "KL On",
                MacroAction::UnlockKeys => "KL Off",
                MacroAction::LockMouse => "ML On",
                MacroAction::UnlockMouse => "ML Off",
                MacroAction::EnableMacroPreset => "PresetOn",
                MacroAction::DisableMacroPreset => "PresetOff",
                MacroAction::StartTimerPreset => "Start Timer",
                MacroAction::PauseTimerPreset => "Pause Timer",
                MacroAction::StopTimerPreset => "Stop Timer",
                MacroAction::EnableStep => "StepOn",
                MacroAction::DisableStep => "StepOff",
                MacroAction::MouseLeftClick => "LClick",
                MacroAction::MouseLeftDown => "LDown",
                MacroAction::MouseLeftUp => "LUp",
                MacroAction::MouseRightClick => "RClick",
                MacroAction::MouseRightDown => "RDown",
                MacroAction::MouseRightUp => "RUp",
                MacroAction::MouseMiddleClick => "MClick",
                MacroAction::MouseMiddleDown => "MDown",
                MacroAction::MouseMiddleUp => "MUp",
                MacroAction::MouseX1Click => "X1",
                MacroAction::MouseX1Down => "X1Dn",
                MacroAction::MouseX1Up => "X1Up",
                MacroAction::MouseX2Click => "X2",
                MacroAction::MouseX2Down => "X2Dn",
                MacroAction::MouseX2Up => "X2Up",
                MacroAction::MouseWheelUp => "WhUp",
                MacroAction::MouseWheelDown => "WhDn",
                MacroAction::MouseMoveAbsolute => "MoveTo",
                MacroAction::MouseMoveRelative => "MoveBy",
                MacroAction::IfStart => "IfStart",
                MacroAction::Else => "Else",
                MacroAction::IfEnd => "IfEnd",
                MacroAction::SetVariable => "SetVar",
                MacroAction::DrawGeometry => "DrawGeo",
                MacroAction::ShowGeometryPreset => "ShowGeo",
                MacroAction::HideGeometryPreset => "HideGeo",
                MacroAction::OcrSearch => "OCR",
                _ => "Legacy",
            },
        }
    }

    fn macro_action_pair_tag(action: MacroAction) -> Option<&'static str> {
        match action {
            MacroAction::KeyDown | MacroAction::KeyUp => Some("KEY"),
            MacroAction::LockKeys | MacroAction::UnlockKeys => Some("KLOCK"),
            MacroAction::LockMouse | MacroAction::UnlockMouse => Some("MLOCK"),
            _ => None,
        }
    }

    fn macro_action_selected_label(action: MacroAction, language: UiLanguage) -> String {
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
    ) -> egui::WidgetText {
        let mut job = egui::text::LayoutJob::default();
        let weak_color = Color32::from_gray(224);
        let icon_format = egui::TextFormat {
            font_id: egui::FontId::new(13.0, FontFamily::Name(MATERIAL_ICONS_FONT.into())),
            color: weak_color,
            ..Default::default()
        };
        let text_format = egui::TextFormat {
            font_id: egui::FontId::new(13.0, FontFamily::Proportional),
            color: weak_color,
            ..Default::default()
        };
        job.append(
            &char::from_u32(Self::macro_action_icon(action) as u32)
                .unwrap_or('?')
                .to_string(),
            0.0,
            icon_format,
        );
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
                | MacroAction::TriggerCommandPreset
                | MacroAction::EnableCrosshairProfile
                | MacroAction::EnablePinPreset
                | MacroAction::PlayMousePathPreset
                | MacroAction::ApplyMouseSensitivityPreset
                | MacroAction::EnableZoomPreset
                | MacroAction::PlaySoundPreset
                | MacroAction::PlayVideoPreset
                | MacroAction::EnableMacroPreset
                | MacroAction::DisableMacroPreset
                | MacroAction::StartTimerPreset
                | MacroAction::PauseTimerPreset
                | MacroAction::StopTimerPreset
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
                | MacroAction::DisableCrosshair
                | MacroAction::DisableZoom
                | MacroAction::DisablePin
                | MacroAction::HideHud
                | MacroAction::LockMouse
                | MacroAction::UnlockMouse
                | MacroAction::Wait
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
        match language {
            UiLanguage::Vietnamese => match mode {
                MacroTriggerMode::Press => "Nhấn",
                MacroTriggerMode::Hold => "Giữ",
                MacroTriggerMode::Release => "Thả",
            },
            UiLanguage::English => match mode {
                MacroTriggerMode::Press => "Press",
                MacroTriggerMode::Hold => "Hold",
                MacroTriggerMode::Release => "Release",
            },
            UiLanguage::Icon => match mode {
                MacroTriggerMode::Press => "Press",
                MacroTriggerMode::Hold => "Hold",
                MacroTriggerMode::Release => "Release",
            },
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

    fn sized_button(ui: &mut egui::Ui, width: f32, label: &str) -> egui::Response {
        ui.add_sized([width, 24.0], Button::new(label))
    }

    fn sound_style_toggle_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
        ui.add_sized([84.0, 24.0], Button::new(label))
    }

    fn sound_style_remove_button(ui: &mut egui::Ui) -> egui::Response {
        ui.add_sized(
            [36.0, 24.0],
            Button::new(Self::material_icon_text(0xe872, 18.0)),
        )
    }

    fn sound_style_icon_button(ui: &mut egui::Ui, icon: RichText) -> egui::Response {
        ui.add_sized([36.0, 24.0], Button::new(icon))
    }

    fn is_copy_feedback_active(until: Option<Instant>) -> bool {
        until.is_some_and(|deadline| Instant::now() < deadline)
    }

    fn macro_share_code_kind_from_text(text: &str) -> MacroShareCodeKind {
        let payload = text.trim();
        if payload.starts_with("MN2_STEP:") || payload.starts_with("MN_STEP:") {
            MacroShareCodeKind::Step
        } else if payload.starts_with("MN2_PRESET:") || payload.starts_with("MN_PRESET:") {
            MacroShareCodeKind::Preset
        } else if payload.starts_with("MN2_GROUP:") || payload.starts_with("MN_GROUP:") {
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

    fn enabled_icon_button(ui: &mut egui::Ui, enabled: bool) -> egui::Response {
        let icon = if enabled { 0xe5ca } else { 0xe835 };
        let fill = if enabled {
            Color32::from_rgba_premultiplied(72, 156, 116, 120)
        } else {
            ui.visuals().faint_bg_color
        };
        let stroke = if enabled {
            Color32::from_rgb(126, 224, 182)
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        ui.add_sized(
            [36.0, 24.0],
            Button::new(Self::material_icon_text(icon, 18.0))
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, stroke)),
        )
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
        let mut rgba = [color.r, color.g, color.b, color.a];
        let response = ui.color_edit_button_srgba_unmultiplied(&mut rgba);
        if response.changed() {
            color.r = rgba[0];
            color.g = rgba[1];
            color.b = rgba[2];
            color.a = rgba[3];
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

        if response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                let dist_tl = pointer_pos.distance(rect.left_top());
                let dist_tr = pointer_pos.distance(rect.right_top());
                let dist_bl = pointer_pos.distance(rect.left_bottom());
                let dist_br = pointer_pos.distance(rect.right_bottom());

                let nearest_on_box = egui::pos2(
                    pointer_pos.x.clamp(rect.left(), rect.right()),
                    pointer_pos.y.clamp(rect.top(), rect.bottom()),
                );
                let dist_to_box = pointer_pos.distance(nearest_on_box);

                active_handle = if dist_tl < 14.0 {
                    SelectionDragHandle::TopLeft
                } else if dist_tr < 14.0 {
                    SelectionDragHandle::TopRight
                } else if dist_bl < 14.0 {
                    SelectionDragHandle::BottomLeft
                } else if dist_br < 14.0 {
                    SelectionDragHandle::BottomRight
                } else if (pointer_pos.x - rect.left()).abs() < 10.0
                    && pointer_pos.y >= rect.top()
                    && pointer_pos.y <= rect.bottom()
                {
                    SelectionDragHandle::Left
                } else if (pointer_pos.x - rect.right()).abs() < 10.0
                    && pointer_pos.y >= rect.top()
                    && pointer_pos.y <= rect.bottom()
                {
                    SelectionDragHandle::Right
                } else if (pointer_pos.y - rect.top()).abs() < 10.0
                    && pointer_pos.x >= rect.left()
                    && pointer_pos.x <= rect.right()
                {
                    SelectionDragHandle::Top
                } else if (pointer_pos.y - rect.bottom()).abs() < 10.0
                    && pointer_pos.x >= rect.left()
                    && pointer_pos.x <= rect.right()
                {
                    SelectionDragHandle::Bottom
                } else if rect.contains(pointer_pos) {
                    SelectionDragHandle::Center
                } else if dist_to_box < 20.0 {
                    SelectionDragHandle::Center
                } else {
                    SelectionDragHandle::None
                };
                ui.data_mut(|d| d.insert_temp(drag_id, active_handle));
            }
        }

        let tr_primary_down = ui.input(|i| i.pointer.primary_down());
        let tr_delta = ui.input(|i| i.pointer.delta());
        if tr_primary_down && active_handle != SelectionDragHandle::None {
            let delta = tr_delta;
            let shift_pressed = ui.input(|i| i.modifiers.shift);
            let original_aspect = if preset.height > 0 {
                preset.width as f32 / preset.height as f32
            } else {
                16.0 / 9.0
            };
            let lock_aspect = if shift_pressed { original_aspect } else { 0.0 };

            changed = true;

            match active_handle {
                SelectionDragHandle::Center => {
                    rect = rect.translate(delta);
                }
                SelectionDragHandle::Right => {
                    let new_w = (rect.width() + delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.max.x = rect.min.x + new_w;
                        rect.max.y = rect.min.y + new_h;
                    } else {
                        rect.max.x = rect.min.x + new_w;
                    }
                }
                SelectionDragHandle::Left => {
                    let new_w = (rect.width() - delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.min.x = rect.max.x - new_w;
                        rect.min.y = rect.max.y - new_h;
                    } else {
                        rect.min.x = rect.max.x - new_w;
                    }
                }
                SelectionDragHandle::Bottom => {
                    let new_h = (rect.height() + delta.y).max(min_size.y);
                    if lock_aspect > 0.0 {
                        let new_w = new_h * lock_aspect;
                        rect.max.x = rect.min.x + new_w;
                        rect.max.y = rect.min.y + new_h;
                    } else {
                        rect.max.y = rect.min.y + new_h;
                    }
                }
                SelectionDragHandle::Top => {
                    let new_h = (rect.height() - delta.y).max(min_size.y);
                    if lock_aspect > 0.0 {
                        let new_w = new_h * lock_aspect;
                        rect.min.x = rect.max.x - new_w;
                        rect.min.y = rect.max.y - new_h;
                    } else {
                        rect.min.y = rect.max.y - new_h;
                    }
                }
                SelectionDragHandle::BottomRight => {
                    let new_w = (rect.width() + delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.max.x = rect.min.x + new_w;
                        rect.max.y = rect.min.y + new_h;
                    } else {
                        let new_h = (rect.height() + delta.y).max(min_size.y);
                        rect.max.x = rect.min.x + new_w;
                        rect.max.y = rect.min.y + new_h;
                    }
                }
                SelectionDragHandle::TopLeft => {
                    let new_w = (rect.width() - delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.min.x = rect.max.x - new_w;
                        rect.min.y = rect.max.y - new_h;
                    } else {
                        let new_h = (rect.height() - delta.y).max(min_size.y);
                        rect.min.x = rect.max.x - new_w;
                        rect.min.y = rect.max.y - new_h;
                    }
                }
                SelectionDragHandle::TopRight => {
                    let new_w = (rect.width() + delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.max.x = rect.min.x + new_w;
                        rect.min.y = rect.max.y - new_h;
                    } else {
                        let new_h = (rect.height() - delta.y).max(min_size.y);
                        rect.max.x = rect.min.x + new_w;
                        rect.min.y = rect.max.y - new_h;
                    }
                }
                SelectionDragHandle::BottomLeft => {
                    let new_w = (rect.width() - delta.x).max(min_size.x);
                    if lock_aspect > 0.0 {
                        let new_h = new_w / lock_aspect;
                        rect.min.x = rect.max.x - new_w;
                        rect.max.y = rect.min.y + new_h;
                    } else {
                        let new_h = (rect.height() + delta.y).max(min_size.y);
                        rect.min.x = rect.max.x - new_w;
                        rect.max.y = rect.min.y + new_h;
                    }
                }
                SelectionDragHandle::None => {}
            }

            if rect.left() < preview_rect.left() {
                rect = rect.translate(vec2(preview_rect.left() - rect.left(), 0.0));
            }
            if rect.top() < preview_rect.top() {
                rect = rect.translate(vec2(0.0, preview_rect.top() - rect.top()));
            }
            if rect.right() > preview_rect.right() {
                rect = rect.translate(vec2(preview_rect.right() - rect.right(), 0.0));
            }
            if rect.bottom() > preview_rect.bottom() {
                rect = rect.translate(vec2(0.0, preview_rect.bottom() - rect.bottom()));
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

        if ui.input(|i| i.pointer.any_released()) {
            active_handle = SelectionDragHandle::None;
            ui.data_mut(|d| d.insert_temp(drag_id, active_handle));
        }

        if response.hovered() || active_handle != SelectionDragHandle::None {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.hover_pos()) {
                let dist_tl = pointer_pos.distance(rect.left_top());
                let dist_tr = pointer_pos.distance(rect.right_top());
                let dist_bl = pointer_pos.distance(rect.left_bottom());
                let dist_br = pointer_pos.distance(rect.right_bottom());

                let handle_to_use = if active_handle != SelectionDragHandle::None {
                    active_handle
                } else if dist_tl < 14.0 {
                    SelectionDragHandle::TopLeft
                } else if dist_tr < 14.0 {
                    SelectionDragHandle::TopRight
                } else if dist_bl < 14.0 {
                    SelectionDragHandle::BottomLeft
                } else if dist_br < 14.0 {
                    SelectionDragHandle::BottomRight
                } else if (pointer_pos.x - rect.left()).abs() < 10.0
                    && pointer_pos.y >= rect.top()
                    && pointer_pos.y <= rect.bottom()
                {
                    SelectionDragHandle::Left
                } else if (pointer_pos.x - rect.right()).abs() < 10.0
                    && pointer_pos.y >= rect.top()
                    && pointer_pos.y <= rect.bottom()
                {
                    SelectionDragHandle::Right
                } else if (pointer_pos.y - rect.top()).abs() < 10.0
                    && pointer_pos.x >= rect.left()
                    && pointer_pos.x <= rect.right()
                {
                    SelectionDragHandle::Top
                } else if (pointer_pos.y - rect.bottom()).abs() < 10.0
                    && pointer_pos.x >= rect.left()
                    && pointer_pos.x <= rect.right()
                {
                    SelectionDragHandle::Bottom
                } else if rect.contains(pointer_pos) {
                    SelectionDragHandle::Center
                } else {
                    SelectionDragHandle::None
                };

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
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "00:00.000",
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
            RichText::new(Self::tr_lang(language, "Capturing...", "Đang bắt..."))
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
        if response.hovered() {
            egui::show_tooltip_at_pointer(
                ui.ctx(),
                ui.layer_id(),
                response.id.with("instant-tip"),
                |ui| {
                    ui.set_max_width(280.0);
                    ui.label(text.into());
                },
            );
        }
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
            let id = self.state.next_master_preset_id.max(1);
            self.state.next_master_preset_id = id + 1;
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

    fn ensure_master_presets(&mut self) {
        if self.ensure_master_presets_without_persist() {
            self.persist();
        }
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
                self.state
                    .window_presets
                    .iter()
                    .position(|preset| preset.id == item.id)
                    .unwrap_or(usize::MAX)
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
                self.state
                    .window_focus_presets
                    .iter()
                    .position(|preset| preset.id == item.id)
                    .unwrap_or(usize::MAX)
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
                self.state
                    .zoom_presets
                    .iter()
                    .position(|preset| preset.id == item.id)
                    .unwrap_or(usize::MAX)
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
                        group
                            .presets
                            .iter()
                            .position(|preset| preset.id == item.id)
                            .unwrap_or(usize::MAX)
                    });
                }
            }
            preset.macro_groups.sort_by_key(|item| {
                self.state
                    .macro_groups
                    .iter()
                    .position(|group| group.id == item.id)
                    .unwrap_or(usize::MAX)
            });
        }
    }

    fn add_master_preset_from_current(&mut self) {
        let id = self.state.next_master_preset_id.max(1);
        self.state.next_master_preset_id = id + 1;
        let preset = self.capture_master_preset_snapshot(id, format!("Mode {id}"));
        self.state.master_presets.push(preset);
        self.state.selected_master_preset_id = Some(id);
        self.persist();
        self.status = format!("Captured current hotkey setup into mode {id}.");
    }

    fn update_master_preset_from_current(&mut self, preset_id: u32) {
        let replacement = self
            .state
            .master_presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .map(|preset| (preset.collapsed, preset.name.clone()));
        let Some((collapsed, name)) = replacement else {
            return;
        };
        let mut snapshot = self.capture_master_preset_snapshot(preset_id, name);
        snapshot.collapsed = collapsed;
        if let Some(existing) = self
            .state
            .master_presets
            .iter_mut()
            .find(|preset| preset.id == preset_id)
        {
            *existing = snapshot;
        }
        self.persist();
        self.status = format!("Updated mode {preset_id} from current toggles.");
    }

    fn apply_master_preset(&mut self, preset_id: u32) {
        let Some(preset) = self
            .state
            .master_presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .cloned()
        else {
            return;
        };
        self.state.selected_master_preset_id = Some(preset_id);
        self.state.macros_master_enabled = preset.macros_master_enabled;
        self.state.window_expand_controls.enabled = preset.window_expand_controls_enabled;

        for item in &preset.window_presets {
            if let Some(window_preset) = self
                .state
                .window_presets
                .iter_mut()
                .find(|preset| preset.id == item.id)
            {
                window_preset.enabled = item.enabled;
                window_preset.animate_enabled = item.animate_enabled;
                window_preset.restore_titlebar_enabled = item.restore_titlebar_enabled;
            }
        }
        for item in &preset.window_focus_presets {
            if let Some(focus_preset) = self
                .state
                .window_focus_presets
                .iter_mut()
                .find(|preset| preset.id == item.id)
            {
                focus_preset.enabled = item.enabled;
            }
        }
        for item in &preset.zoom_presets {
            if let Some(zoom_preset) = self
                .state
                .zoom_presets
                .iter_mut()
                .find(|preset| preset.id == item.id)
            {
                zoom_preset.enabled = item.enabled;
            }
        }
        for group_state in &preset.macro_groups {
            if let Some(group) = self
                .state
                .macro_groups
                .iter_mut()
                .find(|group| group.id == group_state.id)
            {
                group.enabled = group_state.enabled;
                for preset_state in &group_state.presets {
                    if let Some(macro_preset) = group
                        .presets
                        .iter_mut()
                        .find(|preset| preset.id == preset_state.id)
                    {
                        macro_preset.enabled = preset_state.enabled;
                    }
                }
            }
        }
        self.sync_window_presets();
        self.sync_macro_presets();
        self.sync_macro_master_enabled();
        self.persist();
        self.status = format!("Applied mode: {}.", preset.name);
    }

    fn add_macro_group(&mut self) {
        let id = self.state.next_macro_group_id.max(1);
        self.state.next_macro_group_id = id + 1;
        let mut group = MacroGroup::new(id);
        group.name = self.unique_macro_group_name(&group.name);
        let preset_id = self.state.next_macro_preset_id.max(1);
        self.state.next_macro_preset_id = preset_id + 1;
        group.presets = vec![MacroPreset::new(preset_id)];
        self.state.macro_groups.push(group);
        self.pending_macro_group_scroll_target = Some(id);
        self.reconcile_master_presets();
        self.sync_macro_presets();
        self.status = format!("Added macro group {id}.");
    }

    fn add_macro_preset_to_group(&mut self, group_id: u32) {
        let id = self.state.next_macro_preset_id.max(1);
        self.state.next_macro_preset_id = id + 1;
        if let Some(group) = self
            .state
            .macro_groups
            .iter_mut()
            .find(|group| group.id == group_id)
        {
            group.presets.push(MacroPreset::new(id));
            self.reconcile_master_presets();
            self.sync_macro_presets();
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
        let open_windows = if self.open_windows.is_empty() {
            "None".to_owned()
        } else {
            self.open_windows.join("\n- ")
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
             - IMPORTANT: You MUST also generate an appropriate, concise, and descriptive name for the 'name' field (in the same language as the user request, maximum 3-5 words, e.g., 'Start MsPaint' or 'Mở Paint') that summarizes what the new command does. Do not leave the 'name' field unchanged if the command's behavior is changed.\n\
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
            self.settings_popup_open = true;
            self.state.groq_settings.details_open = true;
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
                if let Some(group) = self
                    .state
                    .macro_groups
                    .iter()
                    .find(|group| group.id == group_id)
                {
                    if let Some(preset) = group.presets.iter().find(|preset| preset.id == preset_id)
                    {
                        if let Some(step_index) = step_index {
                            if let Some(step) = preset.steps.get(step_index) {
                                temp_preset.command = step.command_preset_command.clone();
                                temp_preset.use_powershell = step.command_preset_use_powershell;
                            }
                        } else {
                            temp_preset.command =
                                preset.hold_stop_step.command_preset_command.clone();
                            temp_preset.use_powershell =
                                preset.hold_stop_step.command_preset_use_powershell;
                        }
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
                    let new_fallback_name = if cmd_lower.contains("shutdown") {
                        "Tắt máy".to_owned()
                    } else if cmd_lower.contains("mspaint") || cmd_lower.contains("pbrush") {
                        "Mở Paint".to_owned()
                    } else if cmd_lower.contains("calc") {
                        "Mở Máy tính".to_owned()
                    } else if cmd_lower.contains("notepad") {
                        "Mở Notepad".to_owned()
                    } else if cmd_lower.contains("discord") {
                        "Mở Discord".to_owned()
                    } else if cmd_lower.contains("chrome") {
                        "Mở Chrome".to_owned()
                    } else if cmd_lower.contains("edge") || cmd_lower.contains("msedge") {
                        "Mở Edge".to_owned()
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
                                format!("Mở {}", capitalized)
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
                            preset.hold_stop_step.command_preset_command = temp_preset.command;
                            preset.hold_stop_step.command_preset_use_powershell =
                                temp_preset.use_powershell;
                            preset.hold_stop_step.key = temp_preset.name.clone();
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
            if preset.name.trim().eq_ignore_ascii_case(old_name.trim())
                && preset.command.trim() != old_name.trim()
            {
                let cmd_lower = preset.command.to_ascii_lowercase();
                let is_vietnamese = old_name.chars().any(|c| c as u32 > 127);
                let new_fallback_name = if cmd_lower.contains("shutdown") {
                    if is_vietnamese {
                        "Tắt máy".to_owned()
                    } else {
                        "Shutdown".to_owned()
                    }
                } else if cmd_lower.contains("mspaint") || cmd_lower.contains("pbrush") {
                    if is_vietnamese {
                        "Mở Paint".to_owned()
                    } else {
                        "Start Paint".to_owned()
                    }
                } else if cmd_lower.contains("calc") {
                    if is_vietnamese {
                        "Mở Máy tính".to_owned()
                    } else {
                        "Start Calculator".to_owned()
                    }
                } else if cmd_lower.contains("notepad") {
                    if is_vietnamese {
                        "Mở Notepad".to_owned()
                    } else {
                        "Start Notepad".to_owned()
                    }
                } else if cmd_lower.contains("discord") {
                    if is_vietnamese {
                        "Mở Discord".to_owned()
                    } else {
                        "Start Discord".to_owned()
                    }
                } else if cmd_lower.contains("chrome") {
                    if is_vietnamese {
                        "Mở Chrome".to_owned()
                    } else {
                        "Start Chrome".to_owned()
                    }
                } else if cmd_lower.contains("edge") || cmd_lower.contains("msedge") {
                    if is_vietnamese {
                        "Mở Edge".to_owned()
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
                                format!("Mở {}", capitalized)
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
                }
            }
            new_name
        };
        self.sync_command_presets();
        self.persist();
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
                    }
                }
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.command_ai_job = None;
                self.command_ai_feedback = Some("AI generation stopped unexpectedly.".to_owned());
                self.status = "AI generation stopped unexpectedly.".to_owned();
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
            .position(|preset| preset.name.trim().eq_ignore_ascii_case(&name))
        {
            let preset = &mut self.state.command_presets[existing_index];
            preset.name = name.clone();
            preset.command = command;
            preset.use_powershell = use_powershell;
            preset.collapsed = true;
            return Some(preset.id);
        }

        let id = self.state.next_command_preset_id.max(1);
        self.state.next_command_preset_id = id + 1;
        let mut preset = CommandPreset::new(id);
        preset.name = name;
        preset.command = command;
        preset.use_powershell = use_powershell;
        preset.collapsed = true;
        self.state.command_presets.push(preset);
        Some(id)
    }

    fn add_macro_folder(&mut self) {
        let id = self.state.next_macro_folder_id.max(1);
        self.state.next_macro_folder_id = id + 1;
        self.state.macro_folders.push(MacroFolder::new(id));
        self.status = format!("Added macro folder {id}.");
    }

    fn add_macro_group_to_folder(&mut self, folder_id: u32) {
        let id = self.state.next_macro_group_id.max(1);
        self.state.next_macro_group_id = id + 1;
        let mut group = MacroGroup::new(id);
        group.name = self.unique_macro_group_name(&group.name);
        group.folder_id = Some(folder_id);
        let preset_id = self.state.next_macro_preset_id.max(1);
        self.state.next_macro_preset_id = preset_id + 1;
        group.presets = vec![MacroPreset::new(preset_id)];
        self.state.macro_groups.push(group);
        self.pending_macro_group_scroll_target = Some(id);
        self.reconcile_master_presets();
        self.sync_macro_presets();
        self.status = format!("Added macro group {id} to folder.");
    }

    fn clone_macro_preset_with_new_id(&mut self, source: &MacroPreset) -> MacroPreset {
        let new_preset_id = self.state.next_macro_preset_id.max(1);
        self.state.next_macro_preset_id = new_preset_id + 1;
        let mut preset = source.clone();
        let old_preset_id = preset.id;
        preset.id = new_preset_id;
        preset.collapsed = true;
        Self::remap_macro_step_self_ref(&mut preset.hold_stop_step, old_preset_id, new_preset_id);
        for step in &mut preset.steps {
            Self::remap_macro_step_self_ref(step, old_preset_id, new_preset_id);
        }
        preset
    }

    fn remap_macro_step_self_ref(step: &mut MacroStep, old_preset_id: u32, new_preset_id: u32) {
        if matches!(
            step.action,
            MacroAction::TriggerMacroPreset
                | MacroAction::EnableMacroPreset
                | MacroAction::DisableMacroPreset
        ) && let Ok(id) = step.key.trim().parse::<u32>()
            && id == old_preset_id
        {
            step.key = new_preset_id.to_string();
        }
    }

    fn clone_macro_group_with_new_ids(
        &mut self,
        source_group: &MacroGroup,
        target_folder_id: Option<u32>,
    ) -> MacroGroup {
        let new_group_id = self.state.next_macro_group_id.max(1);
        self.state.next_macro_group_id = new_group_id + 1;

        let mut copied_group = source_group.clone();
        copied_group.id = new_group_id;
        copied_group.name = format!("{} Copy", copied_group.name);
        copied_group.name = self.unique_macro_group_name(&copied_group.name);
        copied_group.folder_id = target_folder_id;

        let mut preset_id_map = HashMap::new();
        for preset in &mut copied_group.presets {
            let old_id = preset.id;
            let new_preset_id = self.state.next_macro_preset_id.max(1);
            self.state.next_macro_preset_id = new_preset_id + 1;
            preset.id = new_preset_id;
            preset.collapsed = true;
            preset_id_map.insert(old_id, new_preset_id);
        }

        for preset in &mut copied_group.presets {
            Self::remap_macro_step_group_refs(&mut preset.hold_stop_step, &preset_id_map);
            for step in &mut preset.steps {
                Self::remap_macro_step_group_refs(step, &preset_id_map);
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

    fn remap_macro_step_group_refs(step: &mut MacroStep, preset_id_map: &HashMap<u32, u32>) {
        if matches!(
            step.action,
            MacroAction::TriggerMacroPreset
                | MacroAction::EnableMacroPreset
                | MacroAction::DisableMacroPreset
        ) && let Ok(old_id) = step.key.trim().parse::<u32>()
            && let Some(new_id) = preset_id_map.get(&old_id)
        {
            step.key = new_id.to_string();
        }
    }

    fn set_active_macro_folder_view(&mut self, folder_id: Option<u32>) {
        self.active_macro_folder_view = folder_id;
        self.selected_macro_groups.clear();
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
        self.status = format!(
            "Copied {} macro group(s).",
            self.macro_group_clipboard.len()
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
        if let Some(group) = self.state.macro_groups.iter().find(|g| g.id == group_id) {
            if let Some(preset) = group.presets.iter().find(|p| p.id == preset_id) {
                for &index in &selected_indices {
                    if let Some(step) = preset.steps.get(index) {
                        clipboard.push(step.clone());
                    }
                }
            } else {
                self.status = "Macro preset not found.".to_owned();
                return;
            }
        } else {
            self.status = "Macro group not found.".to_owned();
            return;
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

        if let Some(group) = self
            .state
            .macro_groups
            .iter_mut()
            .find(|g| g.id == group_id)
        {
            if let Some(preset) = group.presets.iter_mut().find(|p| p.id == preset_id) {
                for index in selected_indices {
                    if index < preset.steps.len() {
                        preset.steps.remove(index);
                    }
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

        let Some(group) = self
            .state
            .macro_groups
            .iter_mut()
            .find(|g| g.id == group_id)
        else {
            self.status = "Macro group not found.".to_owned();
            return None;
        };
        let Some(preset) = group.presets.iter_mut().find(|p| p.id == preset_id) else {
            self.status = "Macro preset not found.".to_owned();
            return None;
        };
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

        let Some(group) = self
            .state
            .macro_groups
            .iter_mut()
            .find(|g| g.id == group_id)
        else {
            self.status = "Macro group not found.".to_owned();
            return None;
        };
        let Some(preset) = group.presets.iter_mut().find(|p| p.id == preset_id) else {
            self.status = "Macro preset not found.".to_owned();
            return None;
        };

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
            self.status = format!("Pasted {} macro group copy(s).", sources.len());
        }

        self.reconcile_master_presets();
        self.sync_macro_presets();
        self.persist_macro_presets();
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
        self.reconcile_master_presets();
        self.sync_macro_presets();
        self.persist_macro_presets();
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
            match self.state.ui_language {
                UiLanguage::Vietnamese => {
                    "Đang bắt trigger. Giữ rồi nhả để lưu, nhấn lại nút bắt để hủy.".to_owned()
                }
                _ => "Capturing triggers. Hold keys, then release to save. Click Capture again to cancel.".to_owned(),
            }
        } else {
            status
        };
    }

    fn capture_request_keeps_open(&self, target: &CaptureRequest) -> bool {
        match target {
            CaptureRequest::MacroPresetHotkey(_, _) => true,
            CaptureRequest::MacroPresetRecordHotkey(_, _) => true,
            CaptureRequest::CommandPresetHotkey(_) => true,
            CaptureRequest::MacroPresetReleaseWaitKey(_, _) => true,
            CaptureRequest::WindowPresetHotkey(_) => true,
            CaptureRequest::WindowFocusPresetHotkey(_) => true,
            CaptureRequest::PinPresetHotkey(_) => true,
            CaptureRequest::MouseSensitivityPresetHotkey(_) => true,
            CaptureRequest::VisionPresetHotkey(_) => true,
            CaptureRequest::MacroPresetHoldStopInput(_, _) => false,
            CaptureRequest::MacroStepInput {
                group_id,
                preset_id,
                step_index,
                ..
            } => false,
            _ => false,
        }
    }

    fn capture_request_accepts_mouse(&self, target: &CaptureRequest) -> bool {
        matches!(
            target,
            CaptureRequest::MacroPresetHotkey(_, _)
                | CaptureRequest::MacroPresetRecordHotkey(_, _)
                | CaptureRequest::MacroPresetReleaseWaitKey(_, _)
                | CaptureRequest::MacroPresetHoldStopInput(_, _)
                | CaptureRequest::CommandPresetHotkey(_)
                | CaptureRequest::WindowPresetHotkey(_)
                | CaptureRequest::WindowFocusPresetHotkey(_)
                | CaptureRequest::WindowPresetAnimateHotkey(_)
                | CaptureRequest::WindowPresetTitlebarHotkey(_)
                | CaptureRequest::WindowExpandHotkey(_)
                | CaptureRequest::PinPresetHotkey(_)
                | CaptureRequest::MouseSensitivityPresetHotkey(_)
                | CaptureRequest::ZoomPresetHotkey(_)
                | CaptureRequest::VisionPresetHotkey(_)
                | CaptureRequest::MacrosMasterHotkey
                | CaptureRequest::MacroStepInput { .. }
        )
    }

    fn capture_request_registers_on_press(&self, target: &CaptureRequest) -> bool {
        match target {
            CaptureRequest::MacroPresetHoldStopInput(group_id, preset_id) => self
                .state
                .macro_groups
                .iter()
                .find(|group| group.id == *group_id)
                .and_then(|group| group.presets.iter().find(|preset| preset.id == *preset_id))
                .is_some_and(|preset| {
                    matches!(
                        preset.hold_stop_step.action,
                        MacroAction::LockKeys | MacroAction::UnlockKeys
                    ) || (preset.hold_stop_step.action == MacroAction::StopIfKeyPressed
                        && preset.hold_stop_step.get_break_loop_mode() == "StopKey")
                }),
            CaptureRequest::MacroStepInput {
                group_id,
                preset_id,
                step_index,
                extra_cond_index,
            } => {
                if extra_cond_index.is_some() {
                    return false;
                }
                self.state
                    .macro_groups
                    .iter()
                    .find(|group| group.id == *group_id)
                    .and_then(|group| {
                        group.presets.iter().find(|preset| preset.id == *preset_id)
                    })
                    .and_then(|preset| preset.steps.get(*step_index))
                    .is_some_and(|step| {
                        matches!(step.action, MacroAction::LockKeys | MacroAction::UnlockKeys)
                            || (step.action == MacroAction::StopIfKeyPressed
                                && step.get_break_loop_mode() == "StopKey")
                    })
            }
            _ => false,
        }
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

    fn capture_info_window_placement(
        ctx: &egui::Context,
        pointer: Option<egui::Pos2>,
    ) -> (egui::Pos2, egui::Vec2) {
        let (left, top, width, height) = window_list::virtual_screen_bounds();
        let ppp = ctx.pixels_per_point().max(0.5);
        let size = vec2(240.0, 288.0);
        let margin = 18.0;
        let viewport_rect = egui::Rect::from_min_max(
            pos2(left as f32 / ppp, top as f32 / ppp),
            pos2(
                (left as f32 + width as f32) / ppp,
                (top as f32 + height as f32) / ppp,
            ),
        );
        let candidates = [
            egui::Rect::from_min_size(
                viewport_rect.right_top() - vec2(size.x + margin, -margin),
                size,
            ),
            egui::Rect::from_min_size(viewport_rect.left_top() + vec2(margin, margin), size),
            egui::Rect::from_min_size(
                viewport_rect.right_bottom() - vec2(size.x + margin, size.y + margin),
                size,
            ),
            egui::Rect::from_min_size(
                viewport_rect.left_bottom() + vec2(margin, -(size.y + margin)),
                size,
            ),
        ];
        let pos = if let Some(pointer) = pointer {
            let pointer_safe_zone = egui::Rect::from_center_size(pointer, vec2(320.0, 320.0));
            candidates
                .into_iter()
                .find(|candidate| !candidate.intersects(pointer_safe_zone))
                .unwrap_or_else(|| {
                    candidates
                        .into_iter()
                        .max_by(|a, b| {
                            let a_dist = a.center().distance_sq(pointer);
                            let b_dist = b.center().distance_sq(pointer);
                            a_dist
                                .partial_cmp(&b_dist)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .unwrap_or(candidates[0])
                })
                .min
        } else {
            candidates[0].min
        };
        (pos, size)
    }

    fn refresh_capture_info_window(&mut self, ctx: &egui::Context) {
        let pointer = Self::current_screen_cursor_pos().map(|(x, y)| {
            let ppp = ctx.pixels_per_point().max(0.5);
            egui::pos2(x as f32 / ppp, y as f32 / ppp)
        });
        let (pos, size) = Self::capture_info_window_placement(ctx, pointer);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }

    fn show_capture_info_window(&mut self, ctx: &egui::Context) {
        self.refresh_capture_info_window(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    #[cfg(windows)]
    #[cfg(not(windows))]
    fn poll_mouse_move_absolute_capture(&mut self, _ctx: &egui::Context) {}

    fn pick_point_button_text(language: UiLanguage, active: bool) -> RichText {
        if active {
            RichText::new(Self::tr_lang(language, "Picking...", "Đang chọn..."))
                .strong()
                .color(Color32::from_rgb(255, 232, 96))
        } else {
            RichText::new(Self::tr_lang(language, "Pick", "Pick"))
        }
    }

    #[cfg(windows)]
    #[cfg(not(windows))]
    fn spawn_image_search_point_capture_thread(
        ui_tx: Sender<UiCommand>,
        ctx: egui::Context,
        _preset_id: u32,
        _priority_anchor: bool,
    ) {
        let _ = ui_tx.send(UiCommand::VisionPointCaptureCancelled(
            "Image point capture is only supported on Windows.".to_owned(),
        ));
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    #[cfg(windows)]
    #[cfg(not(windows))]
    fn spawn_image_search_region_capture_thread(
        ui_tx: Sender<UiCommand>,
        ctx: egui::Context,
        _preset_id: u32,
        _template_mode: bool,
    ) {
        let _ = ui_tx.send(UiCommand::VisionPointCaptureCancelled(
            "Image area capture is only supported on Windows.".to_owned(),
        ));
        ctx.request_repaint();
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
                self.sync_vision_presets();
                self.persist();
            }
            (CaptureRequest::MacrosMasterHotkey, CapturedInput::Binding(binding)) => {
                self.state.macros_master_hotkey = Some(binding);
                self.sync_macro_master_hotkey();
                self.persist();
                self.status = match self.state.ui_language {
                    UiLanguage::Vietnamese => "Đã gán hotkey bật/tắt macro.".to_owned(),
                    _ => "Captured the macro master hotkey.".to_owned(),
                };
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
                        let label = if preset.hold_stop_step.action
                            == MacroAction::StopIfKeyPressed
                        {
                            "hold-stop stop key"
                        } else {
                            "hold-stop lock key"
                        };
                        if existing.iter().any(|part| part.eq_ignore_ascii_case(&key)) {
                            self.status = format!("Key {key} is already in that {label} list.");
                        } else if existing.is_empty() {
                            preset.hold_stop_step.key = key.clone();
                            self.status =
                                format!("Captured {label} {key} for macro {preset_id}.");
                        } else {
                            preset.hold_stop_step.key =
                                format!("{},{}", preset.hold_stop_step.key.trim(), key);
                            self.status =
                                format!("Added {label} {key} for macro {preset_id}.");
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
        match self.state.ui_language {
            UiLanguage::Vietnamese => {
                if combo_keys.len() == 1 {
                    format!(
                        "Đã nhận phím: {label}. Giữ thêm phím khác để thành combo, hoặc thả ra để lưu."
                    )
                } else {
                    format!("Đã nhận combo: {label}. Thả ra để lưu.")
                }
            }
            _ => {
                if combo_keys.len() == 1 {
                    format!(
                        "Captured key: {label}. Hold another key to form a combo, or release to save."
                    )
                } else {
                    format!("Captured combo: {label}. Release to save.")
                }
            }
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
        self.sync_macro_presets();
        self.sync_macro_master_enabled();
        self.persist();
    }

    fn persist_timer_presets(&mut self) {
        self.sync_timer_presets();
        self.persist();
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

    fn settings_card_frame(ui: &egui::Ui) -> egui::Frame {
        egui::Frame::group(ui.style())
            .fill(Color32::from_rgba_premultiplied(32, 36, 42, 160))
            .stroke(egui::Stroke::new(
                1.0,
                Color32::from_rgba_premultiplied(90, 100, 115, 80),
            ))
            .corner_radius(14.0)
            .inner_margin(egui::Margin::same(16))
    }

    fn refresh_macro_ai_debug_text_from_trace(&mut self) {}

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
            if app_state_needs_cjk_fallback(&self.state) {
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
            if Self::active_panel_needs_open_windows(panel) {
                self.ensure_open_windows_ready(false);
            }
            if Self::active_panel_needs_audio_sense_devices(panel) {
                self.ensure_audio_sense_devices_ready(false);
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
}

impl eframe::App for CrosshairApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if matches!(self.state.active_panel, AppPanel::Zoom | AppPanel::Modes) {
            self.state.active_panel = AppPanel::Pin;
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

        while let Ok(command) = self.ui_rx.try_recv() {
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
                UiCommand::StartupIconLoaded(icon) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(icon)));
                }
                UiCommand::SyncMacroGroups(groups, status) => {
                    self.state.macro_groups = groups;
                    self.persist_macro_presets();
                    self.status = status;
                }
                UiCommand::SyncCrosshairProfiles(profiles, status) => {
                    self.state.profiles = profiles;
                    if self.state.profiles.is_empty() {
                        self.state.profiles.push(ProfileRecord::default());
                    }
                    self.persist();
                    self.status = status;
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
                    if let Some(group) = self
                        .state
                        .macro_groups
                        .iter_mut()
                        .find(|g| g.id == group_id)
                    {
                        if let Some(preset) = group.presets.iter_mut().find(|p| p.id == preset_id) {
                            if preset.steps.len() == 1
                                && preset.steps[0].action == MacroAction::KeyPress
                                && preset.steps[0].key.is_empty()
                                && preset.steps[0].delay_ms == 100
                            {
                                preset.steps.clear();
                            }
                            preset.steps.push(step);
                        }
                    }
                    ctx.request_repaint();
                }
                UiCommand::MacroRealtimeStepRemoved(group_id, preset_id) => {
                    if let Some(group) = self
                        .state
                        .macro_groups
                        .iter_mut()
                        .find(|g| g.id == group_id)
                    {
                        if let Some(preset) = group.presets.iter_mut().find(|p| p.id == preset_id) {
                            preset.steps.pop();
                        }
                    }
                    ctx.request_repaint();
                }
                UiCommand::MacroRecordingFinished(group_id, preset_id, _events, status) => {
                    if let Some(group) = self
                        .state
                        .macro_groups
                        .iter_mut()
                        .find(|g| g.id == group_id)
                    {
                        if let Some(preset) = group.presets.iter_mut().find(|p| p.id == preset_id) {
                            if let Some(record_hotkey) = &preset.record_hotkey {
                                let hotkey_keys: Vec<String> =
                                    crate::hotkey::binding_key_names(record_hotkey)
                                        .into_iter()
                                        .map(|k| k.trim().to_ascii_lowercase())
                                        .collect();
                                while let Some(last) = preset.steps.last() {
                                    if last.action == MacroAction::KeyPress {
                                        let k = last.key.trim().to_ascii_lowercase();
                                        if hotkey_keys.contains(&k) {
                                            preset.steps.pop();
                                            continue;
                                        }
                                    }
                                    break;
                                }
                            }
                            if preset.steps.is_empty() {
                                preset.steps.push(MacroStep::default());
                            }
                        }
                    }
                    self.active_macro_record_preset_id = None;
                    self.persist();
                    self.status = status;
                    ctx.request_repaint();
                }
                UiCommand::MousePathRecordingFinished(preset_id, events, status) => {
                    if let Some(preset) = self
                        .state
                        .mouse_path_presets
                        .iter_mut()
                        .find(|preset| preset.id == preset_id)
                    {
                        preset.events = events;
                    }
                    self.active_mouse_record_preset_id = None;
                    self.persist_mouse_path_presets();
                    self.status = status;
                    if self.mouse_path_draw_capture_preset_id == Some(preset_id) {
                        self.mouse_path_draw_capture_preset_id = None;
                        self.restore_mouse_path_draw_capture_window(ctx);
                    }
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
                UiCommand::VisionCaptureMouseDown { screen_x, screen_y } => {
                    if self.vision_capture_active {
                        self.handle_image_search_capture_mouse_down(ctx, screen_x, screen_y);
                    }
                }
                UiCommand::VisionCaptureMouseMove { screen_x, screen_y } => {
                    if self.vision_capture_active {
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
                UiCommand::MouseMoveAbsolutePointCaptured { .. } => {}
                UiCommand::MouseMoveAbsoluteCaptureCancelled => {}
                UiCommand::UpdateCheckStarted => {
                    self.update_status = UpdateStatus::Checking;
                }
                UiCommand::UpdateAvailable(version, body, url) => {
                    self.update_status = UpdateStatus::Available(version, body, url);
                }
                UiCommand::UpdateDownloadStarted => {
                    self.update_status = UpdateStatus::Downloading;
                }
                UiCommand::UpdateDownloadFinished(new_exe_path) => {
                    self.update_status = UpdateStatus::ReadyToRestart(new_exe_path);
                }
                UiCommand::UpdateError(e) => {
                    self.update_status = UpdateStatus::Error(e);
                }
                UiCommand::UpdateUpToDate => {
                    self.update_status = UpdateStatus::UpToDate;
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
                    let image = ColorImage::from_rgba_unmultiplied([frame.width, frame.height], &frame.rgba);
                    if let Some(cache) = self.zoom_preview_cache.get_mut(&cache_id) {
                        cache.view.texture.set(image, TextureOptions::LINEAR);
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
                UiCommand::VideoFrameLoaded {
                    preset_id,
                    path,
                    start_ms,
                    max_width,
                    max_height,
                    width,
                    height,
                    rgba,
                } => {
                    let image = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                    if let Some(cache) = self.video_preview_cache.get_mut(&preset_id) {
                        cache.view.texture.set(image, TextureOptions::LINEAR);
                        cache.updated_at = Instant::now();
                        cache.source_path = path;
                        cache.start_ms = start_ms;
                        cache.max_width = max_width;
                        cache.max_height = max_height;
                        cache.view.width = width;
                        cache.view.height = height;
                    } else {
                        let texture = ctx.load_texture(
                            format!("video_preview_{preset_id}"),
                            image,
                            TextureOptions::LINEAR,
                        );
                        let view = VideoPreviewView {
                            texture,
                            width,
                            height,
                        };
                        self.video_preview_cache.insert(
                            preset_id,
                            VideoPreviewCache {
                                updated_at: Instant::now(),
                                source_path: path,
                                start_ms,
                                max_width,
                                max_height,
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
                    self.audio_waveforms.insert(path.clone(), waveform);
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
                    for preset in &mut self.state.audio_settings.video_presets {
                        if preset.clip.file_path.trim() == path
                            && self
                                .video_preset_clip_duration_ms
                                .get(&preset.id)
                                .copied()
                                .flatten()
                                .is_none()
                        {
                            self.video_preset_clip_duration_ms
                                .insert(preset.id, duration_ms);
                        }
                    }
                    if self.state.audio_settings.startup.file_path.trim() == path {
                        self.startup_clip_duration_ms = duration_ms;
                    }
                    if self.state.audio_settings.exit.file_path.trim() == path {
                        self.exit_clip_duration_ms = duration_ms;
                    }
                    ctx.request_repaint();
                }
                UiCommand::OpenWindowsLoaded { windows, status } => {
                    self.open_windows = windows;
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
                UiCommand::VideoPlaybackFinished(preset_id) => {
                    if self.active_video_overlay_preset_id == Some(preset_id) {
                        self.active_video_overlay_preset_id = None;
                    }
                    ctx.request_repaint();
                }
            }
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
                            "Cài đặt OpenCV thành công.",
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

        if let Some(job) = &self.interception_download_job {
            if job.is_finished() {
                let job = self.interception_download_job.take().unwrap();
                match job.join() {
                    Ok(Ok(())) => {
                        self.interception_package_downloaded = true;
                        self.status = Self::tr_lang(
                            self.state.ui_language,
                            "Interception package downloaded successfully.",
                            "Cài đặt Interception driver thành công.",
                        )
                        .to_owned();
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
                        self.interception_driver_installed =
                            crate::platform::is_interception_driver_installed();
                        self.interception_driver_needs_restart = self.interception_driver_installed;
                        self.status = Self::tr_lang(
                            self.state.ui_language,
                            "Interception driver installed. Restart your PC for it to take effect.",
                            "Đã cài driver Interception. Hãy khởi động lại máy để driver có hiệu lực."
                        ).to_owned();
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
                        self.interception_driver_installed =
                            crate::platform::is_interception_driver_installed();
                        self.interception_driver_needs_restart = true;
                        self.status = Self::tr_lang(
                            self.state.ui_language,
                            "Interception driver removed. Restart your PC to finish cleanup.",
                            "Đã gỡ driver Interception. Hãy khởi động lại máy để hoàn tất dọn dẹp.",
                        )
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
            if self.state.active_panel == AppPanel::Macros && !self.panel_is_warmed(AppPanel::Macros)
            {
                self.macro_panel_render_limit = 8;
            }
            if matches!(self.last_active_panel, AppPanel::Sound | AppPanel::Media)
                && !matches!(self.state.active_panel, AppPanel::Sound | AppPanel::Media)
            {
                self.clear_video_preview_cache();
            }
            self.last_active_panel = self.state.active_panel;
        }

        let viewport_focused = ctx.input(|input| input.viewport().focused != Some(false));
        let keep_pin_preview = viewport_focused && self.state.active_panel == AppPanel::Pin;
        if !keep_pin_preview && self.disable_pin_preview_modes() {
            self.persist();
        }
        let keep_toolbox_preview = viewport_focused && self.state.active_panel == AppPanel::Hud;
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
        let keep_geometry_preview = viewport_focused && self.state.active_panel == AppPanel::Geometry;
        if !keep_geometry_preview && (self.geometry_preview_target.is_some() || self.geometry_preset_preview_target.is_some()) {
            self.geometry_preview_target = None;
            self.geometry_preview_sent = None;
            self.geometry_preset_preview_target = None;
            let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometrySpec(None));
            let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometryPreset(None));
        }

        let keep_macro_geometry_preview = viewport_focused && self.state.active_panel == AppPanel::Macros;
        if !keep_macro_geometry_preview && self.draw_geometry_step_preview_target.is_some() {
            self.draw_geometry_step_preview_target = None;
            self.draw_geometry_step_preview_sent = None;
            let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometrySpec(None));
        } else if let Some((group_id, preset_id, step_index, is_hold_stop)) = self.draw_geometry_step_preview_target {
            let preview_spec = self.state.macro_groups.iter()
                .find(|g| g.id == group_id)
                .and_then(|g| g.presets.iter().find(|p| p.id == preset_id))
                .and_then(|p| {
                    if is_hold_stop {
                        Some(&p.hold_stop_step)
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
                let _ = self
                    .overlay_tx
                    .send(crate::overlay::OverlayCommand::PreviewGeometrySpec(preview_spec));
            }
        }

        if viewport_focused
            && self.state.show_window
            && Self::active_panel_needs_open_windows(self.state.active_panel)
        {
            self.ensure_open_windows_ready(false);
        }

        if viewport_focused
            && self.state.show_window
            && Self::active_panel_needs_audio_sense_devices(self.state.active_panel)
        {
            self.ensure_audio_sense_devices_ready(false);
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
        if self.mouse_move_absolute_capture_target.is_some() {
            self.poll_mouse_move_absolute_capture(ctx);
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
        if self.render_mouse_move_absolute_capture_overlay(ctx) {
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
                    .inner_margin(egui::Margin::symmetric(10, 3)),
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
                                ui.add_sized(
                                    [38.0, 30.0],
                                    self.titlebar_button(
                                        Self::material_icon_text(0xe5cd, 18.0),
                                        false,
                                        true,
                                    ),
                                ),
                                show_icon_tooltips,
                                self.tr("Exit", "Thoát"),
                            );
                            if exit_response.clicked() {
                                let _ = self.overlay_tx.send(OverlayCommand::Exit);
                            }
                            let hide_response = Self::hover_if(
                                ui.add_sized(
                                    [38.0, 30.0],
                                    self.titlebar_button(
                                        Self::material_icon_text(0xe8a4, 18.0),
                                        false,
                                        false,
                                    ),
                                ),
                                show_icon_tooltips,
                                self.tr("Hide to tray", "Ẩn xuống khay"),
                            );
                            if hide_response.clicked() {
                                self.hide_to_tray(ctx);
                            }
                            let maximize_response = Self::hover_if(
                                ui.add_sized(
                                    [38.0, 30.0],
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
                                ui.add_sized(
                                    [38.0, 30.0],
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
                                ui.add_sized(
                                    [38.0, 30.0],
                                    self.titlebar_button(self.theme_button_text(), false, false),
                                ),
                                show_icon_tooltips,
                                self.titlebar_theme_tooltip(),
                            );
                            if theme_response.clicked() {
                                self.toggle_theme_mode();
                            }
                            let language_response = Self::hover_if(
                                ui.add_sized(
                                    [38.0, 30.0],
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
                                ui.add_sized(
                                    [38.0, 30.0],
                                    if let Some(texture) = vietnamese_input_texture.as_ref() {
                                        let image = Image::new((texture.id(), vec2(20.0, 20.0)));
                                        let (fill, stroke) = if self.state.ui_theme
                                            == UiThemeMode::Dark
                                        {
                                            (
                                                Color32::from_rgba_premultiplied(54, 67, 88, 88),
                                                Color32::from_rgb(74, 92, 118),
                                            )
                                        } else {
                                            (
                                                Color32::from_rgba_premultiplied(
                                                    220, 228, 238, 165,
                                                ),
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
                            let settings_response = Self::hover_if(
                                ui.add_sized(
                                    [38.0, 30.0],
                                    self.titlebar_button(
                                        Self::material_icon_text(0xe8b8, 18.0),
                                        false,
                                        false,
                                    ),
                                ),
                                show_icon_tooltips,
                                Self::tr_lang(self.state.ui_language, "Settings", "Settings"),
                            );
                            if settings_response.clicked() {
                                self.settings_popup_open = !self.settings_popup_open;
                            }

                        ui.add_space(8.0);

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
                                    egui::Frame::new()
                                        .fill(button_fill)
                                        .stroke(egui::Stroke::new(1.0, accent.gamma_multiply(0.45)))
                                        .corner_radius(8.0)
                                        .inner_margin(egui::Margin::symmetric(10, 4))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(self.app_brand_title())
                                                        .strong()
                                                        .size(14.0),
                                                );
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new(format!(
                                                        "v{}",
                                                        self.app_version_label()
                                                    ))
                                                    .size(9.0)
                                                    .color(
                                                        if self.state.ui_theme == UiThemeMode::Dark
                                                        {
                                                            Color32::from_rgb(175, 194, 221)
                                                        } else {
                                                            Color32::from_rgb(80, 96, 128)
                                                        },
                                                    ),
                                                );
                                            });
                                        });
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
                ui.horizontal_wrapped(|ui| {
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
                        let response = ui.add(self.top_tab_button(text, selected, emphasized));
                        if response.clicked() {
                            self.state.active_panel = panel;
                        }
                    }
                    if self.active_audio_editor.is_some() {
                        let text = RichText::new(self.panel_label(AppPanel::Media));
                        let response = ui.add(self.top_tab_button(
                            text,
                            self.state.active_panel == AppPanel::Media,
                            false,
                        ));
                        if response.clicked() {
                            self.state.active_panel = AppPanel::Media;
                        }
                    }
                    let text = RichText::new(self.panel_label(AppPanel::Hud));
                    let response = ui.add(self.top_tab_button(
                        text,
                        self.state.active_panel == AppPanel::Hud,
                        false,
                    ));
                    if response.clicked() {
                        self.state.active_panel = AppPanel::Hud;
                    }
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
            if self.active_pitch_preview_preset_id.take().is_some() || self.audio_sense_test_active {
                self.pitch_monitor.stop();
                self.audio_sense_test_active = false;
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
                } else if active_panel == AppPanel::Macros || active_panel == AppPanel::Modes {
                    self.render_macro_panel(ui);
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
                                AppPanel::Mouse => self.render_mouse_panel(ui),
                                AppPanel::Vision => self.render_vision_panel(ui, ctx),
                                AppPanel::AudioSense => self.render_audiosense_panel(ui),
                                AppPanel::Ocr => self.render_ocr_panel(ui),
                                AppPanel::Geometry => self.render_geometry_panel(ui),
                                AppPanel::Zoom => self.render_pin_panel(ui),
                                AppPanel::Modes => unreachable!(),
                                AppPanel::Macros => unreachable!(),
                                AppPanel::Commands => self.render_commands_panel(ui),
                                AppPanel::Sound => self.render_sound_panel(ui),
                                AppPanel::Hud => self.render_hud_panel(ui),
                                AppPanel::Media => self.render_media_panel(ui),
                            }
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

        if self.settings_popup_open {
            if self.capture_target.is_none()
                && ctx.input(|input| input.key_pressed(egui::Key::Escape))
            {
                self.settings_popup_open = false;
            } else {
                self.render_modal_backdrop(ctx, true);
                let (panel_size, panel_pos) =
                    Self::centered_modal_placement(ctx, vec2(600.0, 620.0), vec2(500.0, 500.0));
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
                            .inner_margin(Margin::same(20))
                            .show(ui, |ui| {
                                ui.set_min_size(panel_size);
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

        if self.variable_inspector_open {
            let mut open = self.variable_inspector_open;
            let screen_center = ctx.screen_rect().center();
            egui::Window::new(Self::tr_lang(self.state.ui_language, "Variables", "Biến"))
                .open(&mut open)
                .fixed_pos(screen_center)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_size(egui::vec2(820.0, 430.0))
                .resizable(false)
                .movable(false)
                .collapsible(false)
                .show(ctx, |ui| {
                    self.render_variable_inspector(ui);
                });
            self.variable_inspector_open = open;
        }

        if self.startup_show_pending && self.state.show_window {
            self.startup_show_pending = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.startup_gate_release_pending = true;
            self.startup_gate_frames_remaining = 1;
            ctx.request_repaint();
        }

        self.poll_capture_input(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
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
        self.persist();
    }
}

pub(crate) fn audio_duration(clip: &AudioClipSettings) -> Option<u64> {
    if clip.file_path.trim().is_empty() {
        None
    } else {
        audio::load_duration_ms(&clip.file_path).ok()
    }
}

pub(crate) fn video_duration(clip: &VideoClipSettings) -> Option<u64> {
    if clip.file_path.trim().is_empty() {
        None
    } else {
        crate::media::load_video_metadata(&clip.file_path)
            .ok()
            .map(|meta| meta.duration_ms)
    }
}




