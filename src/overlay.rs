#![allow(unsafe_op_in_unsafe_fn)]
#[derive(Debug, Clone)]
pub struct MacroRecordingEvent {
    pub key: Option<String>,
    pub action: crate::model::MacroAction,
    pub delay_ms: u64,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub struct MacroRecordingSession {
    pub group_id: u32,
    pub preset_id: u32,
    pub last_event_at: std::time::Instant,
    pub events: Vec<MacroRecordingEvent>,
    pub pressed_key_vks: std::collections::HashSet<u32>,
}

#[cfg(windows)]
mod windows_overlay {

    #[path = "../math_expr.rs"]
    pub mod math_expr;
    #[path = "../drawing.rs"]
    pub mod drawing;
    #[path = "../arduino.rs"]
    pub mod arduino;
    #[path = "../vision.rs"]
    pub mod vision;
    #[path = "../audio_sense.rs"]
    pub mod audio_sense;
    #[path = "../native_capture.rs"]
    pub mod native_capture;

    pub use math_expr::*;
    pub use drawing::*;
    pub use arduino::*;
    pub use vision::*;
    pub use audio_sense::*;
    pub use native_capture::*;

    use super::{MacroRecordingEvent, MacroRecordingSession};
    use crate::ui::{VisionCaptureTarget, VisionCaptureMode, MouseMoveAbsoluteCaptureTarget};
    use std::{
        borrow::Cow,
        collections::{HashMap, HashSet},
        ffi::{CString, c_void},
        mem::size_of,
        os::windows::process::CommandExt,
        path::PathBuf,
        process::Command,
        ptr::null_mut,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicIsize, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };
    use arboard::{Clipboard, ImageData};
    use anyhow::{Context, Result, bail};
    use crossbeam_channel::{Receiver, Sender};
    use eframe::egui;
    use hidapi::HidApi;
    use once_cell::sync::Lazy;
    use opencv::{
        core::{self as cv, Mat, Size},
        imgproc,
        prelude::*,
    };
    use parking_lot::Mutex;
    use windows::{
        Win32::{
            Devices::HumanInterfaceDevice::HidD_SetOutputReport,
            Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
            Storage::FileSystem::{
                CreateFileA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
                OPEN_EXISTING, WriteFile,
            },
            Graphics::{
                Dwm::{
                    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
                    DWM_TNP_RECTSOURCE, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
                    DWMWA_BORDER_COLOR, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
                    DwmRegisterThumbnail, DwmSetWindowAttribute, DwmUnregisterThumbnail,
                    DwmUpdateThumbnailProperties,
                },
                Gdi::{
                    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO,
                    BITMAPINFOHEADER, BLENDFUNCTION, BeginPaint, CLIP_DEFAULT_PRECIS,
                    ClientToScreen, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateRectRgn,
                    DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER, DT_SINGLELINE,
                    DT_VCENTER,
                    DeleteDC, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_MEDIUM, GetDC,
                    GetMonitorInfoW, HDC, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO,
                    MonitorFromWindow, OUT_DEFAULT_PRECIS, PAINTSTRUCT, ReleaseDC, SRCCOPY,
                    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, StretchDIBits,
                    TRANSPARENT,
                },
            },
            Media::Audio::{
                Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole,
                eRender,
            },
            System::{
                Com::{
                    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                    CoUninitialize,
                },
                LibraryLoader::GetModuleHandleW,
                Threading::{CREATE_NO_WINDOW, GetCurrentProcessId},
            },
            UI::{
                Accessibility::{
                    HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent,
                },
                Input::KeyboardAndMouse::{
                    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
                    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE,
                    MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE,
                    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
                    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
                    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP,
                    MOUSEINPUT, MapVirtualKeyW, RegisterHotKey, SendInput, UnregisterHotKey,
                    VIRTUAL_KEY,
                },
                Shell::{
                    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                    NOTIFYICONDATAW, Shell_NotifyIconW,
                },
                WindowsAndMessaging::{
                    AppendMenuW, CREATESTRUCTW, CallNextHookEx, CreatePopupMenu, CreateWindowExW,
                    DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW,
                    EVENT_SYSTEM_FOREGROUND, GA_ROOT, GW_OWNER, GWLP_USERDATA, GetAncestor,
                    GetClassNameW, GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageW,
                    GetSystemMetrics, GetWindow,
                    GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HC_ACTION, HHOOK,
                    HMENU, HTTRANSPARENT, HWND_TOPMOST, IDC_ARROW, IMAGE_ICON, IsZoomed,
                    KBDLLHOOKSTRUCT, KillTimer, LR_LOADFROMFILE, LoadCursorW,
                    LoadImageW, MA_NOACTIVATE, MF_SEPARATOR, MF_STRING, MSG, MSLLHOOKSTRUCT,
                    PostMessageW, PostQuitMessage, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN,
                    SPI_GETMOUSESPEED, SPI_SETMOUSESPEED, SW_HIDE, SW_RESTORE, SW_SHOWNA,
                    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
                    SetCursorPos, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos,
                    SetWindowsHookExW, ShowWindow, SystemParametersInfoW, TPM_BOTTOMALIGN,
                    TPM_LEFTALIGN, TrackPopupMenu, TranslateMessage, ULW_ALPHA,
                    UnhookWindowsHookEx, UpdateLayeredWindow, WH_KEYBOARD_LL, WH_MOUSE_LL,
                    WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WM_APP, WM_COMMAND, WM_CREATE,
                    WM_DESTROY, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
                    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL,
                    WM_NCCREATE, WM_NCHITTEST, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
                    WM_SYSKEYUP, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW, WS_CAPTION,
                    WINEVENT_OUTOFCONTEXT, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                    WS_EX_TRANSPARENT, WS_OVERLAPPEDWINDOW, WS_POPUP, WindowFromPoint,
                },
            },
        },
        core::{PCSTR, PCWSTR, w},
    };
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum InterceptionRuntimeStatus {
        Active,
        FallbackToSendInput,
        Unavailable,
    }

    impl InterceptionRuntimeStatus {
        fn label(self) -> &'static str {
            match self {
                Self::Active => "Interception: Active",
                Self::FallbackToSendInput => "Interception: Fallback to SendInput",
                Self::Unavailable => "Interception: Unavailable",
            }
        }
    }

    use crate::{
        ai, audio, audiosense, hotkey, media,
        model::{
            ArduinoTransport, AudioSensePreset, AudioSenseSpec, AudioSettings, CommandPreset, CrosshairStyle,
            HotkeyBinding, HudPreset, GeometryShapeKind, GeometrySpec, IfConditionType, MacroAction,
            MacroGroup, MacroPreset, MacroStep, MacroTriggerMode, MousePathEvent,
            MousePathEventKind, MousePathPreset, MouseSensitivityPreset, PinOverlayStyle,
            PinPreset, ProfileRecord, RgbaColor, SoundLibraryItem, SoundPreset,
            TimerPreset, VideoPreset, VisionPreset, VisionSettings, WindowAnchor, WindowExpandControls,
            WindowExpandDirection, WindowFocusPreset, WindowPreset,
        },
        render::{RenderedCrosshair, RenderedSvgImage, render_crosshair, render_svg_image},
        storage::AppPaths,
        window_list,
    };
    use image::{RgbaImage, imageops::FilterType};
    #[path = "../window_preset.rs"]
    mod window_preset;
    const HOTKEY_ID: i32 = 1001;
    const TIMER_ID: usize = 1;
    const TRAY_UID: u32 = 7001;
    const XBUTTON1_DATA: u16 = 0x0001;
    const XBUTTON2_DATA: u16 = 0x0002;
    const WMAPP_TRAYICON: u32 = WM_APP + 1;
    const WMAPP_PROCESS_QUEUE: u32 = WM_APP + 2;
    const WMAPP_WINDOW_FOCUS_CHANGED: u32 = WM_APP + 3;
    const WMAPP_WINDOW_LOCATION_CHANGED: u32 = WM_APP + 4;
    const MACRO_PRESET_BASE_ID: i32 = 10000;
    const FOCUS_TRIGGER_TIMER_ID: usize = 2;
    const SCREEN_DRAW_TIMER_ID: usize = 3;
    const SCREEN_DRAW_REFRESH_INTERVAL_MS: u32 = 16;
    const SCREEN_DRAW_MIN_FRAME_INTERVAL_MS: u64 = 6;
    const SCREEN_DRAW_TOOLBAR_WIDTH: i32 = 408;
    const SCREEN_DRAW_TOOLBAR_HEIGHT: i32 = 78;
    const SCREEN_DRAW_TOOLBAR_CLOSE_X: i32 = 370;
    const SCREEN_DRAW_TOOLBAR_CAPTURE_X: i32 = 326;
    #[derive(Debug, Clone)]
    struct VisionRunOutcome {
        matched: bool,
        status: String,
    }

    const MENU_SHOW: usize = 2002;
    const MENU_EXIT: usize = 2003;
    static SUPPRESSED_MACRO_HOTKEYS: Lazy<Mutex<HashSet<i32>>> =
        Lazy::new(|| Mutex::new(HashSet::new()));
    static STOP_REQUESTED_MACRO_PRESETS: Lazy<Mutex<HashSet<u32>>> =
        Lazy::new(|| Mutex::new(HashSet::new()));
    static FORCE_STOP_REQUESTED_MACRO_PRESETS: Lazy<Mutex<HashSet<u32>>> =
        Lazy::new(|| Mutex::new(HashSet::new()));
    pub(crate) static HUD_DISPLAY: Lazy<Mutex<Option<HudDisplayState>>> = Lazy::new(|| Mutex::new(None));
    static HUD_PREVIEW_DISPLAY: Lazy<Mutex<Option<HudDisplayState>>> =
        Lazy::new(|| Mutex::new(None));
    pub(crate) static ACTIVE_VIDEO_PRESET_ID: Lazy<Mutex<Option<u32>>> = Lazy::new(|| Mutex::new(None));
    pub(crate) static ACTIVE_VIDEO_EXPIRES: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));
    static MOUSE_RECORDING: Lazy<Mutex<Option<MouseRecordingSession>>> =
        Lazy::new(|| Mutex::new(None));
    static MOUSE_PATH_PREVIEW: Lazy<Mutex<Option<MousePathPreviewSession>>> =
        Lazy::new(|| Mutex::new(None));
    static MACRO_RECORDING: Lazy<Mutex<Option<MacroRecordingSession>>> =
        Lazy::new(|| Mutex::new(None));
    static SCREEN_DRAW_STATE: Lazy<Mutex<ScreenDrawState>> =
        Lazy::new(|| Mutex::new(ScreenDrawState::default()));
    static SCREEN_DRAW_HWND: AtomicIsize = AtomicIsize::new(0);





    pub(crate) static HOOK_STATE: Lazy<Mutex<HookState>> =
        Lazy::new(|| Mutex::new(HookState::default()));
    static ACTIVE_VIDEO_STOP: Lazy<Mutex<Option<Arc<AtomicBool>>>> = Lazy::new(|| Mutex::new(None));
    static ACTIVE_VIDEO_THREAD: Lazy<Mutex<Option<thread::JoinHandle<()>>>> =
        Lazy::new(|| Mutex::new(None));
    static SYNTHETIC_MOUSE_TRIGGER_SUPPRESSION: Lazy<Mutex<HashMap<String, usize>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    static SWALLOWED_MOUSE_TRIGGER_RELEASES: Lazy<Mutex<HashSet<String>>> =
        Lazy::new(|| Mutex::new(HashSet::new()));
    pub static ACTIVE_MACRO_STEPS: Lazy<Mutex<HashMap<u32, HashSet<usize>>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    static GEOMETRY_SVG_CACHE: Lazy<Mutex<HashMap<(String, u32, u32, u32, i32), RenderedSvgImage>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    pub fn add_active_step(preset_id: u32, step_index: usize) {
        let mut active = ACTIVE_MACRO_STEPS.lock();
        active.entry(preset_id).or_default().insert(step_index);
        drop(active);
        request_ui_repaint();
    }

    pub fn remove_active_step(preset_id: u32, step_index: usize) {
        let mut active = ACTIVE_MACRO_STEPS.lock();
        if let Some(set) = active.get_mut(&preset_id) {
            set.remove(&step_index);
            if set.is_empty() {
                active.remove(&preset_id);
            }
        }

        drop(active);
        request_ui_repaint();
    }

    pub struct ActiveStepGuard {
        preset_id: u32,
        step_index: usize,
    }

    impl ActiveStepGuard {
        pub fn new(preset_id: u32, step_index: usize) -> Self {
            add_active_step(preset_id, step_index);
            Self {
                preset_id,
                step_index,
            }
        }
    }

    impl Drop for ActiveStepGuard {
        fn drop(&mut self) {
            remove_active_step(self.preset_id, self.step_index);
        }
    }

    pub fn is_vision_following_active_by_spec(spec: &str) -> bool {
        if let Ok(preset) = vision_preset_by_id(spec) {
            HOOK_STATE
                .lock()
                .vision_following_presets
                .contains(&preset.id)
        } else {
            false
        }
    }

    pub fn is_timer_preset_active(t_id: Option<u32>) -> bool {
        if let Some(id) = t_id {
            HOOK_STATE
                .lock()
                .active_timers
                .get(&id)
                .map(|s| s.running)
                .unwrap_or(false)
        } else {
            false
        }
    }

    static OVERLAY_COMMAND_TX: Lazy<Mutex<Option<Sender<OverlayCommand>>>> =
        Lazy::new(|| Mutex::new(None));
    static RANDOM_STATE: Lazy<std::sync::atomic::AtomicU64> = Lazy::new(|| {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        std::sync::atomic::AtomicU64::new(seed ^ 0x9E37_79B9_7F4A_7C15)
    });
    static SEARCH_AREA_OVERLAY_REFRESH_PENDING: AtomicBool = AtomicBool::new(false);
    static UI_CONTEXT: Lazy<Mutex<Option<egui::Context>>> = Lazy::new(|| Mutex::new(None));
    static CONTROLLER_HWND: AtomicIsize = AtomicIsize::new(0);
    static ACTIVE_HIGHLIGHT_HWND: AtomicIsize = AtomicIsize::new(0);
    static ACTIVE_PIN_SOURCE_HWND: AtomicIsize = AtomicIsize::new(0);
    static PROTRACTOR_HWND: AtomicIsize = AtomicIsize::new(0);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ProtractorDragTarget {
        Close,
        Needle1,
        Needle2,
        ResizeGrip,
        Body,
        ThicknessSlider,
        CalibrationButton,
    }

    struct ProtractorState {
        enabled: bool,
        scale: f32,
        needle1_angle: f32,
        needle2_angle: f32,
        center_x: i32,
        center_y: i32,
        thickness: f32,
        calibrating: bool,
        ui_language: crate::model::UiLanguage,
    }

    static PROTRACTOR_STATE: Lazy<Mutex<ProtractorState>> = Lazy::new(|| Mutex::new(ProtractorState {
        enabled: false,
        scale: 1.0,
        needle1_angle: 0.0,
        needle2_angle: 90.0,
        center_x: 500,
        center_y: 500,
        thickness: 2.0,
        calibrating: false,
        ui_language: crate::model::UiLanguage::English,
    }));

    static PROTRACTOR_DRAG_TARGET: Lazy<Mutex<Option<ProtractorDragTarget>>> = Lazy::new(|| Mutex::new(None));
    static PROTRACTOR_DRAG_START_MOUSE: Lazy<Mutex<POINT>> = Lazy::new(|| Mutex::new(POINT::default()));
    static PROTRACTOR_DRAG_START_CENTER: Lazy<Mutex<(i32, i32)>> = Lazy::new(|| Mutex::new((0, 0)));
    static PROTRACTOR_DRAG_START_ANGLE: Lazy<Mutex<f32>> = Lazy::new(|| Mutex::new(0.0));
    static PROTRACTOR_DRAG_START_SCALE: Lazy<Mutex<f32>> = Lazy::new(|| Mutex::new(1.0));
    static CACHED_APP_UI_HWND: AtomicIsize = AtomicIsize::new(0);
    pub static UI_WINDOW_RECT_LEFT: std::sync::atomic::AtomicI32 =
        std::sync::atomic::AtomicI32::new(0);
    pub static UI_WINDOW_RECT_TOP: std::sync::atomic::AtomicI32 =
        std::sync::atomic::AtomicI32::new(0);
    pub static UI_WINDOW_RECT_RIGHT: std::sync::atomic::AtomicI32 =
        std::sync::atomic::AtomicI32::new(0);
    pub static UI_WINDOW_RECT_BOTTOM: std::sync::atomic::AtomicI32 =
        std::sync::atomic::AtomicI32::new(0);
    pub static UI_WINDOW_VISIBLE: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    pub static UI_WINDOW_FOREGROUND: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    pub static FOREGROUND_WINDOW_HWND: std::sync::atomic::AtomicIsize =
        std::sync::atomic::AtomicIsize::new(0);
    pub static FOREGROUND_WINDOW_TITLE: Lazy<Mutex<Option<String>>> =
        Lazy::new(|| Mutex::new(None));
    pub static RUNTIME_VARIABLES: Lazy<Mutex<std::collections::HashMap<String, f64>>> =
        Lazy::new(|| Mutex::new(std::collections::HashMap::new()));
    pub static TEXT_VARIABLES: Lazy<Mutex<std::collections::HashMap<String, String>>> =
        Lazy::new(|| Mutex::new(std::collections::HashMap::new()));


    #[derive(Debug, Clone)]
    pub enum OverlayCommand {
        Update(CrosshairStyle),
        UpdateProfiles(Vec<ProfileRecord>),
        UpdateCrosshairProfile {
            index: usize,
            profile: ProfileRecord,
        },
        UpdateWindowPresets(Vec<WindowPreset>),
        UpdateWindowFocusPresets(Vec<WindowFocusPreset>),
        UpdateWindowLayouts(Vec<crate::model::WindowLayout>),
        ApplyWindowLayout(crate::model::WindowLayout),
        #[allow(dead_code)]
        UpdateWindowExpandControls(WindowExpandControls),
        UpdatePinPresets(Vec<PinPreset>),
        UpdateMousePathPresets(Vec<MousePathPreset>),
        PreviewMousePath(Option<(u32, Vec<MousePathEvent>, Option<u64>)>),
        UpdateMouseSensitivityPresets(Vec<MouseSensitivityPreset>),
        UpdateMouseSensitivitySettings {
            restore_on_exit: bool,
            restore_speed: u32,
        },
        UpdateMacroDelays {
            mouse_click_delay_ms: u32,
            keyboard_key_press_delay_ms: u32,
        },
        UpdateKeyboardArrowMouseSettings {
            enabled: bool,
            step_px: u32,
        },
        UpdateVisionPresets(Vec<VisionPreset>),
        UpdateAudioSensePresets(Vec<AudioSensePreset>),
        UpdateGeometryPresets(Vec<crate::model::GeometryPreset>),
        PreviewGeometrySpec(Option<GeometrySpec>),
        PreviewGeometryPreset(Option<u32>),
        RefreshSearchAreaOverlay,
        InvalidateVisionWaits(Vec<u32>),
        ApplyMouseSensitivityPreset(u32),
        RestoreMouseSensitivity,
        UpdateHudPresets(Vec<HudPreset>),
        UpdateCommandPresets(Vec<CommandPreset>),
        UpdateGroqSettings(crate::model::GroqSettings),
        PreviewHudPreset(Vec<HudPreset>),
        UpdateMacroPresets(Vec<MacroGroup>),
        UpdateAudioSettings(AudioSettings),
        PlayVideoPreset(u32),
        PlayVideoPresetFrom(u32, u64),
        StopVideoPlayback,
        SetMacrosMasterEnabled(bool),
        SetWindowsKeyLocked(bool),
        SetNativeFocusHighlightEnabled(bool),
        UpdateVisionSettings(VisionSettings),
        SetArduinoFlashInProgress(bool),
        SetVietnameseInputEnabled(bool),
        UpdateMacrosMasterHotkey(Option<HotkeyBinding>),
        RefreshPinOverlay,
        SetVisionCaptureMouseBlocked {
            blocked: bool,
            is_region_mode: bool,
        },
        BeginMousePathDrawCapture {
            preset_id: u32,
            preset_name: String,
        },
        CancelMousePathDrawCapture,
        SetUiVisible(bool),
        SetTrayIconVisible(bool),
        Exit,
        ToggleMacroRecording(u32, u32, String),
        UpdateTimerPresets(Vec<TimerPreset>),
        PreviewTimerPreset(Option<TimerPreset>),
        UpdateOcrPresets(Vec<crate::model::OcrPreset>),
        SetFocusHighlightConfig {
            color: crate::model::RgbaColor,
            rainbow: bool,
        },
        SetProtractorEnabled(bool),
        UpdateProtractorConfig {
            scale: f32,
            needle1_angle: f32,
            needle2_angle: f32,
            center_x: i32,
            center_y: i32,
            thickness: f32,
            calibrating: bool,
            ui_language: crate::model::UiLanguage,
        },
        UpdateQuickKeyDisplayConfig {
            enabled: bool,
            center_x: i32,
            center_y: i32,
            size: f32,
        },
        ShowQuickKeyDisplay(Option<String>),
        UpdateScreenDrawConfig {
            enabled: bool,
            trigger: Option<HotkeyBinding>,
            pass_trigger_through: bool,
            color: RgbaColor,
            brush_size: f32,
            smoothing: bool,
            smoothing_amount: f32,
        },
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ScreenDrawControl {
        None,
        MoveToolbar,
        BrushSize,
        SmoothingAmount,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ScreenDrawHit {
        Canvas,
        ToolbarBody,
        Close,
        Color,
        BrushSize,
        Eraser,
        Smoothing,
        SmoothingAmount,
        CaptureRegion,
    }

    impl Default for ScreenDrawControl {
        fn default() -> Self {
            Self::None
        }
    }

    #[derive(Clone)]
    struct ScreenDrawStroke {
        points: Vec<POINT>,
        color: RgbaColor,
        brush_size: f32,
        eraser: bool,
        smoothing: bool,
        smoothing_amount: f32,
    }

    #[derive(Clone, Copy)]
    struct ScreenDrawDirtyRect {
        left: usize,
        top: usize,
        right: usize,
        bottom: usize,
    }

    impl ScreenDrawDirtyRect {
        fn full(width: usize, height: usize) -> Self {
            Self {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            }
        }

        fn normalized(self, width: usize, height: usize) -> Option<Self> {
            let left = self.left.min(width);
            let top = self.top.min(height);
            let right = self.right.min(width);
            let bottom = self.bottom.min(height);
            if left >= right || top >= bottom {
                None
            } else {
                Some(Self {
                    left,
                    top,
                    right,
                    bottom,
                })
            }
        }

        fn union(self, other: Self) -> Self {
            Self {
                left: self.left.min(other.left),
                top: self.top.min(other.top),
                right: self.right.max(other.right),
                bottom: self.bottom.max(other.bottom),
            }
        }
    }

    struct ScreenDrawState {
        enabled: bool,
        active: bool,
        trigger: Option<HotkeyBinding>,
        pass_trigger_through: bool,
        color: RgbaColor,
        brush_size: f32,
        eraser: bool,
        smoothing: bool,
        smoothing_amount: f32,
        toolbar_x: i32,
        toolbar_y: i32,
        active_control: ScreenDrawControl,
        drag_offset_x: i32,
        drag_offset_y: i32,
        current_stroke: Option<ScreenDrawStroke>,
        strokes: Vec<ScreenDrawStroke>,
        canvas_width: usize,
        canvas_height: usize,
        committed_rgba: Vec<u8>,
        frame_rgba: Vec<u8>,
        committed_dirty: bool,
        pending_repaint: bool,
        capturing_region: bool,
        last_present_at: Option<Instant>,
        dirty_rect: Option<ScreenDrawDirtyRect>,
        live_stroke_rect: Option<ScreenDrawDirtyRect>,
        surface_dc: isize,
        surface_bitmap: isize,
        surface_old_bitmap: isize,
        surface_bits: usize,
        surface_bits_len: usize,
        surface_width: usize,
        surface_height: usize,
    }

    impl Default for ScreenDrawState {
        fn default() -> Self {
            Self {
                enabled: false,
                active: false,
                trigger: None,
                pass_trigger_through: false,
                color: RgbaColor {
                    r: 0,
                    g: 255,
                    b: 170,
                    a: 255,
                },
                brush_size: 10.0,
                eraser: false,
                smoothing: false,
                smoothing_amount: 0.45,
                toolbar_x: 24,
                toolbar_y: 24,
                active_control: ScreenDrawControl::None,
                drag_offset_x: 0,
                drag_offset_y: 0,
                current_stroke: None,
                strokes: Vec::new(),
                canvas_width: 0,
                canvas_height: 0,
                committed_rgba: Vec::new(),
                frame_rgba: Vec::new(),
                committed_dirty: true,
                pending_repaint: false,
                capturing_region: false,
                last_present_at: None,
                dirty_rect: None,
                live_stroke_rect: None,
                surface_dc: 0,
                surface_bitmap: 0,
                surface_old_bitmap: 0,
                surface_bits: 0,
                surface_bits_len: 0,
                surface_width: 0,
                surface_height: 0,
            }
        }
    }

    #[derive(Debug, Clone)]
    pub enum UiCommand {
        ShowWindow,
        Exit,
        StartupIconLoaded(std::sync::Arc<eframe::egui::IconData>),
        StartupStateLoaded {
            state: crate::model::AppState,
            startup_state_dirty: bool,
        },
        StartupStateLoadFailed(String),
        SyncMacroGroups(Vec<MacroGroup>, String),
        SyncCrosshairProfiles(Vec<ProfileRecord>, String),
        SetMacrosMasterEnabled(bool, String),
        SetVietnameseInputEnabled(bool, String),
        MousePathRecordingStarted(u32, String),
        MousePathRecordingFinished(u32, Vec<MousePathEvent>, String),
        MousePathDrawCaptureCancelled(String),
        ScreenDrawCaptureStatus(String),
        VisionFinished(String),
        MacroStepInlineFeedback {
            preset_id: u32,
            step_index: usize,
            message: String,
            open_groq_settings: bool,
        },
        VisionCaptureMouseDown {
            screen_x: i32,
            screen_y: i32,
        },
        VisionCaptureMouseMove {
            screen_x: i32,
            screen_y: i32,
        },
        VisionCaptureMouseUp {
            screen_x: i32,
            screen_y: i32,
        },
        VisionPointCaptured {
            preset_id: u32,
            priority_anchor: bool,
            screen_x: i32,
            screen_y: i32,
            color: Option<RgbaColor>,
        },
        VisionRegionPreview {
            screen_x: i32,
            screen_y: i32,
            width: i32,
            height: i32,
        },
        VisionRegionCaptured {
            preset_id: u32,
            template_mode: bool,
            screen_x: i32,
            screen_y: i32,
            width: i32,
            height: i32,
        },
        VisionPointCaptureCancelled(String),
        MouseMoveAbsolutePointCaptured {
            group_id: Option<u32>,
            preset_id: u32,
            step_index: usize,
            is_if_start: bool,
            extra_cond_index: Option<usize>,
            screen_x: i32,
            screen_y: i32,
            color: Option<RgbaColor>,
        },
        MouseMoveAbsoluteCaptureCancelled,
        UpdateCheckStarted,
        UpdateAvailable(String, String, String), // version, body, download_url

        MacroRecordingStarted(u32, String),
        MacroRecordingFinished(u32, u32, Vec<MacroRecordingEvent>, String),
        MacroRealtimeStepAdded(u32, u32, crate::model::MacroStep),
        MacroRealtimeStepRemoved(u32, u32),
        UpdateDownloadStarted,
        UpdateDownloadFinished(String), // new_exe_path

        UpdateError(String),
        UpdateUpToDate,
        SetInterceptionStatus(String),
        CustomCommandResult {
            preset_id: u32,
            output: String,
        },
        AudioWaveformLoaded {
            path: String,
            waveform: Vec<f32>,
            duration_ms: Option<u64>,
        },
        OpenWindowsLoaded {
            windows: Vec<String>,
            status: Option<String>,
        },
        AudioSenseDevicesLoaded {
            devices: Vec<String>,
        },
        VideoFrameLoaded {
            preset_id: u32,
            path: String,
            start_ms: u64,
            max_width: i32,
            max_height: i32,
            width: usize,
            height: usize,
            rgba: Vec<u8>,
        },
        WindowPreviewLoaded {
            cache_id: u32,
            source_window_key: Option<String>,
            source_window_extra_keys: Vec<String>,
            match_duplicate_window_titles: bool,
            frame: crate::window_list::WindowPreviewFrame,
        },
        VideoPlaybackFinished(u32),
        SetProtractorEnabled(bool),
        UpdateProtractorConfig {
            scale: f32,
            needle1_angle: f32,
            needle2_angle: f32,
            center_x: i32,
            center_y: i32,
            thickness: f32,
        },
        RequestProtractorCalibration {
            was_minimized: bool,
        },
        NativeVisionCaptureFinished {
            target: VisionCaptureTarget,
            mode: VisionCaptureMode,
            result: NativeCaptureResult,
            capture_frame: Option<crate::window_list::ScreenCaptureFrame>,
        },
        NativeProtractorCalibrationFinished {
            result: NativeCaptureResult,
            was_minimized: bool,
        },
        NativeMouseMoveAbsoluteCaptureFinished {
            target: MouseMoveAbsoluteCaptureTarget,
            result: NativeCaptureResult,
            capture_frame: Option<crate::window_list::ScreenCaptureFrame>,
        },
    }

    pub struct OverlayHandle {
        tx: Sender<OverlayCommand>,
    }

    impl OverlayHandle {
        pub fn send(&self, command: OverlayCommand) {
            let _ = self.tx.send(command);
            wake_command_queue();
        }
    }

    pub fn wake_command_queue() {
        unsafe {
            let hwnd = HWND(CONTROLLER_HWND.load(Ordering::Relaxed) as *mut c_void);
            if !hwnd.0.is_null() {
                let _ = PostMessageW(Some(hwnd), WMAPP_PROCESS_QUEUE, WPARAM(0), LPARAM(0));
            }
        }
    }


    /// Directly close the Arduino runtime transport and mark flash in progress.
    /// Called from the flash thread to guarantee the COM port is released
    /// before avrdude attempts to claim it, without relying on async channel timing.

    /// Re-enable background Arduino connection after flash is complete.




    pub fn set_ui_context(ctx: egui::Context) {
        *UI_CONTEXT.lock() = Some(ctx);
    }

    pub fn request_ui_repaint() {
        if let Some(ctx) = UI_CONTEXT.lock().as_ref() {
            ctx.request_repaint();
        }
    }

    #[derive(Debug, Clone)]
    struct ActiveTimerState {
        running: bool,
        start_time: Option<Instant>,
        elapsed_ms: u64,
        on_complete_macro_preset_id: Option<u32>,
    }

    impl ActiveTimerState {
        fn get_elapsed_ms(&self) -> u64 {
            if self.running {
                if let Some(start) = self.start_time {
                    self.elapsed_ms + start.elapsed().as_millis() as u64
                } else {
                    self.elapsed_ms
                }
            } else {
                self.elapsed_ms
            }
        }
    }

    pub(crate) struct HookState {
        pub(crate) ui_tx: Option<Sender<UiCommand>>,
        window_presets: Vec<WindowPreset>,
        window_focus_presets: Vec<WindowFocusPreset>,
        window_layouts: Vec<crate::model::WindowLayout>,
        window_expand_controls: WindowExpandControls,
        pin_presets: Vec<PinPreset>,
        mouse_path_presets: Vec<MousePathPreset>,
        mouse_sensitivity_presets: Vec<MouseSensitivityPreset>,
        active_mouse_sensitivity_preset_id: Option<u32>,
        mouse_sensitivity_restore_speed: Option<u32>,
        keyboard_arrow_mouse_enabled: bool,
        keyboard_arrow_mouse_step_px: u32,
        vision_presets: Vec<VisionPreset>,
        audio_sense_presets: Vec<AudioSensePreset>,
        active_audio_sense_keys: HashSet<String>,
        active_audio_sense_snapshots: std::collections::HashMap<String, crate::audiosense::PitchSnapshot>,
        geometry_presets: Vec<crate::model::GeometryPreset>,
        active_geometry_preset_ids: HashSet<u32>,
        active_geometry_preset_owner_ids: HashMap<(u32, usize), u32>,
        active_geometry_preset_owner_expires: HashMap<(u32, usize), Instant>,
        active_geometry_preset_instances: HashMap<(u32, usize), ActiveGeometryPresetInstance>,
        active_geometry_preset_activation_order: Vec<(u32, usize)>,
        active_geometry_steps: HashMap<(u32, usize), crate::model::GeometrySpec>,
        rendered_geometry_steps: HashMap<(u32, usize), GeometryRenderShape>,
        active_geometry_steps_expires: HashMap<(u32, usize), Instant>,
        last_geometry_overlay_refresh_at: Option<Instant>,
        active_crosshair_expires: Option<Instant>,
        active_pin_expires: Option<Instant>,
        preview_geometry_spec: Option<GeometrySpec>,
        preview_geometry_preset_id: Option<u32>,
        vision_following_presets: HashSet<u32>,
        vision_dir: PathBuf,
        opencv_dll_path: PathBuf,
        interception_dll_path: PathBuf,
        use_interception: bool,
        use_arduino_mouse: bool,
        arduino_transport: ArduinoTransport,
        arduino_com_port: String,
        arduino_vid: String,
        arduino_pid: String,
        arduino_flash_in_progress: bool,
        interception_runtime_status: InterceptionRuntimeStatus,
        mouse_sensitivity_restore_on_exit: bool,
        mouse_sensitivity_exit_restore_speed: u32,
        macro_mouse_click_delay_ms: u32,
        macro_keyboard_key_press_delay_ms: u32,
        active_pin_preset_id: Option<u32>,
        vision_capture_mouse_blocked: bool,
        vision_capture_is_region_mode: bool,
        vision_capture_anchor: Option<(i32, i32)>,
        pub(crate) vision_capture_preview_regions: Vec<VisionRegion>,
        pub(crate) vision_preview_source: Option<(u32, usize)>,
        mouse_path_draw_capture: Option<MousePathDrawCaptureSession>,
        hud_presets: Vec<HudPreset>,
        ocr_presets: Vec<crate::model::OcrPreset>,
        command_presets: Vec<CommandPreset>,
        groq_settings: crate::model::GroqSettings,
        macro_groups: Vec<MacroGroup>,
        macros_master_enabled: bool,
        windows_key_locked: bool,
        macros_master_hotkey: Option<HotkeyBinding>,
        vietnamese_input_enabled: bool,
        locked_inputs: HashMap<String, usize>,
        mouse_move_locks: MouseMoveLockCounts,
        mouse_move_lock_anchor: Option<POINT>,
        current_style: CrosshairStyle,
        profiles: Vec<ProfileRecord>,
        sound_presets: Vec<SoundPreset>,
        video_presets: Vec<VideoPreset>,
        sound_library: Vec<SoundLibraryItem>,
        active_hold_macros: HashMap<u32, ActiveHoldMacro>,
        timer_presets: Vec<TimerPreset>,
        active_timers: HashMap<u32, ActiveTimerState>,
        next_hold_run_token: u64,
        pending_tray_toggle: Option<bool>,
        tray_double_click_suppress_next_up: bool,
        active_crosshair_profile_name: Option<String>,
        stop_ignore_keys: HashMap<u32, String>,
        press_trigger_suppression: HashMap<String, usize>,
        pending_press_trigger_keys: HashSet<String>,
        pending_window_focus_trigger: Option<isize>,
        pending_window_focus_stable_polls: u8,
        last_dispatched_window_focus_hwnd: Option<isize>,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
        held_inputs: HashSet<String>,
        pressed_inputs: HashSet<String>,
        held_mouse_buttons: HashSet<String>,
        last_scroll_up_at: Option<std::time::Instant>,
        last_scroll_down_at: Option<std::time::Instant>,
    }

    impl Default for HookState {
        fn default() -> Self {
            Self {
                ui_tx: None,
                window_presets: Vec::new(),
                window_focus_presets: Vec::new(),
                window_layouts: Vec::new(),
                window_expand_controls: WindowExpandControls::default(),
                pin_presets: Vec::new(),
                mouse_path_presets: Vec::new(),
                mouse_sensitivity_presets: Vec::new(),
                active_mouse_sensitivity_preset_id: None,
                mouse_sensitivity_restore_speed: None,
                keyboard_arrow_mouse_enabled: false,
                keyboard_arrow_mouse_step_px: 12,
                vision_presets: Vec::new(),
                audio_sense_presets: Vec::new(),
                active_audio_sense_keys: HashSet::new(),
                active_audio_sense_snapshots: std::collections::HashMap::new(),
                geometry_presets: Vec::new(),
                active_geometry_preset_ids: HashSet::new(),
                active_geometry_preset_owner_ids: HashMap::new(),
                active_geometry_preset_owner_expires: HashMap::new(),
                active_geometry_preset_instances: HashMap::new(),
                active_geometry_preset_activation_order: Vec::new(),
                active_geometry_steps: HashMap::new(),
                rendered_geometry_steps: HashMap::new(),
                active_geometry_steps_expires: HashMap::new(),
                last_geometry_overlay_refresh_at: None,
                active_crosshair_expires: None,
                active_pin_expires: None,
                preview_geometry_spec: None,
                preview_geometry_preset_id: None,
                vision_following_presets: HashSet::new(),
                vision_dir: PathBuf::new(),
                opencv_dll_path: PathBuf::new(),
                interception_dll_path: PathBuf::new(),
                use_interception: false,
                use_arduino_mouse: false,
                arduino_transport: ArduinoTransport::Serial,
                arduino_com_port: String::new(),
                arduino_vid: "0x2341".to_owned(),
                arduino_pid: "0x8036".to_owned(),
                arduino_flash_in_progress: false,
                interception_runtime_status: InterceptionRuntimeStatus::Unavailable,
                mouse_sensitivity_restore_on_exit: false,
                mouse_sensitivity_exit_restore_speed: 6,
                macro_mouse_click_delay_ms: 16,
                macro_keyboard_key_press_delay_ms: 0,
                active_pin_preset_id: None,
                vision_capture_mouse_blocked: false,
                vision_capture_is_region_mode: false,
                vision_capture_anchor: None,
                vision_capture_preview_regions: Vec::new(),
                vision_preview_source: None,
                mouse_path_draw_capture: None,
                hud_presets: Vec::new(),
                ocr_presets: Vec::new(),
                command_presets: Vec::new(),
                groq_settings: crate::model::GroqSettings::default(),
                macro_groups: Vec::new(),
                macros_master_enabled: true,
                windows_key_locked: false,
                macros_master_hotkey: None,
                vietnamese_input_enabled: false,
                locked_inputs: HashMap::new(),
                mouse_move_locks: MouseMoveLockCounts::default(),
                mouse_move_lock_anchor: None,
                current_style: CrosshairStyle::default(),
                profiles: Vec::new(),
                sound_presets: Vec::new(),
                video_presets: Vec::new(),
                sound_library: Vec::new(),
                active_hold_macros: HashMap::new(),
                timer_presets: Vec::new(),
                active_timers: HashMap::new(),
                next_hold_run_token: 1,
                pending_tray_toggle: None,
                tray_double_click_suppress_next_up: false,
                active_crosshair_profile_name: None,
                stop_ignore_keys: HashMap::new(),
                press_trigger_suppression: HashMap::new(),
                pending_press_trigger_keys: HashSet::new(),
                pending_window_focus_trigger: None,
                pending_window_focus_stable_polls: 0,
                last_dispatched_window_focus_hwnd: None,
                ctrl: false,
                alt: false,
                shift: false,
                win: false,
                held_inputs: HashSet::new(),
                pressed_inputs: HashSet::new(),
                held_mouse_buttons: HashSet::new(),
                last_scroll_up_at: None,
                last_scroll_down_at: None,
            }
        }
    }

    fn set_interception_runtime_status(status: InterceptionRuntimeStatus) {
        let mut hook_state = HOOK_STATE.lock();
        if hook_state.interception_runtime_status == status {
            return;
        }

        hook_state.interception_runtime_status = status;
        if let Some(tx) = hook_state.ui_tx.clone() {
            let _ = tx.send(UiCommand::SetInterceptionStatus(status.label().to_owned()));
        }
    }

    struct Runtime {
        rx: Receiver<OverlayCommand>,
        ui_tx: Sender<UiCommand>,
        paths: AppPaths,
        style: CrosshairStyle,
        window_presets: Vec<WindowPreset>,
        window_focus_presets: Vec<WindowFocusPreset>,
        window_layouts: Vec<crate::model::WindowLayout>,
        pin_presets: Vec<PinPreset>,
        mouse_path_presets: Vec<MousePathPreset>,
        macro_groups: Vec<MacroGroup>,
        audio_settings: AudioSettings,
        registered_window_hotkeys: HashMap<i32, WindowHotkeyAction>,
        registered_macro_hotkeys: HashMap<i32, MacroPreset>,
        overlay_hwnd: HWND,
        mouse_trail_hwnd: HWND,
        search_area_hwnd: HWND,
        dynamic_geometry_hwnd: HWND,
        focus_highlight_hwnd: HWND,
        hud_hwnd: HWND,
        key_display_hwnd: HWND,
        screen_draw_hwnd: HWND,
        pin_hwnd: HWND,
        last_pin_update: Instant,
        hud_display: Option<HudDisplayState>,
        quick_key_display_enabled: bool,
        quick_key_display_center_x: i32,
        quick_key_display_center_y: i32,
        quick_key_display_size: f32,
        quick_key_display_entries: Vec<String>,
        quick_key_display_hide_at: Option<Instant>,
        tray_menu: HMENU,
        keyboard_hook: HHOOK,
        mouse_hook: HHOOK,
        window_focus_event_hook: HWINEVENTHOOK,
        window_location_event_hook: HWINEVENTHOOK,
        running: Arc<AtomicBool>,
        active_pin_thumbnail: Option<ActivePinThumbnail>,
        timer_interval_ms: u32,
        timer_presets: Vec<TimerPreset>,
        preview_timer_preset: Option<TimerPreset>,
        timer_hwnds: HashMap<u32, HWND>,
        ui_visible: bool,
        ui_foreground: bool,
        native_focus_highlight_enabled: bool,
        focus_highlight_color: crate::model::RgbaColor,
        focus_highlight_rainbow: bool,
        focus_highlight_rainbow_hue: f32,
        protractor_hwnd: HWND,
        active_focus_highlight_hwnd: Option<HWND>,
        cached_search_overlay_regions: Vec<VisionRegion>,
        cached_search_overlay_preview_regions: Vec<VisionRegion>,
        cached_search_overlay_static_geometry: Vec<GeometryRenderShape>,
        search_area_overlay_visible: bool,
        dynamic_geometry_overlay_visible: bool,
    }

    struct MouseRecordingSession {
        preset_id: u32,
        last_event_at: Instant,
        events: Vec<MousePathEvent>,
        dirty: bool,
        movement_only: bool,
    }

    struct MousePathPreviewSession {
        events: Vec<MousePathEvent>,
        points: Vec<POINT>,
        playback_started_at: Option<Instant>,
        playback_from_ms: u64,
        playback_marker: Option<POINT>,
        dirty: bool,
    }

    #[derive(Debug, Clone)]
    struct MousePathDrawCaptureSession {
        preset_id: u32,
        preset_name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum MacroRunFlow {
        Continue,
        BreakLoop,
        StopExecution,
        JumpTo(usize),
    }

    #[derive(Clone)]
    struct ActiveHoldMacro {
        trigger: HotkeyBinding,
        release_steps: Vec<MacroStep>,
        hold_stop_step: Option<MacroStep>,
        image_search_preset_ids: Vec<u32>,
        locked_keys: Vec<String>,
        locked_mouse_masks: Vec<MouseMoveLockMask>,
        run_token: u64,
        completed: bool,
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct MouseMoveLockMask {
        left: bool,
        right: bool,
        up: bool,
        down: bool,
    }

    impl MouseMoveLockMask {
        fn any(self) -> bool {
            self.left || self.right || self.up || self.down
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    struct MouseMoveLockCounts {
        left: usize,
        right: usize,
        up: usize,
        down: usize,
    }

    impl MouseMoveLockCounts {
        fn add(&mut self, mask: MouseMoveLockMask) {
            if mask.left {
                self.left = self.left.saturating_add(1);
            }
            if mask.right {
                self.right = self.right.saturating_add(1);
            }
            if mask.up {
                self.up = self.up.saturating_add(1);
            }
            if mask.down {
                self.down = self.down.saturating_add(1);
            }
        }

        fn remove(&mut self, mask: MouseMoveLockMask) {
            if mask.left && self.left > 0 {
                self.left -= 1;
            }
            if mask.right && self.right > 0 {
                self.right -= 1;
            }
            if mask.up && self.up > 0 {
                self.up -= 1;
            }
            if mask.down && self.down > 0 {
                self.down -= 1;
            }
        }

        fn any(self) -> bool {
            self.left > 0 || self.right > 0 || self.up > 0 || self.down > 0
        }
    }

    #[derive(Clone, PartialEq)]
    struct HudDisplayState {
        owner_preset_id: Option<u32>,
        preset_id: Option<u32>,
        text: String,
        text_color: RgbaColor,
        background_color: RgbaColor,
        background_opacity: f32,
        rounded_background: bool,
        font_size: f32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        auto_hide_on_owner_completion: bool,
        expires_at: Option<Instant>,
    }

    struct ActivePinThumbnail {
        preset_id: u32,
        source_hwnd: HWND,
        thumbnail_id: Option<isize>,
        overlay_style: PinOverlayStyle,
        last_target_bounds: (i32, i32, i32, i32),
        last_source_crop: Option<(i32, i32, i32, i32)>,
    }

    #[allow(dead_code)]
    enum WindowHotkeyAction {
        Apply(WindowPreset),
        Focus(WindowFocusPreset),
        Animate(WindowPreset),
        RestoreTitleBar(WindowPreset),
        ApplyLayout(crate::model::WindowLayout),
    }

    pub fn start(
        paths: AppPaths,
        initial_style: CrosshairStyle,
        ui_tx: Sender<UiCommand>,
    ) -> Result<OverlayHandle> {
        let (tx, rx) = crossbeam_channel::unbounded();
        *OVERLAY_COMMAND_TX.lock() = Some(tx.clone());
        SEARCH_AREA_OVERLAY_REFRESH_PENDING.store(false, Ordering::Release);
        let running = Arc::new(AtomicBool::new(true));
        let worker_running = running.clone();
        let poll_running = running.clone();

        // Background thread to manage Arduino serial connection
        let conn_manager_running = running.clone();
        thread::spawn(move || {
            let mut last_attempt = Instant::now() - Duration::from_secs(5);
            while conn_manager_running.load(Ordering::Relaxed) {
                let (use_arduino, transport, com_port, vid, pid, flash_in_progress) = {
                    let state = HOOK_STATE.lock();
                    (
                        state.use_arduino_mouse,
                        state.arduino_transport,
                        state.arduino_com_port.clone(),
                        state.arduino_vid.clone(),
                        state.arduino_pid.clone(),
                        state.arduino_flash_in_progress,
                    )
                };

                if use_arduino && !flash_in_progress {
                    match transport {
                        ArduinoTransport::Serial if !com_port.is_empty() => {
                            let mut hid_guard = ARDUINO_HID_DEVICE.lock();
                            let mut hid_name_guard = CURRENT_ARDUINO_HID_NAME.lock();
                            *hid_guard = None;
                            *hid_name_guard = String::new();
                            drop(hid_guard);
                            drop(hid_name_guard);

                            let mut name_guard = CURRENT_ARDUINO_PORT_NAME.lock();
                            let mut port_guard = ARDUINO_PORT.lock();

                            if HOOK_STATE.lock().arduino_flash_in_progress {
                                *port_guard = None;
                                *name_guard = String::new();
                                thread::sleep(Duration::from_millis(500));
                                continue;
                            }

                            if *name_guard != com_port || port_guard.is_none() {
                                *port_guard = None;
                                *name_guard = String::new();

                                if last_attempt.elapsed() >= Duration::from_secs(3) {
                                    last_attempt = Instant::now();
                                    match serialport::new(&com_port, 115200)
                                        .timeout(Duration::from_millis(10))
                                        .open()
                                    {
                                        Ok(p) => {
                                            *port_guard = Some(p);
                                            *name_guard = com_port.clone();
                                        }
                                        Err(_) => {}
                                    }
                                }
                            }
                        }
                        ArduinoTransport::Hid => {
                            let mut port_guard = ARDUINO_PORT.lock();
                            let mut port_name_guard = CURRENT_ARDUINO_PORT_NAME.lock();
                            *port_guard = None;
                            *port_name_guard = String::new();
                            drop(port_guard);
                            drop(port_name_guard);

                            let target_vid = parse_hex_u16_runtime(&vid, 0x2341);
                            let target_pid = parse_hex_u16_runtime(&pid, 0x8036);
                            let mut hid_guard = ARDUINO_HID_DEVICE.lock();
                            let mut hid_name_guard = CURRENT_ARDUINO_HID_NAME.lock();

                            if hid_guard.is_none() && last_attempt.elapsed() >= Duration::from_secs(3) {
                                last_attempt = Instant::now();
                                if let Ok(runtime) =
                                    open_arduino_hid_device(target_vid, target_pid)
                                {
                                    *hid_name_guard = runtime.path.clone();
                                    *hid_guard = Some(runtime);
                                }
                            }
                        }
                        ArduinoTransport::Serial => {
                            close_arduino_runtime_handles();
                        }
                    }
                } else {
                    close_arduino_runtime_handles();
                }

                thread::sleep(Duration::from_millis(500));
            }
        });
        thread::spawn(move || {
            while poll_running.load(Ordering::Relaxed) {
                unsafe {
                    let foreground = HWND(
                        FOREGROUND_WINDOW_HWND.load(Ordering::Relaxed) as *mut std::ffi::c_void,
                    );
                    let mut ui_in_foreground = false;
                    let mut ui_visible = false;
                    let mut ui_rect = windows::Win32::Foundation::RECT::default();
                    if let Some(ui_hwnd) = find_app_ui_window() {
                        ui_visible =
                            windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(ui_hwnd)
                                .as_bool();
                        if ui_visible {
                            let _ = GetWindowRect(ui_hwnd, &mut ui_rect);
                        }

                        if !foreground.0.is_null() {
                            let root = GetAncestor(foreground, GA_ROOT);
                            if !root.0.is_null() && root == ui_hwnd {
                                ui_in_foreground = true;
                            }
                        }
                    }

                    UI_WINDOW_FOREGROUND.store(ui_in_foreground, Ordering::Relaxed);
                    UI_WINDOW_VISIBLE.store(ui_visible, Ordering::Relaxed);
                    if ui_visible {
                        UI_WINDOW_RECT_LEFT.store(ui_rect.left, Ordering::Relaxed);
                        UI_WINDOW_RECT_TOP.store(ui_rect.top, Ordering::Relaxed);
                        UI_WINDOW_RECT_RIGHT.store(ui_rect.right, Ordering::Relaxed);
                        UI_WINDOW_RECT_BOTTOM.store(ui_rect.bottom, Ordering::Relaxed);
                    }
                }

                thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        thread::spawn(move || {
            let result = run_thread(paths, initial_style, rx, ui_tx, worker_running.clone());
            if let Err(error) = result {
                eprintln!("overlay error: {error:#}");
            }

            worker_running.store(false, Ordering::Relaxed);
        });
        Ok(OverlayHandle { tx })
    }

    fn run_thread(
        paths: AppPaths,
        initial_style: CrosshairStyle,
        rx: Receiver<OverlayCommand>,
        ui_tx: Sender<UiCommand>,
        running: Arc<AtomicBool>,
    ) -> Result<()> {
        {
            let mut hook_state = HOOK_STATE.lock();
            hook_state.vision_dir = paths.vision_dir.clone();
            hook_state.opencv_dll_path = paths.opencv_dll.clone();
            hook_state.interception_dll_path = paths.interception_dll.clone();
        }

        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            register_class(
                instance,
                w!("CrosshairController"),
                Some(controller_wnd_proc),
            )?;
            register_class(instance, w!("CrosshairOverlay"), Some(overlay_wnd_proc))?;
            register_class(instance, w!("CrosshairToolbox"), Some(hud_wnd_proc))?;
            register_class(instance, w!("MacroNestScreenDraw"), Some(screen_draw_wnd_proc))?;
            let overlay_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE,
                w!("CrosshairOverlay"),
                w!("CrosshairOverlay"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                Some(instance),
                None,
            )?;
            let mouse_trail_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE,
                w!("CrosshairOverlay"),
                w!("CrosshairMouseTrail"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                Some(instance),
                None,
            )?;
            let search_area_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE,
                w!("CrosshairOverlay"),
                w!("CrosshairSearchArea"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                Some(instance),
                None,
            )?;
            let dynamic_geometry_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE,
                w!("CrosshairOverlay"),
                w!("CrosshairDynamicGeometry"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                Some(instance),
                None,
            )?;
            let focus_highlight_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE,
                w!("CrosshairOverlay"),
                w!("CrosshairFocusHighlight"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                Some(instance),
                None,
            )?;
            let hud_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                w!("CrosshairToolbox"),
                w!("CrosshairToolbox"),
                WS_POPUP,
                0,
                0,
                360,
                44,
                None,
                None,
                Some(instance),
                None,
            )?;
            let key_display_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                w!("CrosshairToolbox"),
                w!("CrosshairKeyDisplay"),
                WS_POPUP,
                0,
                0,
                160,
                64,
                None,
                None,
                Some(instance),
                None,
            )?;
            let screen_draw_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                w!("MacroNestScreenDraw"),
                w!("MacroNestScreenDraw"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                Some(instance),
                None,
            )?;
            SCREEN_DRAW_HWND.store(screen_draw_hwnd.0 as isize, Ordering::Relaxed);
            let pin_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                w!("CrosshairOverlay"),
                w!("CrosshairPinHost"),
                WS_POPUP,
                0,
                0,
                320,
                180,
                None,
                None,
                Some(instance),
                None,
            )?;
            let protractor_hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE,
                w!("CrosshairOverlay"),
                w!("CrosshairProtractor"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                Some(instance),
                None,
            )?;
            PROTRACTOR_HWND.store(protractor_hwnd.0 as isize, Ordering::Relaxed);
            let tray_menu = CreatePopupMenu()?;
            let _ = AppendMenuW(tray_menu, MF_STRING, MENU_SHOW, w!("Open settings"));
            let _ = AppendMenuW(tray_menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(tray_menu, MF_STRING, MENU_EXIT, w!("Exit"));
            {
                let mut hook_state = HOOK_STATE.lock();
                hook_state.ui_tx = Some(ui_tx.clone());
            }

            let runtime = Box::new(Runtime {
                rx,
                ui_tx,
                paths,
                style: initial_style,
                window_presets: Vec::new(),
                window_focus_presets: Vec::new(),
                window_layouts: Vec::new(),
                pin_presets: Vec::new(),
                mouse_path_presets: Vec::new(),
                macro_groups: Vec::new(),
                audio_settings: AudioSettings::default(),
                registered_window_hotkeys: HashMap::new(),
                registered_macro_hotkeys: HashMap::new(),
                overlay_hwnd,
                mouse_trail_hwnd,
                search_area_hwnd,
                dynamic_geometry_hwnd,
                focus_highlight_hwnd,
                hud_hwnd,
                key_display_hwnd,
                screen_draw_hwnd,
                pin_hwnd,
                last_pin_update: Instant::now() - Duration::from_secs(1),
                hud_display: None,
                quick_key_display_enabled: false,
                quick_key_display_center_x: GetSystemMetrics(SM_CXSCREEN).max(1) / 2,
                quick_key_display_center_y: GetSystemMetrics(SM_CYSCREEN).max(1) / 2,
                quick_key_display_size: 36.0,
                quick_key_display_entries: Vec::new(),
                quick_key_display_hide_at: None,
                tray_menu,
                keyboard_hook: HHOOK::default(),
                mouse_hook: HHOOK::default(),
                window_focus_event_hook: HWINEVENTHOOK::default(),
                window_location_event_hook: HWINEVENTHOOK::default(),
                running,
                active_pin_thumbnail: None,
                timer_interval_ms: 500,
                timer_presets: Vec::new(),
                preview_timer_preset: None,
                timer_hwnds: HashMap::new(),
                ui_visible: true,
                ui_foreground: true,
                native_focus_highlight_enabled: false,
                focus_highlight_color: crate::model::RgbaColor { r: 126, g: 224, b: 182, a: 235 },
                focus_highlight_rainbow: false,
                focus_highlight_rainbow_hue: 0.0,
                protractor_hwnd,
                active_focus_highlight_hwnd: None,
                cached_search_overlay_regions: Vec::new(),
                cached_search_overlay_preview_regions: Vec::new(),
                cached_search_overlay_static_geometry: Vec::new(),
                search_area_overlay_visible: false,
                dynamic_geometry_overlay_visible: false,
            });
            let _controller_hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("CrosshairController"),
                w!("CrosshairController"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                Some(Box::into_raw(runtime) as *const c_void),
            )?;
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).into() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        Ok(())
    }

    unsafe fn register_class(
        instance: HINSTANCE,
        name: PCWSTR,
        proc: Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>,
    ) -> Result<()> {
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        let class = WNDCLASSW {
            lpfnWndProc: proc,
            hInstance: instance,
            lpszClassName: name,
            hCursor: cursor,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            bail!("Failed to register the window class");
        }

        Ok(())
    }

    unsafe extern "system" fn overlay_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let protractor_hwnd = PROTRACTOR_HWND.load(Ordering::Relaxed);
        if protractor_hwnd != 0 && hwnd.0 as isize == protractor_hwnd {
            match msg {
                WM_NCHITTEST => {
                    let sx = (lparam.0 & 0xFFFF) as i16 as i32;
                    let sy = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                    let mut pt = POINT { x: sx, y: sy };
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);

                    let (scale, needle1, needle2) = {
                        let state = PROTRACTOR_STATE.lock();
                        (state.scale, state.needle1_angle, state.needle2_angle)
                    };

                    let base_radius = 150.0;
                    let radius = (scale * base_radius) as i32;
                    let padding = (scale * 30.0) as i32;
                    let half_size = radius + padding;
                    
                    let cx = half_size;
                    let cy = half_size;

                    let dx = pt.x - cx;
                    let dy = pt.y - cy;
                    let dist_sq = dx * dx + dy * dy;
                    let dist = (dist_sq as f32).sqrt();

                    let size = 2 * half_size;

                    // Thickness slider hit test
                    let slider_left = cx - 30;
                    let slider_right = cx + 30;
                    let slider_top = size - 18;
                    let slider_bottom = size - 6;
                    if pt.x >= slider_left - 4 && pt.x <= slider_right + 4 && pt.y >= slider_top - 4 && pt.y <= slider_bottom + 4 {
                        return LRESULT(1isize); // HTCLIENT
                    }

                    if pt.x >= size - 24 && pt.x < size - 8 && pt.y >= 8 && pt.y < 24 {
                        return LRESULT(1isize); // HTCLIENT
                    }

                    if pt.x >= 8 && pt.x <= 88 && pt.y >= 8 && pt.y <= 28 {
                        return LRESULT(1isize); // HTCLIENT
                    }

                    let rad1 = (needle1 as f32).to_radians();
                    let n1x = cx + (radius as f32 * rad1.cos()) as i32;
                    let n1y = cy + (radius as f32 * rad1.sin()) as i32;
                    if (pt.x - n1x).pow(2) + (pt.y - n1y).pow(2) <= 12 * 12 {
                        return LRESULT(1isize); // HTCLIENT
                    }

                    let rad2 = (needle2 as f32).to_radians();
                    let n2x = cx + (radius as f32 * rad2.cos()) as i32;
                    let n2y = cy + (radius as f32 * rad2.sin()) as i32;
                    if (pt.x - n2x).pow(2) + (pt.y - n2y).pow(2) <= 12 * 12 {
                        return LRESULT(1isize); // HTCLIENT
                    }

                    let rad_g = (-45.0_f32).to_radians();
                    let gx = cx + (radius as f32 * rad_g.cos()) as i32;
                    let gy = cy + (radius as f32 * rad_g.sin()) as i32;
                    if (pt.x - gx).pow(2) + (pt.y - gy).pow(2) <= 14 * 14 {
                        return LRESULT(1isize); // HTCLIENT
                    }

                    if dist <= radius as f32 + 12.0 * scale {
                        return LRESULT(1isize); // HTCLIENT
                    }

                    return LRESULT(HTTRANSPARENT as isize);
                }

                WM_LBUTTONDOWN => {
                    let mx = (lparam.0 & 0xFFFF) as i16 as i32;
                    let my = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                    
                    let (scale, needle1, needle2, cx_val, cy_val) = {
                        let state = PROTRACTOR_STATE.lock();
                        (state.scale, state.needle1_angle, state.needle2_angle, state.center_x, state.center_y)
                    };

                    let base_radius = 150.0;
                    let radius = (scale * base_radius) as i32;
                    let padding = (scale * 30.0) as i32;
                    let half_size = radius + padding;
                    let size = 2 * half_size;
                    let cx = half_size;
                    let cy = half_size;

                    let dx = mx - cx;
                    let dy = my - cy;
                    let dist_sq = dx * dx + dy * dy;
                    let dist = (dist_sq as f32).sqrt();

                    let mut hit = None;

                    // Thickness slider hit test
                    let slider_left = cx - 30;
                    let slider_right = cx + 30;
                    let slider_top = size - 18;
                    let slider_bottom = size - 6;
                    if mx >= slider_left - 4 && mx <= slider_right + 4 && my >= slider_top - 4 && my <= slider_bottom + 4 {
                        hit = Some(ProtractorDragTarget::ThicknessSlider);
                    }

                    if hit.is_none() && mx >= size - 24 && mx < size - 8 && my >= 8 && my < 24 {
                        hit = Some(ProtractorDragTarget::Close);
                    }
                    if hit.is_none() && mx >= 8 && mx <= 88 && my >= 8 && my <= 28 {
                        hit = Some(ProtractorDragTarget::CalibrationButton);
                    }
                    if hit.is_none() {
                        let rad1 = (needle1 as f32).to_radians();
                        let n1x = cx + (radius as f32 * rad1.cos()) as i32;
                        let n1y = cy + (radius as f32 * rad1.sin()) as i32;
                        if (mx - n1x).pow(2) + (my - n1y).pow(2) <= 12 * 12 {
                            hit = Some(ProtractorDragTarget::Needle1);
                        }
                    }
                    if hit.is_none() {
                        let rad2 = (needle2 as f32).to_radians();
                        let n2x = cx + (radius as f32 * rad2.cos()) as i32;
                        let n2y = cy + (radius as f32 * rad2.sin()) as i32;
                        if (mx - n2x).pow(2) + (my - n2y).pow(2) <= 12 * 12 {
                            hit = Some(ProtractorDragTarget::Needle2);
                        }
                    }
                    if hit.is_none() {
                        let rad_g = (-45.0_f32).to_radians();
                        let gx = cx + (radius as f32 * rad_g.cos()) as i32;
                        let gy = cy + (radius as f32 * rad_g.sin()) as i32;
                        if (mx - gx).pow(2) + (my - gy).pow(2) <= 14 * 14 {
                            hit = Some(ProtractorDragTarget::ResizeGrip);
                        }
                    }
                    if hit.is_none() {
                        if dist <= radius as f32 + 12.0 * scale {
                            hit = Some(ProtractorDragTarget::Body);
                        }
                    }

                    if let Some(target) = hit {
                        if target == ProtractorDragTarget::Close {
                            PROTRACTOR_STATE.lock().enabled = false;
                            let _ = ShowWindow(hwnd, SW_HIDE);
                            if let Some(ui_tx) = &HOOK_STATE.lock().ui_tx {
                                let _ = ui_tx.send(UiCommand::SetProtractorEnabled(false));
                            }
                        } else if target == ProtractorDragTarget::CalibrationButton {
                            let mut was_minimized = false;
                            unsafe {
                                if let Some(app_hwnd) = find_app_ui_window() {
                                    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_RESTORE, SetForegroundWindow, IsIconic};
                                    was_minimized = IsIconic(app_hwnd).as_bool();
                                    if was_minimized {
                                        let _ = ShowWindow(app_hwnd, SW_RESTORE);
                                    }
                                    let _ = SetForegroundWindow(app_hwnd);
                                }
                            }
                            if let Some(ui_tx) = &HOOK_STATE.lock().ui_tx {
                                let _ = ui_tx.send(UiCommand::RequestProtractorCalibration { was_minimized });
                            }
                        } else {
                            let mut mouse_screen = POINT::default();
                            let _ = GetCursorPos(&mut mouse_screen);

                            *PROTRACTOR_DRAG_TARGET.lock() = Some(target);
                            *PROTRACTOR_DRAG_START_MOUSE.lock() = mouse_screen;
                            *PROTRACTOR_DRAG_START_CENTER.lock() = (cx_val, cy_val);
                            
                            let start_ang = match target {
                                ProtractorDragTarget::Needle1 => needle1,
                                ProtractorDragTarget::Needle2 => needle2,
                                _ => 0.0,
                            };
                            *PROTRACTOR_DRAG_START_ANGLE.lock() = start_ang;
                            *PROTRACTOR_DRAG_START_SCALE.lock() = scale;

                            windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                        }
                    }
                    return LRESULT(0);
                }

                WM_MOUSEMOVE => {
                    let drag_target = *PROTRACTOR_DRAG_TARGET.lock();
                    if let Some(target) = drag_target {
                        let mut mouse_screen = POINT::default();
                        let _ = GetCursorPos(&mut mouse_screen);

                        let start_mouse = *PROTRACTOR_DRAG_START_MOUSE.lock();
                        let start_center = *PROTRACTOR_DRAG_START_CENTER.lock();

                        match target {
                            ProtractorDragTarget::Body => {
                                let dx = mouse_screen.x - start_mouse.x;
                                let dy = mouse_screen.y - start_mouse.y;
                                let new_cx = start_center.0 + dx;
                                let new_cy = start_center.1 + dy;
                                
                                {
                                    let mut state = PROTRACTOR_STATE.lock();
                                    state.center_x = new_cx;
                                    state.center_y = new_cy;
                                }

                                if let Some(runtime) = runtime_mut(HWND(CONTROLLER_HWND.load(Ordering::Relaxed) as *mut c_void)) {
                                    let _ = paint_protractor_overlay(runtime);
                                }
                            }
                            ProtractorDragTarget::Needle1 | ProtractorDragTarget::Needle2 => {
                                let cx = start_center.0;
                                let cy = start_center.1;
                                let dx = mouse_screen.x - cx;
                                let dy = mouse_screen.y - cy;
                                let mut angle = (dy as f32).atan2(dx as f32).to_degrees();
                                if angle < 0.0 { angle += 360.0; }

                                {
                                    let mut state = PROTRACTOR_STATE.lock();
                                    if target == ProtractorDragTarget::Needle1 {
                                        state.needle1_angle = angle;
                                    } else {
                                        state.needle2_angle = angle;
                                    }
                                }

                                if let Some(runtime) = runtime_mut(HWND(CONTROLLER_HWND.load(Ordering::Relaxed) as *mut c_void)) {
                                    let _ = paint_protractor_overlay(runtime);
                                }
                            }
                            ProtractorDragTarget::ResizeGrip => {
                                let cx = start_center.0;
                                let cy = start_center.1;
                                let dx = mouse_screen.x - cx;
                                let dy = mouse_screen.y - cy;
                                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                                let new_scale = (dist / 150.0).clamp(0.4, 2.5);

                                {
                                    let mut state = PROTRACTOR_STATE.lock();
                                    state.scale = new_scale;
                                }

                                if let Some(runtime) = runtime_mut(HWND(CONTROLLER_HWND.load(Ordering::Relaxed) as *mut c_void)) {
                                    let _ = paint_protractor_overlay(runtime);
                                }
                            }
                            ProtractorDragTarget::ThicknessSlider => {
                                let mut pt = mouse_screen;
                                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);

                                let scale = {
                                    let state = PROTRACTOR_STATE.lock();
                                    state.scale
                                };
                                let radius = (scale * 150.0) as i32;
                                let padding = (scale * 30.0) as i32;
                                let half_size = radius + padding;
                                let cx = half_size;
                                let slider_left = cx - 30;

                                let t_frac = ((pt.x - slider_left) as f32 / 60.0).clamp(0.0, 1.0);
                                let new_thick = 1.0 + t_frac * 7.0; // 1.0 to 8.0

                                {
                                    let mut state = PROTRACTOR_STATE.lock();
                                    state.thickness = new_thick;
                                }

                                if let Some(runtime) = runtime_mut(HWND(CONTROLLER_HWND.load(Ordering::Relaxed) as *mut c_void)) {
                                    let _ = paint_protractor_overlay(runtime);
                                }
                            }
                            ProtractorDragTarget::Close => {} // no drag behavior for close button
                            ProtractorDragTarget::CalibrationButton => {}
                        }
                    }
                    return LRESULT(0);
                }

                WM_LBUTTONUP => {
                    let was_dragging = PROTRACTOR_DRAG_TARGET.lock().is_some();
                    if was_dragging {
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                        *PROTRACTOR_DRAG_TARGET.lock() = None;

                        let (scale, needle1, needle2, cx, cy, thickness) = {
                            let state = PROTRACTOR_STATE.lock();
                            (state.scale, state.needle1_angle, state.needle2_angle, state.center_x, state.center_y, state.thickness)
                        };

                        if let Some(ui_tx) = &HOOK_STATE.lock().ui_tx {
                            let _ = ui_tx.send(UiCommand::UpdateProtractorConfig {
                                scale,
                                needle1_angle: needle1,
                                needle2_angle: needle2,
                                center_x: cx,
                                center_y: cy,
                                thickness,
                            });
                        }
                    }
                    return LRESULT(0);
                }
                _ => {}
            }
        }

        if msg == WM_NCHITTEST {
            return LRESULT(HTTRANSPARENT as isize);
        }

        if msg == WM_MOUSEACTIVATE {
            return LRESULT(MA_NOACTIVATE as isize);
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    unsafe extern "system" fn controller_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_NCCREATE => {
                let create = lparam.0 as *const CREATESTRUCTW;
                let runtime = (*create).lpCreateParams as *mut Runtime;
                SetWindowLongPtrW(
                    hwnd,
                    WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0),
                    runtime as isize,
                );
                LRESULT(1)
            }

            WM_CREATE => {
                CONTROLLER_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
                if let Some(runtime) = runtime_mut(hwnd) {
                    // let _ = add_tray_icon(hwnd); // Removed: Tray icon only appears when hidden

                    let _ =
                        RegisterHotKey(Some(hwnd), HOTKEY_ID, MOD_CONTROL | MOD_ALT, b'X' as u32);
                    update_foreground_window(GetForegroundWindow());
                    let _ = set_window_focus_event_hook_enabled(runtime, true);
                    let _ = SetTimer(Some(hwnd), TIMER_ID, 500, None);
                    let _ = set_input_hooks_enabled(runtime, false);
                    let _ = refresh_overlay(runtime);
                }

                LRESULT(0)
            }

            WM_TIMER => {
                if let Some(runtime) = runtime_mut(hwnd) {
                    if wparam.0 == FOCUS_TRIGGER_TIMER_ID {
                        if !process_pending_window_focus_trigger() {
                            let _ = KillTimer(Some(hwnd), FOCUS_TRIGGER_TIMER_ID);
                        }
                        return LRESULT(0);
                    }

                    process_pending_commands(hwnd, runtime);
                    let ui_foreground = is_ui_in_foreground();
                    if ui_foreground != runtime.ui_foreground {
                        runtime.ui_foreground = ui_foreground;
                        let _ = set_input_hooks_enabled(runtime, desired_hooks_enabled(runtime));
                        let _ = refresh_overlay(runtime);
                        if ui_foreground {
                            reset_all_input_and_locks();
                            let _ = ShowWindow(runtime.pin_hwnd, SW_HIDE);
                            let _ = ShowWindow(runtime.hud_hwnd, SW_HIDE);
                            runtime.quick_key_display_entries.clear();
                            runtime.quick_key_display_hide_at = None;
                            let _ = ShowWindow(runtime.key_display_hwnd, SW_HIDE);
                        } else {
                            clear_transient_input_state();
                            let _ = refresh_pin_overlay(runtime);
                            let _ = refresh_hud(runtime);
                            let _ = refresh_quick_key_display(runtime);
                            let _ = refresh_mouse_record_trail(runtime);
                        }
                    }

                    if ui_foreground {
                        poll_macro_keyboard_recording();
                    }

                    let preview_active = MOUSE_PATH_PREVIEW.lock().is_some();
                    let mouse_recording_active = MOUSE_RECORDING.lock().is_some();
                    let mouse_trail_visible =
                        windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(
                            runtime.mouse_trail_hwnd,
                        )
                        .as_bool();
                    if mouse_recording_active || mouse_trail_visible || preview_active {
                        let _ = refresh_mouse_record_trail(runtime);
                    }

                    if !is_ui_in_foreground() {
                        apply_keyboard_arrow_mouse_movement();
                        let pin_active = runtime.active_pin_thumbnail.is_some()
                            || HOOK_STATE.lock().active_pin_preset_id.is_some();
                        if pin_active {
                            let _ = refresh_pin_overlay(runtime);
                        }

                        let toolbox_active = HUD_DISPLAY.lock().is_some()
                            || HUD_PREVIEW_DISPLAY.lock().is_some()
                            || runtime.hud_display.is_some();
                        if toolbox_active {
                            let _ = refresh_hud(runtime);
                        }

                        if runtime.quick_key_display_enabled
                            || !runtime.quick_key_display_entries.is_empty()
                        {
                            let _ = refresh_quick_key_display(runtime);
                        }
                    }

                    let _ = refresh_search_area_overlay(runtime);
                    let _ = refresh_timer_overlays(runtime);

                    if runtime.native_focus_highlight_enabled
                        && runtime.focus_highlight_rainbow
                        && runtime.active_focus_highlight_hwnd.is_some()
                    {
                        let target_hwnd = runtime.active_focus_highlight_hwnd.unwrap();
                        runtime.focus_highlight_rainbow_hue = (runtime.focus_highlight_rainbow_hue + 0.015) % 1.0;
                        let _ = paint_focus_highlight_overlay(runtime, target_hwnd);
                    }

                    refresh_overlay_timer(hwnd, runtime);
                }

                LRESULT(0)
            }

            WMAPP_WINDOW_FOCUS_CHANGED => {
                let foreground = GetForegroundWindow();
                if let Some(runtime) = runtime_mut(hwnd) {
                    update_native_focus_highlight(runtime, foreground);
                }
                handle_window_focus_event(hwnd, foreground);
                LRESULT(0)
            }

            WMAPP_WINDOW_LOCATION_CHANGED => {
                let target_hwnd = HWND(wparam.0 as *mut c_void);
                if let Some(runtime) = runtime_mut(hwnd) {
                    let active_hwnd = ACTIVE_HIGHLIGHT_HWND.load(Ordering::Relaxed);
                    let pin_source_hwnd = ACTIVE_PIN_SOURCE_HWND.load(Ordering::Relaxed);

                    if active_hwnd != 0 && target_hwnd.0 as isize == active_hwnd {
                        if runtime.native_focus_highlight_enabled
                            && runtime.active_focus_highlight_hwnd == Some(target_hwnd)
                        {
                            let _ = paint_focus_highlight_overlay(runtime, target_hwnd);
                        }
                    }

                    if pin_source_hwnd != 0 && target_hwnd.0 as isize == pin_source_hwnd {
                        let _ = refresh_pin_overlay(runtime);
                    }
                }
                LRESULT(0)
            }

            WMAPP_PROCESS_QUEUE => {
                if let Some(runtime) = runtime_mut(hwnd) {
                    process_pending_commands(hwnd, runtime);
                    let _ = refresh_search_area_overlay(runtime);
                    let _ = refresh_timer_overlays(runtime);
                    refresh_overlay_timer(hwnd, runtime);
                }

                LRESULT(0)
            }

            WM_HOTKEY => {
                if let Some(runtime) = runtime_mut(hwnd) {
                    if is_ui_in_foreground() {
                        return LRESULT(0);
                    }

                    let hotkey_id = wparam.0 as i32;
                    if hotkey_id == HOTKEY_ID {
                        runtime.style.enabled = !runtime.style.enabled;
                        let _ = refresh_overlay(runtime);
                    } else if let Some(action) = runtime.registered_window_hotkeys.get(&hotkey_id) {
                        match action {
                            WindowHotkeyAction::Apply(preset) => {
                                let _ = apply_window_preset(preset);
                            }

                            WindowHotkeyAction::Focus(preset) => {
                                let _ = focus_window_for_preset(preset);
                            }

                            WindowHotkeyAction::Animate(preset) => {
                                let preset = preset.clone();
                                thread::spawn(move || {
                                    let _ = apply_window_preset_animated(&preset);
                                });
                            }

                            WindowHotkeyAction::RestoreTitleBar(preset) => {
                                let _ = restore_window_title_bar_for_preset(preset);
                            }

                            WindowHotkeyAction::ApplyLayout(layout) => {
                                let layout = layout.clone();
                                thread::spawn(move || {
                                    let _ = window_preset::apply_window_layout(&layout);
                                });
                            }
                        }
                    } else if let Some(preset) = runtime.registered_macro_hotkeys.get(&hotkey_id) {
                        if !SUPPRESSED_MACRO_HOTKEYS.lock().contains(&hotkey_id) {
                            let trigger_key = preset
                                .hotkey
                                .as_ref()
                                .map(|binding| binding.key.clone())
                                .unwrap_or_default();
                            let _ = play_macro_preset(
                                hotkey_id,
                                preset.clone(),
                                None,
                                Vec::new(),
                                false,
                                trigger_key,
                            );
                        }
                    }
                }

                LRESULT(0)
            }

            WM_COMMAND => {
                if let Some(runtime) = runtime_mut(hwnd) {
                    match wparam.0 {
                        MENU_SHOW => {
                            mark_ui_visible(runtime, true);
                            refresh_overlay_timer(hwnd, runtime);
                            show_ui_window_native();
                            let _ = runtime.ui_tx.send(UiCommand::ShowWindow);
                        }

                        MENU_EXIT => {
                            let _ = runtime.ui_tx.send(UiCommand::Exit);
                            let _ = shutdown_application(hwnd, runtime);
                        }

                        _ => {}
                    }
                }

                LRESULT(0)
            }

            WMAPP_TRAYICON => {
                match lparam.0 as u32 {
                    WM_RBUTTONUP => {
                        if let Some(runtime) = runtime_mut(hwnd) {
                            let mut point = POINT::default();
                            let _ = GetCursorPos(&mut point);
                            let _ = SetForegroundWindow(hwnd);
                            let _ = TrackPopupMenu(
                                runtime.tray_menu,
                                TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                                point.x,
                                point.y,
                                Some(0),
                                hwnd,
                                None,
                            );
                        }
                    }

                    WM_LBUTTONUP => {
                        if let Some(runtime) = runtime_mut(hwnd) {
                            let suppress_next_up = {
                                let mut hook_state = HOOK_STATE.lock();
                                if hook_state.tray_double_click_suppress_next_up {
                                    hook_state.tray_double_click_suppress_next_up = false;
                                    true
                                } else {
                                    false
                                }
                            };
                            if suppress_next_up {
                                return LRESULT(0);
                            }

                            if runtime.ui_visible {
                                let (enabled, previous) = {
                                    let mut hook_state = HOOK_STATE.lock();
                                    let previous = hook_state.macros_master_enabled;
                                    hook_state.macros_master_enabled =
                                        !hook_state.macros_master_enabled;
                                    (hook_state.macros_master_enabled, previous)
                                };
                                let _ = previous;
                                let _ = update_tray_icon(hwnd, enabled);
                                let status = if enabled {
                                    "Enabled all macros globally.".to_owned()
                                } else {
                                    "Disabled all macros globally.".to_owned()
                                };
                                let _ = runtime
                                    .ui_tx
                                    .send(UiCommand::SetMacrosMasterEnabled(enabled, status));
                                request_ui_repaint();
                            } else {
                                let (enabled, previous) = {
                                    let mut hook_state = HOOK_STATE.lock();
                                    let previous = hook_state.macros_master_enabled;
                                    hook_state.macros_master_enabled =
                                        !hook_state.macros_master_enabled;
                                    hook_state.pending_tray_toggle = Some(previous);
                                    (hook_state.macros_master_enabled, previous)
                                };
                                let _ = previous;
                                let _ = unsafe { update_tray_icon(hwnd, enabled) };
                                let status = if enabled {
                                    "Enabled all macros globally.".to_owned()
                                } else {
                                    "Disabled all macros globally.".to_owned()
                                };
                                let _ = runtime
                                    .ui_tx
                                    .send(UiCommand::SetMacrosMasterEnabled(enabled, status));
                                request_ui_repaint();
                            }
                        }
                    }

                    WM_LBUTTONDBLCLK => {
                        if let Some(runtime) = runtime_mut(hwnd) {
                            {
                                let mut hook_state = HOOK_STATE.lock();
                                if let Some(previous) = hook_state.pending_tray_toggle.take() {
                                    hook_state.macros_master_enabled = previous;
                                    let _ = unsafe { update_tray_icon(hwnd, previous) };
                                    let status = if previous {
                                        "Enabled all macros globally.".to_owned()
                                    } else {
                                        "Disabled all macros globally.".to_owned()
                                    };
                                    let _ = runtime
                                        .ui_tx
                                        .send(UiCommand::SetMacrosMasterEnabled(previous, status));
                                }

                                hook_state.tray_double_click_suppress_next_up = true;
                            }

                            show_ui_window_native();
                            mark_ui_visible(runtime, true);
                            refresh_overlay_timer(hwnd, runtime);
                            let _ = runtime.ui_tx.send(UiCommand::ShowWindow);
                            request_ui_repaint();
                            wake_command_queue();
                        }
                    }

                    _ => {}
                }

                LRESULT(0)
            }

            WM_DESTROY => {
                CONTROLLER_HWND.store(0, Ordering::Relaxed);
                let _ = KillTimer(Some(hwnd), TIMER_ID);
                unregister_all_hotkeys(hwnd, runtime_mut(hwnd));
                let _ = Shell_NotifyIconW(NIM_DELETE, &notify_icon(hwnd));
                if let Some(runtime) = runtime_mut(hwnd) {
                    runtime.running.store(false, Ordering::Relaxed);
                    clear_native_focus_highlight(runtime);
                    let _ = DestroyMenu(runtime.tray_menu);
                    let _ = ShowWindow(runtime.overlay_hwnd, SW_HIDE);
                    let _ = ShowWindow(runtime.hud_hwnd, SW_HIDE);
                    let _ = ShowWindow(runtime.key_display_hwnd, SW_HIDE);
                    let _ = ShowWindow(runtime.screen_draw_hwnd, SW_HIDE);
                    let _ = ShowWindow(runtime.focus_highlight_hwnd, SW_HIDE);
                    let _ = set_window_focus_event_hook_enabled(runtime, false);
                    let _ = set_window_location_event_hook_enabled(runtime, false);
                    let _ = set_input_hooks_enabled(runtime, false);
                }

                let mut hook_state = HOOK_STATE.lock();
                hook_state.ui_tx = None;
                hook_state.window_presets.clear();
                hook_state.window_expand_controls = WindowExpandControls::default();
                hook_state.macro_groups.clear();
                hook_state.locked_inputs.clear();
                hook_state.mouse_move_locks = MouseMoveLockCounts::default();
                hook_state.mouse_move_lock_anchor = None;
                hook_state.profiles.clear();
                hook_state.sound_presets.clear();
                hook_state.active_hold_macros.clear();
                hook_state.held_mouse_buttons.clear();
                *OVERLAY_COMMAND_TX.lock() = None;
                SEARCH_AREA_OVERLAY_REFRESH_PENDING.store(false, Ordering::Release);
                let ptr = GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0));
                if ptr != 0 {
                    let _runtime = Box::from_raw(ptr as *mut Runtime);
                }

                PostQuitMessage(0);
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe extern "system" fn hud_wnd_proc(
        hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_NCHITTEST => {
                return LRESULT(HTTRANSPARENT as isize);
            }

            WM_MOUSEACTIVATE => {
                return LRESULT(MA_NOACTIVATE as isize);
            }

            windows::Win32::UI::WindowsAndMessaging::WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut paint);
                let _ = EndPaint(hwnd, &paint);
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, msg, _wparam, _lparam),
        }
    }

    unsafe extern "system" fn screen_draw_wnd_proc(
        hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_NCHITTEST => {
                return LRESULT(HTTRANSPARENT as isize);
            }
            WM_MOUSEACTIVATE => {
                return LRESULT(MA_NOACTIVATE as isize);
            }
            WM_TIMER => {
                if _wparam.0 == SCREEN_DRAW_TIMER_ID {
                    let should_paint = {
                        let state = SCREEN_DRAW_STATE.lock();
                        state.active && state.pending_repaint
                    };
                    if should_paint {
                        let _ = paint_screen_draw_overlay(hwnd);
                    }
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, _wparam, _lparam)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut paint);
                let _ = EndPaint(hwnd, &paint);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, _wparam, _lparam),
        }
    }

    unsafe extern "system" fn low_level_keyboard_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32 {
            let info = *(lparam.0 as *const KBDLLHOOKSTRUCT);
            let msg = wparam.0 as u32;
            let is_key_event = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP);
            let injected = info.flags.0 & 0x10 != 0;
            if is_key_event && !injected {
                let is_key_down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
                let is_key_up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);
                if is_key_down && info.vkCode == 0x1B && is_mouse_path_draw_capture_active() {
                    cancel_mouse_path_draw_capture("Mouse path draw cancelled.".to_owned());
                    update_modifier_state(info.vkCode, is_key_down);
                    return LRESULT(1);
                }

                let key_name = hotkey::vk_to_key_name(info.vkCode).map(str::to_owned);
                if !is_ui_in_foreground()
                    && let Some(key_name) = key_name.as_ref()
                {
                    update_quick_key_display_key(key_name, is_key_down, is_key_up);
                }
                if let Some(key_name) = key_name.clone() {
                    if is_key_down {
                        let binding = binding_from_trigger_event(&key_name);
                        if process_screen_draw_hotkey(&binding, is_repeat_key(&key_name)) {
                            update_modifier_state(info.vkCode, is_key_down);
                            return LRESULT(1);
                        }
                    }
                }
                let windows_key_locked = {
                    let hook_state = HOOK_STATE.lock();
                    hook_state.windows_key_locked
                };
                if windows_key_locked && matches!(info.vkCode, 0x5B | 0x5C)
                {
                    if let Some(key_name) = key_name.as_ref() {
                        update_held_key(key_name, is_key_down, is_key_up);
                    }
                    update_modifier_state(info.vkCode, is_key_down);
                    return LRESULT(1);
                }
                if is_key_down && !is_ui_in_foreground() {
                    let mut rec_guard = MACRO_RECORDING.lock();
                    if let Some(session) = rec_guard.as_mut() {
                        let now = std::time::Instant::now();
                        let delay_ms = now
                            .saturating_duration_since(session.last_event_at)
                            .as_millis()
                            .min(u64::MAX as u128) as u64;
                        if let Some(k_name) = key_name.clone() {
                            session.last_event_at = now;
                            session.events.push(MacroRecordingEvent {
                                key: Some(k_name.clone()),
                                action: crate::model::MacroAction::KeyPress,
                                delay_ms,
                                x: 0,
                                y: 0,
                            });
                            if let Some(tx) = &HOOK_STATE.lock().ui_tx {
                                let mut step = crate::model::MacroStep::default();
                                step.action = crate::model::MacroAction::KeyPress;
                                step.delay_ms = delay_ms;
                                step.key = k_name;
                                let _ = tx.send(UiCommand::MacroRealtimeStepAdded(
                                    session.group_id,
                                    session.preset_id,
                                    step,
                                ));
                            }
                        }
                    }
                }

                // Global record toggle hotkey processing

                if let Some(key_name) = key_name.clone() {
                    let binding = binding_from_trigger_event(&key_name);
                    if is_key_down {
                        let repeat = is_repeat_key(&key_name);
                        if let Some(swallow) = process_macro_record_hotkey(&binding, repeat) {
                            update_modifier_state(info.vkCode, is_key_down);
                            if swallow {
                                return LRESULT(1);
                            }
                        }

                        if let Some(swallow) = process_mouse_path_record_hotkey(&binding, repeat) {
                            update_modifier_state(info.vkCode, is_key_down);
                            if swallow {
                                return LRESULT(1);
                            }
                        }
                    }
                }

                // Skip normal hotkeys if UI is focused

                if is_ui_in_foreground() {
                    if let Some(key_name) = key_name.clone() {
                        update_held_key(&key_name, is_key_down, is_key_up);
                    }

                    update_modifier_state(info.vkCode, is_key_down);
                    return CallNextHookEx(None, code, wparam, lparam);
                }

                if let Some(key_name) = key_name.clone() {
                    let binding = binding_from_trigger_event(&key_name);
                    if key_name.eq_ignore_ascii_case("Tab") && binding.alt {
                        update_held_key(&key_name, is_key_down, is_key_up);
                        update_modifier_state(info.vkCode, is_key_down);
                        return CallNextHookEx(None, code, wparam, lparam);
                    }

                    let mut swallow = false;
                    if is_key_down {
                        let repeat = is_repeat_key(&key_name);
                        if let Some(binding_swallow) = process_binding_press(&binding, repeat) {
                            swallow |= binding_swallow;
                        }
                    }

                    update_held_key(&key_name, is_key_down, is_key_up);
                    if is_key_up {
                        swallow |= process_binding_release(&binding);
                    }

                    let macros_master_enabled = {
                        let hook_state = HOOK_STATE.lock();
                        hook_state.macros_master_enabled
                    };
                    if macros_master_enabled {
                        swallow |= binding_matches_any_hold_macro(&binding);
                        swallow |= is_locked_input(&key_name);
                    }

                    swallow |= keyboard_arrow_mouse_should_swallow(&key_name);
                    update_modifier_state(info.vkCode, is_key_down);
                    return if swallow {
                        LRESULT(1)
                    } else {
                        CallNextHookEx(None, code, wparam, lparam)
                    };
                }

                update_modifier_state(info.vkCode, is_key_down);
            }
        }

        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe extern "system" fn low_level_mouse_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32 {
            let info = *(lparam.0 as *const MSLLHOOKSTRUCT);
            let injected = info.flags & 0x01 != 0;
            if injected {
                return CallNextHookEx(None, code, wparam, lparam);
            }

            let message = wparam.0 as u32;
            if message == WM_MOUSEWHEEL {
                let delta = ((info.mouseData >> 16) & 0xFFFF) as i16;
                let mut hook_state = HOOK_STATE.lock();
                if delta > 0 {
                    hook_state.last_scroll_up_at = Some(std::time::Instant::now());
                } else if delta < 0 {
                    hook_state.last_scroll_down_at = Some(std::time::Instant::now());
                }
            }

            record_mouse_event(message, &info);
            record_macro_mouse_event(message, &info);
            let active_mouse_path_draw_capture = HOOK_STATE.lock().mouse_path_draw_capture.clone();
            if let Some(draw_capture) = active_mouse_path_draw_capture {
                match message {
                    WM_LBUTTONDOWN => {
                        update_held_mouse_button(message, ((info.mouseData >> 16) & 0xFFFF) as u16);
                        if MOUSE_RECORDING.lock().is_none() {
                            start_mouse_path_draw_recording(&draw_capture, info.pt);
                        }

                        wake_command_queue();
                        return LRESULT(1);
                    }
                    WM_LBUTTONUP => {
                        update_held_mouse_button(message, ((info.mouseData >> 16) & 0xFFFF) as u16);
                        finish_mouse_path_draw_capture();
                        wake_command_queue();
                        return LRESULT(1);
                    }
                    WM_RBUTTONDOWN
                    | WM_RBUTTONUP
                    | WM_MBUTTONDOWN
                    | windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP
                    | WM_XBUTTONDOWN
                    | WM_XBUTTONUP
                    | WM_MOUSEWHEEL => {
                        update_held_mouse_button(message, ((info.mouseData >> 16) & 0xFFFF) as u16);
                        return LRESULT(1);
                    }
                    _ => {}
                }
            }

            let mouse_data = ((info.mouseData >> 16) & 0xFFFF) as u16;
            let screen_draw_event_key = mouse_binding_name_from_message(message, mouse_data);
            if screen_draw_active() {
                if screen_draw_event_key.is_some() {
                    update_held_mouse_button(message, mouse_data);
                    if !is_ui_in_foreground()
                        && let Some(key_name) = screen_draw_event_key
                    {
                        let is_key_down = !matches!(
                            message,
                            WM_LBUTTONUP
                                | WM_RBUTTONUP
                                | windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP
                                | WM_XBUTTONUP
                        );
                        let is_key_up = !is_key_down && message != WM_MOUSEWHEEL;
                        update_quick_key_display_key(key_name, is_key_down, is_key_up);
                    }
                }
                if process_screen_draw_mouse_event(message, info.pt) {
                    return LRESULT(1);
                }
            }

            // 1. Immediately bypass WM_MOUSEMOVE to keep mouse movement extremely smooth and lock-free!

            if message == WM_MOUSEMOVE && !is_vision_capture_mouse_blocked() {
                if handle_locked_mouse_move(info.pt) {
                    return LRESULT(1);
                }
                return CallNextHookEx(None, code, wparam, lparam);
            }

            // 2. If MacroNest UI is in the foreground, bypass all mouse events.

            if UI_WINDOW_FOREGROUND.load(Ordering::Relaxed) && !is_vision_capture_mouse_blocked() {
                return CallNextHookEx(None, code, wparam, lparam);
            }

            // 3. For actual click/wheel events (extremely rare), check if the physical click target

            // is actually the MacroNest window. This ensures that clicks on game windows that cover/obscure

            // MacroNest in the background are NOT bypassed, allowing macro triggering to work perfectly!

            let hwnd_at_point = WindowFromPoint(info.pt);
            if !hwnd_at_point.0.is_null() && !is_vision_capture_mouse_blocked() {
                let root = GetAncestor(hwnd_at_point, GA_ROOT);
                if !root.0.is_null() && window_belongs_to_current_process(root) {
                    return CallNextHookEx(None, code, wparam, lparam);
                }
            }

            if is_mouse_locked() {
                match message {
                    WM_MOUSEMOVE
                    | WM_MOUSEWHEEL
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_RBUTTONDOWN
                    | WM_RBUTTONUP
                    | WM_MBUTTONDOWN
                    | windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP
                    | WM_XBUTTONDOWN
                    | WM_XBUTTONUP => {
                        update_held_mouse_button(message, ((info.mouseData >> 16) & 0xFFFF) as u16);
                        return LRESULT(1);
                    }

                    _ => {}
                }
            }

            if is_vision_capture_mouse_blocked() {
                match message {
                    WM_MOUSEMOVE => {
                        let mut hook_state = HOOK_STATE.lock();
                        let left_held = hook_state.held_mouse_buttons.contains("MouseLeft");
                        if left_held {
                            if let Some((start_x, start_y)) = hook_state.vision_capture_anchor {
                                let left = start_x.min(info.pt.x);
                                let top = start_y.min(info.pt.y);
                                let width = (start_x - info.pt.x).abs().max(1);
                                let height = (start_y - info.pt.y).abs().max(1);
                                let region = VisionRegion {
                                    left,
                                    top,
                                    width,
                                    height,
                                    is_circle: false,
                                    angle_offset_deg: None,
                                    angle_span_deg: None,
                                };
                                if hook_state.vision_capture_preview_regions.get(0) != Some(&region)
                                {
                                    hook_state.vision_capture_preview_regions = vec![region];
                                }
                            }
                        }

                        let ui_tx = hook_state.ui_tx.clone();
                        drop(hook_state);
                        if let Some(ui_tx) = ui_tx {
                            let _ = ui_tx.send(UiCommand::VisionCaptureMouseMove {
                                screen_x: info.pt.x,
                                screen_y: info.pt.y,
                            });
                        }

                        wake_command_queue();
                        return CallNextHookEx(None, code, wparam, lparam);
                    }

                    WM_LBUTTONDOWN => {
                        update_held_mouse_button(message, ((info.mouseData >> 16) & 0xFFFF) as u16);
                        let mut hook_state = HOOK_STATE.lock();
                        if hook_state.vision_capture_is_region_mode {
                            hook_state.vision_capture_anchor = Some((info.pt.x, info.pt.y));
                            hook_state.vision_capture_preview_regions = vec![VisionRegion {
                                left: info.pt.x,
                                top: info.pt.y,
                                width: 1,
                                height: 1,
                                is_circle: false,
                                angle_offset_deg: None,
                                angle_span_deg: None,
                            }];
                        }

                        let ui_tx = hook_state.ui_tx.clone();
                        drop(hook_state);
                        if let Some(ui_tx) = ui_tx {
                            let _ = ui_tx.send(UiCommand::VisionCaptureMouseDown {
                                screen_x: info.pt.x,
                                screen_y: info.pt.y,
                            });
                        }

                        wake_command_queue();
                        return LRESULT(1);
                    }

                    WM_LBUTTONUP => {
                        update_held_mouse_button(message, ((info.mouseData >> 16) & 0xFFFF) as u16);
                        let mut hook_state = HOOK_STATE.lock();
                        hook_state.vision_capture_anchor = None;
                        hook_state.vision_capture_preview_regions = Vec::new();
                        hook_state.vision_preview_source = None;
                        let ui_tx = hook_state.ui_tx.clone();
                        drop(hook_state);
                        if let Some(ui_tx) = ui_tx {
                            let _ = ui_tx.send(UiCommand::VisionCaptureMouseUp {
                                screen_x: info.pt.x,
                                screen_y: info.pt.y,
                            });
                        }

                        wake_command_queue();
                        return LRESULT(1);
                    }

                    WM_MOUSEWHEEL
                    | WM_RBUTTONDOWN
                    | WM_RBUTTONUP
                    | WM_MBUTTONDOWN
                    | windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP
                    | WM_XBUTTONDOWN
                    | WM_XBUTTONUP => {
                        update_held_mouse_button(message, ((info.mouseData >> 16) & 0xFFFF) as u16);
                        return LRESULT(1);
                    }

                    _ => {}
                }
            }

            let recording_active =
                MOUSE_RECORDING.lock().is_some() || MACRO_RECORDING.lock().is_some();
            if recording_active {
                return CallNextHookEx(None, code, wparam, lparam);
            }

            let event = match wparam.0 as u32 {
                WM_LBUTTONDOWN => Some((binding_from_trigger_event("MouseLeft"), true)),
                WM_LBUTTONUP => Some((binding_from_trigger_event("MouseLeft"), false)),
                WM_RBUTTONDOWN => Some((binding_from_trigger_event("MouseRight"), true)),
                WM_RBUTTONUP => Some((binding_from_trigger_event("MouseRight"), false)),
                WM_MBUTTONDOWN => Some((binding_from_trigger_event("MouseMiddle"), true)),
                windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP => {
                    Some((binding_from_trigger_event("MouseMiddle"), false))
                }

                WM_XBUTTONDOWN if (mouse_data & XBUTTON2_DATA) != 0 => {
                    Some((binding_from_trigger_event("MouseX2"), true))
                }

                WM_XBUTTONUP if (mouse_data & XBUTTON2_DATA) != 0 => {
                    Some((binding_from_trigger_event("MouseX2"), false))
                }

                WM_XBUTTONDOWN if (mouse_data & XBUTTON1_DATA) != 0 => {
                    Some((binding_from_trigger_event("MouseX1"), true))
                }

                WM_XBUTTONUP if (mouse_data & XBUTTON1_DATA) != 0 => {
                    Some((binding_from_trigger_event("MouseX1"), false))
                }

                WM_MOUSEWHEEL => {
                    let data = mouse_data as i16;
                    let name = if data > 0 {
                        "MouseWheelUp"
                    } else {
                        "MouseWheelDown"
                    };
                    Some((binding_from_trigger_event(name), true))
                }

                _ => None,
            };
            if let Some((binding, is_down)) = event {
                let event_key_name = mouse_binding_name_from_message(
                    message,
                    ((info.mouseData >> 16) & 0xFFFF) as u16,
                );
                update_held_mouse_button(message, ((info.mouseData >> 16) & 0xFFFF) as u16);
                if !is_ui_in_foreground()
                    && let Some(key_name) = event_key_name
                {
                    let is_key_up = matches!(
                        message,
                        WM_LBUTTONUP
                            | WM_RBUTTONUP
                            | windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP
                            | WM_XBUTTONUP
                    );
                    update_quick_key_display_key(key_name, is_down, is_key_up);
                }
                if let Some(key_name) = event_key_name
                    && consume_suppressed_mouse_trigger(key_name)
                {
                    return CallNextHookEx(None, code, wparam, lparam);
                }

                let swallow_release = if !is_down {
                    event_key_name
                        .map(consume_swallowed_mouse_trigger_release)
                        .unwrap_or(false)
                } else {
                    false
                };
                let mut swallow = if is_down {
                    process_binding_press(&binding, false).unwrap_or(false)
                } else {
                    process_binding_release(&binding)
                };
                if is_down
                    && swallow
                    && let Some(key_name) = event_key_name
                {
                    swallow_mouse_trigger_until_release(key_name);
                }

                swallow |= swallow_release;
                let macros_master_enabled = {
                    let hook_state = HOOK_STATE.lock();
                    hook_state.macros_master_enabled
                };
                if macros_master_enabled {
                    swallow |= binding_matches_any_hold_macro(&binding);
                }

                return if swallow {
                    LRESULT(1)
                } else {
                    CallNextHookEx(None, code, wparam, lparam)
                };
            }
        }

        CallNextHookEx(None, code, wparam, lparam)
    }

    fn binding_from_event(key_name: &str) -> HotkeyBinding {
        let ctrl_down = unsafe { GetAsyncKeyState(0x11) } < 0;
        let alt_down = unsafe { GetAsyncKeyState(0x12) } < 0;
        let shift_down = unsafe { GetAsyncKeyState(0x10) } < 0;
        let win_down =
            unsafe { GetAsyncKeyState(0x5B) } < 0 || unsafe { GetAsyncKeyState(0x5C) } < 0;
        let mut combo_keys = {
            let hook_state = HOOK_STATE.lock();
            let mut keys = hook_state
                .held_inputs
                .iter()
                .cloned()
                .chain(hook_state.held_mouse_buttons.iter().cloned())
                .collect::<Vec<_>>();
            keys.push(key_name.to_owned());
            keys
        };
        combo_keys.retain(|key| !key.trim().is_empty());
        combo_keys.sort_by(|a, b| {
            let rank_a = hotkey_binding_rank(a);
            let rank_b = hotkey_binding_rank(b);
            rank_a
                .cmp(&rank_b)
                .then_with(|| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()))
        });
        combo_keys.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        HotkeyBinding {
            ctrl: ctrl_down && !key_name.eq_ignore_ascii_case("Ctrl"),
            alt: alt_down && !key_name.eq_ignore_ascii_case("Alt"),
            shift: shift_down && !key_name.eq_ignore_ascii_case("Shift"),
            win: win_down && !key_name.eq_ignore_ascii_case("Win"),
            key: key_name.to_owned(),
            combo_keys,
        }
    }

    fn binding_from_trigger_event(key_name: &str) -> HotkeyBinding {
        let ctrl_down = unsafe { GetAsyncKeyState(0x11) } < 0;
        let alt_down = unsafe { GetAsyncKeyState(0x12) } < 0;
        let shift_down = unsafe { GetAsyncKeyState(0x10) } < 0;
        let win_down =
            unsafe { GetAsyncKeyState(0x5B) } < 0 || unsafe { GetAsyncKeyState(0x5C) } < 0;
        let mut combo_keys = vec![key_name.to_owned()];
        if ctrl_down {
            combo_keys.push("Ctrl".to_owned());
        }

        if alt_down {
            combo_keys.push("Alt".to_owned());
        }

        if shift_down {
            combo_keys.push("Shift".to_owned());
        }

        if win_down {
            combo_keys.push("Win".to_owned());
        }

        combo_keys.sort_by(|a, b| {
            let rank_a = hotkey_binding_rank(a);
            let rank_b = hotkey_binding_rank(b);
            rank_a
                .cmp(&rank_b)
                .then_with(|| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()))
        });
        combo_keys.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        HotkeyBinding {
            ctrl: ctrl_down && !key_name.eq_ignore_ascii_case("Ctrl"),
            alt: alt_down && !key_name.eq_ignore_ascii_case("Alt"),
            shift: shift_down && !key_name.eq_ignore_ascii_case("Shift"),
            win: win_down && !key_name.eq_ignore_ascii_case("Win"),
            key: key_name.to_owned(),
            combo_keys,
        }
    }

    fn hotkey_binding_rank(name: &str) -> (u8, String) {
        let normalized = name.trim().to_ascii_lowercase();
        let rank = match normalized.as_str() {
            "ctrl" | "control" => 0,
            "alt" => 1,
            "shift" => 2,
            "win" | "meta" => 3,
            _ => 4,
        };
        (rank, normalized)
    }

    fn process_mouse_path_record_hotkey(binding: &HotkeyBinding, is_repeat: bool) -> Option<bool> {
        if is_repeat {
            return None;
        }

        let matched = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .mouse_path_presets
                .iter()
                .find(|preset| {
                    preset.enabled
                        && preset
                            .record_hotkey
                            .as_ref()
                            .is_some_and(|hotkey| hotkey::binding_matches(hotkey, binding))
                })
                .cloned()
        };
        let Some(preset) = matched else {
            return None;
        };
        toggle_mouse_recording(preset.id, preset.name);
        Some(true)
    }






    fn process_image_search_hotkey(binding: &HotkeyBinding, is_repeat: bool) -> Option<bool> {
        if is_repeat {
            return None;
        }

        let (matched, ui_tx) = {
            let hook_state = HOOK_STATE.lock();
            let matched = hook_state
                .vision_presets
                .iter()
                .filter(|preset| {
                    preset.enabled
                        && window_focus_matches(
                            preset.target_window_title.as_deref(),
                            &preset.extra_target_window_titles,
                            preset.match_duplicate_window_titles,
                        )
                        && preset_trigger_matches(
                            preset.hotkey.as_ref(),
                            &preset.trigger_keys,
                            binding,
                        )
                })
                .cloned()
                .collect::<Vec<_>>();
            (matched, hook_state.ui_tx.clone())
        };
        if matched.is_empty() {
            return None;
        }

        for preset in matched {
            if preset.repeat_until_triggered_again {
                let active = {
                    let mut hook_state = HOOK_STATE.lock();
                    if hook_state.vision_following_presets.contains(&preset.id) {
                        hook_state.vision_following_presets.remove(&preset.id);
                        false
                    } else {
                        hook_state.vision_following_presets.insert(preset.id);
                        true
                    }
                };
                if !active {
                    if let Some(tx) = ui_tx.as_ref() {
                        let _ = tx.send(UiCommand::VisionFinished(format!(
                            "{}: repeat mode stopped.",
                            preset.name
                        )));
                    }

                    continue;
                }

                let ui_tx = ui_tx.clone();
                set_image_search_following_active(preset.id, true);
                thread::spawn(move || run_image_search_follow_loop(preset, ui_tx, None));
                continue;
            }

            let ui_tx = ui_tx.clone();
            thread::spawn(move || {
                let status = match run_vision_once(&preset) {
                    Ok(status) => status,
                    Err(error) => format!("Vision search failed: {error}"),
                };
                if let Some(tx) = ui_tx {
                    let _ = tx.send(UiCommand::VisionFinished(format!(
                        "{}: {status}",
                        preset.name
                    )));
                }
            });
        }

        Some(true)
    }

    fn toggle_mouse_recording(preset_id: u32, preset_name: String) {
        let finished = {
            let mut guard = MOUSE_RECORDING.lock();
            if guard
                .as_ref()
                .is_some_and(|session| session.preset_id == preset_id)
            {
                guard
                    .take()
                    .map(|session| (session.preset_id, session.events))
            } else {
                *guard = Some(MouseRecordingSession {
                    preset_id,
                    last_event_at: Instant::now(),
                    events: Vec::new(),
                    dirty: true,
                    movement_only: false,
                });
                None
            }
        };
        let ui_tx = HOOK_STATE.lock().ui_tx.clone();
        if let Some((finished_id, events)) = finished {
            if let Some(tx) = ui_tx {
                let _ = tx.send(UiCommand::MousePathRecordingFinished(
                    finished_id,
                    events,
                    format!("Saved mouse record for {preset_name}."),
                ));
            }
        } else if let Some(tx) = ui_tx {
            let _ = tx.send(UiCommand::MousePathRecordingStarted(
                preset_id,
                format!("Recording mouse path for {preset_name}. Press the hotkey again to stop."),
            ));
        }
    }

    fn is_mouse_path_draw_capture_active() -> bool {
        HOOK_STATE.lock().mouse_path_draw_capture.is_some()
    }

    fn begin_mouse_path_draw_capture(preset_id: u32, preset_name: String) {
        {
            let mut hook_state = HOOK_STATE.lock();
            hook_state.mouse_path_draw_capture = Some(MousePathDrawCaptureSession {
                preset_id,
                preset_name,
            });
        }

        *MOUSE_RECORDING.lock() = None;
        request_ui_repaint();
    }

    fn cancel_mouse_path_draw_capture(status: String) {
        {
            let mut hook_state = HOOK_STATE.lock();
            hook_state.mouse_path_draw_capture = None;
        }

        *MOUSE_RECORDING.lock() = None;
        show_ui_window_native();
        if let Some(tx) = HOOK_STATE.lock().ui_tx.clone() {
            let _ = tx.send(UiCommand::ShowWindow);
            let _ = tx.send(UiCommand::MousePathDrawCaptureCancelled(status));
        }

        request_ui_repaint();
    }

    fn start_mouse_path_draw_recording(session: &MousePathDrawCaptureSession, point: POINT) {
        {
            let mut guard = MOUSE_RECORDING.lock();
            *guard = Some(MouseRecordingSession {
                preset_id: session.preset_id,
                last_event_at: Instant::now(),
                events: vec![MousePathEvent {
                    kind: MousePathEventKind::Move,
                    x: point.x,
                    y: point.y,
                    delay_ms: 0,
                }],
                dirty: true,
                movement_only: true,
            });
        }

        if let Some(tx) = HOOK_STATE.lock().ui_tx.clone() {
            let _ = tx.send(UiCommand::MousePathRecordingStarted(
                session.preset_id,
                format!(
                    "Recording mouse path for {}. Release left mouse to save.",
                    session.preset_name
                ),
            ));
        }

        request_ui_repaint();
    }

    fn finish_mouse_path_draw_capture() {
        let active = {
            let mut hook_state = HOOK_STATE.lock();
            hook_state.mouse_path_draw_capture.take()
        };
        let Some(active) = active else {
            return;
        };
        let finished = MOUSE_RECORDING
            .lock()
            .take()
            .map(|session| (session.preset_id, session.events));
        show_ui_window_native();
        if let Some(tx) = HOOK_STATE.lock().ui_tx.clone() {
            let _ = tx.send(UiCommand::ShowWindow);
            if let Some((preset_id, events)) = finished {
                let _ = tx.send(UiCommand::MousePathRecordingFinished(
                    preset_id,
                    events,
                    format!("Saved mouse record for {}.", active.preset_name),
                ));
            } else {
                let _ = tx.send(UiCommand::MousePathDrawCaptureCancelled(format!(
                    "Mouse path draw cancelled for {}.",
                    active.preset_name
                )));
            }
        }

        request_ui_repaint();
    }

    fn macro_record_scan_keys() -> Vec<u32> {
        let mut keys = Vec::new();
        keys.extend(0x08..=0x0D);
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

    fn poll_macro_keyboard_recording() {
        if !is_ui_in_foreground() {
            return;
        }

        let mut guard = MACRO_RECORDING.lock();
        let Some(session) = guard.as_mut() else {
            return;
        };
        let now = Instant::now();
        for vk in macro_record_scan_keys() {
            let pressed = unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
            if pressed {
                if !session.pressed_key_vks.insert(vk) {
                    continue;
                }

                let Some(key_name) = hotkey::vk_to_key_name(vk).map(str::to_owned) else {
                    continue;
                };
                let delay_ms = now
                    .saturating_duration_since(session.last_event_at)
                    .as_millis()
                    .min(u64::MAX as u128) as u64;
                session.last_event_at = now;
                session.events.push(MacroRecordingEvent {
                    key: Some(key_name.clone()),
                    action: crate::model::MacroAction::KeyPress,
                    delay_ms,
                    x: 0,
                    y: 0,
                });
                if let Some(tx) = &HOOK_STATE.lock().ui_tx {
                    let mut step = crate::model::MacroStep::default();
                    step.action = crate::model::MacroAction::KeyPress;
                    step.delay_ms = delay_ms;
                    step.key = key_name;
                    let _ = tx.send(UiCommand::MacroRealtimeStepAdded(
                        session.group_id,
                        session.preset_id,
                        step,
                    ));
                }
            } else {
                session.pressed_key_vks.remove(&vk);
            }
        }
    }

    fn process_mouse_sensitivity_hotkey(binding: &HotkeyBinding, is_repeat: bool) -> Option<bool> {
        if is_repeat {
            return None;
        }

        let matched = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .mouse_sensitivity_presets
                .iter()
                .find(|preset| {
                    preset.enabled
                        && window_focus_matches(
                            preset.target_window_title.as_deref(),
                            &preset.extra_target_window_titles,
                            preset.match_duplicate_window_titles,
                        )
                        && preset_trigger_matches(
                            preset.hotkey.as_ref(),
                            &preset.trigger_keys,
                            binding,
                        )
                })
                .cloned()
        };
        let Some(preset) = matched else {
            return None;
        };
        let _ = toggle_mouse_sensitivity_preset(&preset);
        Some(true)
    }

    fn record_macro_mouse_event(message: u32, info: &MSLLHOOKSTRUCT) {
        let mut guard = MACRO_RECORDING.lock();
        let Some(session) = guard.as_mut() else {
            return;
        };
        // 1. Identify the event kind first and return early if it's not a recorded macro mouse action.

        // This avoids calling the heavy is_click_inside_ui() for every single pixel of WM_MOUSEMOVE!

        let kind = match message {
            WM_MOUSEMOVE => Some(crate::model::MacroAction::MouseMoveAbsolute),
            WM_LBUTTONDOWN => Some(crate::model::MacroAction::MouseLeftClick),
            WM_RBUTTONDOWN => Some(crate::model::MacroAction::MouseRightClick),
            WM_MBUTTONDOWN => Some(crate::model::MacroAction::MouseMiddleClick),
            WM_XBUTTONDOWN => {
                let xbutton = ((info.mouseData >> 16) & 0xFFFF) as u16;
                if (xbutton & XBUTTON2_DATA) != 0 {
                    Some(crate::model::MacroAction::MouseX2Click)
                } else if (xbutton & XBUTTON1_DATA) != 0 {
                    Some(crate::model::MacroAction::MouseX1Click)
                } else {
                    None
                }
            }

            WM_MOUSEWHEEL => {
                let data = ((info.mouseData >> 16) & 0xFFFF) as i16;
                if data > 0 {
                    Some(crate::model::MacroAction::MouseWheelUp)
                } else {
                    Some(crate::model::MacroAction::MouseWheelDown)
                }
            }

            _ => None,
        };
        let Some(action) = kind else {
            return;
        };
        let now = std::time::Instant::now();
        let delay_ms = now
            .saturating_duration_since(session.last_event_at)
            .as_millis()
            .min(u64::MAX as u128) as u64;
        session.last_event_at = now;
        session.events.push(MacroRecordingEvent {
            key: None,
            action,
            delay_ms,
            x: info.pt.x,
            y: info.pt.y,
        });
    }

    fn toggle_macro_recording(group_id: u32, preset_id: u32, preset_name: String) {
        let finished = {
            let mut guard = MACRO_RECORDING.lock();
            if guard.is_some() {
                let session = guard.take().unwrap();
                if session.preset_id == preset_id {
                    Some((session.group_id, session.preset_id, session.events, true))
                } else {
                    *guard = Some(MacroRecordingSession {
                        group_id,
                        preset_id,
                        last_event_at: std::time::Instant::now(),
                        events: Vec::new(),
                        pressed_key_vks: std::collections::HashSet::new(),
                    });
                    Some((session.group_id, session.preset_id, session.events, false))
                }
            } else {
                *guard = Some(MacroRecordingSession {
                    group_id,
                    preset_id,
                    last_event_at: std::time::Instant::now(),
                    events: Vec::new(),
                    pressed_key_vks: std::collections::HashSet::new(),
                });
                None
            }
        };
        let ui_tx = HOOK_STATE.lock().ui_tx.clone();
        if let Some((finished_group_id, finished_preset_id, events, is_same)) = finished {
            if let Some(tx) = &ui_tx {
                let _ = tx.send(UiCommand::MacroRecordingFinished(
                    finished_group_id,
                    finished_preset_id,
                    events,
                    format!("Saved macro record."),
                ));
            }

            if !is_same {
                if let Some(tx) = &ui_tx {
                    let _ = tx.send(UiCommand::MacroRecordingStarted(
                        preset_id,
                        format!(
                            "Recording macro for {preset_name}. Press Stop in the UI to finish."
                        ),
                    ));
                }
            }
        } else if let Some(tx) = ui_tx {
            let _ = tx.send(UiCommand::MacroRecordingStarted(
                preset_id,
                format!("Recording macro for {preset_name}. Press Stop in the UI to finish."),
            ));
        }
    }

    fn process_macro_record_hotkey(binding: &HotkeyBinding, is_repeat: bool) -> Option<bool> {
        if is_repeat {
            return None;
        }

        let matched = {
            let hook_state = HOOK_STATE.lock();
            let mut found = None;
            for group in &hook_state.macro_groups {
                for preset in &group.presets {
                    if let Some(record_hotkey) = &preset.record_hotkey {
                        if hotkey::binding_matches(record_hotkey, binding) {
                            found = Some((group.id, preset.id, group.name.clone()));
                            break;
                        }
                    }
                }

                if found.is_some() {
                    break;
                }
            }

            found
        };
        if let Some((group_id, preset_id, group_name)) = matched {
            toggle_macro_recording(group_id, preset_id, group_name);
            Some(true)
        } else {
            None
        }
    }

    fn record_mouse_event(message: u32, info: &MSLLHOOKSTRUCT) {
        let mut guard = MOUSE_RECORDING.lock();
        let Some(session) = guard.as_mut() else {
            return;
        };
        let now = Instant::now();
        let delay_ms = now
            .saturating_duration_since(session.last_event_at)
            .as_millis()
            .min(u64::MAX as u128) as u64;
        session.last_event_at = now;
        let point = info.pt;
        let kind = match (message, ((info.mouseData >> 16) & 0xFFFF) as u16) {
            (WM_MOUSEMOVE, _) => Some(MousePathEventKind::Move),
            (WM_LBUTTONDOWN, _) => Some(MousePathEventKind::LeftDown),
            (WM_LBUTTONUP, _) => Some(MousePathEventKind::LeftUp),
            (WM_RBUTTONDOWN, _) => Some(MousePathEventKind::RightDown),
            (WM_RBUTTONUP, _) => Some(MousePathEventKind::RightUp),
            (WM_MBUTTONDOWN, _) => Some(MousePathEventKind::MiddleDown),
            (windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP, _) => {
                Some(MousePathEventKind::MiddleUp)
            }

            (WM_MOUSEWHEEL, data) if (data as i16) > 0 => Some(MousePathEventKind::WheelUp),
            (WM_MOUSEWHEEL, _) => Some(MousePathEventKind::WheelDown),
            _ => None,
        };
        let Some(kind) = kind else {
            return;
        };
        if session.movement_only && !matches!(kind, MousePathEventKind::Move) {
            return;
        }

        if matches!(kind, MousePathEventKind::Move)
            && session.events.last().is_some_and(|last| {
                matches!(last.kind, MousePathEventKind::Move)
                    && last.x == point.x
                    && last.y == point.y
            })
        {
            return;
        }

        session.events.push(MousePathEvent {
            kind,
            x: point.x,
            y: point.y,
            delay_ms,
        });
        session.dirty = true;
    }

    fn release_trigger_ready(
        wait_key_spec: &str,
        require_all_inputs_released: bool,
        _released_key: &str,
    ) -> bool {
        let wait_keys = parse_locked_keys(wait_key_spec);
        let hook_state = HOOK_STATE.lock();
        if wait_keys.iter().any(|wait_key| {
            hook_state
                .held_inputs
                .iter()
                .any(|held| held.eq_ignore_ascii_case(wait_key))
                || hook_state
                    .held_mouse_buttons
                    .iter()
                    .any(|held| held.eq_ignore_ascii_case(wait_key))
        }) {
            return false;
        }

        if !require_all_inputs_released {
            return true;
        }

        hook_state.held_inputs.is_empty() && hook_state.held_mouse_buttons.is_empty()
    }

    fn binding_is_single_key(binding: &HotkeyBinding) -> bool {
        hotkey::binding_key_names(binding).len() == 1
    }

    fn mouse_trigger_is_physically_down(trigger: &HotkeyBinding) -> bool {
        let Some(vk) = hotkey::key_name_to_vk(&trigger.key) else {
            return true;
        };
        if !hotkey::is_mouse_key_name(&trigger.key) {
            return true;
        }

        (unsafe { GetAsyncKeyState(vk as i32) }) < 0
    }

    fn reconcile_active_hold_mouse_macros() {
        let stale_ids = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .active_hold_macros
                .iter()
                .filter_map(|(preset_id, active)| {
                    (!mouse_trigger_is_physically_down(&active.trigger)).then_some(*preset_id)
                })
                .collect::<Vec<_>>()
        };
        for preset_id in stale_ids {
            deactivate_hold_macro(preset_id);
        }
    }

    fn hold_macro_release_matches(active: &ActiveHoldMacro, binding: &HotkeyBinding) -> bool {
        active.trigger.key.eq_ignore_ascii_case(&binding.key)
    }

    fn binding_matches_any_hold_macro(binding: &HotkeyBinding) -> bool {
        let hook_state = HOOK_STATE.lock();
        if !hook_state.macros_master_enabled {
            return false;
        }

        hook_state.macro_groups.iter().any(|group| {
            group.enabled
                && macro_target_matches(group)
                && group.presets.iter().any(|preset| {
                    preset.enabled
                        && preset.trigger_mode == MacroTriggerMode::Hold
                        && !preset.pass_through_hold
                        && macro_preset_trigger_matches(preset, binding)
                })
        })
    }

    fn preset_blocks_trigger_input(preset: &MacroPreset) -> bool {
        match preset.trigger_mode {
            MacroTriggerMode::Press => !preset.pass_through_press,
            MacroTriggerMode::Hold => !preset.pass_through_hold,
            MacroTriggerMode::Release => false,
            MacroTriggerMode::WindowFocus => false,
        }
    }

    fn trigger_binding_matches(expected: &HotkeyBinding, observed: &HotkeyBinding) -> bool {
        let expected_keys = hotkey::binding_key_names(expected);
        if expected_keys.is_empty() {
            return false;
        }

        let observed_keys = hotkey::binding_key_names(observed)
            .into_iter()
            .map(|key| key.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        expected_keys
            .into_iter()
            .map(|key| key.to_ascii_lowercase())
            .all(|key| observed_keys.contains(&key))
    }

    fn remove_pending_press_trigger_key(key_name: &str) -> Option<String> {
        let mut hook_state = HOOK_STATE.lock();
        let pending = hook_state
            .pending_press_trigger_keys
            .iter()
            .find(|pending| pending.eq_ignore_ascii_case(key_name))
            .cloned()?;
        hook_state.pending_press_trigger_keys.remove(&pending);
        Some(pending)
    }

    fn consume_pending_press_trigger_keys(binding: &HotkeyBinding) -> Vec<String> {
        let combo_keys = hotkey::binding_key_names(binding);
        let mut hook_state = HOOK_STATE.lock();
        let mut consumed = Vec::new();
        for key in combo_keys {
            if let Some(pending) = hook_state
                .pending_press_trigger_keys
                .iter()
                .find(|pending| pending.eq_ignore_ascii_case(&key))
                .cloned()
            {
                hook_state.pending_press_trigger_keys.remove(&pending);
                consumed.push(pending);
            }
        }

        consumed
    }

    fn fire_pending_press_triggers(binding: &HotkeyBinding) -> bool {
        let Some(_) = remove_pending_press_trigger_key(&binding.key) else {
            return false;
        };
        let press_matches = {
            let hook_state = HOOK_STATE.lock();
            let mut press_matches: Vec<(MacroPreset, Option<String>, Vec<String>, bool, String)> =
                Vec::new();
            for group in &hook_state.macro_groups {
                if !group.enabled {
                    continue;
                }

                if !macro_target_matches(group) {
                    continue;
                }

                for preset in &group.presets {
                    if !preset.enabled
                        || preset.trigger_mode != MacroTriggerMode::Press
                        || !macro_preset_trigger_matches(preset, binding)
                    {
                        continue;
                    }

                    press_matches.push((
                        preset.clone(),
                        group.target_window_title.clone(),
                        group.extra_target_window_titles.clone(),
                        group.match_duplicate_window_titles,
                        binding.key.clone(),
                    ));
                }
            }

            press_matches
        };
        for (
            preset,
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
            trigger_key,
        ) in press_matches
        {
            let hotkey_id = MACRO_PRESET_BASE_ID + preset.id as i32;
            if !SUPPRESSED_MACRO_HOTKEYS.lock().contains(&hotkey_id) {
                let _ = play_macro_preset(
                    hotkey_id,
                    preset,
                    target_window_title,
                    extra_target_window_titles,
                    match_duplicate_window_titles,
                    trigger_key,
                );
            } else {
                STOP_REQUESTED_MACRO_PRESETS.lock().insert(preset.id);
            }
        }

        true
    }

    fn process_binding_press(binding: &HotkeyBinding, is_repeat: bool) -> Option<bool> {
        if let Some(swallow) = process_mouse_sensitivity_hotkey(binding, is_repeat) {
            return Some(swallow);
        }

        if let Some(swallow) = process_image_search_hotkey(binding, is_repeat) {
            return Some(swallow);
        }

        let master_toggle = {
            let mut hook_state = HOOK_STATE.lock();
            let matches_master_hotkey = hook_state
                .macros_master_hotkey
                .as_ref()
                .is_some_and(|hotkey| hotkey::binding_matches(hotkey, binding));
            if matches_master_hotkey {
                hook_state.macros_master_enabled = !hook_state.macros_master_enabled;
                let enabled = hook_state.macros_master_enabled;
                let status = if enabled {
                    "Enabled macros globally.".to_owned()
                } else {
                    "Disabled macros globally.".to_owned()
                };
                Some((enabled, status))
            } else {
                None
            }
        };
        if let Some((enabled, status)) = master_toggle {
            send_ui_command(UiCommand::SetMacrosMasterEnabled(enabled, status));
            send_overlay_command(OverlayCommand::SetMacrosMasterEnabled(enabled));
            return Some(true);
        }

        let is_record_hotkey = {
            let hook_state = HOOK_STATE.lock();
            hook_state.macro_groups.iter().any(|g| {
                g.presets.iter().any(|p| {
                    p.record_hotkey
                        .as_ref()
                        .is_some_and(|h| hotkey::binding_matches(h, binding))
                })
            }) || hook_state.mouse_path_presets.iter().any(|p| {
                p.record_hotkey
                    .as_ref()
                    .is_some_and(|h| hotkey::binding_matches(h, binding))
            })
        };
        if is_ui_in_foreground() && !is_record_hotkey {
            return Some(false);
        }

        let hook_state = HOOK_STATE.lock();
        let mut matched_any_window = false;
        let mut window_actions = Vec::new();
        for preset in &hook_state.window_presets {
            if !preset.enabled {
                continue;
            }

            if !window_focus_matches(
                preset.target_window_title.as_deref(),
                &preset.extra_target_window_titles,
                false,
            ) {
                continue;
            }

            if preset_trigger_matches(preset.hotkey.as_ref(), &preset.trigger_keys, binding)
                && !is_repeat
            {
                matched_any_window = true;
                if preset.animate_enabled {
                    window_actions.push(WindowHotkeyAction::Animate(preset.clone()));
                } else {
                    window_actions.push(WindowHotkeyAction::Apply(preset.clone()));
                }
            }
        }

        for preset in &hook_state.window_focus_presets {
            if !preset.enabled {
                continue;
            }

            if preset_trigger_matches(preset.hotkey.as_ref(), &preset.trigger_keys, binding)
                && !is_repeat
            {
                matched_any_window = true;
                window_actions.push(WindowHotkeyAction::Focus(preset.clone()));
            }
        }

        for layout in &hook_state.window_layouts {
            if !layout.enabled {
                continue;
            }

            if preset_trigger_matches(layout.hotkey.as_ref(), &layout.trigger_keys, binding)
                && !is_repeat
            {
                matched_any_window = true;
                window_actions.push(WindowHotkeyAction::ApplyLayout(layout.clone()));
            }
        }

        let mut pin_toggle_id = None;
        for preset in &hook_state.pin_presets {
            if !preset.enabled {
                continue;
            }

            if preset_trigger_matches(preset.hotkey.as_ref(), &preset.trigger_keys, binding)
                && !is_repeat
            {
                pin_toggle_id = Some(preset.id);
                break;
            }
        }

        if let Some(preset_id) = pin_toggle_id {
            drop(hook_state);
            let mut hook_state = HOOK_STATE.lock();
            if hook_state.active_pin_preset_id == Some(preset_id) {
                hook_state.active_pin_preset_id = None;
            } else {
                hook_state.active_pin_preset_id = Some(preset_id);
            }

            return Some(false);
        }

        if !hook_state.macros_master_enabled {
            drop(hook_state);
            for action in window_actions {
                match action {
                    WindowHotkeyAction::Apply(preset) => {
                        let _ = apply_window_preset(&preset);
                    }

                    WindowHotkeyAction::Focus(preset) => {
                        let _ = focus_window_for_preset(&preset);
                    }

                    WindowHotkeyAction::Animate(preset) => {
                        thread::spawn(move || {
                            let _ = apply_window_preset_animated(&preset);
                        });
                    }

                    WindowHotkeyAction::RestoreTitleBar(preset) => {
                        let _ = restore_window_title_bar_for_preset(&preset);
                    }

                    WindowHotkeyAction::ApplyLayout(layout) => {
                        thread::spawn(move || {
                            let _ = window_preset::apply_window_layout(&layout);
                        });
                    }
                }
            }

            return Some(false);
        }

        let mut matched_any_macro = false;
        let mut hold_matches: Vec<(
            MacroPreset,
            HotkeyBinding,
            Option<String>,
            Vec<String>,
            bool,
            String,
        )> = Vec::new();
        let mut press_matches: Vec<(MacroPreset, Option<String>, Vec<String>, bool, String)> =
            Vec::new();
        let mut matched_any_press = false;
        let mut matched_blocking_macro = false;
        for group in &hook_state.macro_groups {
            if !group.enabled {
                continue;
            }

            if !macro_target_matches(group) {
                continue;
            }

            for preset in &group.presets {
                if !preset.enabled {
                    continue;
                }

                if !macro_preset_trigger_matches(preset, &binding) {
                    continue;
                }

                if preset.trigger_mode == MacroTriggerMode::Hold {
                    matched_any_macro = true;
                    matched_blocking_macro |= preset_blocks_trigger_input(preset);
                    if !hook_state.active_hold_macros.contains_key(&preset.id) {
                        hold_matches.push((
                            preset.clone(),
                            binding.clone(),
                            group.target_window_title.clone(),
                            group.extra_target_window_titles.clone(),
                            group.match_duplicate_window_titles,
                            binding.key.clone(),
                        ));
                    }

                    continue;
                }

                if preset.trigger_mode == MacroTriggerMode::Release {
                    matched_any_macro = true;
                    continue;
                }

                matched_any_macro = true;
                matched_any_press = true;
                matched_blocking_macro |= preset_blocks_trigger_input(preset);
                if is_repeat {
                    continue;
                }

                press_matches.push((
                    preset.clone(),
                    group.target_window_title.clone(),
                    group.extra_target_window_titles.clone(),
                    group.match_duplicate_window_titles,
                    binding.key.clone(),
                ));
            }
        }

        drop(hook_state);
        if matched_any_press && matched_blocking_macro {
            increment_press_trigger_suppression(&binding.key);
        }

        for action in window_actions {
            match action {
                WindowHotkeyAction::Apply(preset) => {
                    let _ = apply_window_preset(&preset);
                }

                WindowHotkeyAction::Focus(preset) => {
                    let _ = focus_window_for_preset(&preset);
                }

                WindowHotkeyAction::Animate(preset) => {
                    thread::spawn(move || {
                        let _ = apply_window_preset_animated(&preset);
                    });
                }

                WindowHotkeyAction::RestoreTitleBar(preset) => {
                    let _ = restore_window_title_bar_for_preset(&preset);
                }

                WindowHotkeyAction::ApplyLayout(layout) => {
                    thread::spawn(move || {
                        let _ = window_preset::apply_window_layout(&layout);
                    });
                }
            }
        }

        for (
            preset,
            trigger,
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
            trigger_key,
        ) in hold_matches
        {
            activate_hold_macro(
                preset,
                trigger,
                target_window_title,
                extra_target_window_titles,
                match_duplicate_window_titles,
                trigger_key,
            );
        }

        for (
            preset,
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
            trigger_key,
        ) in press_matches
        {
            let hotkey_id = MACRO_PRESET_BASE_ID + preset.id as i32;
            if !SUPPRESSED_MACRO_HOTKEYS.lock().contains(&hotkey_id) {
                let _ = play_macro_preset(
                    hotkey_id,
                    preset,
                    target_window_title,
                    extra_target_window_titles,
                    match_duplicate_window_titles,
                    trigger_key,
                );
            } else {
                STOP_REQUESTED_MACRO_PRESETS.lock().insert(preset.id);
            }
        }

        if matched_any_macro {
            return Some(matched_blocking_macro);
        }

        Some(matched_any_window)
    }

    fn process_binding_release(binding: &HotkeyBinding) -> bool {
        let suppressed_press_release = is_press_trigger_suppressed(&binding.key);
        if suppressed_press_release {
            decrement_press_trigger_suppression(&binding.key);
        }

        let mut release_matches: Vec<(MacroPreset, Option<String>, Vec<String>, bool)> = Vec::new();
        let preset_ids = {
            let hook_state = HOOK_STATE.lock();
            for group in &hook_state.macro_groups {
                if !group.enabled {
                    continue;
                }

                if !macro_target_matches(group) {
                    continue;
                }

                for preset in &group.presets {
                    if !preset.enabled {
                        continue;
                    }

                    if preset.trigger_mode != MacroTriggerMode::Release {
                        continue;
                    }

                    if !macro_preset_trigger_matches(preset, binding) {
                        continue;
                    }

                    release_matches.push((
                        preset.clone(),
                        group.target_window_title.clone(),
                        group.extra_target_window_titles.clone(),
                        group.match_duplicate_window_titles,
                    ));
                }
            }

            hook_state
                .active_hold_macros
                .iter()
                .filter(|(_, active)| hold_macro_release_matches(active, binding))
                .map(|(preset_id, _)| *preset_id)
                .collect::<Vec<_>>()
        };
        for (
            preset,
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
        ) in release_matches
        {
            if !release_trigger_ready(
                &preset.release_wait_key,
                preset.release_requires_all_inputs_released,
                &binding.key,
            ) {
                continue;
            }

            let hotkey_id = MACRO_PRESET_BASE_ID + preset.id as i32;
            if STOP_REQUESTED_MACRO_PRESETS.lock().contains(&preset.id) {
                continue;
            }

            let _ = play_macro_preset(
                hotkey_id,
                preset,
                target_window_title,
                extra_target_window_titles,
                match_duplicate_window_titles,
                binding.key.clone(),
            );
        }

        let had_hold_matches = !preset_ids.is_empty();
        if had_hold_matches {
            for preset_id in preset_ids {
                deactivate_hold_macro(preset_id);
            }
        }

        // If the key press was already suppressed as a hotkey trigger, also

        // swallow the matching key-up so games and apps do not see a leaked tap.

        if suppressed_press_release {
            return true;
        }

        // Release triggers should not swallow the key-up event. They are meant to

        // observe the release and run actions, not to lock the source key.

        let _ = had_hold_matches;
        false
    }

    fn increment_press_trigger_suppression(key_name: &str) {
        let mut hook_state = HOOK_STATE.lock();
        *hook_state
            .press_trigger_suppression
            .entry(key_name.to_owned())
            .or_insert(0) += 1;
    }

    fn decrement_press_trigger_suppression(key_name: &str) {
        let mut hook_state = HOOK_STATE.lock();
        if let Some(count) = hook_state.press_trigger_suppression.get_mut(key_name) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                hook_state.press_trigger_suppression.remove(key_name);
            }
        }
    }

    fn is_press_trigger_suppressed(key_name: &str) -> bool {
        HOOK_STATE
            .lock()
            .press_trigger_suppression
            .get(key_name)
            .copied()
            .unwrap_or_default()
            > 0
    }

    fn is_locked_input(key_name: &str) -> bool {
        HOOK_STATE
            .lock()
            .locked_inputs
            .get(key_name)
            .copied()
            .unwrap_or_default()
            > 0
    }

    fn current_mouse_speed() -> Result<u32> {
        let mut speed = 10u32;
        unsafe {
            SystemParametersInfoW(
                SPI_GETMOUSESPEED,
                0,
                Some((&mut speed as *mut u32).cast()),
                Default::default(),
            )
            .context("Failed to read mouse speed")?;
        }

        Ok(speed.clamp(1, 20))
    }

    fn current_system_volume_percent() -> Option<i32> {
        let need_uninit = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
        let result = unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).ok()?;
            let volume = endpoint.GetMasterVolumeLevelScalar().ok()?;
            Some((volume.clamp(0.0, 1.0) * 100.0).round() as i32)
        };
        if need_uninit {
            unsafe {
                CoUninitialize();
            }
        }

        result
    }

    fn set_mouse_speed(speed: u32) -> Result<()> {
        let speed = speed.clamp(1, 20);
        std::thread::spawn(move || unsafe {
            let _ = SystemParametersInfoW(
                SPI_SETMOUSESPEED,
                0,
                Some(speed as usize as *mut c_void),
                Default::default(),
            );
        });
        Ok(())
    }

    fn apply_mouse_sensitivity_preset(preset: &MouseSensitivityPreset) -> Result<()> {
        let mut hook_state = HOOK_STATE.lock();
        if hook_state.mouse_sensitivity_restore_speed.is_none() {
            hook_state.mouse_sensitivity_restore_speed = Some(current_mouse_speed()?);
        }

        hook_state.active_mouse_sensitivity_preset_id = Some(preset.id);
        drop(hook_state);
        set_mouse_speed(preset.speed)?;
        Ok(())
    }

    fn restore_mouse_sensitivity() -> Result<()> {
        let restore_speed = {
            let mut hook_state = HOOK_STATE.lock();
            let restore_speed = hook_state.mouse_sensitivity_restore_speed.take();
            hook_state.active_mouse_sensitivity_preset_id = None;
            restore_speed
        };
        if let Some(speed) = restore_speed {
            set_mouse_speed(speed)?;
        }

        Ok(())
    }

    fn restore_mouse_sensitivity_on_exit() -> Result<()> {
        let (enabled, speed) = {
            let hook_state = HOOK_STATE.lock();
            (
                hook_state.mouse_sensitivity_restore_on_exit,
                hook_state.mouse_sensitivity_exit_restore_speed,
            )
        };
        if enabled {
            set_mouse_speed(speed)?;
        }

        Ok(())
    }

    fn toggle_mouse_sensitivity_preset(preset: &MouseSensitivityPreset) -> Result<()> {
        let should_restore = {
            let hook_state = HOOK_STATE.lock();
            hook_state.active_mouse_sensitivity_preset_id == Some(preset.id)
        };
        if should_restore {
            restore_mouse_sensitivity()
        } else {
            apply_mouse_sensitivity_preset(preset)
        }
    }

    fn parse_mouse_sensitivity_preset_id(key: &str) -> Option<u32> {
        key.trim().parse::<u32>().ok()
    }

    fn update_modifier_state(vk: u32, is_key_down: bool) {
        let mut hook_state = HOOK_STATE.lock();
        match vk {
            0x10 | 0xA0 | 0xA1 => hook_state.shift = is_key_down,
            0x11 | 0xA2 | 0xA3 => hook_state.ctrl = is_key_down,
            0x12 | 0xA4 | 0xA5 => hook_state.alt = is_key_down,
            0x5B | 0x5C => hook_state.win = is_key_down,
            _ => {}
        }
    }

    fn update_held_key(key_name: &str, is_key_down: bool, is_key_up: bool) {
        let mut hook_state = HOOK_STATE.lock();
        if is_key_down {
            hook_state.held_inputs.insert(key_name.to_owned());
            let ignored_for_stop = hook_state
                .stop_ignore_keys
                .values()
                .any(|ignored| ignored.eq_ignore_ascii_case(key_name));
            if !ignored_for_stop {
                hook_state.pressed_inputs.insert(key_name.to_owned());
            }
        } else if is_key_up {
            hook_state.held_inputs.remove(key_name);
            hook_state
                .stop_ignore_keys
                .retain(|_, ignored| !ignored.eq_ignore_ascii_case(key_name));
        }
    }

    fn update_held_mouse_button(message: u32, mouse_data: u16) {
        let key_name = mouse_binding_name_from_message(message, mouse_data);
        let Some(key_name) = key_name else {
            return;
        };
        let is_down = matches!(
            message,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        );
        let mut hook_state = HOOK_STATE.lock();
        if is_down {
            hook_state.held_mouse_buttons.insert(key_name.to_owned());
        } else {
            hook_state.held_mouse_buttons.remove(key_name);
        }
    }

    fn mouse_binding_name_from_message(message: u32, mouse_data: u16) -> Option<&'static str> {
        match message {
            WM_LBUTTONDOWN | WM_LBUTTONUP => Some("MouseLeft"),
            WM_RBUTTONDOWN | WM_RBUTTONUP => Some("MouseRight"),
            WM_MBUTTONDOWN | windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP => {
                Some("MouseMiddle")
            }

            WM_XBUTTONDOWN | WM_XBUTTONUP if (mouse_data & XBUTTON2_DATA) != 0 => Some("MouseX2"),
            WM_XBUTTONDOWN | WM_XBUTTONUP if (mouse_data & XBUTTON1_DATA) != 0 => Some("MouseX1"),
            WM_MOUSEWHEEL => {
                if (mouse_data as i16) > 0 {
                    Some("MouseWheelUp")
                } else {
                    Some("MouseWheelDown")
                }
            }

            _ => None,
        }
    }

    fn suppress_next_mouse_trigger(key_name: &str) {
        let mut guard = SYNTHETIC_MOUSE_TRIGGER_SUPPRESSION.lock();
        *guard.entry(key_name.to_owned()).or_insert(0) += 1;
    }

    fn swallow_mouse_trigger_until_release(key_name: &str) {
        SWALLOWED_MOUSE_TRIGGER_RELEASES
            .lock()
            .insert(key_name.to_owned());
    }

    fn consume_swallowed_mouse_trigger_release(key_name: &str) -> bool {
        SWALLOWED_MOUSE_TRIGGER_RELEASES.lock().remove(key_name)
    }

    fn consume_suppressed_mouse_trigger(key_name: &str) -> bool {
        let mut guard = SYNTHETIC_MOUSE_TRIGGER_SUPPRESSION.lock();
        let Some(count) = guard.get_mut(key_name) else {
            return false;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            guard.remove(key_name);
        }

        true
    }

    fn deactivate_all_hold_macros() {
        let preset_ids: Vec<u32> = {
            let hook_state = HOOK_STATE.lock();
            hook_state.active_hold_macros.keys().cloned().collect()
        };
        for preset_id in preset_ids {
            deactivate_hold_macro(preset_id);
        }
    }

    fn reset_all_input_and_locks() {
        deactivate_all_hold_macros();
        let mut hook_state = HOOK_STATE.lock();
        hook_state.mouse_move_locks = MouseMoveLockCounts::default();
        hook_state.mouse_move_lock_anchor = None;
        hook_state.held_inputs.clear();
        hook_state.locked_inputs.clear();
        hook_state.held_mouse_buttons.clear();
        hook_state.ctrl = false;
        hook_state.alt = false;
        hook_state.shift = false;
        hook_state.win = false;
        hook_state.keyboard_arrow_mouse_enabled = false;
    }

    fn clear_transient_input_state() {
        let mut hook_state = HOOK_STATE.lock();
        hook_state.ctrl = false;
        hook_state.alt = false;
        hook_state.shift = false;
        hook_state.win = false;
        hook_state.held_inputs.clear();
        hook_state.held_mouse_buttons.clear();
    }

    fn cancel_pending_tray_toggle() {
        let mut hook_state = HOOK_STATE.lock();
        hook_state.pending_tray_toggle = None;
    }

    fn stop_key_triggered(preset_id: u32, key_name: &str) -> bool {
        let mut hook_state = HOOK_STATE.lock();
        if hook_state
            .stop_ignore_keys
            .get(&preset_id)
            .is_some_and(|ignored| ignored.eq_ignore_ascii_case(key_name))
        {
            return false;
        }

        if let Some(pressed) = hook_state
            .pressed_inputs
            .iter()
            .find(|pressed| pressed.eq_ignore_ascii_case(key_name))
            .cloned()
        {
            hook_state.pressed_inputs.remove(&pressed);
            return true;
        }

        hook_state
            .held_inputs
            .iter()
            .any(|held| held.eq_ignore_ascii_case(key_name))
    }

    fn is_repeat_key(key_name: &str) -> bool {
        HOOK_STATE.lock().held_inputs.contains(key_name)
    }

    fn is_mouse_locked() -> bool {
        HOOK_STATE.lock().mouse_move_locks.any()
    }

    fn handle_locked_mouse_move(point: POINT) -> bool {
        let maybe_allowed = {
            let mut hook_state = HOOK_STATE.lock();
            if !hook_state.mouse_move_locks.any() {
                return false;
            }

            let anchor = hook_state.mouse_move_lock_anchor.unwrap_or(point);
            let mut allowed = anchor;
            if point.x < anchor.x && hook_state.mouse_move_locks.left > 0 {
                allowed.x = anchor.x;
            } else if point.x > anchor.x && hook_state.mouse_move_locks.right > 0 {
                allowed.x = anchor.x;
            } else {
                allowed.x = point.x;
            }

            if point.y < anchor.y && hook_state.mouse_move_locks.up > 0 {
                allowed.y = anchor.y;
            } else if point.y > anchor.y && hook_state.mouse_move_locks.down > 0 {
                allowed.y = anchor.y;
            } else {
                allowed.y = point.y;
            }

            hook_state.mouse_move_lock_anchor = Some(allowed);
            Some(allowed)
        };
        let Some(allowed) = maybe_allowed else {
            return false;
        };
        if allowed.x == point.x && allowed.y == point.y {
            false
        } else {
            unsafe {
                let _ = SetCursorPos(allowed.x, allowed.y);
            }
            true
        }
    }

    fn is_vision_capture_mouse_blocked() -> bool {
        HOOK_STATE.lock().vision_capture_mouse_blocked
    }

    fn clear_stuck_mouse_lock() {
        let mut hook_state = HOOK_STATE.lock();
        if !hook_state.mouse_move_locks.any() {
            return;
        }

        hook_state.mouse_move_locks = MouseMoveLockCounts::default();
        hook_state.mouse_move_lock_anchor = None;
        for active in hook_state.active_hold_macros.values_mut() {
            active.locked_mouse_masks.clear();
        }
    }

    fn is_keyboard_arrow_mouse_key(key_name: &str) -> bool {
        matches!(key_name, "Left" | "Right" | "Up" | "Down")
    }

    fn keyboard_arrow_mouse_delta() -> Option<(i32, i32)> {
        let hook_state = HOOK_STATE.lock();
        if !hook_state.keyboard_arrow_mouse_enabled {
            return None;
        }

        let step = hook_state.keyboard_arrow_mouse_step_px as i32;
        let mut dx = 0i32;
        let mut dy = 0i32;
        if hook_state.held_inputs.contains("Left") {
            dx -= step;
        }

        if hook_state.held_inputs.contains("Right") {
            dx += step;
        }

        if hook_state.held_inputs.contains("Up") {
            dy -= step;
        }

        if hook_state.held_inputs.contains("Down") {
            dy += step;
        }

        if dx == 0 && dy == 0 {
            None
        } else {
            Some((dx, dy))
        }
    }

    fn keyboard_arrow_mouse_should_swallow(key_name: &str) -> bool {
        let hook_state = HOOK_STATE.lock();
        hook_state.keyboard_arrow_mouse_enabled && is_keyboard_arrow_mouse_key(key_name)
    }

    fn keyboard_arrow_mouse_is_active() -> bool {
        let hook_state = HOOK_STATE.lock();
        hook_state.keyboard_arrow_mouse_enabled
            && hook_state
                .held_inputs
                .iter()
                .any(|key_name| is_keyboard_arrow_mouse_key(key_name))
    }

    fn quick_key_display_label(key_name: &str) -> String {
        match key_name {
            "MouseLeft" => "LMB".to_owned(),
            "MouseRight" => "RMB".to_owned(),
            "MouseMiddle" => "MMB".to_owned(),
            "MouseX1" => "Mouse 4".to_owned(),
            "MouseX2" => "Mouse 5".to_owned(),
            "MouseWheelUp" => "Wheel Up".to_owned(),
            "MouseWheelDown" => "Wheel Down".to_owned(),
            _ => key_name.to_owned(),
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum QuickKeyDisplayPalette {
        Keyboard,
        Mouse,
        Wheel,
    }

    #[derive(Clone)]
    struct QuickKeyDisplayTextRun {
        text: String,
        rect: RECT,
        color: COLORREF,
    }

    fn quick_key_display_parts(label: &str) -> Vec<String> {
        label
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    fn quick_key_display_entry_base_and_count(entry: &str) -> (String, u32) {
        if let Some((base, suffix)) = entry.rsplit_once(" (x")
            && let Some(value) = suffix.strip_suffix(')')
            && let Ok(count) = value.parse::<u32>()
            && count > 1
        {
            return (base.to_owned(), count);
        }
        (entry.to_owned(), 1)
    }

    fn quick_key_display_format_entry(text: &str, count: u32) -> String {
        if count > 1 {
            format!("{text} (x{count})")
        } else {
            text.to_owned()
        }
    }

    fn quick_key_display_should_replace(previous: &str, next: &str) -> bool {
        let previous_parts = quick_key_display_parts(previous);
        let next_parts = quick_key_display_parts(next);
        !previous_parts.is_empty()
            && next_parts.len() >= previous_parts.len()
            && previous_parts
                .iter()
                .zip(next_parts.iter())
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
    }

    fn quick_key_display_text_for_event(
        key_name: &str,
        is_key_down: bool,
        is_key_up: bool,
    ) -> Option<String> {
        if !is_key_down && !is_key_up {
            return None;
        }

        let mut combo_keys = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .held_inputs
                .iter()
                .cloned()
                .chain(hook_state.held_mouse_buttons.iter().cloned())
                .collect::<Vec<_>>()
        };
        if is_key_down {
            combo_keys.push(key_name.to_owned());
        } else if is_key_up {
            combo_keys.retain(|existing| !existing.eq_ignore_ascii_case(key_name));
        }
        combo_keys.retain(|key| !key.trim().is_empty());
        combo_keys.sort_by(|a, b| {
            let rank_a = hotkey_binding_rank(a);
            let rank_b = hotkey_binding_rank(b);
            rank_a
                .cmp(&rank_b)
                .then_with(|| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()))
        });
        combo_keys.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        if combo_keys.is_empty() {
            return None;
        }

        let key = combo_keys
            .iter()
            .rev()
            .find(|key| !hotkey::is_modifier_key_name(key))
            .cloned()
            .or_else(|| combo_keys.last().cloned())
            .unwrap_or_default();
        let binding = HotkeyBinding {
            ctrl: combo_keys.iter().any(|key| key.eq_ignore_ascii_case("Ctrl")),
            alt: combo_keys.iter().any(|key| key.eq_ignore_ascii_case("Alt")),
            shift: combo_keys.iter().any(|key| key.eq_ignore_ascii_case("Shift")),
            win: combo_keys.iter().any(|key| key.eq_ignore_ascii_case("Win")),
            key,
            combo_keys,
        };
        let labels = hotkey::binding_key_names(&binding)
            .into_iter()
            .map(|key| quick_key_display_label(&key))
            .collect::<Vec<_>>();
        if labels.is_empty() {
            None
        } else {
            Some(labels.join("+"))
        }
    }

    fn push_quick_key_display_entry(runtime: &mut Runtime, text: String) {
        const QUICK_KEY_DISPLAY_MAX_ENTRIES: usize = 4;

        let should_append_duplicate = runtime.quick_key_display_hide_at.is_some();
        match runtime.quick_key_display_entries.last_mut() {
            Some(last) => {
                let (last_base, last_count) = quick_key_display_entry_base_and_count(last);
                if last_base.eq_ignore_ascii_case(&text) {
                    *last = quick_key_display_format_entry(&text, last_count.saturating_add(1));
                } else if !should_append_duplicate
                    && quick_key_display_should_replace(&last_base, &text)
                {
                    *last = quick_key_display_format_entry(&text, 1);
                } else {
                    runtime
                        .quick_key_display_entries
                        .push(quick_key_display_format_entry(&text, 1));
                    if runtime.quick_key_display_entries.len() > QUICK_KEY_DISPLAY_MAX_ENTRIES {
                        let drop_count = runtime.quick_key_display_entries.len()
                            - QUICK_KEY_DISPLAY_MAX_ENTRIES;
                        runtime.quick_key_display_entries.drain(0..drop_count);
                    }
                }
            }
            _ => {
                runtime
                    .quick_key_display_entries
                    .push(quick_key_display_format_entry(&text, 1));
                if runtime.quick_key_display_entries.len() > QUICK_KEY_DISPLAY_MAX_ENTRIES {
                    let drop_count =
                        runtime.quick_key_display_entries.len() - QUICK_KEY_DISPLAY_MAX_ENTRIES;
                    runtime.quick_key_display_entries.drain(0..drop_count);
                }
            }
        }
    }

    fn quick_key_display_palette(label: &str) -> QuickKeyDisplayPalette {
        let lower = label.to_ascii_lowercase();
        if lower.contains("wheel") {
            QuickKeyDisplayPalette::Wheel
        } else if matches!(
            lower.as_str(),
            "lmb" | "rmb" | "mmb" | "mouse 4" | "mouse 5"
        ) || lower.contains("mouse")
        {
            QuickKeyDisplayPalette::Mouse
        } else {
            QuickKeyDisplayPalette::Keyboard
        }
    }

    fn quick_key_display_keycap_width(label: &str, font_size: f32, cap_height: i32) -> i32 {
        let length = label.chars().count().max(1) as f32;
        let char_width = if length <= 2.0 { 0.84 } else { 0.66 };
        ((length * font_size * char_width) + (cap_height as f32 * 0.74))
            .round()
            .max((cap_height as f32 * 0.92).round()) as i32
    }

    fn quick_key_display_layout_size(entries: &[String], font_size: f32) -> (i32, i32) {
        let cap_height = (font_size * 1.12 + 18.0).round().max(44.0) as i32;
        let outer_pad_x = (font_size * 0.46).round().max(16.0) as i32;
        let outer_pad_y = (font_size * 0.34).round().max(10.0) as i32;
        let combo_gap = (font_size * 0.14).round().max(4.0) as i32;
        let plus_width = (font_size * 0.48).round().max(10.0) as i32;
        let entry_gap = (font_size * 0.52).round().max(18.0) as i32;

        let mut width = outer_pad_x * 2;
        for (entry_index, entry) in entries.iter().enumerate() {
            let parts = quick_key_display_parts(entry);
            for (part_index, part) in parts.iter().enumerate() {
                width += quick_key_display_keycap_width(part, font_size, cap_height);
                if part_index + 1 < parts.len() {
                    width += combo_gap * 2 + plus_width;
                }
            }
            if entry_index + 1 < entries.len() {
                width += entry_gap;
            }
        }

        let height = cap_height + outer_pad_y * 2 + 6;
        (width.max(cap_height), height.max(cap_height))
    }

    fn quick_key_display_colorref(r: u8, g: u8, b: u8) -> COLORREF {
        COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
    }

    fn quick_key_display_alpha(color: [u8; 4], alpha_scale: f32) -> [u8; 4] {
        [
            color[0],
            color[1],
            color[2],
            ((color[3] as f32) * alpha_scale.clamp(0.0, 1.0))
                .round()
                .clamp(0.0, 255.0) as u8,
        ]
    }

    fn quick_key_display_palette_colors(
        palette: QuickKeyDisplayPalette,
    ) -> ([u8; 4], [u8; 4], [u8; 4], [u8; 4]) {
        match palette {
            QuickKeyDisplayPalette::Keyboard => (
                [24, 33, 44, 244],
                [34, 47, 60, 214],
                [112, 235, 192, 196],
                [241, 255, 248, 255],
            ),
            QuickKeyDisplayPalette::Mouse => (
                [33, 30, 24, 244],
                [56, 47, 35, 220],
                [255, 206, 120, 204],
                [255, 247, 230, 255],
            ),
            QuickKeyDisplayPalette::Wheel => (
                [22, 29, 40, 244],
                [32, 44, 62, 220],
                [132, 204, 255, 204],
                [240, 248, 255, 255],
            ),
        }
    }

    fn update_quick_key_display_key(key_name: &str, is_key_down: bool, is_key_up: bool) {
        let display = if is_key_down {
            quick_key_display_text_for_event(key_name, is_key_down, false)
        } else if is_key_up {
            None
        } else {
            quick_key_display_text_for_event(key_name, is_key_down, is_key_up)
        };
        send_overlay_command(OverlayCommand::ShowQuickKeyDisplay(display));
    }

    fn process_screen_draw_hotkey(binding: &HotkeyBinding, is_repeat: bool) -> bool {
        if is_repeat {
            return false;
        }
        let (matches_trigger, pass_trigger_through) = {
            let state = SCREEN_DRAW_STATE.lock();
            (
                state.enabled
                    && !state.capturing_region
                    && state
                        .trigger
                        .as_ref()
                        .is_some_and(|trigger| hotkey::binding_matches(trigger, binding)),
                state.pass_trigger_through,
            )
        };
        if !matches_trigger {
            return false;
        }

        let hwnd_raw = SCREEN_DRAW_HWND.load(Ordering::Relaxed);
        if hwnd_raw == 0 {
            return true;
        }
        unsafe {
            let hwnd = HWND(hwnd_raw as *mut c_void);
            toggle_screen_draw_overlay(hwnd);
        }
        !pass_trigger_through
    }

    fn apply_keyboard_arrow_mouse_movement() {
        if let Some((dx, dy)) = keyboard_arrow_mouse_delta() {
            let _ = send_mouse_move_relative(dx, dy);
        }
    }

    unsafe fn runtime_mut(hwnd: HWND) -> Option<&'static mut Runtime> {
        let ptr = GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA.0));
        if ptr == 0 {
            None
        } else {
            Some(&mut *(ptr as *mut Runtime))
        }
    }

    unsafe fn process_pending_commands(hwnd: HWND, runtime: &mut Runtime) {
        while let Ok(command) = runtime.rx.try_recv() {
            match command {
                OverlayCommand::Update(style) => {
                    runtime.style = style.clone();
                    HOOK_STATE.lock().current_style = style;
                    let _ = refresh_overlay(runtime);
                }

                OverlayCommand::UpdateProfiles(profiles) => {
                    HOOK_STATE.lock().profiles = profiles;
                    let _ = refresh_overlay(runtime);
                }

                OverlayCommand::UpdateCrosshairProfile { index, profile } => {
                    let mut hook_state = HOOK_STATE.lock();
                    if let Some(existing) = hook_state.profiles.get_mut(index) {
                        *existing = profile;
                    } else {
                        hook_state.profiles.push(profile);
                    }

                    drop(hook_state);
                    let _ = refresh_overlay(runtime);
                }

                OverlayCommand::UpdateWindowPresets(presets) => {
                    runtime.window_presets = presets;
                    let _ = sync_window_hotkeys(hwnd, runtime);
                }

                OverlayCommand::UpdateWindowLayouts(layouts) => {
                    runtime.window_layouts = layouts;
                    let _ = sync_window_hotkeys(hwnd, runtime);
                }

                OverlayCommand::ApplyWindowLayout(layout) => {
                    let _ = window_preset::apply_window_layout(&layout);
                }

                OverlayCommand::UpdateWindowFocusPresets(presets) => {
                    runtime.window_focus_presets = presets;
                    let _ = sync_window_hotkeys(hwnd, runtime);
                }

                OverlayCommand::UpdateWindowExpandControls(controls) => {
                    HOOK_STATE.lock().window_expand_controls = controls;
                }

                OverlayCommand::UpdatePinPresets(presets) => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.pin_presets = presets.clone();
                    runtime.pin_presets = presets;
                    if let Some(active_id) = hook_state.active_pin_preset_id
                        && !hook_state
                            .pin_presets
                            .iter()
                            .any(|preset| preset.id == active_id)
                    {
                        hook_state.active_pin_preset_id = None;
                    }
                }

                OverlayCommand::UpdateMousePathPresets(presets) => {
                    HOOK_STATE.lock().mouse_path_presets = presets.clone();
                    runtime.mouse_path_presets = presets;
                }

                OverlayCommand::PreviewMousePath(preview) => {
                    let mut preview_guard = MOUSE_PATH_PREVIEW.lock();
                    *preview_guard = preview.map(|(_, events, playback_from_ms)| MousePathPreviewSession {
                        points: events
                            .iter()
                            .filter(|event| matches!(event.kind, MousePathEventKind::Move))
                            .map(|event| POINT {
                                x: event.x,
                                y: event.y,
                            })
                            .collect(),
                        events,
                        playback_started_at: Some(Instant::now()),
                        playback_from_ms: playback_from_ms.unwrap_or(0),
                        playback_marker: None,
                        dirty: true,
                    });
                    drop(preview_guard);
                    let _ = refresh_mouse_record_trail(runtime);
                }

                OverlayCommand::UpdateMouseSensitivityPresets(presets) => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.mouse_sensitivity_presets = presets.clone();
                    if let Some(active_id) = hook_state.active_mouse_sensitivity_preset_id
                        && !hook_state
                            .mouse_sensitivity_presets
                            .iter()
                            .any(|preset| preset.id == active_id)
                    {
                        hook_state.active_mouse_sensitivity_preset_id = None;
                        hook_state.mouse_sensitivity_restore_speed = None;
                    }
                }

                OverlayCommand::UpdateMouseSensitivitySettings {
                    restore_on_exit,
                    restore_speed,
                } => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.mouse_sensitivity_restore_on_exit = restore_on_exit;
                    hook_state.mouse_sensitivity_exit_restore_speed = restore_speed.clamp(1, 20);
                }

                OverlayCommand::UpdateKeyboardArrowMouseSettings { enabled, step_px } => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.keyboard_arrow_mouse_enabled = enabled;
                    hook_state.keyboard_arrow_mouse_step_px = step_px.clamp(1, 100) as u32;
                }

                OverlayCommand::UpdateMacroDelays {
                    mouse_click_delay_ms,
                    keyboard_key_press_delay_ms,
                } => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.macro_mouse_click_delay_ms = mouse_click_delay_ms;
                    hook_state.macro_keyboard_key_press_delay_ms = keyboard_key_press_delay_ms;
                }

                OverlayCommand::UpdateVisionPresets(presets) => {
                    {
                        let mut hook_state = HOOK_STATE.lock();
                        hook_state.vision_presets = presets;
                        let valid_ids: HashSet<u32> = hook_state
                            .vision_presets
                            .iter()
                            .map(|preset| preset.id)
                            .collect();
                        hook_state
                            .vision_following_presets
                            .retain(|preset_id| valid_ids.contains(preset_id));
                    }

                    let _ = refresh_search_area_overlay(runtime);
                }

                OverlayCommand::UpdateAudioSensePresets(presets) => {
                    HOOK_STATE.lock().audio_sense_presets = presets;
                }

                OverlayCommand::UpdateGeometryPresets(presets) => {
                    HOOK_STATE.lock().geometry_presets = presets;
                    let _ = refresh_search_area_overlay(runtime);
                }

                OverlayCommand::PreviewGeometrySpec(spec) => {
                    HOOK_STATE.lock().preview_geometry_spec = spec;
                    let _ = refresh_search_area_overlay(runtime);
                }

                OverlayCommand::PreviewGeometryPreset(preset_id) => {
                    HOOK_STATE.lock().preview_geometry_preset_id = preset_id;
                    let _ = refresh_search_area_overlay(runtime);
                }


                OverlayCommand::RefreshSearchAreaOverlay => {
                    SEARCH_AREA_OVERLAY_REFRESH_PENDING.store(false, Ordering::Release);
                    let _ = refresh_search_area_overlay(runtime);
                }

                OverlayCommand::InvalidateVisionWaits(preset_ids) => {
                    let mut guard = IMAGE_SEARCH_WAIT_GENERATIONS.lock();
                    for preset_id in preset_ids {
                        let generation = guard.entry(preset_id).or_insert(0);
                        *generation = generation.saturating_add(1);
                    }
                }

                OverlayCommand::ApplyMouseSensitivityPreset(preset_id) => {
                    // Tách riêng để drop lock NGAY sau khi lấy dữ liệu, tránh deadlock

                    let preset_opt = {
                        HOOK_STATE
                            .lock()
                            .mouse_sensitivity_presets
                            .iter()
                            .find(|preset| preset.id == preset_id)
                            .cloned()
                    };
                    if let Some(preset) = preset_opt {
                        let _ = apply_mouse_sensitivity_preset(&preset);
                    }
                }

                OverlayCommand::RestoreMouseSensitivity => {
                    let _ = restore_mouse_sensitivity();
                }

                OverlayCommand::UpdateHudPresets(presets) => {
                    HOOK_STATE.lock().hud_presets = presets;
                }

                OverlayCommand::UpdateCommandPresets(presets) => {
                    HOOK_STATE.lock().command_presets = presets;
                }

                OverlayCommand::UpdateGroqSettings(settings) => {
                    HOOK_STATE.lock().groq_settings = settings;
                }

                OverlayCommand::PreviewHudPreset(presets) => {
                    *HUD_PREVIEW_DISPLAY.lock() = presets
                        .into_iter()
                        .next()
                        .map(toolbox_preview_display_from_preset);
                    let _ = refresh_hud(runtime);
                }

                OverlayCommand::UpdateOcrPresets(presets) => {
                    HOOK_STATE.lock().ocr_presets = presets;
                }

                OverlayCommand::UpdateMacroPresets(presets) => {
                    let previous_enabled: HashMap<u32, bool> = runtime
                        .macro_groups
                        .iter()
                        .flat_map(|group| {
                            group.presets.iter().map(|preset| {
                                (preset.id, group.enabled && preset.enabled)
                            })
                        })
                        .collect();
                    let next_enabled: HashMap<u32, bool> = presets
                        .iter()
                        .flat_map(|group| {
                            group.presets.iter().map(|preset| {
                                (preset.id, group.enabled && preset.enabled)
                            })
                        })
                        .collect();
                    let presets_to_stop: Vec<u32> = previous_enabled
                        .iter()
                        .filter_map(|(preset_id, was_enabled)| {
                            if *was_enabled && !next_enabled.get(preset_id).copied().unwrap_or(false)
                            {
                                Some(*preset_id)
                            } else {
                                None
                            }
                        })
                        .collect();
                    runtime.macro_groups = presets;
                    let _ = sync_macro_hotkeys(hwnd, runtime);
                    for preset_id in presets_to_stop {
                        STOP_REQUESTED_MACRO_PRESETS.lock().insert(preset_id);
                        deactivate_hold_macro(preset_id);
                    }
                    for (&preset_id, &is_enabled) in &next_enabled {
                        if is_enabled {
                            STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset_id);
                        }
                    }
                }

                OverlayCommand::UpdateAudioSettings(settings) => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.sound_presets = settings.presets.clone();
                    hook_state.video_presets = settings.video_presets.clone();
                    runtime.audio_settings = settings;
                }

                OverlayCommand::PlayVideoPreset(preset_id) => {
                    let _ = play_video_preset_by_id(preset_id);
                }

                OverlayCommand::PlayVideoPresetFrom(preset_id, start_ms) => {
                    let _ = play_video_preset_by_id_from(preset_id, start_ms);
                }

                OverlayCommand::StopVideoPlayback => {
                    stop_active_video_preset_playback();
                }

                OverlayCommand::SetMacrosMasterEnabled(enabled) => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.macros_master_enabled = enabled;
                    if !enabled {
                        hook_state.locked_inputs.clear();
                        hook_state.press_trigger_suppression.clear();
                        hook_state.active_hold_macros.clear();
                    }

                    drop(hook_state);
                    let _ = update_tray_icon(hwnd, enabled);
                }

                OverlayCommand::SetWindowsKeyLocked(locked) => {
                    HOOK_STATE.lock().windows_key_locked = locked;
                }

                OverlayCommand::SetNativeFocusHighlightEnabled(enabled) => {
                    runtime.native_focus_highlight_enabled = enabled;
                    if enabled {
                        update_native_focus_highlight(runtime, GetForegroundWindow());
                    } else {
                        clear_native_focus_highlight(runtime);
                    }
                }

                OverlayCommand::SetFocusHighlightConfig { color, rainbow } => {
                    runtime.focus_highlight_color = color;
                    runtime.focus_highlight_rainbow = rainbow;
                    if let Some(target) = runtime.active_focus_highlight_hwnd {
                        let _ = paint_focus_highlight_overlay(runtime, target);
                    }
                }

                OverlayCommand::UpdateQuickKeyDisplayConfig {
                    enabled,
                    center_x,
                    center_y,
                    size,
                } => {
                    runtime.quick_key_display_enabled = enabled;
                    runtime.quick_key_display_center_x = center_x;
                    runtime.quick_key_display_center_y = center_y;
                    runtime.quick_key_display_size = size.clamp(18.0, 96.0);
                    if !enabled {
                        runtime.quick_key_display_entries.clear();
                        runtime.quick_key_display_hide_at = None;
                    }
                    let _ = refresh_quick_key_display(runtime);
                }

                OverlayCommand::ShowQuickKeyDisplay(text) => {
                    match text.and_then(|value| {
                        let trimmed = value.trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_owned())
                        }
                    }) {
                        Some(text) => {
                            push_quick_key_display_entry(runtime, text);
                            runtime.quick_key_display_hide_at = None;
                        }
                        None => {
                            if !runtime.quick_key_display_entries.is_empty() {
                                runtime.quick_key_display_hide_at =
                                    Some(Instant::now() + QUICK_KEY_DISPLAY_HIDE_DELAY);
                            }
                        }
                    }
                    let _ = refresh_quick_key_display(runtime);
                }

                OverlayCommand::UpdateScreenDrawConfig {
                    enabled,
                    trigger,
                    pass_trigger_through,
                    color,
                    brush_size,
                    smoothing,
                    smoothing_amount,
                } => {
                    {
                        let mut state = SCREEN_DRAW_STATE.lock();
                        state.enabled = enabled;
                        state.trigger = trigger;
                        state.pass_trigger_through = pass_trigger_through;
                        state.color = color;
                        state.brush_size = brush_size.clamp(2.0, 80.0);
                        state.smoothing = smoothing;
                        state.smoothing_amount = smoothing_amount.clamp(0.0, 1.0);
                        if !enabled && state.active {
                            deactivate_screen_draw(&mut state);
                        }
                    }
                    unsafe {
                        set_screen_draw_refresh_timer(
                            runtime.screen_draw_hwnd,
                            SCREEN_DRAW_STATE.lock().active,
                        );
                    }
                    let _ = refresh_screen_draw_overlay(runtime);
                }

                OverlayCommand::SetProtractorEnabled(enabled) => {
                    {
                        let mut state = PROTRACTOR_STATE.lock();
                        state.enabled = enabled;
                    }
                    if enabled {
                        let _ = ShowWindow(runtime.protractor_hwnd, SW_SHOWNA);
                        let _ = paint_protractor_overlay(runtime);
                    } else {
                        let _ = ShowWindow(runtime.protractor_hwnd, SW_HIDE);
                    }
                }

                OverlayCommand::UpdateProtractorConfig {
                    scale,
                    needle1_angle,
                    needle2_angle,
                    center_x,
                    center_y,
                    thickness,
                    calibrating,
                    ui_language,
                } => {
                    let enabled = {
                        let mut state = PROTRACTOR_STATE.lock();
                        state.scale = scale;
                        state.needle1_angle = needle1_angle;
                        state.needle2_angle = needle2_angle;
                        state.center_x = center_x;
                        state.center_y = center_y;
                        state.thickness = thickness;
                        state.calibrating = calibrating;
                        state.ui_language = ui_language;
                        state.enabled
                    };
                    if enabled {
                        let _ = paint_protractor_overlay(runtime);
                    }
                }

                OverlayCommand::UpdateVisionSettings(settings) => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.use_interception = settings.use_interception;
                    hook_state.use_arduino_mouse = settings.use_arduino_mouse;
                    hook_state.arduino_transport = settings.arduino_transport;
                    hook_state.arduino_com_port = settings.arduino_com_port.clone();
                    hook_state.arduino_vid = settings.arduino_vid.clone();
                    hook_state.arduino_pid = settings.arduino_pid.clone();
                }

                OverlayCommand::SetArduinoFlashInProgress(in_progress) => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.arduino_flash_in_progress = in_progress;
                    if in_progress {
                        // Close all runtime transports immediately so avrdude can use the port.
                        close_arduino_runtime_handles();
                    }
                }

                OverlayCommand::SetTrayIconVisible(visible) => {
                    if visible {
                        let _ = add_tray_icon(hwnd);
                    } else {
                        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &notify_icon(hwnd)) };
                    }
                }

                OverlayCommand::SetVietnameseInputEnabled(enabled) => {
                    HOOK_STATE.lock().vietnamese_input_enabled = enabled;
                }

                OverlayCommand::UpdateMacrosMasterHotkey(binding) => {
                    HOOK_STATE.lock().macros_master_hotkey = binding;
                }

                OverlayCommand::RefreshPinOverlay => {
                    let _ = refresh_pin_overlay(runtime);
                }

                OverlayCommand::SetVisionCaptureMouseBlocked {
                    blocked,
                    is_region_mode,
                } => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.vision_capture_mouse_blocked = blocked;
                    hook_state.vision_capture_is_region_mode = is_region_mode;
                    if !blocked {
                        hook_state.vision_capture_anchor = None;
                        hook_state.vision_capture_preview_regions = Vec::new();
                        hook_state.vision_preview_source = None;
                    }
                }

                OverlayCommand::BeginMousePathDrawCapture {
                    preset_id,
                    preset_name,
                } => {
                    begin_mouse_path_draw_capture(preset_id, preset_name);
                }

                OverlayCommand::CancelMousePathDrawCapture => {
                    cancel_mouse_path_draw_capture("Mouse path draw cancelled.".to_owned());
                }

                OverlayCommand::SetUiVisible(visible) => {
                    runtime.ui_visible = visible;
                    if visible {
                        cancel_pending_tray_toggle();
                        let _ = set_input_hooks_enabled(runtime, desired_hooks_enabled(runtime));
                        let _ = ShowWindow(runtime.pin_hwnd, SW_HIDE);
                        let _ = ShowWindow(runtime.mouse_trail_hwnd, SW_HIDE);
                    } else {
                        *HUD_PREVIEW_DISPLAY.lock() = None;
                        let _ = set_input_hooks_enabled(runtime, desired_hooks_enabled(runtime));
                        let _ = refresh_overlay(runtime);
                        let _ = refresh_pin_overlay(runtime);
                        let _ = refresh_hud(runtime);
                    }
                }

                OverlayCommand::ToggleMacroRecording(group_id, preset_id, preset_name) => {
                    toggle_macro_recording(group_id, preset_id, preset_name);
                }

                OverlayCommand::UpdateTimerPresets(presets) => {
                    let mut hook_state = HOOK_STATE.lock();
                    hook_state.timer_presets = presets.clone();
                    runtime.timer_presets = presets;
                }

                OverlayCommand::PreviewTimerPreset(preset) => {
                    runtime.preview_timer_preset = preset;
                }

                OverlayCommand::Exit => {
                    let _ = runtime.ui_tx.send(UiCommand::Exit);
                    let _ = shutdown_application(hwnd, runtime);
                }
            }
        }
    }

    unsafe fn mark_ui_visible(runtime: &mut Runtime, visible: bool) {
        runtime.ui_visible = visible;
        let _ = set_input_hooks_enabled(runtime, desired_hooks_enabled(runtime));
        if visible {
            let _ = ShowWindow(runtime.pin_hwnd, SW_HIDE);
            let _ = ShowWindow(runtime.hud_hwnd, SW_HIDE);
            let _ = ShowWindow(runtime.mouse_trail_hwnd, SW_HIDE);
        }
    }

    unsafe fn refresh_overlay(runtime: &mut Runtime) -> Result<()> {
        let visible_profiles = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .profiles
                .iter()
                .filter(|profile| profile.enabled)
                .cloned()
                .collect::<Vec<_>>()
        };
        if visible_profiles.is_empty() {
            let _ = ShowWindow(runtime.overlay_hwnd, SW_HIDE);
            return Ok(());
        }

        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        struct ActiveCrosshair {
            layer: RgbaImage,
            left: i32,
            top: i32,
        }

        let mut actives = Vec::new();
        for profile in &visible_profiles {
            let custom_path = profile
                .style
                .custom_asset
                .as_ref()
                .map(|name| runtime.paths.asset_path(name));
            let rendered = render_crosshair(&profile.style, custom_path.as_deref())?;
            let layer = RgbaImage::from_raw(rendered.width, rendered.height, rendered.rgba)
                .context("Failed to build crosshair layer")?;
            let left = profile.style.x_offset - rendered.center_x;
            let top = profile.style.y_offset - rendered.center_y;
            min_x = min_x.min(left);
            min_y = min_y.min(top);
            max_x = max_x.max(left + rendered.width as i32);
            max_y = max_y.max(top + rendered.height as i32);
            actives.push(ActiveCrosshair { layer, left, top });
        }

        let width = (max_x - min_x).max(1) as u32;
        let height = (max_y - min_y).max(1) as u32;
        let mut canvas = RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 0]));
        for active in actives {
            let rel_left = (active.left - min_x) as i64;
            let rel_top = (active.top - min_y) as i64;
            image::imageops::overlay(&mut canvas, &active.layer, rel_left, rel_top);
        }

        paint_crosshair_canvas(runtime.overlay_hwnd, canvas, min_x, min_y)?;
        let _ = ShowWindow(runtime.overlay_hwnd, SW_SHOWNA);
        Ok(())
    }

    unsafe fn paint_crosshair_canvas(hwnd: HWND, canvas: RgbaImage, x: i32, y: i32) -> Result<()> {
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            width as i32,
            height as i32,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            bail!("Failed to acquire the screen DC");
        }

        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create a memory DC");
        }

        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(
            Some(screen_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )
        .context("Failed to create a DIB section")?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to map the DIB section");
        }

        let _previous = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        std::ptr::copy_nonoverlapping(
            canvas.as_raw().as_ptr(),
            bits as *mut u8,
            canvas.as_raw().len(),
        );
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            None,
            Some(&SIZE {
                cx: width as i32,
                cy: height as i32,
            }),
            Some(mem_dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        Ok(())
    }

    fn refresh_hud(runtime: &mut Runtime) -> Result<()> {
        let display = {
            let mut preview_guard = HUD_PREVIEW_DISPLAY.lock();
            if let Some(active) = preview_guard.as_ref()
                && let Some(expires_at) = active.expires_at
                && Instant::now() >= expires_at
            {
                *preview_guard = None;
            }

            if let Some(preview) = preview_guard.clone() {
                Some(preview)
            } else {
                let mut guard = HUD_DISPLAY.lock();
                if let Some(active) = guard.as_ref()
                    && let Some(expires_at) = active.expires_at
                    && Instant::now() >= expires_at
                {
                    *guard = None;
                }

                guard.clone()
            }
        };
        let Some(mut display) = display else {
            let _ = unsafe { ShowWindow(runtime.hud_hwnd, SW_HIDE) };
            runtime.hud_display = None;
            return Ok(());
        };
        display.text = resolve_variables_in_text(&display.text);
        if runtime.hud_display.as_ref() == Some(&display) {
            return Ok(());
        }

        runtime.hud_display = Some(display.clone());
        unsafe { paint_hud(runtime.hud_hwnd, &display) }
    }

    fn refresh_quick_key_display(runtime: &mut Runtime) -> Result<()> {
        if is_ui_in_foreground() || !runtime.quick_key_display_enabled {
            runtime.quick_key_display_entries.clear();
            runtime.quick_key_display_hide_at = None;
            let _ = unsafe { ShowWindow(runtime.key_display_hwnd, SW_HIDE) };
            return Ok(());
        }

        if let Some(hide_at) = runtime.quick_key_display_hide_at
            && Instant::now() >= hide_at
        {
            runtime.quick_key_display_entries.clear();
            runtime.quick_key_display_hide_at = None;
        }

        if runtime.quick_key_display_entries.is_empty() {
            let _ = unsafe { ShowWindow(runtime.key_display_hwnd, SW_HIDE) };
            return Ok(());
        }
        let font_size = runtime.quick_key_display_size.clamp(18.0, 96.0);
        let entries = runtime.quick_key_display_entries.clone();
        let (width, height) = quick_key_display_layout_size(&entries, font_size);
        let x = runtime.quick_key_display_center_x - (width / 2);
        let y = runtime.quick_key_display_center_y - (height / 2);
        unsafe {
            paint_quick_key_display(
                runtime.key_display_hwnd,
                &entries,
                font_size,
                x,
                y,
                width,
                height,
            )
        }
    }

    fn refresh_screen_draw_overlay(runtime: &mut Runtime) -> Result<()> {
        let state = SCREEN_DRAW_STATE.lock();
        let active = state.active;
        let capturing_region = state.capturing_region;
        drop(state);
        if active {
            if capturing_region {
                let _ = unsafe { ShowWindow(runtime.screen_draw_hwnd, SW_HIDE) };
                Ok(())
            } else {
                unsafe { paint_screen_draw_overlay(runtime.screen_draw_hwnd) }
            }
        } else {
            let _ = unsafe { ShowWindow(runtime.screen_draw_hwnd, SW_HIDE) };
            Ok(())
        }
    }

    unsafe fn toggle_screen_draw_overlay(hwnd: HWND) {
        let active = {
            let mut state = SCREEN_DRAW_STATE.lock();
            if !state.enabled {
                return;
            }
            if state.active {
                deactivate_screen_draw(&mut state);
            } else {
                state.active = true;
                state.capturing_region = false;
                state.current_stroke = None;
                state.active_control = ScreenDrawControl::None;
                state.pending_repaint = true;
                state.dirty_rect = Some(ScreenDrawDirtyRect::full(
                    state.canvas_width.max(1),
                    state.canvas_height.max(1),
                ));
                state.live_stroke_rect = None;
            }
            state.active
        };

        if active {
            set_screen_draw_refresh_timer(hwnd, true);
            let _ = paint_screen_draw_overlay(hwnd);
        } else {
            set_screen_draw_refresh_timer(hwnd, false);
            let _ = clear_screen_draw_overlay_window(hwnd);
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }

    fn screen_draw_active() -> bool {
        let state = SCREEN_DRAW_STATE.lock();
        state.active && !state.capturing_region
    }

    fn screen_draw_local_point_from_screen(point: POINT) -> POINT {
        let (screen_x, screen_y, _, _) = window_list::virtual_screen_bounds();
        POINT {
            x: point.x - screen_x,
            y: point.y - screen_y,
        }
    }

    unsafe fn set_screen_draw_refresh_timer(hwnd: HWND, active: bool) {
        if active {
            let _ = SetTimer(
                Some(hwnd),
                SCREEN_DRAW_TIMER_ID,
                SCREEN_DRAW_REFRESH_INTERVAL_MS,
                None,
            );
        } else {
            let _ = KillTimer(Some(hwnd), SCREEN_DRAW_TIMER_ID);
        }
    }

    fn mark_screen_draw_repaint_pending(state: &mut ScreenDrawState) {
        state.pending_repaint = true;
    }

    fn screen_draw_toolbar_rect(state: &ScreenDrawState) -> ScreenDrawDirtyRect {
        ScreenDrawDirtyRect {
            left: state.toolbar_x.max(0) as usize,
            top: state.toolbar_y.max(0) as usize,
            right: (state.toolbar_x + SCREEN_DRAW_TOOLBAR_WIDTH).max(0) as usize,
            bottom: (state.toolbar_y + SCREEN_DRAW_TOOLBAR_HEIGHT).max(0) as usize,
        }
    }

    fn mark_screen_draw_dirty(state: &mut ScreenDrawState, rect: ScreenDrawDirtyRect) {
        state.dirty_rect = Some(match state.dirty_rect {
            Some(existing) => existing.union(rect),
            None => rect,
        });
    }

    fn current_screen_draw_stroke_rect(stroke: &ScreenDrawStroke) -> Option<ScreenDrawDirtyRect> {
        let first = stroke.points.first()?;
        let mut min_x = first.x;
        let mut min_y = first.y;
        let mut max_x = first.x;
        let mut max_y = first.y;
        for point in stroke.points.iter().skip(1) {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }
        let pad = (stroke.brush_size.ceil() as i32 + 6).max(4);
        Some(ScreenDrawDirtyRect {
            left: min_x.saturating_sub(pad).max(0) as usize,
            top: min_y.saturating_sub(pad).max(0) as usize,
            right: (max_x + pad + 1).max(0) as usize,
            bottom: (max_y + pad + 1).max(0) as usize,
        })
    }

    fn sync_screen_draw_live_stroke_dirty(state: &mut ScreenDrawState) {
        if let Some(previous) = state.live_stroke_rect.take() {
            mark_screen_draw_dirty(state, previous);
        }
        if let Some(current) = state
            .current_stroke
            .as_ref()
            .and_then(current_screen_draw_stroke_rect)
        {
            mark_screen_draw_dirty(state, current);
            state.live_stroke_rect = Some(current);
        }
    }

    fn mark_screen_draw_toolbar_dirty(
        state: &mut ScreenDrawState,
        previous: ScreenDrawDirtyRect,
    ) {
        mark_screen_draw_dirty(state, previous);
        mark_screen_draw_dirty(state, screen_draw_toolbar_rect(state));
    }

    fn screen_draw_should_present_immediately() -> bool {
        let state = SCREEN_DRAW_STATE.lock();
        if !state.active || !state.pending_repaint {
            return false;
        }
        let now = Instant::now();
        if let Some(last_present_at) = state.last_present_at
            && now.duration_since(last_present_at)
                < Duration::from_millis(SCREEN_DRAW_MIN_FRAME_INTERVAL_MS)
        {
            return false;
        }
        true
    }

    fn deactivate_screen_draw(state: &mut ScreenDrawState) {
        state.active = false;
        state.current_stroke = None;
        state.active_control = ScreenDrawControl::None;
        state.capturing_region = false;
        state.strokes.clear();
        state.canvas_width = 0;
        state.canvas_height = 0;
        state.committed_rgba.clear();
        state.frame_rgba.clear();
        state.committed_dirty = true;
        state.pending_repaint = false;
        state.last_present_at = None;
        state.dirty_rect = None;
        state.live_stroke_rect = None;
        release_screen_draw_surface(state);
    }

    fn release_screen_draw_surface(state: &mut ScreenDrawState) {
        unsafe {
            if state.surface_dc != 0 {
                let surface_dc = HDC(state.surface_dc as *mut c_void);
                if state.surface_bitmap != 0 {
                    let _ = SelectObject(
                        surface_dc,
                        HGDIOBJ(state.surface_old_bitmap as *mut c_void),
                    );
                    let _ = DeleteObject(HGDIOBJ(state.surface_bitmap as *mut c_void));
                }
                let _ = DeleteDC(surface_dc);
            }
        }
        state.surface_dc = 0;
        state.surface_bitmap = 0;
        state.surface_old_bitmap = 0;
        state.surface_bits = 0;
        state.surface_bits_len = 0;
        state.surface_width = 0;
        state.surface_height = 0;
    }

    unsafe fn clear_screen_draw_overlay_window(hwnd: HWND) -> Result<()> {
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 1,
                biHeight: -1,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits_ptr: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        )?;
        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        let pixels = std::slice::from_raw_parts_mut(bits_ptr as *mut u8, 4);
        pixels.fill(0);
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&POINT { x: 0, y: 0 }),
            Some(&SIZE { cx: 1, cy: 1 }),
            Some(mem_dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        Ok(())
    }

    fn screen_draw_handle_button_down(point: POINT, right_button: bool) -> bool {
        let mut start_region_capture = false;
        let mut state = SCREEN_DRAW_STATE.lock();
        if !state.active || state.capturing_region {
            return false;
        }
        if right_button {
            start_screen_draw_stroke(&mut state, point, true);
            sync_screen_draw_live_stroke_dirty(&mut state);
            mark_screen_draw_repaint_pending(&mut state);
            return true;
        }
        match screen_draw_hit(&state, point) {
            ScreenDrawHit::Close => {
                deactivate_screen_draw(&mut state);
            }
            ScreenDrawHit::Color => {
                state.color = next_screen_draw_color(state.color);
            }
            ScreenDrawHit::BrushSize => {
                let toolbar_rect = screen_draw_toolbar_rect(&state);
                state.active_control = ScreenDrawControl::BrushSize;
                update_screen_draw_brush_slider(&mut state, point.x);
                mark_screen_draw_toolbar_dirty(&mut state, toolbar_rect);
            }
            ScreenDrawHit::Eraser => {
                state.eraser = !state.eraser;
                let toolbar_rect = screen_draw_toolbar_rect(&state);
                mark_screen_draw_dirty(&mut state, toolbar_rect);
            }
            ScreenDrawHit::Smoothing => {
                state.smoothing = !state.smoothing;
                let toolbar_rect = screen_draw_toolbar_rect(&state);
                mark_screen_draw_dirty(&mut state, toolbar_rect);
            }
            ScreenDrawHit::SmoothingAmount => {
                let toolbar_rect = screen_draw_toolbar_rect(&state);
                state.active_control = ScreenDrawControl::SmoothingAmount;
                update_screen_draw_smoothing_slider(&mut state, point.x);
                mark_screen_draw_toolbar_dirty(&mut state, toolbar_rect);
            }
            ScreenDrawHit::CaptureRegion => {
                if !state.capturing_region {
                    state.capturing_region = true;
                    state.active_control = ScreenDrawControl::None;
                    state.current_stroke = None;
                    state.pending_repaint = false;
                    start_region_capture = true;
                }
            }
            ScreenDrawHit::ToolbarBody => {
                state.active_control = ScreenDrawControl::MoveToolbar;
                state.drag_offset_x = point.x - state.toolbar_x;
                state.drag_offset_y = point.y - state.toolbar_y;
            }
            ScreenDrawHit::Canvas => {
                let eraser = state.eraser;
                start_screen_draw_stroke(&mut state, point, eraser);
                sync_screen_draw_live_stroke_dirty(&mut state);
            }
        }
        if state.active && !start_region_capture {
            mark_screen_draw_repaint_pending(&mut state);
        }
        drop(state);
        if start_region_capture {
            let hwnd_raw = SCREEN_DRAW_HWND.load(Ordering::Relaxed);
            if hwnd_raw != 0 {
                unsafe {
                    let hwnd = HWND(hwnd_raw as *mut c_void);
                    set_screen_draw_refresh_timer(hwnd, false);
                    let _ = clear_screen_draw_overlay_window(hwnd);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
            begin_screen_draw_region_capture();
        }
        true
    }

    fn begin_screen_draw_region_capture() {
        let hwnd_raw = SCREEN_DRAW_HWND.load(Ordering::Relaxed);
        thread::spawn(move || {
            let status = run_screen_draw_region_capture_flow();
            restore_screen_draw_after_region_capture(hwnd_raw);
            if let Some(tx) = HOOK_STATE.lock().ui_tx.clone() {
                let _ = tx.send(UiCommand::ScreenDrawCaptureStatus(status));
            }
        });
    }

    fn run_screen_draw_region_capture_flow() -> String {
        match capture_screen_draw_region_to_clipboard() {
            Ok(copied) if copied => "Copied annotated screen region to clipboard.".to_owned(),
            Ok(_) => "Screen draw capture cancelled.".to_owned(),
            Err(error) => format!("Screen draw capture failed: {error}"),
        }
    }

    fn capture_screen_draw_region_to_clipboard() -> Result<bool> {
        let Some((x, y, width, height)) = select_screen_draw_capture_region()? else {
            return Ok(false);
        };
        let capture = build_screen_draw_capture_region(x, y, width, height)?;
        copy_screen_draw_capture_to_clipboard(&capture)?;
        Ok(true)
    }

    fn select_screen_draw_capture_region() -> Result<Option<(i32, i32, i32, i32)>> {
        let is_down = |vk: i32| unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };

        while is_down(0x01) {
            if is_down(0x1B) {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(6));
        }

        set_screen_draw_region_capture_mouse_blocked(true);

        let mut origin: Option<(i32, i32)> = None;
        let result = loop {
            if is_down(0x1B) || is_down(0x02) {
                break Ok(None);
            }

            let mut point = POINT::default();
            if unsafe { GetCursorPos(&mut point).is_ok() } {
                if is_down(0x01) {
                    origin.get_or_insert((point.x, point.y));
                } else if let Some(start) = origin {
                    let x = start.0.min(point.x);
                    let y = start.1.min(point.y);
                    let width = (start.0 - point.x).abs();
                    let height = (start.1 - point.y).abs();
                    if width >= 2 && height >= 2 {
                        break Ok(Some((x, y, width, height)));
                    }
                    break Ok(None);
                }
            }

            thread::sleep(Duration::from_millis(8));
        };

        set_screen_draw_region_capture_mouse_blocked(false);
        result
    }

    fn set_screen_draw_region_capture_mouse_blocked(blocked: bool) {
        let mut hook_state = HOOK_STATE.lock();
        hook_state.vision_capture_mouse_blocked = blocked;
        hook_state.vision_capture_is_region_mode = blocked;
        hook_state.vision_capture_anchor = None;
        hook_state.vision_capture_preview_regions = Vec::new();
        hook_state.vision_preview_source = None;
        drop(hook_state);
        wake_command_queue();
    }

    fn build_screen_draw_capture_region(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<window_list::ScreenCaptureFrame> {
        let (capture_x, capture_y, capture_w, capture_h) =
            normalize_screen_draw_capture_region(x, y, width, height)?;
        thread::sleep(Duration::from_millis(16));
        let mut capture = window_list::capture_virtual_screen_region(
            capture_x,
            capture_y,
            capture_w,
            capture_h,
        )
        .ok_or_else(|| anyhow::anyhow!("Failed to capture the selected screen region"))?;
        let draw_layer = build_screen_draw_capture_region_layer(
            capture_x,
            capture_y,
            capture.width as i32,
            capture.height as i32,
        )?;
        blend_screen_draw_layer_onto_capture(capture.rgba.as_mut_slice(), draw_layer.as_slice());
        Ok(capture)
    }

    fn normalize_screen_draw_capture_region(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(i32, i32, i32, i32)> {
        let (screen_x, screen_y, screen_w, screen_h) = window_list::virtual_screen_bounds();
        if screen_w <= 0 || screen_h <= 0 {
            bail!("Virtual screen is unavailable");
        }

        let screen_right = screen_x + screen_w;
        let screen_bottom = screen_y + screen_h;
        let capture_x = x.clamp(screen_x, screen_right - 1);
        let capture_y = y.clamp(screen_y, screen_bottom - 1);
        let capture_w = width.max(1).min(screen_right - capture_x);
        let capture_h = height.max(1).min(screen_bottom - capture_y);
        if capture_w <= 0 || capture_h <= 0 {
            bail!("Selected capture region is empty");
        }

        Ok((capture_x, capture_y, capture_w, capture_h))
    }

    fn build_screen_draw_capture_region_layer(
        capture_x: i32,
        capture_y: i32,
        capture_w: i32,
        capture_h: i32,
    ) -> Result<Vec<u8>> {
        let (screen_x, screen_y, screen_w, screen_h) = window_list::virtual_screen_bounds();
        if screen_w <= 0 || screen_h <= 0 {
            bail!("Virtual screen is unavailable");
        }

        let mut state = SCREEN_DRAW_STATE.lock();
        if !state.active {
            bail!("Screen draw is not active");
        }

        let canvas_width = screen_w as usize;
        let canvas_height = screen_h as usize;
        ensure_screen_draw_canvas(&mut state, canvas_width, canvas_height);
        if state.committed_dirty {
            rebuild_screen_draw_canvas(&mut state);
        }

        let rel_x = (capture_x - screen_x).clamp(0, screen_w.saturating_sub(1)) as usize;
        let rel_y = (capture_y - screen_y).clamp(0, screen_h.saturating_sub(1)) as usize;
        let copy_w = (capture_w.max(1) as usize).min(canvas_width.saturating_sub(rel_x));
        let copy_h = (capture_h.max(1) as usize).min(canvas_height.saturating_sub(rel_y));
        if copy_w == 0 || copy_h == 0 {
            bail!("Selected capture region is empty");
        }

        let mut rgba = vec![0u8; copy_w * copy_h * 4];
        for row in 0..copy_h {
            let src_index = ((rel_y + row) * canvas_width + rel_x) * 4;
            let dst_index = row * copy_w * 4;
            let byte_count = copy_w * 4;
            rgba[dst_index..dst_index + byte_count]
                .copy_from_slice(&state.committed_rgba[src_index..src_index + byte_count]);
        }

        Ok(rgba)
    }

    fn blend_screen_draw_layer_onto_capture(dst: &mut [u8], src: &[u8]) {
        let pixel_count = (dst.len() / 4).min(src.len() / 4);
        for index in 0..pixel_count {
            let offset = index * 4;
            let src_a = src[offset + 3];
            if src_a == 0 {
                continue;
            }
            blend_premultiplied_rgba(
                &mut dst[offset..offset + 4],
                src[offset],
                src[offset + 1],
                src[offset + 2],
                src_a,
            );
            dst[offset + 3] = 255;
        }
    }

    fn copy_screen_draw_capture_to_clipboard(
        capture: &window_list::ScreenCaptureFrame,
    ) -> Result<()> {
        let mut clipboard = Clipboard::new().context("Failed to open the clipboard")?;
        clipboard
            .set_image(ImageData {
                width: capture.width,
                height: capture.height,
                bytes: Cow::Owned(capture.rgba.clone()),
            })
            .context("Failed to copy the annotated screenshot to the clipboard")
    }

    fn restore_screen_draw_after_region_capture(hwnd_raw: isize) {
        let mut state = SCREEN_DRAW_STATE.lock();
        state.capturing_region = false;
        let active = state.active;
        if active {
            let (screen_x, screen_y, screen_w, screen_h) = window_list::virtual_screen_bounds();
            let _ = (screen_x, screen_y);
            state.pending_repaint = true;
            state.dirty_rect = Some(ScreenDrawDirtyRect::full(
                screen_w.max(1) as usize,
                screen_h.max(1) as usize,
            ));
        }
        drop(state);

        if hwnd_raw == 0 {
            return;
        }

        unsafe {
            let hwnd = HWND(hwnd_raw as *mut c_void);
            if active {
                set_screen_draw_refresh_timer(hwnd, true);
                let _ = paint_screen_draw_overlay(hwnd);
            } else {
                set_screen_draw_refresh_timer(hwnd, false);
                let _ = clear_screen_draw_overlay_window(hwnd);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
    }

    fn screen_draw_handle_move(point: POINT) -> bool {
        let mut state = SCREEN_DRAW_STATE.lock();
        if !state.active || state.capturing_region {
            return false;
        }
        match state.active_control {
            ScreenDrawControl::MoveToolbar => {
                let (_, _, screen_w, screen_h) = window_list::virtual_screen_bounds();
                let toolbar_rect = screen_draw_toolbar_rect(&state);
                state.toolbar_x = (point.x - state.drag_offset_x)
                    .clamp(0, (screen_w - SCREEN_DRAW_TOOLBAR_WIDTH).max(0));
                state.toolbar_y = (point.y - state.drag_offset_y)
                    .clamp(0, (screen_h - SCREEN_DRAW_TOOLBAR_HEIGHT).max(0));
                mark_screen_draw_toolbar_dirty(&mut state, toolbar_rect);
                mark_screen_draw_repaint_pending(&mut state);
                true
            }
            ScreenDrawControl::BrushSize => {
                let toolbar_rect = screen_draw_toolbar_rect(&state);
                update_screen_draw_brush_slider(&mut state, point.x);
                mark_screen_draw_toolbar_dirty(&mut state, toolbar_rect);
                mark_screen_draw_repaint_pending(&mut state);
                true
            }
            ScreenDrawControl::SmoothingAmount => {
                let toolbar_rect = screen_draw_toolbar_rect(&state);
                update_screen_draw_smoothing_slider(&mut state, point.x);
                mark_screen_draw_toolbar_dirty(&mut state, toolbar_rect);
                mark_screen_draw_repaint_pending(&mut state);
                true
            }
            ScreenDrawControl::None => {
                if let Some(stroke) = state.current_stroke.as_mut() {
                    let changed = append_screen_draw_point(stroke, point);
                    if changed {
                        sync_screen_draw_live_stroke_dirty(&mut state);
                        mark_screen_draw_repaint_pending(&mut state);
                    }
                    changed
                } else {
                    false
                }
            }
        }
    }

    fn screen_draw_handle_button_up() -> bool {
        let mut state = SCREEN_DRAW_STATE.lock();
        if !state.active || state.capturing_region {
            return false;
        }
        if let Some(previous) = state.live_stroke_rect.take() {
            mark_screen_draw_dirty(&mut state, previous);
        }
        if let Some(stroke) = state.current_stroke.take()
        {
            if !stroke.points.is_empty() {
                if state.canvas_width > 0
                    && state.canvas_height > 0
                    && !state.committed_rgba.is_empty()
                    && !state.committed_dirty
                {
                    let canvas_width = state.canvas_width as u32;
                    let canvas_height = state.canvas_height as u32;
                    if let Some(mut pixmap) = tiny_skia::PixmapMut::from_bytes(
                        state.committed_rgba.as_mut_slice(),
                        canvas_width,
                        canvas_height,
                    ) {
                        render_screen_draw_stroke_skia(&mut pixmap, &stroke);
                    } else {
                        state.committed_dirty = true;
                    }
                } else {
                    state.committed_dirty = true;
                }
                state.strokes.push(stroke);
            }
        }
        state.active_control = ScreenDrawControl::None;
        if state.active {
            mark_screen_draw_repaint_pending(&mut state);
        }
        true
    }

    fn append_screen_draw_point(stroke: &mut ScreenDrawStroke, point: POINT) -> bool {
        let Some(last) = stroke.points.last().copied() else {
            stroke.points.push(point);
            return true;
        };

        let dx = (point.x - last.x) as f32;
        let dy = (point.y - last.y) as f32;
        let min_distance = if stroke.smoothing { 1.6 } else { 0.9 };
        if dx * dx + dy * dy < min_distance * min_distance {
            return false;
        }

        stroke.points.push(point);
        true
    }

    fn process_screen_draw_mouse_event(message: u32, screen_point: POINT) -> bool {
        if !screen_draw_active() {
            return false;
        }
        let point = screen_draw_local_point_from_screen(screen_point);
        let (handled, repaint) = match message {
            WM_LBUTTONDOWN => {
                let handled = screen_draw_handle_button_down(point, false);
                (handled, handled)
            }
            WM_RBUTTONDOWN => {
                let handled = screen_draw_handle_button_down(point, true);
                (handled, handled)
            }
            WM_MOUSEMOVE => {
                let repaint = screen_draw_handle_move(point);
                (false, repaint)
            }
            WM_LBUTTONUP | WM_RBUTTONUP => {
                let handled = screen_draw_handle_button_up();
                (handled, handled)
            }
            WM_MBUTTONDOWN
            | windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP
            | WM_XBUTTONDOWN
            | WM_XBUTTONUP
            | WM_MOUSEWHEEL => (true, false),
            _ => (false, false),
        };
        if handled || repaint {
            let hwnd_raw = SCREEN_DRAW_HWND.load(Ordering::Relaxed);
            if hwnd_raw != 0 {
                let hwnd = HWND(hwnd_raw as *mut c_void);
                unsafe {
                    set_screen_draw_refresh_timer(hwnd, screen_draw_active());
                    if message != WM_MOUSEMOVE || screen_draw_should_present_immediately() {
                        let _ = paint_screen_draw_overlay(hwnd);
                    }
                }
            }
        }
        handled
    }

    fn screen_draw_lparam_point(lparam: LPARAM) -> POINT {
        POINT {
            x: (lparam.0 & 0xFFFF) as i16 as i32,
            y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
        }
    }

    fn screen_draw_hit(state: &ScreenDrawState, point: POINT) -> ScreenDrawHit {
        let x = point.x - state.toolbar_x;
        let y = point.y - state.toolbar_y;
        if x < 0 || y < 0 || x > SCREEN_DRAW_TOOLBAR_WIDTH || y > SCREEN_DRAW_TOOLBAR_HEIGHT {
            return ScreenDrawHit::Canvas;
        }
        if x >= SCREEN_DRAW_TOOLBAR_CLOSE_X
            && x <= SCREEN_DRAW_TOOLBAR_CLOSE_X + 26
            && y >= 10
            && y <= 30
        {
            return ScreenDrawHit::Close;
        }
        if x >= 14 && x <= 50 && y >= 20 && y <= 56 {
            return ScreenDrawHit::Color;
        }
        if x >= 68 && x <= 158 && y >= 22 && y <= 54 {
            return ScreenDrawHit::BrushSize;
        }
        if x >= 172 && x <= 208 && y >= 20 && y <= 56 {
            return ScreenDrawHit::Eraser;
        }
        if x >= 224 && x <= 252 && y >= 24 && y <= 52 {
            return ScreenDrawHit::Smoothing;
        }
        if x >= 264 && x <= 318 && y >= 22 && y <= 54 {
            return ScreenDrawHit::SmoothingAmount;
        }
        if x >= SCREEN_DRAW_TOOLBAR_CAPTURE_X
            && x <= SCREEN_DRAW_TOOLBAR_CAPTURE_X + 36
            && y >= 20
            && y <= 56
        {
            return ScreenDrawHit::CaptureRegion;
        }
        ScreenDrawHit::ToolbarBody
    }

    fn update_screen_draw_brush_slider(state: &mut ScreenDrawState, x: i32) {
        let left = state.toolbar_x + 68;
        let t = ((x - left) as f32 / 90.0).clamp(0.0, 1.0);
        state.brush_size = 2.0 + t * 78.0;
    }

    fn update_screen_draw_smoothing_slider(state: &mut ScreenDrawState, x: i32) {
        let left = state.toolbar_x + 264;
        state.smoothing_amount = ((x - left) as f32 / 54.0).clamp(0.0, 1.0);
    }

    fn start_screen_draw_stroke(state: &mut ScreenDrawState, point: POINT, force_eraser: bool) {
        state.current_stroke = Some(ScreenDrawStroke {
            points: vec![point],
            color: state.color,
            brush_size: state.brush_size,
            eraser: force_eraser || state.eraser,
            smoothing: state.smoothing,
            smoothing_amount: state.smoothing_amount,
        });
    }

    fn next_screen_draw_color(color: RgbaColor) -> RgbaColor {
        const PALETTE: [RgbaColor; 8] = [
            RgbaColor { r: 0, g: 255, b: 170, a: 255 },
            RgbaColor { r: 255, g: 96, b: 96, a: 255 },
            RgbaColor { r: 255, g: 224, b: 96, a: 255 },
            RgbaColor { r: 96, g: 176, b: 255, a: 255 },
            RgbaColor { r: 255, g: 128, b: 224, a: 255 },
            RgbaColor { r: 255, g: 255, b: 255, a: 255 },
            RgbaColor { r: 32, g: 32, b: 32, a: 255 },
            RgbaColor { r: 126, g: 224, b: 182, a: 255 },
        ];
        let index = PALETTE
            .iter()
            .position(|entry| entry.r == color.r && entry.g == color.g && entry.b == color.b)
            .unwrap_or(0);
        PALETTE[(index + 1) % PALETTE.len()]
    }

    fn ensure_screen_draw_canvas(
        state: &mut ScreenDrawState,
        width: usize,
        height: usize,
    ) -> bool {
        if state.canvas_width == width
            && state.canvas_height == height
            && state.committed_rgba.len() == width * height * 4
        {
            return true;
        }

        let byte_len = width.saturating_mul(height).saturating_mul(4);
        state.canvas_width = width;
        state.canvas_height = height;
        state.committed_rgba.clear();
        state.committed_rgba.resize(byte_len, 0);
        state.frame_rgba.clear();
        state.frame_rgba.resize(byte_len, 0);
        state.committed_dirty = true;
        state.dirty_rect = Some(ScreenDrawDirtyRect::full(width, height));
        state.live_stroke_rect = None;
        release_screen_draw_surface(state);
        true
    }

    fn render_screen_draw_stroke_skia(
        pixmap: &mut tiny_skia::PixmapMut,
        stroke: &ScreenDrawStroke,
    ) {
        let filtered_points = filtered_screen_draw_points(
            &stroke.points,
            if stroke.smoothing { 1.2 } else { 0.6 },
        );
        let points = if stroke.smoothing {
            smoothed_screen_draw_points(&filtered_points, stroke.smoothing_amount)
        } else {
            filtered_points
        };
        if points.is_empty() {
            return;
        }

        let mut paint = tiny_skia::Paint::default();
        paint.anti_alias = true;
        if stroke.eraser {
            paint.blend_mode = tiny_skia::BlendMode::Clear;
        } else {
            paint.set_color(tiny_skia::Color::from_rgba8(
                stroke.color.r,
                stroke.color.g,
                stroke.color.b,
                stroke.color.a,
            ));
        }

        if points.len() == 1 {
            let mut pb = tiny_skia::PathBuilder::new();
            pb.push_circle(
                points[0].x as f32,
                points[0].y as f32,
                (stroke.brush_size.max(1.0) * 0.5).max(0.75),
            );
            if let Some(path) = pb.finish() {
                pixmap.fill_path(
                    &path,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
            return;
        }

        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(points[0].x as f32, points[0].y as f32);
        if stroke.smoothing && points.len() >= 3 {
            for index in 1..points.len() - 1 {
                let current = points[index];
                let next = points[index + 1];
                pb.quad_to(
                    current.x as f32,
                    current.y as f32,
                    (current.x + next.x) as f32 * 0.5,
                    (current.y + next.y) as f32 * 0.5,
                );
            }
            let last = *points.last().unwrap_or(&points[0]);
            pb.line_to(last.x as f32, last.y as f32);
        } else {
            for point in points.iter().skip(1) {
                pb.line_to(point.x as f32, point.y as f32);
            }
        }
        if let Some(path) = pb.finish() {
            let stroke_style = tiny_skia::Stroke {
                width: stroke.brush_size.max(1.0),
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..Default::default()
            };
            pixmap.stroke_path(
                &path,
                &paint,
                &stroke_style,
                tiny_skia::Transform::identity(),
                None,
            );
        }
    }

    fn rebuild_screen_draw_canvas(state: &mut ScreenDrawState) {
        if state.canvas_width == 0 || state.canvas_height == 0 || state.committed_rgba.is_empty() {
            return;
        }
        state.committed_rgba.fill(0);
        if let Some(mut pixmap) = tiny_skia::PixmapMut::from_bytes(
            state.committed_rgba.as_mut_slice(),
            state.canvas_width as u32,
            state.canvas_height as u32,
        ) {
            for stroke in &state.strokes {
                render_screen_draw_stroke_skia(&mut pixmap, stroke);
            }
            state.committed_dirty = false;
        }
    }

    unsafe fn ensure_screen_draw_surface(
        state: &mut ScreenDrawState,
        width: usize,
        height: usize,
    ) -> Result<()> {
        if state.surface_width == width
            && state.surface_height == height
            && state.surface_dc != 0
            && state.surface_bits != 0
            && state.surface_bits_len == width.saturating_mul(height).saturating_mul(4)
        {
            return Ok(());
        }

        release_screen_draw_surface(state);
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?;
        let _ = ReleaseDC(None, screen_dc);
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(mem_dc);
            bail!("Failed to allocate persistent screen draw surface");
        }

        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        state.surface_dc = mem_dc.0 as isize;
        state.surface_bitmap = bitmap.0 as isize;
        state.surface_old_bitmap = old_bitmap.0 as isize;
        state.surface_bits = bits as usize;
        state.surface_bits_len = width.saturating_mul(height).saturating_mul(4);
        state.surface_width = width;
        state.surface_height = height;
        state.dirty_rect = Some(ScreenDrawDirtyRect::full(width, height));
        Ok(())
    }

    fn copy_screen_draw_rgba_region(
        src: &[u8],
        dst: &mut [u8],
        width: usize,
        rect: ScreenDrawDirtyRect,
    ) {
        for y in rect.top..rect.bottom {
            let row_start = (y * width + rect.left) * 4;
            let row_end = (y * width + rect.right) * 4;
            dst[row_start..row_end].copy_from_slice(&src[row_start..row_end]);
        }
    }

    fn copy_screen_draw_rgba_to_bgra_region(
        src: &[u8],
        dst: &mut [u8],
        width: usize,
        rect: ScreenDrawDirtyRect,
    ) {
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let offset = (y * width + x) * 4;
                dst[offset] = src[offset + 2];
                dst[offset + 1] = src[offset + 1];
                dst[offset + 2] = src[offset];
                dst[offset + 3] = src[offset + 3];
            }
        }
    }

    unsafe fn paint_screen_draw_overlay(hwnd: HWND) -> Result<()> {
        let (screen_x, screen_y, screen_w, screen_h) = window_list::virtual_screen_bounds();
        if screen_w <= 0 || screen_h <= 0 {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return Ok(());
        }
        let mut state_guard = SCREEN_DRAW_STATE.lock();
        if !state_guard.active || state_guard.capturing_region {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return Ok(());
        }
        let width = screen_w as usize;
        let height = screen_h as usize;
        ensure_screen_draw_canvas(&mut state_guard, width, height);
        ensure_screen_draw_surface(&mut state_guard, width, height)?;
        if state_guard.committed_dirty {
            rebuild_screen_draw_canvas(&mut state_guard);
        }
        let dirty_rect = state_guard
            .dirty_rect
            .take()
            .and_then(|rect| rect.normalized(width, height))
            .unwrap_or_else(|| ScreenDrawDirtyRect::full(width, height));
        {
            let ScreenDrawState {
                committed_rgba,
                frame_rgba,
                ..
            } = &mut *state_guard;
            copy_screen_draw_rgba_region(
                committed_rgba.as_slice(),
                frame_rgba.as_mut_slice(),
                width,
                dirty_rect,
            );
        }
        if let Some(stroke) = state_guard.current_stroke.clone()
            && let Some(mut pixmap) =
                tiny_skia::PixmapMut::from_bytes(
                    state_guard.frame_rgba.as_mut_slice(),
                    width as u32,
                    height as u32,
                )
        {
            render_screen_draw_stroke_skia(&mut pixmap, &stroke);
        }
        let toolbar_x = state_guard.toolbar_x;
        let toolbar_y = state_guard.toolbar_y;
        let toolbar_color = state_guard.color;
        let toolbar_brush_size = state_guard.brush_size;
        let toolbar_eraser = state_guard.eraser;
        let toolbar_smoothing = state_guard.smoothing;
        let toolbar_smoothing_amount = state_guard.smoothing_amount;
        draw_screen_draw_toolbar_rgba(
            state_guard.frame_rgba.as_mut_slice(),
            width,
            height,
            toolbar_x,
            toolbar_y,
            toolbar_color,
            toolbar_brush_size,
            toolbar_eraser,
            toolbar_smoothing,
            toolbar_smoothing_amount,
        );

        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            screen_x,
            screen_y,
            screen_w,
            screen_h,
            SWP_NOACTIVATE,
        );

        let screen_dc = GetDC(None);
        let pixels = std::slice::from_raw_parts_mut(
            state_guard.surface_bits as *mut u8,
            state_guard.surface_bits_len,
        );
        copy_screen_draw_rgba_to_bgra_region(
            state_guard.frame_rgba.as_slice(),
            pixels,
            width,
            dirty_rect,
        );
        let surface_dc = HDC(state_guard.surface_dc as *mut c_void);
        state_guard.pending_repaint = false;
        state_guard.last_present_at = Some(Instant::now());
        drop(state_guard);
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&POINT { x: screen_x, y: screen_y }),
            Some(&SIZE { cx: screen_w, cy: screen_h }),
            Some(surface_dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = ReleaseDC(None, screen_dc);
        let _ = ShowWindow(hwnd, SW_SHOWNA);
        Ok(())
    }

    fn smoothed_screen_draw_points(points: &[POINT], amount: f32) -> Vec<POINT> {
        if points.len() < 3 {
            return points.to_vec();
        }
        let radius = (1.0 + amount.clamp(0.0, 1.0) * 2.0).round() as usize;
        let mut result = Vec::with_capacity(points.len());
        for index in 0..points.len() {
            let start = index.saturating_sub(radius);
            let end = (index + radius + 1).min(points.len());
            let count = (end - start) as i32;
            let mut sx = 0i32;
            let mut sy = 0i32;
            for point in &points[start..end] {
                sx += point.x;
                sy += point.y;
            }
            result.push(POINT {
                x: sx / count,
                y: sy / count,
            });
        }
        result
    }

    fn filtered_screen_draw_points(points: &[POINT], min_distance: f32) -> Vec<POINT> {
        if points.len() < 2 {
            return points.to_vec();
        }
        let min_distance_sq = min_distance.max(0.25).powi(2);
        let mut filtered = Vec::with_capacity(points.len());
        for point in points {
            let should_push = filtered.last().is_none_or(|last: &POINT| {
                let dx = (point.x - last.x) as f32;
                let dy = (point.y - last.y) as f32;
                dx * dx + dy * dy >= min_distance_sq
            });
            if should_push {
                filtered.push(*point);
            }
        }
        filtered
    }

    fn draw_screen_draw_slider_skia(
        pixmap: &mut tiny_skia::Pixmap,
        x: f32,
        y: f32,
        slider_width: f32,
        value: f32,
    ) {
        fill_skia_rounded_rect(
            pixmap,
            x,
            y - 3.0,
            slider_width,
            6.0,
            3.0,
            [90, 108, 132, 224],
        );
        fill_skia_rounded_rect(
            pixmap,
            x,
            y - 3.0,
            value.clamp(0.0, 1.0) * slider_width,
            6.0,
            3.0,
            [120, 214, 176, 224],
        );
        let knob_x = x + value.clamp(0.0, 1.0) * slider_width;
        draw_skia_circle_fill(pixmap, knob_x, y, 11.0, [244, 248, 255, 255]);
        draw_skia_circle_outline(pixmap, knob_x, y, 11.0, [255, 255, 255, 66], 1.0);
        draw_skia_circle_fill(pixmap, knob_x, y, 4.0, [64, 84, 108, 140]);
    }

    fn draw_screen_draw_toolbar_rgba(
        pixels: &mut [u8],
        width: usize,
        height: usize,
        toolbar_x: i32,
        toolbar_y: i32,
        color: RgbaColor,
        brush_size: f32,
        eraser: bool,
        smoothing: bool,
        smoothing_amount: f32,
    ) {
        let toolbar_w = SCREEN_DRAW_TOOLBAR_WIDTH as usize;
        let toolbar_h = 72usize;
        let mut pixmap = match tiny_skia::Pixmap::new(toolbar_w as u32, toolbar_h as u32) {
            Some(pixmap) => pixmap,
            None => return,
        };

        fill_skia_rounded_rect(
            &mut pixmap,
            2.0,
            4.0,
            (toolbar_w as f32 - 4.0).max(1.0),
            68.0,
            16.0,
            [0, 0, 0, 72],
        );
        fill_skia_rounded_rect(
            &mut pixmap,
            0.0,
            0.0,
            toolbar_w as f32,
            72.0,
            16.0,
            [28, 36, 48, 232],
        );
        fill_skia_rounded_rect(
            &mut pixmap,
            1.0,
            1.0,
            (toolbar_w as f32 - 2.0).max(1.0),
            26.0,
            15.0,
            [255, 255, 255, 12],
        );
        stroke_skia_rounded_rect(
            &mut pixmap,
            0.5,
            0.5,
            (toolbar_w as f32 - 1.0).max(1.0),
            71.0,
            16.0,
            1.0,
            [220, 232, 248, 32],
        );

        let close_x = SCREEN_DRAW_TOOLBAR_CLOSE_X as f32;
        fill_skia_rounded_rect(&mut pixmap, close_x, 10.0, 26.0, 20.0, 8.0, [82, 96, 120, 214]);
        draw_skia_line(
            &mut pixmap,
            close_x + 7.0,
            16.0,
            close_x + 19.0,
            24.0,
            [255, 255, 255, 255],
            2.0,
        );
        draw_skia_line(
            &mut pixmap,
            close_x + 19.0,
            16.0,
            close_x + 7.0,
            24.0,
            [255, 255, 255, 255],
            2.0,
        );

        fill_skia_rounded_rect(&mut pixmap, 13.0, 19.0, 38.0, 38.0, 11.0, [236, 244, 255, 84]);
        fill_skia_rounded_rect(
            &mut pixmap,
            15.0,
            21.0,
            34.0,
            34.0,
            10.0,
            [color.r, color.g, color.b, color.a],
        );
        stroke_skia_rounded_rect(&mut pixmap, 14.5, 20.5, 35.0, 35.0, 10.0, 1.0, [255, 255, 255, 48]);

        draw_screen_draw_slider_skia(
            &mut pixmap,
            68.0,
            38.0,
            90.0,
            ((brush_size - 2.0) / 78.0).clamp(0.0, 1.0),
        );

        let eraser_fill = if eraser {
            [100, 188, 156, 255]
        } else {
            [76, 90, 112, 220]
        };
        fill_skia_rounded_rect(&mut pixmap, 172.0, 20.0, 36.0, 36.0, 10.0, eraser_fill);
        stroke_skia_rounded_rect(&mut pixmap, 172.5, 20.5, 35.0, 35.0, 10.0, 1.0, [255, 255, 255, 34]);
        {
            let mut pb = tiny_skia::PathBuilder::new();
            pb.move_to(182.0, 42.0);
            pb.line_to(194.0, 30.0);
            pb.line_to(201.0, 37.0);
            pb.line_to(189.0, 49.0);
            pb.close();
            if let Some(path) = pb.finish() {
                let mut paint = tiny_skia::Paint::default();
                paint.set_color(tiny_skia::Color::from_rgba8(255, 255, 255, 248));
                paint.anti_alias = true;
                pixmap.fill_path(
                    &path,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
        }
        draw_skia_line(&mut pixmap, 186.0, 45.0, 196.0, 45.0, [62, 74, 92, 255], 2.0);

        let smooth_fill = if smoothing {
            [100, 188, 156, 255]
        } else {
            [76, 90, 112, 220]
        };
        fill_skia_rounded_rect(&mut pixmap, 224.0, 24.0, 28.0, 28.0, 8.0, smooth_fill);
        stroke_skia_rounded_rect(&mut pixmap, 224.5, 24.5, 27.0, 27.0, 8.0, 1.0, [255, 255, 255, 34]);
        if smoothing {
            draw_skia_line(&mut pixmap, 230.0, 38.0, 237.0, 45.0, [255, 255, 255, 255], 2.0);
            draw_skia_line(&mut pixmap, 237.0, 45.0, 247.0, 31.0, [255, 255, 255, 255], 2.0);
        } else {
            draw_skia_line(&mut pixmap, 230.0, 40.0, 246.0, 34.0, [255, 255, 255, 210], 2.0);
            draw_skia_line(&mut pixmap, 230.0, 36.0, 246.0, 40.0, [255, 255, 255, 160], 1.4);
        }

        draw_screen_draw_slider_skia(
            &mut pixmap,
            264.0,
            38.0,
            54.0,
            smoothing_amount.clamp(0.0, 1.0),
        );

        fill_skia_rounded_rect(
            &mut pixmap,
            SCREEN_DRAW_TOOLBAR_CAPTURE_X as f32,
            20.0,
            36.0,
            36.0,
            10.0,
            [86, 106, 132, 224],
        );
        stroke_skia_rounded_rect(
            &mut pixmap,
            SCREEN_DRAW_TOOLBAR_CAPTURE_X as f32 + 0.5,
            20.5,
            35.0,
            35.0,
            10.0,
            1.0,
            [255, 255, 255, 34],
        );
        fill_skia_rounded_rect(
            &mut pixmap,
            SCREEN_DRAW_TOOLBAR_CAPTURE_X as f32 + 8.0,
            28.0,
            20.0,
            14.0,
            4.0,
            [240, 246, 255, 246],
        );
        draw_skia_line(
            &mut pixmap,
            SCREEN_DRAW_TOOLBAR_CAPTURE_X as f32 + 12.0,
            28.0,
            SCREEN_DRAW_TOOLBAR_CAPTURE_X as f32 + 16.0,
            24.0,
            [240, 246, 255, 246],
            2.0,
        );
        draw_skia_line(
            &mut pixmap,
            SCREEN_DRAW_TOOLBAR_CAPTURE_X as f32 + 16.0,
            24.0,
            SCREEN_DRAW_TOOLBAR_CAPTURE_X as f32 + 22.0,
            24.0,
            [240, 246, 255, 246],
            2.0,
        );
        draw_skia_circle_fill(
            &mut pixmap,
            SCREEN_DRAW_TOOLBAR_CAPTURE_X as f32 + 18.0,
            35.0,
            4.6,
            [74, 98, 128, 255],
        );
        draw_skia_circle_outline(
            &mut pixmap,
            SCREEN_DRAW_TOOLBAR_CAPTURE_X as f32 + 18.0,
            35.0,
            4.6,
            [255, 255, 255, 196],
            1.0,
        );

        let data = pixmap.data();
        let base_x = toolbar_x.max(0) as usize;
        let base_y = toolbar_y.max(0) as usize;
        for py in 0..toolbar_h {
            let dst_y = base_y + py;
            if dst_y >= height {
                break;
            }
            for px in 0..toolbar_w {
                let dst_x = base_x + px;
                if dst_x >= width {
                    break;
                }
                let src_offset = (py * toolbar_w + px) * 4;
                let src_r = data[src_offset];
                let src_g = data[src_offset + 1];
                let src_b = data[src_offset + 2];
                let src_a = data[src_offset + 3];
                if src_a == 0 {
                    continue;
                }
                let dst_offset = (dst_y * width + dst_x) * 4;
                blend_premultiplied_rgba(
                    &mut pixels[dst_offset..dst_offset + 4],
                    src_r,
                    src_g,
                    src_b,
                    src_a,
                );
            }
        }
    }

    fn refresh_mouse_record_trail(runtime: &mut Runtime) -> Result<()> {
        let (points, marker) = {
            let mut recording_guard = MOUSE_RECORDING.lock();
            if let Some(session) = recording_guard.as_mut() {
                if !session.dirty {
                    return Ok(());
                }

                session.dirty = false;
                (
                    session
                    .events
                    .iter()
                    .filter(|event| matches!(event.kind, MousePathEventKind::Move))
                    .map(|event| POINT {
                        x: event.x,
                        y: event.y,
                    })
                    .collect::<Vec<_>>(),
                    None,
                )
            } else {
                drop(recording_guard);
                let mut preview_guard = MOUSE_PATH_PREVIEW.lock();
                let Some(session) = preview_guard.as_mut() else {
                    unsafe {
                        let _ = ShowWindow(runtime.mouse_trail_hwnd, SW_HIDE);
                    }
                    return Ok(());
                };
                if let Some(started_at) = session.playback_started_at {
                    let elapsed_ms = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
                    let target_ms = session.playback_from_ms.saturating_add(elapsed_ms);
                    let mut accumulated_ms = 0u64;
                    let mut marker = None;
                    let mut all_points = Vec::new();
                    for event in &session.events {
                        accumulated_ms = accumulated_ms.saturating_add(event.delay_ms);
                        if matches!(event.kind, MousePathEventKind::Move) {
                            let point = POINT { x: event.x, y: event.y };
                            all_points.push(point);
                            if accumulated_ms >= target_ms && marker.is_none() {
                                marker = Some(point);
                            }
                        }
                    }
                    if marker.is_none() {
                        marker = all_points.last().copied();
                        session.playback_started_at = None;
                    }
                    session.playback_marker = marker;
                    session.points = all_points;
                    session.dirty = true;
                }
                if !session.dirty {
                    return Ok(());
                }

                session.dirty = false;
                (session.points.clone(), session.playback_marker)
            }
        };
        if points.is_empty() {
            unsafe {
                let _ = ShowWindow(runtime.mouse_trail_hwnd, SW_HIDE);
            }

            return Ok(());
        }

        unsafe { paint_mouse_trail(runtime.mouse_trail_hwnd, &points, marker) }
    }

    fn refresh_search_area_overlay(runtime: &mut Runtime) -> Result<()> {
        let (regions, preview_regions, static_geometry_shapes, dynamic_geometry_shapes) = {
            let mut hook_state = HOOK_STATE.lock();
            // Clear expired geometries
            let now = Instant::now();
            let mut expired = Vec::new();
            for (key, expires_at) in &hook_state.active_geometry_steps_expires {
                if now >= *expires_at {
                    expired.push(*key);
                }
            }
            for key in &expired {
                hook_state.active_geometry_steps.remove(key);
                hook_state.rendered_geometry_steps.remove(key);
                hook_state.active_geometry_steps_expires.remove(key);
            }

            let mut expired_geometry_owners = Vec::new();
            for (owner, expires_at) in &hook_state.active_geometry_preset_owner_expires {
                if now >= *expires_at {
                    expired_geometry_owners.push(*owner);
                }
            }
            if !expired_geometry_owners.is_empty() {
                for owner in &expired_geometry_owners {
                    remove_active_geometry_preset_owner(&mut hook_state, *owner);
                }
                rebuild_active_geometry_preset_ids(&mut hook_state);
            }

            let regions = hook_state
                .vision_presets
                .iter()
                .filter(|preset| preset.show_search_region_overlay)
                .filter_map(|preset| configured_image_search_region(preset))
                .collect::<Vec<_>>();

            (
                regions,
                hook_state.vision_capture_preview_regions.clone(),
                geometry_overlay_static_shapes(&mut hook_state),
                geometry_overlay_dynamic_shapes(&mut hook_state),
            )
        };
        // Check active crosshair expiration
        let crosshair_expired = {
            let mut hook_state = HOOK_STATE.lock();
            if let Some(exp) = hook_state.active_crosshair_expires {
                if Instant::now() >= exp {
                    hook_state.active_crosshair_expires = None;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if crosshair_expired {
            disable_crosshair_overlay();
        }

        // Check active pin expiration
        let pin_expired = {
            let mut hook_state = HOOK_STATE.lock();
            if let Some(exp) = hook_state.active_pin_expires {
                if Instant::now() >= exp {
                    hook_state.active_pin_expires = None;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if pin_expired {
            disable_pin_overlay();
        }

        // Check active video expiration
        let video_expired = {
            let mut expires_guard = ACTIVE_VIDEO_EXPIRES.lock();
            if let Some(exp) = *expires_guard {
                if Instant::now() >= exp {
                    *expires_guard = None;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if video_expired {
            stop_active_video_preset_playback();
        }

        let search_layer_is_empty = regions.is_empty()
            && preview_regions.is_empty()
            && static_geometry_shapes.is_empty();
        let dynamic_layer_is_empty = dynamic_geometry_shapes.is_empty();

        if search_layer_is_empty {
            if runtime.search_area_overlay_visible {
                unsafe {
                    let _ = ShowWindow(runtime.search_area_hwnd, SW_HIDE);
                }
                runtime.search_area_overlay_visible = false;
            }
            runtime.cached_search_overlay_regions.clear();
            runtime.cached_search_overlay_preview_regions.clear();
            runtime.cached_search_overlay_static_geometry.clear();
        } else {
            let static_changed = !runtime.search_area_overlay_visible
                || runtime.cached_search_overlay_regions != regions
                || runtime.cached_search_overlay_preview_regions != preview_regions
                || runtime.cached_search_overlay_static_geometry != static_geometry_shapes;
            if static_changed {
                unsafe {
                    paint_search_area_overlay(
                        runtime.search_area_hwnd,
                        &regions,
                        &preview_regions,
                        &static_geometry_shapes,
                        &[],
                    )?;
                }
                runtime.cached_search_overlay_regions = regions.clone();
                runtime.cached_search_overlay_preview_regions = preview_regions.clone();
                runtime.cached_search_overlay_static_geometry = static_geometry_shapes.clone();
                runtime.search_area_overlay_visible = true;
            }
        }

        if dynamic_layer_is_empty {
            if runtime.dynamic_geometry_overlay_visible {
                unsafe {
                    let _ = ShowWindow(runtime.dynamic_geometry_hwnd, SW_HIDE);
                }
                runtime.dynamic_geometry_overlay_visible = false;
            }
        } else {
            unsafe {
                paint_search_area_overlay(
                    runtime.dynamic_geometry_hwnd,
                    &[],
                    &[],
                    &[],
                    &dynamic_geometry_shapes,
                )?;
            }
            runtime.dynamic_geometry_overlay_visible = true;
        }

        Ok(())
    }

    fn desired_timer_interval_ms(runtime: &Runtime) -> u32 {
        if runtime.native_focus_highlight_enabled
            && runtime.focus_highlight_rainbow
            && runtime.active_focus_highlight_hwnd.is_some()
        {
            return 30;
        }

        let capture_active = {
            let hook_state = HOOK_STATE.lock();
            !hook_state.vision_capture_preview_regions.is_empty()
                || hook_state.vision_capture_mouse_blocked
        };
        if capture_active {
            return 16;
        }

        let timer_interval = {
            let hook_state = HOOK_STATE.lock();
            let mut min_interval = None;
            // Check preview timer preset

            if let Some(ref preview) = runtime.preview_timer_preset {
                let fps = preview.progress_smoothness_fps.clamp(5, 120);
                let interval = 1000 / fps;
                min_interval = Some(min_interval.unwrap_or(interval).min(interval));
            }

            // Check running active timers

            for preset in &hook_state.timer_presets {
                if let Some(state) = hook_state.active_timers.get(&preset.id) {
                    if state.running {
                        let fps = preset.progress_smoothness_fps.clamp(5, 120);
                        let interval = 1000 / fps;
                        min_interval = Some(min_interval.unwrap_or(interval).min(interval));
                    }
                }
            }

            min_interval
        };
        if let Some(interval) = timer_interval {
            return interval;
        }

        let recording_active = MOUSE_RECORDING.lock().is_some()
            || MACRO_RECORDING.lock().is_some()
            || MOUSE_PATH_PREVIEW.lock().is_some();
        if recording_active {
            return 16;
        }

        if runtime.quick_key_display_enabled
            && (!runtime.quick_key_display_entries.is_empty()
                || runtime.quick_key_display_hide_at.is_some())
        {
            return 33;
        }

        if is_ui_in_foreground() {
            return 100;
        }

        let toolbox_active = HUD_DISPLAY.lock().is_some()
            || HUD_PREVIEW_DISPLAY.lock().is_some()
            || runtime.hud_display.is_some();
        if toolbox_active {
            return 100;
        }

        let pin_active = runtime.active_pin_thumbnail.is_some()
            || HOOK_STATE.lock().active_pin_preset_id.is_some();
        if pin_active {
            return 33;
        }

        if keyboard_arrow_mouse_is_active() {
            return 12;
        }

        if HOOK_STATE.lock().keyboard_arrow_mouse_enabled {
            return 33;
        }

        750
    }

    fn desired_hooks_enabled(_runtime: &Runtime) -> bool {
        true
    }

    const DWM_COLOR_DEFAULT: u32 = 0xFFFF_FFFF;
    const FOCUS_HIGHLIGHT_BORDER_COLOR: u32 = 0x00B6_E07E;
    const QUICK_KEY_DISPLAY_HIDE_DELAY: Duration = Duration::from_millis(700);

    unsafe fn set_native_border_color(hwnd: HWND, color: u32) -> bool {
        if hwnd.0.is_null() {
            return false;
        }

        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &color as *const _ as *const _,
            size_of::<u32>() as u32,
        )
        .is_ok()
    }

    unsafe fn is_native_focus_highlight_target(hwnd: HWND) -> bool {
        if hwnd.0.is_null() || is_internal_app_window(hwnd) || looks_like_main_ui_window(hwnd) {
            return false;
        }

        if !windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() {
            return false;
        }

        let root = GetAncestor(hwnd, GA_ROOT);
        if root.0.is_null() || root != hwnd {
            return false;
        }

        true
    }

    unsafe fn clear_native_focus_highlight(runtime: &mut Runtime) {
        if let Some(previous) = runtime.active_focus_highlight_hwnd.take() {
            let _ = set_native_border_color(previous, DWM_COLOR_DEFAULT);
        }
        ACTIVE_HIGHLIGHT_HWND.store(0, Ordering::Relaxed);
        sync_window_location_hook_state(runtime);
        let _ = ShowWindow(runtime.focus_highlight_hwnd, SW_HIDE);
    }

    fn angle_between(angle: f32, start: f32, end: f32) -> bool {
        let mut s = start % 360.0;
        if s < 0.0 { s += 360.0; }
        let mut e = end % 360.0;
        if e < 0.0 { e += 360.0; }
        let mut a = angle % 360.0;
        if a < 0.0 { a += 360.0; }

        if s <= e {
            a >= s && a <= e
        } else {
            a >= s || a <= e
        }
    }

    fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 4] {
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;
        let (r, g, b) = if h < 60.0 {
            (c, x, 0.0)
        } else if h < 120.0 {
            (x, c, 0.0)
        } else if h < 180.0 {
            (0.0, c, x)
        } else if h < 240.0 {
            (0.0, x, c)
        } else if h < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };
        [
            ((r + m) * 255.0) as u8,
            ((g + m) * 255.0) as u8,
            ((b + m) * 255.0) as u8,
            235,
        ]
    }

    fn blend_rgba_pixel(buf: &mut [u8], w: usize, _h: usize, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 { return; }
        let (x, y) = (x as usize, y as usize);
        if x >= w { return; }
        let off = (y * w + x) * 4;
        if off + 3 >= buf.len() { return; }
        let sa = color[3] as u32;
        let da = buf[off + 3] as u32;
        let out_a = sa + da * (255 - sa) / 255;
        if out_a == 0 { return; }
        buf[off]     = ((color[0] as u32 * sa + buf[off]     as u32 * da * (255 - sa) / 255) / out_a) as u8;
        buf[off + 1] = ((color[1] as u32 * sa + buf[off + 1] as u32 * da * (255 - sa) / 255) / out_a) as u8;
        buf[off + 2] = ((color[2] as u32 * sa + buf[off + 2] as u32 * da * (255 - sa) / 255) / out_a) as u8;
        buf[off + 3] = out_a as u8;
    }

    fn draw_line_rgba(buf: &mut [u8], w: usize, h: usize, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 4]) {
        let (mut x0, mut y0) = (x0, y0);
        let (x1, y1) = (x1, y1);
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        let sx = if x0 < x1 { 1i32 } else { -1i32 };
        let sy = if y0 < y1 { 1i32 } else { -1i32 };
        let mut err = dx - dy;
        loop {
            blend_rgba_pixel(buf, w, h, x0, y0, color);
            if x0 == x1 && y0 == y1 { break; }
            let e2 = 2 * err;
            if e2 > -dy { err -= dy; x0 += sx; }
            if e2 < dx  { err += dx; y0 += sy; }
        }
    }

    fn draw_line_thick_rgba(buf: &mut [u8], w: usize, h: usize, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 4], thickness: i32) {
        let half = thickness / 2;
        for t in -half..=half {
            let len = ((x1 - x0).pow(2) + (y1 - y0).pow(2)) as f32;
            if len < 0.001 { break; }
            let nx = -(y1 - y0) as f32 / len.sqrt();
            let ny =  (x1 - x0) as f32 / len.sqrt();
            let ox = (nx * t as f32).round() as i32;
            let oy = (ny * t as f32).round() as i32;
            draw_line_rgba(buf, w, h, x0 + ox, y0 + oy, x1 + ox, y1 + oy, color);
        }
    }

    fn fill_ellipse_rgba(buf: &mut [u8], w: usize, h: usize, bx: i32, by: i32, bw: i32, bh: i32, color: [u8; 4]) {
        let cx = bx + bw / 2;
        let cy = by + bh / 2;
        let rx = (bw / 2).max(1) as f32;
        let ry = (bh / 2).max(1) as f32;
        for py in by..by + bh {
            for px in bx..bx + bw {
                let dx = (px - cx) as f32 / rx;
                let dy = (py - cy) as f32 / ry;
                if dx * dx + dy * dy <= 1.0 {
                    blend_rgba_pixel(buf, w, h, px, py, color);
                }
            }
        }
    }

    fn draw_ellipse_outline_thick_rgba(buf: &mut [u8], w: usize, h: usize, bx: i32, by: i32, bw: i32, bh: i32, color: [u8; 4], thickness: i32) {
        let cx = bx + bw / 2;
        let cy = by + bh / 2;
        let rx = (bw / 2).max(1) as f32;
        let ry = (bh / 2).max(1) as f32;
        let steps = ((rx.max(ry) * std::f32::consts::PI * 2.0) as i32).max(64);
        for i in 0..steps {
            let t = (i as f32 / steps as f32) * std::f32::consts::PI * 2.0;
            let x = cx + (rx * t.cos()) as i32;
            let y = cy + (ry * t.sin()) as i32;
            for tx in -thickness..=thickness {
                for ty in -thickness..=thickness {
                    if tx * tx + ty * ty <= thickness * thickness {
                        blend_rgba_pixel(buf, w, h, x + tx, y + ty, color);
                    }
                }
            }
        }
    }

    fn fill_rect_rgba(buf: &mut [u8], w: usize, h: usize, x: i32, y: i32, rw: i32, rh: i32, color: [u8; 4]) {
        for py in y..y + rh {
            for px in x..x + rw {
                blend_rgba_pixel(buf, w, h, px, py, color);
            }
        }
    }

    fn draw_skia_line(
        pixmap: &mut tiny_skia::Pixmap,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: [u8; 4],
        stroke_width: f32,
    ) {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        if let Some(path) = pb.finish() {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(color[0], color[1], color[2], color[3]));
            paint.anti_alias = true;
            let mut stroke = tiny_skia::Stroke::default();
            stroke.width = stroke_width;
            pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
        }
    }

    fn draw_skia_circle_outline(
        pixmap: &mut tiny_skia::Pixmap,
        cx: f32,
        cy: f32,
        radius: f32,
        color: [u8; 4],
        stroke_width: f32,
    ) {
        let mut pb = tiny_skia::PathBuilder::new();
        let steps = 180;
        for i in 0..steps {
            let angle = (i as f32 / steps as f32) * std::f32::consts::TAU;
            let px = cx + radius * angle.cos();
            let py = cy + radius * angle.sin();
            if i == 0 {
                pb.move_to(px, py);
            } else {
                pb.line_to(px, py);
            }
        }
        pb.close();
        if let Some(path) = pb.finish() {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(color[0], color[1], color[2], color[3]));
            paint.anti_alias = true;
            let mut stroke = tiny_skia::Stroke::default();
            stroke.width = stroke_width;
            pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
        }
    }

    fn draw_skia_circle_fill(
        pixmap: &mut tiny_skia::Pixmap,
        cx: f32,
        cy: f32,
        radius: f32,
        color: [u8; 4],
    ) {
        let mut pb = tiny_skia::PathBuilder::new();
        let steps = 120;
        for i in 0..steps {
            let angle = (i as f32 / steps as f32) * std::f32::consts::TAU;
            let px = cx + radius * angle.cos();
            let py = cy + radius * angle.sin();
            if i == 0 {
                pb.move_to(px, py);
            } else {
                pb.line_to(px, py);
            }
        }
        pb.close();
        if let Some(path) = pb.finish() {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(color[0], color[1], color[2], color[3]));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
        }
    }

    fn draw_skia_rect_fill(
        pixmap: &mut tiny_skia::Pixmap,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        color: [u8; 4],
    ) {
        if let Some(rect) = tiny_skia::Rect::from_xywh(left, top, width, height) {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(color[0], color[1], color[2], color[3]));
            paint.anti_alias = true;
            pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
        }
    }

    fn draw_skia_rect_outline(
        pixmap: &mut tiny_skia::Pixmap,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        color: [u8; 4],
        stroke_width: f32,
    ) {
        let r = left + width;
        let b = top + height;
        draw_skia_line(pixmap, left, top, r, top, color, stroke_width);
        draw_skia_line(pixmap, r, top, r, b, color, stroke_width);
        draw_skia_line(pixmap, r, b, left, b, color, stroke_width);
        draw_skia_line(pixmap, left, b, left, top, color, stroke_width);
    }

    unsafe fn paint_protractor_overlay(runtime: &Runtime) -> Result<()> {
        let (scale, needle1, needle2, cx_val, cy_val, thickness, calibrating, ui_language) = {
            let state = PROTRACTOR_STATE.lock();
            (state.scale, state.needle1_angle, state.needle2_angle, state.center_x, state.center_y, state.thickness, state.calibrating, state.ui_language)
        };

        let base_radius = 150.0;
        let radius = (scale * base_radius) as i32;
        let padding = (scale * 30.0) as i32;
        let half_size = radius + padding;
        let size = 2 * half_size;
        let width = size.max(1) as u32;
        let height = size.max(1) as u32;

        let win_x = cx_val - half_size;
        let win_y = cy_val - half_size;

        let _ = SetWindowPos(
            runtime.protractor_hwnd,
            Some(HWND_TOPMOST),
            win_x,
            win_y,
            width as i32,
            height as i32,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );

        let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
        let cx = half_size;
        let cy = half_size;

        // 1. Draw angular sector fill between needles
        let mut pb_sector = tiny_skia::PathBuilder::new();
        pb_sector.move_to(cx as f32, cy as f32);
        let mut s = needle1 % 360.0;
        if s < 0.0 { s += 360.0; }
        let mut e = needle2 % 360.0;
        if e < 0.0 { e += 360.0; }
        let mut diff = e - s;
        if diff < 0.0 { diff += 360.0; }
        let sector_steps = (diff.abs() as i32).max(1);
        for i in 0..=sector_steps {
            let deg = s + (diff * (i as f32 / sector_steps as f32));
            let rad = deg.to_radians();
            let px = cx as f32 + radius as f32 * rad.cos();
            let py = cy as f32 + radius as f32 * rad.sin();
            pb_sector.line_to(px, py);
        }
        pb_sector.close();
        if let Some(path) = pb_sector.finish() {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(0, 160, 255, 40));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
        }

        // 2. Draw outer circle
        // Draw black outline backing first for high contrast
        draw_skia_circle_outline(
            &mut pixmap,
            cx as f32,
            cy as f32,
            radius as f32,
            [0, 0, 0, 255],
            thickness + 2.0,
        );
        // Draw white outline
        draw_skia_circle_outline(
            &mut pixmap,
            cx as f32,
            cy as f32,
            radius as f32,
            [255, 255, 255, 255],
            thickness,
        );

        // 3. Draw tick marks every 5 degrees
        for deg in 0..360 {
            if deg % 5 == 0 {
                let len = if deg % 90 == 0 { (15.0 * scale) as i32 }
                          else if deg % 10 == 0 { (10.0 * scale) as i32 }
                          else { (5.0 * scale) as i32 };
                let thick = if deg % 10 == 0 { 2.0 } else { 1.0 };
                
                let rad = (deg as f32).to_radians();
                let r_in = radius - len;
                let x0 = cx as f32 + r_in as f32 * rad.cos();
                let y0 = cy as f32 + r_in as f32 * rad.sin();
                let x1 = cx as f32 + radius as f32 * rad.cos();
                let y1 = cy as f32 + radius as f32 * rad.sin();
                
                // Draw black backing for high contrast
                draw_skia_line(&mut pixmap, x0, y0, x1, y1, [0, 0, 0, 220], thick + 1.5);
                // Draw white tick
                let color = if deg % 90 == 0 { [255, 255, 255, 255] } else { [255, 255, 255, 220] };
                draw_skia_line(&mut pixmap, x0, y0, x1, y1, color, thick);
            }
        }

        // 4. Center crosshair
        draw_skia_circle_fill(&mut pixmap, cx as f32, cy as f32, 4.0, [0, 0, 0, 255]);
        draw_skia_circle_fill(&mut pixmap, cx as f32, cy as f32, 2.0, [255, 92, 141, 255]);
        // Black backing
        draw_skia_line(&mut pixmap, cx as f32 - 12.0, cy as f32, cx as f32 + 12.0, cy as f32, [0, 0, 0, 255], 3.0);
        draw_skia_line(&mut pixmap, cx as f32, cy as f32 - 12.0, cx as f32, cy as f32 + 12.0, [0, 0, 0, 255], 3.0);
        // White crosshair
        draw_skia_line(&mut pixmap, cx as f32 - 12.0, cy as f32, cx as f32 + 12.0, cy as f32, [255, 255, 255, 255], 1.0);
        draw_skia_line(&mut pixmap, cx as f32, cy as f32 - 12.0, cx as f32, cy as f32 + 12.0, [255, 255, 255, 255], 1.0);

        // 5. Needle 1 & handle
        let rad1 = (needle1 as f32).to_radians();
        let n1x = cx as f32 + radius as f32 * rad1.cos();
        let n1y = cy as f32 + radius as f32 * rad1.sin();
        // Black backing
        draw_skia_line(&mut pixmap, cx as f32, cy as f32, n1x, n1y, [0, 0, 0, 255], thickness + 2.0);
        draw_skia_line(&mut pixmap, cx as f32, cy as f32, n1x, n1y, [0, 220, 255, 255], thickness);
        draw_skia_circle_fill(&mut pixmap, n1x, n1y, 7.5, [0, 0, 0, 255]);
        draw_skia_circle_fill(&mut pixmap, n1x, n1y, 6.0, [0, 220, 255, 255]);
        draw_skia_circle_outline(&mut pixmap, n1x, n1y, 6.0, [255, 255, 255, 255], 1.5);

        // 6. Needle 2 & handle
        let rad2 = (needle2 as f32).to_radians();
        let n2x = cx as f32 + radius as f32 * rad2.cos();
        let n2y = cy as f32 + radius as f32 * rad2.sin();
        // Black backing
        draw_skia_line(&mut pixmap, cx as f32, cy as f32, n2x, n2y, [0, 0, 0, 255], thickness + 2.0);
        draw_skia_line(&mut pixmap, cx as f32, cy as f32, n2x, n2y, [255, 92, 141, 255], thickness);
        draw_skia_circle_fill(&mut pixmap, n2x, n2y, 7.5, [0, 0, 0, 255]);
        draw_skia_circle_fill(&mut pixmap, n2x, n2y, 6.0, [255, 92, 141, 255]);
        draw_skia_circle_outline(&mut pixmap, n2x, n2y, 6.0, [255, 255, 255, 255], 1.5);

        // 7. Resize Grip handle
        let rad_g = (-45.0_f32).to_radians();
        let gx = cx as f32 + radius as f32 * rad_g.cos();
        let gy = cy as f32 + radius as f32 * rad_g.sin();
        draw_skia_circle_fill(&mut pixmap, gx, gy, 7.0, [160, 160, 160, 220]);
        draw_skia_circle_outline(&mut pixmap, gx, gy, 7.0, [255, 255, 255, 255], 1.0);

        // 8. Close Button
        draw_skia_rect_fill(&mut pixmap, size as f32 - 24.0, 8.0, 16.0, 16.0, [255, 80, 80, 220]);
        draw_skia_line(&mut pixmap, size as f32 - 21.0, 11.0, size as f32 - 11.0, 21.0, [255, 255, 255, 255], 2.0);
        draw_skia_line(&mut pixmap, size as f32 - 11.0, 11.0, size as f32 - 21.0, 21.0, [255, 255, 255, 255], 2.0);

        // 9. Calibration Button (background/border)
        let btn_bg_color = if calibrating {
            [255, 80, 80, 200]
        } else {
            [0, 160, 255, 180]
        };
        draw_skia_rect_fill(&mut pixmap, 8.0, 8.0, 80.0, 20.0, btn_bg_color);
        draw_skia_rect_outline(&mut pixmap, 8.0, 8.0, 80.0, 20.0, [255, 255, 255, 255], 1.0);

        // 10. Thickness Slider
        let slider_left = cx as f32 - 30.0;
        let slider_right = cx as f32 + 30.0;
        draw_skia_line(&mut pixmap, slider_left, size as f32 - 12.0, slider_right, size as f32 - 12.0, [200, 200, 200, 180], 1.0);
        let t_frac = ((thickness - 1.0) / 7.0).clamp(0.0, 1.0);
        let thumb_x = slider_left + t_frac * 60.0;
        draw_skia_circle_fill(&mut pixmap, thumb_x, size as f32 - 12.0, 4.0, [0, 220, 255, 255]);
        draw_skia_circle_outline(&mut pixmap, thumb_x, size as f32 - 12.0, 4.0, [255, 255, 255, 255], 1.0);

        // GDI render & Text setup
        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            bail!("Failed to acquire screen DC");
        }
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create memory DC");
        }

        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(
            Some(screen_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to map DIB section");
        }

        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        
        // Copy tiny-skia pixels to DIB section bits, swapping R and B channels to match BGRA
        let pixmap_data = pixmap.data();
        let bits_ptr = bits as *mut u8;
        let total_pixels = width as usize * height as usize;
        for i in 0..total_pixels {
            let offset = i * 4;
            let r = pixmap_data[offset];
            let g = pixmap_data[offset + 1];
            let b = pixmap_data[offset + 2];
            let a = pixmap_data[offset + 3];
            unsafe {
                *bits_ptr.add(offset) = b;
                *bits_ptr.add(offset + 1) = g;
                *bits_ptr.add(offset + 2) = r;
                *bits_ptr.add(offset + 3) = a;
            }
        }

        // Draw labels
        let angle_diff = (needle2 - needle1).abs();
        let angle_val = if angle_diff > 180.0 { 360.0 - angle_diff } else { angle_diff };
        let angle_str = format!("{:.1}°", angle_val);
        let center_str = format!("X:{} Y:{}", cx_val, cy_val);

        let p1_x = cx_val + (radius as f32 * rad1.cos()) as i32;
        let p1_y = cy_val + (radius as f32 * rad1.sin()) as i32;
        let p2_x = cx_val + (radius as f32 * rad2.cos()) as i32;
        let p2_y = cy_val + (radius as f32 * rad2.sin()) as i32;
        let p1_str = format!("P1: {}, {}", p1_x, p1_y);
        let p2_str = format!("P2: {}, {}", p2_x, p2_y);

        let font = CreateFontW(
            16, // Font size 16 pixels for larger coordinates
            0,
            0,
            0,
            FW_MEDIUM.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Segoe UI"),
        );
        let old_font = SelectObject(mem_dc, HGDIOBJ(font.0));

        let _ = SetBkMode(mem_dc, TRANSPARENT);
        let _ = SetTextColor(mem_dc, COLORREF(0xFFFFFF));

        let mut r_angle = RECT {
            left: cx - 80,
            top: cy - 40,
            right: cx + 80,
            bottom: cy - 22,
        };
        let mut w_angle = angle_str.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
        let _ = DrawTextW(mem_dc, &mut w_angle, &mut r_angle, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

        let mut r_center = RECT {
            left: cx - 80,
            top: cy - 20,
            right: cx + 80,
            bottom: cy - 2,
        };
        let mut w_center = center_str.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
        let _ = DrawTextW(mem_dc, &mut w_center, &mut r_center, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

        let mut r_p1 = RECT {
            left: cx - 80,
            top: cy + 6,
            right: cx + 80,
            bottom: cy + 24,
        };
        let mut w_p1 = p1_str.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
        let _ = DrawTextW(mem_dc, &mut w_p1, &mut r_p1, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

        let mut r_p2 = RECT {
            left: cx - 80,
            top: cy + 26,
            right: cx + 80,
            bottom: cy + 44,
        };
        let mut w_p2 = p2_str.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
        let _ = DrawTextW(mem_dc, &mut w_p2, &mut r_p2, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

        // Draw Calibration Button text on top
        let mut r_calib = RECT {
            left: 8,
            top: 8,
            right: 88,
            bottom: 28,
        };
        let calib_text = if calibrating {
            match ui_language {
                crate::model::UiLanguage::Vietnamese => "Huỷ",
                _ => "Cancel",
            }
        } else {
            match ui_language {
                crate::model::UiLanguage::Vietnamese => "3 Điểm",
                _ => "3 Points",
            }
        };
        let mut w_calib = calib_text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
        let _ = DrawTextW(mem_dc, &mut w_calib, &mut r_calib, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

        let _ = SelectObject(mem_dc, old_font);
        let _ = DeleteObject(HGDIOBJ(font.0));

        // Fix alpha of GDI text drawn and keep Skia's premultiplied alpha correct
        for i in 0..total_pixels {
            let offset = i * 4;
            let pixel = std::slice::from_raw_parts_mut(bits_ptr.add(offset), 4);
            if pixel[3] == 0 && (pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0) {
                let opacity = pixel[0]; // Use intensity from white text rendering as alpha
                pixel[3] = opacity;
                pixel[0] = opacity;
                pixel[1] = opacity;
            }
        }

        let destination = POINT { x: win_x, y: win_y };
        let source = POINT { x: 0, y: 0 };
        let sz = SIZE { cx: width as i32, cy: height as i32 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        let _ = UpdateLayeredWindow(
            runtime.protractor_hwnd,
            Some(screen_dc),
            Some(&destination),
            Some(&sz),
            Some(mem_dc),
            Some(&source),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        Ok(())
    }

    unsafe fn focus_highlight_rect(hwnd: HWND) -> Option<RECT> {
        let mut rect = RECT::default();
        let frame_ok = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut c_void,
            size_of::<RECT>() as u32,
        )
        .is_ok()
            && rect.right > rect.left
            && rect.bottom > rect.top;
        if frame_ok {
            return Some(rect);
        }

        if GetWindowRect(hwnd, &mut rect).is_ok()
            && rect.right > rect.left
            && rect.bottom > rect.top
        {
            Some(rect)
        } else {
            None
        }
    }

    unsafe fn paint_focus_highlight_overlay(runtime: &Runtime, target: HWND) -> Result<()> {
        let Some(rect) = focus_highlight_rect(target) else {
            let _ = ShowWindow(runtime.focus_highlight_hwnd, SW_HIDE);
            return Ok(());
        };

        let monitor = MonitorFromWindow(target, MONITOR_DEFAULTTONEAREST);
        let mut monitor_info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor_rect = if GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
            monitor_info.rcMonitor
        } else {
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            }
        };

        let margin = 5i32;
        let thickness = 4u32;
        let visible_left = (rect.left - margin).max(monitor_rect.left);
        let visible_top = (rect.top - margin).max(monitor_rect.top);
        let visible_right = (rect.right + margin).min(monitor_rect.right);
        let visible_bottom = (rect.bottom + margin).min(monitor_rect.bottom);
        if visible_right <= visible_left || visible_bottom <= visible_top {
            let _ = ShowWindow(runtime.focus_highlight_hwnd, SW_HIDE);
            return Ok(());
        }

        let width = (visible_right - visible_left).max(1) as u32;
        let height = (visible_bottom - visible_top).max(1) as u32;
        let mut canvas = RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 0]));
        let mut hue = runtime.focus_highlight_rainbow_hue;
        let color = if runtime.focus_highlight_rainbow {
            let rgb = hsv_to_rgb(hue * 360.0, 0.85, 0.95);
            image::Rgba(rgb)
        } else {
            image::Rgba([
                runtime.focus_highlight_color.r,
                runtime.focus_highlight_color.g,
                runtime.focus_highlight_color.b,
                runtime.focus_highlight_color.a,
            ])
        };

        // Draw the 4 edges of the border without scanning the entire inner area.
        // Top edge:
        for y in 0..height.min(thickness) {
            for x in 0..width {
                canvas.put_pixel(x, y, color);
            }
        }
        // Bottom edge:
        if height > thickness {
            let start_y = height.saturating_sub(thickness);
            for y in start_y..height {
                for x in 0..width {
                    canvas.put_pixel(x, y, color);
                }
            }
        }
        // Left edge:
        let vertical_start_y = thickness.min(height);
        let vertical_end_y = height.saturating_sub(thickness);
        if vertical_end_y > vertical_start_y {
            for y in vertical_start_y..vertical_end_y {
                for x in 0..width.min(thickness) {
                    canvas.put_pixel(x, y, color);
                }
            }
        }
        // Right edge:
        if width > thickness && vertical_end_y > vertical_start_y {
            let start_x = width.saturating_sub(thickness);
            for y in vertical_start_y..vertical_end_y {
                for x in start_x..width {
                    canvas.put_pixel(x, y, color);
                }
            }
        }

        paint_crosshair_canvas(
            runtime.focus_highlight_hwnd,
            canvas,
            visible_left,
            visible_top,
        )?;
        let _ = ShowWindow(runtime.focus_highlight_hwnd, SW_SHOWNA);
        Ok(())
    }

    unsafe fn update_native_focus_highlight(runtime: &mut Runtime, foreground: HWND) {
        if !runtime.native_focus_highlight_enabled {
            clear_native_focus_highlight(runtime);
            return;
        }

        if runtime.active_focus_highlight_hwnd == Some(foreground) {
            return;
        }

        clear_native_focus_highlight(runtime);
        if is_native_focus_highlight_target(foreground) {
            let _ = set_native_border_color(foreground, FOCUS_HIGHLIGHT_BORDER_COLOR);
            let _ = paint_focus_highlight_overlay(runtime, foreground);
            runtime.active_focus_highlight_hwnd = Some(foreground);
            ACTIVE_HIGHLIGHT_HWND.store(foreground.0 as isize, Ordering::Relaxed);
            sync_window_location_hook_state(runtime);
        }
    }

    unsafe fn set_window_focus_event_hook_enabled(
        runtime: &mut Runtime,
        enabled: bool,
    ) -> Result<()> {
        if enabled {
            if runtime.window_focus_event_hook.0.is_null() {
                runtime.window_focus_event_hook = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(window_focus_event_proc),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                if runtime.window_focus_event_hook.0.is_null() {
                    bail!("Failed to register window focus event hook");
                }
            }
        } else if !runtime.window_focus_event_hook.0.is_null() {
            let _ = UnhookWinEvent(runtime.window_focus_event_hook);
            runtime.window_focus_event_hook = HWINEVENTHOOK::default();
        }

        Ok(())
    }

    unsafe extern "system" fn window_focus_event_proc(
        _hook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        _id_object: i32,
        _id_child: i32,
        _event_thread: u32,
        _event_time: u32,
    ) {
        if event != EVENT_SYSTEM_FOREGROUND {
            return;
        }

        let controller = HWND(CONTROLLER_HWND.load(Ordering::Relaxed) as *mut c_void);
        if !controller.0.is_null() {
            let _ = PostMessageW(
                Some(controller),
                WMAPP_WINDOW_FOCUS_CHANGED,
                WPARAM(hwnd.0 as usize),
                LPARAM(0),
            );
        }
    }

    const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;

    unsafe fn sync_window_location_hook_state(runtime: &mut Runtime) {
        let active_highlight = ACTIVE_HIGHLIGHT_HWND.load(Ordering::Relaxed);
        let active_pin = ACTIVE_PIN_SOURCE_HWND.load(Ordering::Relaxed);
        let need_hook = active_highlight != 0 || active_pin != 0;
        let _ = set_window_location_event_hook_enabled(runtime, need_hook);
    }

    unsafe fn set_window_location_event_hook_enabled(
        runtime: &mut Runtime,
        enabled: bool,
    ) -> Result<()> {
        if enabled {
            if runtime.window_location_event_hook.0.is_null() {
                runtime.window_location_event_hook = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(window_location_event_proc),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                if runtime.window_location_event_hook.0.is_null() {
                    bail!("Failed to register window location event hook");
                }
            }
        } else if !runtime.window_location_event_hook.0.is_null() {
            let _ = UnhookWinEvent(runtime.window_location_event_hook);
            runtime.window_location_event_hook = HWINEVENTHOOK::default();
        }

        Ok(())
    }

    unsafe extern "system" fn window_location_event_proc(
        _hook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        id_object: i32,
        _id_child: i32,
        _event_thread: u32,
        _event_time: u32,
    ) {
        if event != EVENT_OBJECT_LOCATIONCHANGE || id_object != 0 {
            return;
        }

        let active_hwnd = ACTIVE_HIGHLIGHT_HWND.load(Ordering::Relaxed);
        let pin_source_hwnd = ACTIVE_PIN_SOURCE_HWND.load(Ordering::Relaxed);

        let is_target = (active_hwnd != 0 && hwnd.0 as isize == active_hwnd)
            || (pin_source_hwnd != 0 && hwnd.0 as isize == pin_source_hwnd);

        if !is_target {
            return;
        }

        let controller = HWND(CONTROLLER_HWND.load(Ordering::Relaxed) as *mut c_void);
        if !controller.0.is_null() {
            let _ = PostMessageW(
                Some(controller),
                WMAPP_WINDOW_LOCATION_CHANGED,
                WPARAM(hwnd.0 as usize),
                LPARAM(0),
            );
        }
    }

    unsafe fn set_input_hooks_enabled(runtime: &mut Runtime, enabled: bool) -> Result<()> {
        let instance = GetModuleHandleW(None)?;
        if enabled {
            if runtime.keyboard_hook.0.is_null() {
                runtime.keyboard_hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(low_level_keyboard_proc),
                    Some(instance.into()),
                    0,
                )?;
            }

            if runtime.mouse_hook.0.is_null() {
                runtime.mouse_hook = SetWindowsHookExW(
                    WH_MOUSE_LL,
                    Some(low_level_mouse_proc),
                    Some(instance.into()),
                    0,
                )?;
            }
        } else {
            if !runtime.keyboard_hook.0.is_null() {
                let _ = UnhookWindowsHookEx(runtime.keyboard_hook);
                runtime.keyboard_hook = HHOOK::default();
            }

            if !runtime.mouse_hook.0.is_null() {
                let _ = UnhookWindowsHookEx(runtime.mouse_hook);
                runtime.mouse_hook = HHOOK::default();
            }
        }

        Ok(())
    }

    unsafe fn refresh_overlay_timer(hwnd: HWND, runtime: &mut Runtime) {
        let desired = desired_timer_interval_ms(runtime);
        if desired != runtime.timer_interval_ms {
            let _ = SetTimer(Some(hwnd), TIMER_ID, desired, None);
            runtime.timer_interval_ms = desired;
        }
    }

    fn refresh_pin_overlay(runtime: &mut Runtime) -> Result<()> {
        let active = {
            let hook_state = HOOK_STATE.lock();
            hook_state.active_pin_preset_id.and_then(|id| {
                hook_state
                    .pin_presets
                    .iter()
                    .find(|preset| preset.id == id)
                    .cloned()
            })
        };
        let Some(preset) = active else {
            unsafe {
                if let Some(active) = runtime.active_pin_thumbnail.take()
                    && let Some(thumbnail_id) = active.thumbnail_id
                {
                    let _ = DwmUnregisterThumbnail(thumbnail_id);
                }
                ACTIVE_PIN_SOURCE_HWND.store(0, Ordering::Relaxed);
                sync_window_location_hook_state(runtime);

                let _ = ShowWindow(runtime.pin_hwnd, SW_HIDE);
            }

            runtime.last_pin_update = Instant::now();
            return Ok(());
        };
        if runtime.active_pin_thumbnail.is_some()
            && runtime.last_pin_update.elapsed() < Duration::from_millis(16)
        {
            return Ok(());
        }

        let source = find_target_window_hwnd(
            preset.target_window_title.as_deref(),
            &preset.extra_target_window_titles,
            preset.match_duplicate_window_titles,
            false,
        )
        .context("Pin source window was not found")?;
        unsafe {
            let source_root = GetAncestor(source, GA_ROOT);
            if !source_root.0.is_null()
                && window_belongs_to_current_process(source_root)
                && !is_internal_app_window(source_root)
            {
                let _ = ShowWindow(runtime.pin_hwnd, SW_HIDE);
                runtime.last_pin_update = Instant::now();
                return Ok(());
            }

            let mut client_rect = RECT::default();
            GetClientRect(source, &mut client_rect)?;
            let mut client_top_left = POINT {
                x: client_rect.left,
                y: client_rect.top,
            };
            let mut client_bottom_right = POINT {
                x: client_rect.right,
                y: client_rect.bottom,
            };
            if !ClientToScreen(source, &mut client_top_left).as_bool() {
                return Err(anyhow::anyhow!("Failed to map client top-left to screen"));
            }
            if !ClientToScreen(source, &mut client_bottom_right).as_bool() {
                return Err(anyhow::anyhow!("Failed to map client bottom-right to screen"));
            }
            let source_rect = RECT {
                left: client_top_left.x,
                top: client_top_left.y,
                right: client_bottom_right.x,
                bottom: client_bottom_right.y,
            };
            let mut source_window_rect = RECT::default();
            GetWindowRect(source, &mut source_window_rect)?;
            let source_client_offset_x = source_rect.left - source_window_rect.left;
            let source_client_offset_y = source_rect.top - source_window_rect.top;
            let base_bounds = if preset.use_custom_bounds {
                (
                    preset.x,
                    preset.y,
                    preset.width.max(1),
                    preset.height.max(1),
                )
            } else {
                (
                    source_rect.left,
                    source_rect.top,
                    (source_rect.right - source_rect.left).max(1),
                    (source_rect.bottom - source_rect.top).max(1),
                )
            };
            let target_bounds = base_bounds;
            let source_width = (source_rect.right - source_rect.left).max(1);
            let source_height = (source_rect.bottom - source_rect.top).max(1);
            let source_crop_key = if preset.use_source_crop {
                let crop_x = preset.source_x.clamp(0, source_width.saturating_sub(1));
                let crop_y = preset.source_y.clamp(0, source_height.saturating_sub(1));
                let crop_w = preset
                    .source_width
                    .max(1)
                    .min(source_width.saturating_sub(crop_x).max(1));
                let crop_h = preset
                    .source_height
                    .max(1)
                    .min(source_height.saturating_sub(crop_y).max(1));
                Some((crop_x, crop_y, crop_w, crop_h))
            } else {
                None
            };
            let needs_register = runtime.active_pin_thumbnail.as_ref().is_none_or(|active| {
                active.preset_id != preset.id
                    || active.source_hwnd != source
                    || active.thumbnail_id.is_none()
            });
            if needs_register {
                if let Some(active) = runtime.active_pin_thumbnail.take()
                    && let Some(thumbnail_id) = active.thumbnail_id
                {
                    let _ = DwmUnregisterThumbnail(thumbnail_id);
                }

                let thumbnail_id = DwmRegisterThumbnail(runtime.pin_hwnd, source)?;
                runtime.active_pin_thumbnail = Some(ActivePinThumbnail {
                    preset_id: preset.id,
                    source_hwnd: source,
                    thumbnail_id: Some(thumbnail_id),
                    overlay_style: preset.overlay_style,
                    last_target_bounds: (i32::MIN, i32::MIN, i32::MIN, i32::MIN),
                    last_source_crop: None,
                });
                ACTIVE_PIN_SOURCE_HWND.store(source.0 as isize, Ordering::Relaxed);
                sync_window_location_hook_state(runtime);
            }

            if let Some(active) = runtime.active_pin_thumbnail.as_ref() {
                let mut source_flags = DWM_TNP_SOURCECLIENTAREAONLY;
                let mut source_rect_crop = RECT::default();
                if let Some((crop_x, crop_y, crop_w, crop_h)) = source_crop_key {
                    source_rect_crop = RECT {
                        left: crop_x + source_client_offset_x,
                        top: crop_y + source_client_offset_y,
                        right: crop_x + source_client_offset_x + crop_w,
                        bottom: crop_y + source_client_offset_y + crop_h,
                    };
                    source_flags |= DWM_TNP_RECTSOURCE;
                }

                let needs_apply = active.last_target_bounds != target_bounds
                    || active.last_source_crop != source_crop_key
                    || active.overlay_style != preset.overlay_style;
                if needs_apply {
                    let _ = SetWindowPos(
                        runtime.pin_hwnd,
                        Some(HWND_TOPMOST),
                        target_bounds.0,
                        target_bounds.1,
                        target_bounds.2,
                        target_bounds.3,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    let properties = DWM_THUMBNAIL_PROPERTIES {
                        dwFlags: DWM_TNP_RECTDESTINATION
                            | DWM_TNP_VISIBLE
                            | DWM_TNP_OPACITY
                            | source_flags,
                        rcDestination: RECT {
                            left: 0,
                            top: 0,
                            right: target_bounds.2,
                            bottom: target_bounds.3,
                        },
                        rcSource: source_rect_crop,
                        opacity: 255,
                        fVisible: true.into(),
                        fSourceClientAreaOnly: true.into(),
                        ..Default::default()
                    };
                    if let Some(thumbnail_id) = active.thumbnail_id {
                        let _ = DwmUpdateThumbnailProperties(thumbnail_id, &properties);
                    }

                    let region = CreateRectRgn(0, 0, target_bounds.2, target_bounds.3);
                    if region.0.is_null() {
                        return Err(anyhow::anyhow!("Failed to create pin window region"));
                    }

                    if SetWindowRgn(runtime.pin_hwnd, Some(region), true) == 0 {
                        let _ = DeleteObject(HGDIOBJ(region.0));
                        return Err(anyhow::anyhow!("Failed to apply pin window region"));
                    }

                    if let Some(active_mut) = runtime.active_pin_thumbnail.as_mut() {
                        active_mut.last_target_bounds = target_bounds;
                        active_mut.last_source_crop = source_crop_key;
                        active_mut.overlay_style = preset.overlay_style;
                    }
                }

                let _ = ShowWindow(runtime.pin_hwnd, SW_SHOWNA);
            }
        }

        runtime.last_pin_update = Instant::now();
        Ok(())
    }

    fn pin_overlay_shape_rect(
        style: PinOverlayStyle,
        target_w: i32,
        target_h: i32,
    ) -> (i32, i32, i32, i32) {
        let target_w = target_w.max(1);
        let target_h = target_h.max(1);
        match style {
            PinOverlayStyle::Rectangle => (0, 0, target_w, target_h),
            PinOverlayStyle::Circle => {
                let padding = ((target_w.min(target_h) as f32 * 0.04).round() as i32).max(4);
                let size = (target_w.min(target_h) - padding * 2).max(1);
                ((target_w - size) / 2, (target_h - size) / 2, size, size)
            }

            PinOverlayStyle::HorizontalBar => {
                let width = target_w.max(1);
                let min_height = ((target_h as f32 * 0.12).round() as i32).clamp(18, target_h);
                let bar_height =
                    ((target_h as f32 * 0.24).round() as i32).clamp(min_height, target_h.max(1));
                (
                    (target_w - width) / 2,
                    (target_h - bar_height) / 2,
                    width,
                    bar_height,
                )
            }
        }
    }

    fn point_in_rounded_rect(
        x: i32,
        y: i32,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        radius: f32,
    ) -> bool {
        if width <= 0 || height <= 0 {
            return false;
        }

        let radius = radius
            .max(0.0)
            .min(width as f32 * 0.5)
            .min(height as f32 * 0.5);
        if radius <= 0.0 {
            return x >= left && x < left + width && y >= top && y < top + height;
        }

        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;
        let inner_left = left as f32 + radius;
        let inner_right = left as f32 + width as f32 - radius;
        let inner_top = top as f32 + radius;
        let inner_bottom = top as f32 + height as f32 - radius;
        if (px >= inner_left && px <= inner_right) || (py >= inner_top && py <= inner_bottom) {
            return true;
        }

        let corner_x = if px < inner_left {
            inner_left
        } else {
            inner_right
        };
        let corner_y = if py < inner_top {
            inner_top
        } else {
            inner_bottom
        };
        let dx = px - corner_x;
        let dy = py - corner_y;
        (dx * dx) + (dy * dy) <= radius * radius
    }

    fn render_pin_overlay_bitmap(
        capture: &window_list::ScreenCaptureFrame,
        target_w: i32,
        target_h: i32,
        style: PinOverlayStyle,
        source_crop: Option<(i32, i32, i32, i32)>,
        true_stretch: bool,
    ) -> Result<Vec<u8>> {
        let target_w = target_w.max(1);
        let target_h = target_h.max(1);
        let source = RgbaImage::from_raw(
            capture.width as u32,
            capture.height as u32,
            capture.rgba.clone(),
        )
        .context("Failed to decode pin capture")?;
        let source = if let Some((crop_x, crop_y, crop_w, crop_h)) = source_crop {
            image::imageops::crop_imm(
                &source,
                crop_x.max(0) as u32,
                crop_y.max(0) as u32,
                crop_w.max(1) as u32,
                crop_h.max(1) as u32,
            )
            .to_image()
        } else {
            source
        };
        let (shape_left, shape_top, shape_w, shape_h) =
            pin_overlay_shape_rect(style, target_w, target_h);
        let mut output = vec![0u8; (target_w as usize) * (target_h as usize) * 4];
        let source_w = source.width().max(1);
        let source_h = source.height().max(1);
        let (draw_w, draw_h, draw_x, draw_y, resized) = if true_stretch {
            let resized = image::imageops::resize(
                &source,
                shape_w.max(1) as u32,
                shape_h.max(1) as u32,
                FilterType::CatmullRom,
            );
            (
                shape_w.max(1) as u32,
                shape_h.max(1) as u32,
                shape_left,
                shape_top,
                resized,
            )
        } else {
            let scale = (shape_w.max(1) as f32 / source_w as f32)
                .min(shape_h.max(1) as f32 / source_h as f32)
                .max(0.01);
            let fit_w = (source_w as f32 * scale).round().max(1.0) as u32;
            let fit_h = (source_h as f32 * scale).round().max(1.0) as u32;
            let fit_x = shape_left + ((shape_w - fit_w as i32) / 2).max(0);
            let fit_y = shape_top + ((shape_h - fit_h as i32) / 2).max(0);
            (
                fit_w,
                fit_h,
                fit_x,
                fit_y,
                image::imageops::resize(&source, fit_w, fit_h, FilterType::CatmullRom),
            )
        };
        let resized_pixels = resized.as_raw();
        for y in 0..draw_h as i32 {
            for x in 0..draw_w as i32 {
                let dst_x = draw_x + x;
                let dst_y = draw_y + y;
                if dst_x < 0 || dst_y < 0 || dst_x >= target_w || dst_y >= target_h {
                    continue;
                }

                let inside = match style {
                    PinOverlayStyle::Rectangle => true,
                    PinOverlayStyle::Circle => {
                        point_in_ellipse(dst_x, dst_y, shape_left, shape_top, shape_w, shape_h)
                    }

                    PinOverlayStyle::HorizontalBar => point_in_rounded_rect(
                        dst_x,
                        dst_y,
                        shape_left,
                        shape_top,
                        shape_w,
                        shape_h,
                        shape_h as f32 * 0.5,
                    ),
                };
                if !inside {
                    continue;
                }

                let src_index = ((y as usize) * (draw_w as usize) + x as usize) * 4;
                let dst_index = ((dst_y as usize) * (target_w as usize) + dst_x as usize) * 4;
                output[dst_index..dst_index + 4]
                    .copy_from_slice(&resized_pixels[src_index..src_index + 4]);
            }
        }

        Ok(output)
    }

    unsafe fn paint_pin_overlay(
        hwnd: HWND,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        rgba: &[u8],
    ) -> Result<()> {
        let width = width.max(1);
        let height = height.max(1);
        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            bail!("Failed to acquire the screen DC");
        }

        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create a memory DC");
        }

        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?;
        if bitmap.0.is_null() || bits.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create pin DIB");
        }

        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        let bgra = rgba_to_bgra(rgba);
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
        let destination = POINT { x, y };
        let source = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: width,
            cy: height,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&destination),
            Some(&size),
            Some(mem_dc),
            Some(&source),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        let _ = ShowWindow(hwnd, SW_SHOWNA);
        Ok(())
    }

    fn fill_skia_rounded_rect(
        pixmap: &mut tiny_skia::Pixmap,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: [u8; 4],
    ) {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(left + radius, top);
        pb.line_to(left + width - radius, top);
        pb.quad_to(left + width, top, left + width, top + radius);
        pb.line_to(left + width, top + height - radius);
        pb.quad_to(
            left + width,
            top + height,
            left + width - radius,
            top + height,
        );
        pb.line_to(left + radius, top + height);
        pb.quad_to(left, top + height, left, top + height - radius);
        pb.line_to(left, top + radius);
        pb.quad_to(left, top, left + radius, top);
        pb.close();
        if let Some(path) = pb.finish() {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(
                color[0], color[1], color[2], color[3],
            ));
            paint.anti_alias = true;
            pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                tiny_skia::Transform::identity(),
                None,
            );
        }
    }

    fn stroke_skia_rounded_rect(
        pixmap: &mut tiny_skia::Pixmap,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        radius: f32,
        stroke_width: f32,
        color: [u8; 4],
    ) {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(left + radius, top);
        pb.line_to(left + width - radius, top);
        pb.quad_to(left + width, top, left + width, top + radius);
        pb.line_to(left + width, top + height - radius);
        pb.quad_to(
            left + width,
            top + height,
            left + width - radius,
            top + height,
        );
        pb.line_to(left + radius, top + height);
        pb.quad_to(left, top + height, left, top + height - radius);
        pb.line_to(left, top + radius);
        pb.quad_to(left, top, left + radius, top);
        pb.close();
        if let Some(path) = pb.finish() {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(
                color[0], color[1], color[2], color[3],
            ));
            paint.anti_alias = true;
            let stroke = tiny_skia::Stroke {
                width: stroke_width,
                ..Default::default()
            };
            pixmap.stroke_path(
                &path,
                &paint,
                &stroke,
                tiny_skia::Transform::identity(),
                None,
            );
        }
    }

    fn blend_premultiplied_rgba(dst: &mut [u8], src_r: u8, src_g: u8, src_b: u8, src_a: u8) {
        let inv_alpha = 255u32.saturating_sub(src_a as u32);
        let dst_a = dst[3] as u32;
        dst[0] = (src_r as u32 + (dst[0] as u32 * inv_alpha) / 255) as u8;
        dst[1] = (src_g as u32 + (dst[1] as u32 * inv_alpha) / 255) as u8;
        dst[2] = (src_b as u32 + (dst[2] as u32 * inv_alpha) / 255) as u8;
        dst[3] = (src_a as u32 + (dst_a * inv_alpha) / 255).min(255) as u8;
    }

    fn blend_premultiplied_bgra(dst: &mut [u8], src_b: u8, src_g: u8, src_r: u8, src_a: u8) {
        let inv_alpha = 255u32.saturating_sub(src_a as u32);
        let dst_a = dst[3] as u32;
        dst[0] = (src_b as u32 + (dst[0] as u32 * inv_alpha) / 255) as u8;
        dst[1] = (src_g as u32 + (dst[1] as u32 * inv_alpha) / 255) as u8;
        dst[2] = (src_r as u32 + (dst[2] as u32 * inv_alpha) / 255) as u8;
        dst[3] = (src_a as u32 + (dst_a * inv_alpha) / 255).min(255) as u8;
    }

    unsafe fn paint_quick_key_display(
        hwnd: HWND,
        entries: &[String],
        font_size: f32,
        window_x: i32,
        window_y: i32,
        width: i32,
        height: i32,
    ) -> Result<()> {
        let window_x = window_x.max(0);
        let window_y = window_y.max(0);
        let width = width.max(1);
        let height = height.max(1);
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits_ptr: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        )?;
        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        let bytes_len = (width as usize) * (height as usize) * 4;
        let pixels = std::slice::from_raw_parts_mut(bits_ptr as *mut u8, bytes_len);
        pixels.fill(0);

        let mut pixmap = tiny_skia::Pixmap::new(width as u32, height as u32)
            .ok_or_else(|| anyhow::anyhow!("Failed to allocate quick key display pixmap"))?;
        let cap_height = (font_size * 1.12 + 18.0).round().max(44.0) as i32;
        let cap_radius = (cap_height as f32 * 0.26).clamp(11.0, 18.0);
        let outer_pad_x = (font_size * 0.46).round().max(16.0) as i32;
        let outer_pad_y = (font_size * 0.34).round().max(10.0) as i32;
        let combo_gap = (font_size * 0.14).round().max(4.0) as i32;
        let plus_width = (font_size * 0.48).round().max(10.0) as i32;
        let entry_gap = (font_size * 0.52).round().max(18.0) as i32;

        let mut text_runs = Vec::<QuickKeyDisplayTextRun>::new();
        let mut cursor_x = outer_pad_x;
        let cap_y = outer_pad_y;
        let entry_count = entries.len().max(1) as f32;
        for (entry_index, entry) in entries.iter().enumerate() {
            let alpha_scale = 0.56 + (((entry_index + 1) as f32 / entry_count) * 0.44);
            let parts = quick_key_display_parts(entry);
            for (part_index, part) in parts.iter().enumerate() {
                let palette = quick_key_display_palette(part);
                let (base_fill, inner_fill, border, text_color) =
                    quick_key_display_palette_colors(palette);
                let cap_width = quick_key_display_keycap_width(part, font_size, cap_height);

                fill_skia_rounded_rect(
                    &mut pixmap,
                    cursor_x as f32,
                    (cap_y + 4) as f32,
                    cap_width as f32,
                    cap_height as f32,
                    cap_radius,
                    quick_key_display_alpha([2, 5, 10, 80], alpha_scale * 0.8),
                );
                fill_skia_rounded_rect(
                    &mut pixmap,
                    cursor_x as f32,
                    cap_y as f32,
                    cap_width as f32,
                    cap_height as f32,
                    cap_radius,
                    quick_key_display_alpha(base_fill, alpha_scale),
                );
                fill_skia_rounded_rect(
                    &mut pixmap,
                    (cursor_x + 2) as f32,
                    (cap_y + 2) as f32,
                    (cap_width - 4).max(1) as f32,
                    (cap_height - 5).max(1) as f32,
                    (cap_radius - 2.0).max(2.0),
                    quick_key_display_alpha(inner_fill, alpha_scale * 0.92),
                );
                fill_skia_rounded_rect(
                    &mut pixmap,
                    (cursor_x + 3) as f32,
                    (cap_y + 3) as f32,
                    (cap_width - 6).max(1) as f32,
                    ((cap_height as f32 - 8.0) * 0.46).max(1.0),
                    (cap_radius - 3.0).max(2.0),
                    quick_key_display_alpha([255, 255, 255, 20], alpha_scale),
                );
                fill_skia_rounded_rect(
                    &mut pixmap,
                    (cursor_x + 2) as f32,
                    (cap_y + cap_height - 12) as f32,
                    (cap_width - 4).max(1) as f32,
                    7.0,
                    (cap_radius - 4.0).max(2.0),
                    quick_key_display_alpha([6, 9, 14, 48], alpha_scale),
                );
                stroke_skia_rounded_rect(
                    &mut pixmap,
                    cursor_x as f32 + 0.5,
                    cap_y as f32 + 0.5,
                    (cap_width - 1).max(1) as f32,
                    (cap_height - 1).max(1) as f32,
                    cap_radius,
                    1.1,
                    quick_key_display_alpha(border, alpha_scale),
                );
                stroke_skia_rounded_rect(
                    &mut pixmap,
                    (cursor_x + 2) as f32 + 0.5,
                    (cap_y + 2) as f32 + 0.5,
                    (cap_width - 5).max(1) as f32,
                    (cap_height - 6).max(1) as f32,
                    (cap_radius - 2.0).max(2.0),
                    0.9,
                    quick_key_display_alpha([255, 255, 255, 34], alpha_scale),
                );

                text_runs.push(QuickKeyDisplayTextRun {
                    text: part.clone(),
                    rect: RECT {
                        left: cursor_x + 8,
                        top: cap_y,
                        right: cursor_x + cap_width - 8,
                        bottom: cap_y + cap_height,
                    },
                    color: quick_key_display_colorref(text_color[0], text_color[1], text_color[2]),
                });
                cursor_x += cap_width;

                if part_index + 1 < parts.len() {
                    cursor_x += combo_gap;
                    text_runs.push(QuickKeyDisplayTextRun {
                        text: "+".to_owned(),
                        rect: RECT {
                            left: cursor_x,
                            top: cap_y,
                            right: cursor_x + plus_width,
                            bottom: cap_y + cap_height,
                        },
                        color: quick_key_display_colorref(226, 235, 244),
                    });
                    cursor_x += plus_width + combo_gap;
                }
            }
            if entry_index + 1 < entries.len() {
                cursor_x += entry_gap;
            }
        }

        let pixmap_data = pixmap.data();
        let total_pixels = width as usize * height as usize;
        for i in 0..total_pixels {
            let offset = i * 4;
            let r = pixmap_data[offset];
            let g = pixmap_data[offset + 1];
            let b = pixmap_data[offset + 2];
            let a = pixmap_data[offset + 3];
            pixels[offset] = b;
            pixels[offset + 1] = g;
            pixels[offset + 2] = r;
            pixels[offset + 3] = a;
        }

        let text_mem_dc = CreateCompatibleDC(Some(screen_dc));
        let mut text_bits_ptr: *mut c_void = std::ptr::null_mut();
        let text_bitmap = CreateDIBSection(
            Some(text_mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut text_bits_ptr,
            None,
            0,
        )?;
        let old_text_bitmap = SelectObject(text_mem_dc, HGDIOBJ(text_bitmap.0));
        let text_pixels = std::slice::from_raw_parts_mut(text_bits_ptr as *mut u8, bytes_len);
        text_pixels.fill(0);

        let font_name = "Segoe UI"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let font = CreateFontW(
            -(font_size.round() as i32).max(1),
            0,
            0,
            0,
            FW_MEDIUM.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            FF_DONTCARE.0 as u32,
            PCWSTR(font_name.as_ptr()),
        );
        let old_font = SelectObject(text_mem_dc, HGDIOBJ(font.0));
        let _ = SetBkMode(text_mem_dc, TRANSPARENT);
        for run in &text_runs {
            let _ = SetTextColor(text_mem_dc, run.color);
            let mut wide = run
                .text
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let mut rect = run.rect;
            let _ = DrawTextW(
                text_mem_dc,
                &mut wide,
                &mut rect,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            );
        }

        for i in 0..total_pixels {
            let offset = i * 4;
            let text_b = text_pixels[offset];
            let text_g = text_pixels[offset + 1];
            let text_r = text_pixels[offset + 2];
            let text_a = text_b.max(text_g).max(text_r);
            if text_a == 0 {
                continue;
            }
            blend_premultiplied_bgra(
                &mut pixels[offset..offset + 4],
                text_b,
                text_g,
                text_r,
                text_a,
            );
        }

        let size = SIZE {
            cx: width,
            cy: height,
        };
        let src_pt = POINT { x: 0, y: 0 };
        let pos = POINT {
            x: window_x,
            y: window_y,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&pos),
            Some(&size),
            Some(mem_dc),
            Some(&src_pt),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        let _ = SelectObject(text_mem_dc, old_font);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = SelectObject(text_mem_dc, old_text_bitmap);
        let _ = DeleteObject(HGDIOBJ(text_bitmap.0));
        let _ = DeleteDC(text_mem_dc);
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        let _ = ShowWindow(hwnd, SW_SHOWNA);
        Ok(())
    }

    unsafe fn paint_hud(hwnd: HWND, display: &HudDisplayState) -> Result<()> {
        let window_x = display.x.max(0);
        let window_y = display.y.max(0);
        let width = display.width.max(1);
        let height = display.height.max(1);
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits_ptr: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        )?;
        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        let bg_alpha = (display.background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let bytes_len = (width as usize) * (height as usize) * 4;
        let pixels = std::slice::from_raw_parts_mut(bits_ptr as *mut u8, bytes_len);
        let radius = if display.rounded_background {
            16.0
        } else {
            0.0
        };
        let bg_b = ((display.background_color.b as u32 * bg_alpha as u32) / 255) as u8;
        let bg_g = ((display.background_color.g as u32 * bg_alpha as u32) / 255) as u8;
        let bg_r = ((display.background_color.r as u32 * bg_alpha as u32) / 255) as u8;
        for py in 0..height {
            for px in 0..width {
                let index = ((py as usize) * (width as usize) + (px as usize)) * 4;
                let inside = if radius <= 0.0 {
                    true
                } else {
                    let px_f = px as f32 + 0.5;
                    let py_f = py as f32 + 0.5;
                    let inner_left = radius;
                    let inner_right = width as f32 - radius;
                    let inner_top = radius;
                    let inner_bottom = height as f32 - radius;
                    if (px_f >= inner_left && px_f <= inner_right)
                        || (py_f >= inner_top && py_f <= inner_bottom)
                    {
                        true
                    } else {
                        let corner_x = if px_f < inner_left {
                            inner_left
                        } else {
                            inner_right
                        };
                        let corner_y = if py_f < inner_top {
                            inner_top
                        } else {
                            inner_bottom
                        };
                        let dx = px_f - corner_x;
                        let dy = py_f - corner_y;
                        (dx * dx) + (dy * dy) <= radius * radius
                    }
                };
                if inside && bg_alpha > 0 {
                    pixels[index] = bg_b;
                    pixels[index + 1] = bg_g;
                    pixels[index + 2] = bg_r;
                    pixels[index + 3] = bg_alpha;
                } else {
                    pixels[index] = 0;
                    pixels[index + 1] = 0;
                    pixels[index + 2] = 0;
                    pixels[index + 3] = 0;
                }
            }
        }

        let font_name = "Segoe UI"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let font = CreateFontW(
            -(display.font_size.round() as i32).max(1),
            0,
            0,
            0,
            FW_MEDIUM.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            FF_DONTCARE.0 as u32,
            PCWSTR(font_name.as_ptr()),
        );
        let old_font = SelectObject(mem_dc, HGDIOBJ(font.0));
        let _ = SetBkMode(mem_dc, TRANSPARENT);
        let _ = SetTextColor(
            mem_dc,
            COLORREF(
                ((display.text_color.b as u32) << 16)
                    | ((display.text_color.g as u32) << 8)
                    | (display.text_color.r as u32),
            ),
        );
        let mut text_rect = RECT {
            left: 12,
            top: 4,
            right: width - 12,
            bottom: height - 4,
        };
        let mut wide = display
            .text
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let _ = DrawTextW(
            mem_dc,
            &mut wide,
            &mut text_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        let text_alpha = display.text_color.a.max(1);
        for py in 0..height {
            for px in 0..width {
                let index = ((py as usize) * (width as usize) + (px as usize)) * 4;
                let chunk = &mut pixels[index..index + 4];
                let looks_like_bg = chunk[0] == bg_b
                    && chunk[1] == bg_g
                    && chunk[2] == bg_r
                    && chunk[3] == bg_alpha;
                let alpha = if looks_like_bg {
                    bg_alpha
                } else if chunk[0] == 0 && chunk[1] == 0 && chunk[2] == 0 && chunk[3] == 0 {
                    0
                } else {
                    text_alpha
                };
                chunk[3] = alpha;
                chunk[0] = ((chunk[0] as u32 * alpha as u32) / 255) as u8;
                chunk[1] = ((chunk[1] as u32 * alpha as u32) / 255) as u8;
                chunk[2] = ((chunk[2] as u32 * alpha as u32) / 255) as u8;
            }
        }

        let size = SIZE {
            cx: width,
            cy: height,
        };
        let src_pt = POINT { x: 0, y: 0 };
        let pos = POINT {
            x: window_x,
            y: window_y,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&pos),
            Some(&size),
            Some(mem_dc),
            Some(&src_pt),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        let _ = ShowWindow(hwnd, SW_SHOWNA);
        Ok(())
    }

    fn sync_window_hotkeys(hwnd: HWND, runtime: &mut Runtime) -> Result<()> {
        for hotkey_id in runtime
            .registered_window_hotkeys
            .keys()
            .copied()
            .collect::<Vec<_>>()
        {
            let _ = unsafe { UnregisterHotKey(Some(hwnd), hotkey_id) };
        }

        runtime.registered_window_hotkeys.clear();

        let mut next_hotkey_id: i32 = 0x10000;

        for preset in &runtime.window_layouts {
            if !preset.enabled {
                continue;
            }
            if let Some(hk) = &preset.hotkey {
                if let Some((mods, vk)) = crate::hotkey::to_windows_registration(hk) {
                    let id = next_hotkey_id;
                    next_hotkey_id += 1;
                    if unsafe { RegisterHotKey(Some(hwnd), id, mods, vk) }.is_ok() {
                        runtime
                            .registered_window_hotkeys
                            .insert(id, WindowHotkeyAction::ApplyLayout(preset.clone()));
                    }
                }
            }
        }

        let mut hook_state = HOOK_STATE.lock();
        hook_state.window_presets = runtime.window_presets.clone();
        hook_state.window_focus_presets = runtime.window_focus_presets.clone();
        hook_state.window_layouts = runtime.window_layouts.clone();
        hook_state.pin_presets = runtime.pin_presets.clone();
        Ok(())
    }

    fn sync_macro_hotkeys(hwnd: HWND, runtime: &mut Runtime) -> Result<()> {
        for hotkey_id in runtime
            .registered_macro_hotkeys
            .keys()
            .copied()
            .collect::<Vec<_>>()
        {
            let _ = unsafe { UnregisterHotKey(Some(hwnd), hotkey_id) };
        }

        runtime.registered_macro_hotkeys.clear();
        HOOK_STATE.lock().macro_groups = runtime.macro_groups.clone();
        Ok(())
    }

    fn unregister_all_hotkeys(hwnd: HWND, runtime: Option<&mut Runtime>) {
        let Some(runtime) = runtime else {
            return;
        };
        let _ = unsafe { UnregisterHotKey(Some(hwnd), HOTKEY_ID) };
        for hotkey_id in runtime
            .registered_window_hotkeys
            .keys()
            .copied()
            .collect::<Vec<_>>()
        {
            let _ = unsafe { UnregisterHotKey(Some(hwnd), hotkey_id) };
        }

        for hotkey_id in runtime
            .registered_macro_hotkeys
            .keys()
            .copied()
            .collect::<Vec<_>>()
        {
            let _ = unsafe { UnregisterHotKey(Some(hwnd), hotkey_id) };
        }
    }

    fn play_macro_preset(
        hotkey_id: i32,
        preset: MacroPreset,
        target_window_title: Option<String>,
        extra_target_window_titles: Vec<String>,
        match_duplicate_window_titles: bool,
        trigger_key: String,
    ) -> Result<()> {
        SUPPRESSED_MACRO_HOTKEYS.lock().insert(hotkey_id);
        STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
        FORCE_STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
        HOOK_STATE
            .lock()
            .stop_ignore_keys
            .insert(preset.id, trigger_key);
        thread::spawn(move || {
            MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().clear());
            let cleanup_steps = collect_macro_release_steps(&preset.steps);
            let mut press_locked_keys: Vec<String> = Vec::new();
            let mut press_locked_mouse_masks: Vec<MouseMoveLockMask> = Vec::new();
            let step_indices: Vec<usize> = (0..preset.steps.len()).collect();
            let _ = execute_macro_sequence(
                preset.id,
                &preset.steps,
                &step_indices,
                &mut press_locked_keys,
                &mut press_locked_mouse_masks,
                preset.stop_on_retrigger_immediate,
                target_window_title.as_deref(),
                &extra_target_window_titles,
                match_duplicate_window_titles,
                false,
            );
            for step in cleanup_steps {
                let _ = send_key_event(&step);
            }

            if !press_locked_keys.is_empty() {
                apply_unlock_keys(&press_locked_keys, None);
            }

            for mask in press_locked_mouse_masks {
                apply_unlock_mouse(None, mask);
            }

            let image_search_preset_ids = collect_macro_image_search_start_ids(&preset.steps);
            stop_vision_following_ids(&image_search_preset_ids);
            hide_toolbox_for_owner(preset.id);
            HOOK_STATE.lock().stop_ignore_keys.remove(&preset.id);
            STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
            FORCE_STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
            SUPPRESSED_MACRO_HOTKEYS.lock().remove(&hotkey_id);
        });
        Ok(())
    }

    fn activate_hold_macro(
        preset: MacroPreset,
        trigger: HotkeyBinding,
        target_window_title: Option<String>,
        extra_target_window_titles: Vec<String>,
        match_duplicate_window_titles: bool,
        trigger_key: String,
    ) {
        let stale_run_exists = HOOK_STATE
            .lock()
            .active_hold_macros
            .contains_key(&preset.id);
        if stale_run_exists {
            deactivate_hold_macro(preset.id);
        }

        STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
        FORCE_STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
        HOOK_STATE
            .lock()
            .stop_ignore_keys
            .insert(preset.id, trigger_key);
        let release_steps = collect_macro_release_steps(&preset.steps);
        let hold_stop_step = preset
            .hold_stop_step_enabled
            .then(|| preset.hold_stop_step.clone());
        let image_search_preset_ids = collect_macro_image_search_start_ids(&preset.steps);
        let run_token = {
            let mut hook_state = HOOK_STATE.lock();
            let run_token = hook_state.next_hold_run_token;
            hook_state.next_hold_run_token = hook_state.next_hold_run_token.saturating_add(1);
            hook_state.active_hold_macros.insert(
                preset.id,
                ActiveHoldMacro {
                    trigger,
                    release_steps,
                    hold_stop_step,
                    image_search_preset_ids,
                    locked_keys: Vec::new(),
                    locked_mouse_masks: Vec::new(),
                    run_token,
                    completed: false,
                },
            );
            run_token
        };
        thread::spawn(move || {
            let step_indices: Vec<usize> = (0..preset.steps.len()).collect();
            let flow = execute_hold_macro_sequence(
                preset.id,
                &preset.steps,
                &step_indices,
                preset.stop_on_retrigger_immediate,
                run_token,
                target_window_title.as_deref(),
                &extra_target_window_titles,
                match_duplicate_window_titles,
                false,
            );
            if matches!(flow, MacroRunFlow::Continue) {
                let mut hook_state = HOOK_STATE.lock();
                if let Some(active) = hook_state.active_hold_macros.get_mut(&preset.id)
                    && active.run_token == run_token
                {
                    active.completed = true;
                }
            }
        });
    }

    fn deactivate_hold_macro(preset_id: u32) {
        STOP_REQUESTED_MACRO_PRESETS.lock().insert(preset_id);
        let active = {
            let mut hook_state = HOOK_STATE.lock();
            let Some(active) = hook_state.active_hold_macros.remove(&preset_id) else {
                return;
            };
            active
        };
        let ActiveHoldMacro {
            trigger: _,
            release_steps,
            hold_stop_step,
            image_search_preset_ids,
            locked_keys,
            locked_mouse_masks,
            run_token: _,
            completed,
        } = active;
        for step in release_steps {
            let _ = send_key_event(&step);
        }

        if !locked_keys.is_empty() {
            apply_unlock_keys(&locked_keys, Some(preset_id));
        }

        for mask in locked_mouse_masks {
            apply_unlock_mouse(Some(preset_id), mask);
        }

        if !completed {
            if let Some(step) = hold_stop_step {
                execute_hold_abort_step(preset_id, &step);
            }
        }

        stop_vision_following_ids(&image_search_preset_ids);
        hide_toolbox_for_owner(preset_id);
        HOOK_STATE.lock().stop_ignore_keys.remove(&preset_id);
        FORCE_STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset_id);
    }

    fn current_hold_run_matches(preset_id: u32, run_token: u64) -> bool {
        let hook_state = HOOK_STATE.lock();
        current_hold_run_matches_with_guard(preset_id, run_token, &hook_state)
    }

    fn current_hold_run_matches_with_guard(
        preset_id: u32,
        run_token: u64,
        hook_state: &HookState,
    ) -> bool {
        hook_state
            .active_hold_macros
            .get(&preset_id)
            .is_some_and(|active| active.run_token == run_token)
    }

    fn send_overlay_command(command: OverlayCommand) {
        let is_refresh = matches!(&command, OverlayCommand::RefreshSearchAreaOverlay);
        if is_refresh
            && SEARCH_AREA_OVERLAY_REFRESH_PENDING.swap(true, Ordering::AcqRel)
        {
            return;
        }

        if let Some(tx) = OVERLAY_COMMAND_TX.lock().clone() {
            if tx.send(command).is_ok() {
                wake_command_queue();
                return;
            }
        }

        if is_refresh {
            SEARCH_AREA_OVERLAY_REFRESH_PENDING.store(false, Ordering::Release);
        }
    }

    fn send_ui_command(command: UiCommand) {
        if let Some(tx) = HOOK_STATE.lock().ui_tx.clone() {
            let _ = tx.send(command);
        }
    }

    pub(crate) fn apply_window_preset_by_id(spec: &str) -> Result<()> {
        window_preset::apply_window_preset_by_id(spec)
    }

    pub fn spawn_custom_command(
        preset_id: Option<u32>,
        use_powershell: bool,
        command_text: String,
    ) {
        let command_text = interpolate_variables(&command_text);
        thread::spawn(move || {
            let mut command = if use_powershell {
                let mut cmd = Command::new("powershell.exe");
                cmd.args([
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    &command_text,
                ]);
                cmd
            } else {
                let mut cmd = Command::new("cmd.exe");
                cmd.raw_arg(format!("/C {}", command_text));
                cmd
            };
            let output_res = command.creation_flags(CREATE_NO_WINDOW.0).output();
            let text = match output_res {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let mut combined = String::new();
                    if !stdout.is_empty() {
                        combined.push_str(&stdout);
                    }

                    if !stderr.is_empty() {
                        if !combined.is_empty() {
                            combined.push_str("\n");
                        }

                        combined.push_str("Error:\n");
                        combined.push_str(&stderr);
                    }

                    if combined.is_empty() {
                        combined = if out.status.success() {
                            "Command finished successfully with no output.".to_owned()
                        } else {
                            format!(
                                "Command exited with status code: {}",
                                out.status.code().unwrap_or(-1)
                            )
                        };
                    }

                    combined
                }

                Err(e) => format!("Failed to execute command: {}", e),
            };
            if let Some(id) = preset_id {
                send_ui_command(UiCommand::CustomCommandResult {
                    preset_id: id,
                    output: text,
                });
            }
        });
    }

    fn trigger_custom_preset_by_id(spec: &str) -> Result<()> {
        let spec = spec.trim();
        let preset = {
            let hook_state = HOOK_STATE.lock();
            let by_id = spec.parse::<u32>().ok().and_then(|preset_id| {
                hook_state
                    .command_presets
                    .iter()
                    .find(|preset| preset.id == preset_id)
                    .cloned()
            });
            by_id.or_else(|| {
                hook_state
                    .command_presets
                    .iter()
                    .find(|preset| preset.name.trim().eq_ignore_ascii_case(spec))
                    .cloned()
            })
        }
        .context("Custom preset was not found")?;
        if !preset.enabled {
            bail!("Custom preset is disabled");
        }

        if preset.target_window_title.is_some() || !preset.extra_target_window_titles.is_empty() {
            let foreground = unsafe { GetForegroundWindow() };
            let matches = unsafe {
                window_matches_any_selector(
                    foreground,
                    preset.target_window_title.as_deref(),
                    &preset.extra_target_window_titles,
                    preset.match_duplicate_window_titles,
                )
            };
            if !matches {
                return Ok(());
            }
        }

        let command_text = ai::normalize_command_text(&preset.command);
        if command_text.is_empty() {
            bail!("Custom preset command is empty");
        }

        spawn_custom_command(Some(preset.id), preset.use_powershell, command_text);
        Ok(())
    }

    fn trigger_command_preset_step(step: &MacroStep) -> Result<()> {
        let spec = step.key.trim();
        if spec.is_empty() {
            bail!("Custom preset key is empty");
        }

        let preset = {
            let hook_state = HOOK_STATE.lock();
            let by_id = spec.parse::<u32>().ok().and_then(|preset_id| {
                hook_state
                    .command_presets
                    .iter()
                    .find(|preset| preset.id == preset_id)
                    .cloned()
            });
            by_id.or_else(|| {
                hook_state
                    .command_presets
                    .iter()
                    .find(|preset| preset.name.trim().eq_ignore_ascii_case(spec))
                    .cloned()
            })
        };
        if let Some(preset) = preset {
            if !preset.enabled {
                bail!("Custom preset is disabled");
            }

            if preset.target_window_title.is_some() || !preset.extra_target_window_titles.is_empty()
            {
                let foreground = unsafe { GetForegroundWindow() };
                let matches = unsafe {
                    window_matches_any_selector(
                        foreground,
                        preset.target_window_title.as_deref(),
                        &preset.extra_target_window_titles,
                        preset.match_duplicate_window_titles,
                    )
                };
                if !matches {
                    return Ok(());
                }
            }

            let command_text = ai::normalize_command_text(&preset.command);
            if command_text.is_empty() {
                bail!("Custom preset command is empty");
            }

            spawn_custom_command(Some(preset.id), preset.use_powershell, command_text);
            return Ok(());
        }

        let command_text = ai::normalize_command_text(&step.command_preset_command);
        if command_text.is_empty() {
            bail!("Custom preset was not found");
        }

        spawn_custom_command(None, step.command_preset_use_powershell, command_text);
        Ok(())
    }

    fn summarize_funny_meme_reply_error(error: &anyhow::Error) -> (String, bool) {
        let text = format!("{error:#}");
        let lower = text.to_ascii_lowercase();
        if lower.contains("api key") && lower.contains("groq") && lower.contains("enter") {
            return ("Enter a Groq API key in Settings.".to_owned(), true);
        }
        if lower.contains("401")
            || lower.contains("invalid_api_key")
            || lower.contains("invalid api key")
            || lower.contains("incorrect api key")
            || lower.contains("authentication")
        {
            return ("Groq API key is invalid. Fix it in Settings.".to_owned(), true);
        }
        if lower.contains("403") || lower.contains("forbidden") {
            return ("Groq rejected this API key. Check Settings.".to_owned(), true);
        }
        if lower.contains("429") || lower.contains("rate limit") {
            return ("Groq rate limit hit. Try again soon.".to_owned(), false);
        }
        if lower.contains("empty") && lower.contains("input") {
            return ("Enter a message or variable first.".to_owned(), false);
        }
        if lower.contains("did not return") || lower.contains("empty meme search query") {
            return ("AI did not return a usable meme query.".to_owned(), false);
        }
        if lower.contains("no meme image results") {
            return ("No meme image was found for that query.".to_owned(), false);
        }
        if lower.contains("clipboard") {
            return ("Could not copy the meme image to clipboard.".to_owned(), false);
        }
        ("MemeReply failed. Check API key or try again.".to_owned(), false)
    }

    fn trigger_funny_meme_reply_step(
        preset_id: u32,
        step_index: Option<usize>,
        step: &MacroStep,
    ) -> Result<()> {
        let source_text = interpolate_variables(&step.key);
        let source_text = source_text.trim().to_owned();

        let (groq_settings, ui_tx) = {
            let hook_state = HOOK_STATE.lock();
            (hook_state.groq_settings.clone(), hook_state.ui_tx.clone())
        };

        let result = (|| -> Result<String> {
            if source_text.is_empty() {
                bail!("Funny Meme Reply input is empty");
            }

            ai::copy_funny_meme_reply_to_clipboard(&groq_settings, &source_text)
        })();

        if let Some(tx) = ui_tx {
            let (status, inline_message, open_groq_settings) = match &result {
                Ok(query) => (
                    format!("Funny Meme Reply copied an image for query: {}", query),
                    String::new(),
                    false,
                ),
                Err(error) => {
                    let (short_message, needs_settings) =
                        summarize_funny_meme_reply_error(error);
                    (format!("Funny Meme Reply failed: {short_message}"), short_message, needs_settings)
                }
            };
            let _ = tx.send(UiCommand::VisionFinished(status));
            if let Some(step_index) = step_index {
                let _ = tx.send(UiCommand::MacroStepInlineFeedback {
                    preset_id,
                    step_index,
                    message: inline_message,
                    open_groq_settings,
                });
            }
        }

        result.map(|_| ())
    }

    fn focus_window_by_preset_id(spec: &str) -> Result<()> {
        window_preset::focus_window_by_preset_id(spec)
    }

    fn focus_window_for_preset(preset: &WindowFocusPreset) -> Result<()> {
        window_preset::focus_window_for_preset(preset)
    }



    fn macro_stop_requested(preset_id: u32, stop_immediately_on_retrigger: bool) -> bool {
        if FORCE_STOP_REQUESTED_MACRO_PRESETS.lock().contains(&preset_id) {
            return true;
        }

        if !STOP_REQUESTED_MACRO_PRESETS.lock().contains(&preset_id) {
            return false;
        }

        if stop_immediately_on_retrigger {
            return true;
        }

        HOOK_STATE
            .lock()
            .macro_groups
            .iter()
            .flat_map(|group| group.presets.iter())
            .find(|preset| preset.id == preset_id)
            .is_some_and(|preset| preset.stop_on_retrigger_immediate)
    }

    fn mouse_path_playback_should_stop(
        preset_id: Option<u32>,
        stop_immediately_on_retrigger: bool,
    ) -> bool {
        if preset_id.is_some_and(|id| macro_stop_requested(id, stop_immediately_on_retrigger)) {
            return true;
        }

        preset_id.is_some() && is_ui_in_foreground()
    }

    fn sleep_for_mouse_path_delay(
        preset_id: Option<u32>,
        delay_ms: u64,
        stop_immediately_on_retrigger: bool,
    ) -> bool {
        if delay_ms == 0 {
            return mouse_path_playback_should_stop(preset_id, stop_immediately_on_retrigger);
        }

        let mut remaining_ms = delay_ms;
        while remaining_ms > 0 {
            if mouse_path_playback_should_stop(preset_id, stop_immediately_on_retrigger) {
                return true;
            }

            let chunk_ms = remaining_ms.min(10);
            thread::sleep(Duration::from_millis(chunk_ms));
            remaining_ms = remaining_ms.saturating_sub(chunk_ms);
        }

        mouse_path_playback_should_stop(preset_id, stop_immediately_on_retrigger)
    }

    pub(crate) fn enable_crosshair_profile(spec: &str) -> Result<()> {
        let profile_name = spec.trim();
        if profile_name.is_empty() {
            bail!("Crosshair profile name is empty");
        }

        let mut hook_state = HOOK_STATE.lock();
        let profile_index = hook_state
            .profiles
            .iter()
            .position(|profile| profile.name == profile_name)
            .context("Crosshair profile was not found")?;
        let profile_name_owned = hook_state.profiles[profile_index].name.clone();
        hook_state.profiles[profile_index].enabled = true;
        let mut style = hook_state.profiles[profile_index].style.clone();
        style.enabled = true;
        hook_state.current_style = style.clone();
        hook_state.active_crosshair_profile_name = Some(profile_name_owned.clone());
        let profiles = hook_state.profiles.clone();
        drop(hook_state);
        send_overlay_command(OverlayCommand::Update(style));
        send_ui_command(UiCommand::SyncCrosshairProfiles(
            profiles,
            format!("Enabled crosshair profile {}.", profile_name_owned),
        ));
        Ok(())
    }

    fn disable_crosshair_overlay() {
        let mut hook_state = HOOK_STATE.lock();
        let mut style = hook_state.current_style.clone();
        style.enabled = false;
        hook_state.current_style = style.clone();
        hook_state.active_crosshair_profile_name = None;
        for profile in &mut hook_state.profiles {
            profile.enabled = false;
        }

        let profiles = hook_state.profiles.clone();
        drop(hook_state);
        send_ui_command(UiCommand::SyncCrosshairProfiles(
            profiles,
            "Disabled crosshair overlay.".to_owned(),
        ));
        send_overlay_command(OverlayCommand::Update(style));
    }

    pub(crate) fn enable_pin_preset(spec: &str) -> Result<()> {
        let preset_id = spec
            .trim()
            .parse::<u32>()
            .context("Pin preset id is invalid")?;
        let mut hook_state = HOOK_STATE.lock();
        if !hook_state
            .pin_presets
            .iter()
            .any(|preset| preset.id == preset_id)
        {
            bail!("Pin preset was not found");
        }

        hook_state.active_pin_preset_id = Some(preset_id);
        send_overlay_command(OverlayCommand::RefreshPinOverlay);
        Ok(())
    }

    fn disable_pin_overlay() {
        HOOK_STATE.lock().active_pin_preset_id = None;
        send_overlay_command(OverlayCommand::RefreshPinOverlay);
    }

    pub(crate) fn disable_crosshair_profile(spec: &str) {
        let profile_name = spec.trim();
        if profile_name.is_empty() {
            disable_crosshair_overlay();
            return;
        }

        let mut hook_state = HOOK_STATE.lock();
        let profile_index = hook_state
            .profiles
            .iter()
            .position(|profile| profile.name == profile_name);
        if let Some(idx) = profile_index {
            hook_state.profiles[idx].enabled = false;
            if hook_state.active_crosshair_profile_name.as_deref() == Some(profile_name) {
                let mut style = hook_state.current_style.clone();
                style.enabled = false;
                hook_state.current_style = style.clone();
                hook_state.active_crosshair_profile_name = None;
                let profiles = hook_state.profiles.clone();
                drop(hook_state);
                send_ui_command(UiCommand::SyncCrosshairProfiles(
                    profiles,
                    format!("Disabled crosshair profile {}.", profile_name),
                ));
                send_overlay_command(OverlayCommand::Update(style));
            } else {
                let profiles = hook_state.profiles.clone();
                drop(hook_state);
                send_ui_command(UiCommand::SyncCrosshairProfiles(
                    profiles,
                    format!("Disabled crosshair profile {}.", profile_name),
                ));
            }
        }
    }

    pub(crate) fn disable_pin_preset(spec: &str) {
        let preset_id = match spec.trim().parse::<u32>() {
            Ok(id) => id,
            Err(_) => {
                disable_pin_overlay();
                return;
            }
        };
        let mut hook_state = HOOK_STATE.lock();
        if hook_state.active_pin_preset_id == Some(preset_id) {
            hook_state.active_pin_preset_id = None;
            drop(hook_state);
            send_overlay_command(OverlayCommand::RefreshPinOverlay);
        }
    }

    fn play_sound_preset(spec: &str) -> Result<()> {
        let preset_id = spec
            .trim()
            .parse::<u32>()
            .context("Sound preset id is invalid")?;
        let clip = {
            let hook_state = HOOK_STATE.lock();
            let preset = hook_state
                .sound_presets
                .iter()
                .find(|preset| preset.id == preset_id)
                .cloned()
                .context("Sound preset was not found")?;
            let mut clip = preset.clip.clone();
            clip.enabled = true;
            clip
        };
        audio::play_clip_async(clip);
        Ok(())
    }

    fn play_video_preset(spec: &str) -> Result<()> {
        let preset_id = spec
            .trim()
            .parse::<u32>()
            .context("Video preset id is invalid")?;
        play_video_preset_by_id(preset_id)
    }

    fn play_video_preset_by_id(preset_id: u32) -> Result<()> {
        let preset = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .video_presets
                .iter()
                .find(|preset| preset.id == preset_id)
                .cloned()
                .context("Video preset was not found")?
        };
        let start_ms = preset.clip.start_ms;
        spawn_video_preset_playback_from(preset, start_ms);
        Ok(())
    }

    fn play_video_preset_by_id_from(preset_id: u32, start_ms: u64) -> Result<()> {
        let preset = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .video_presets
                .iter()
                .find(|preset| preset.id == preset_id)
                .cloned()
                .context("Video preset was not found")?
        };
        spawn_video_preset_playback_from(preset, start_ms);
        Ok(())
    }

    fn stop_active_video_preset_playback() {
        audio::stop_video_audio_preview();
        let previous = {
            let mut guard = ACTIVE_VIDEO_STOP.lock();
            guard.take()
        };
        if let Some(previous) = previous {
            previous.store(true, Ordering::Relaxed);
        }
        *ACTIVE_VIDEO_PRESET_ID.lock() = None;
    }

    fn spawn_video_preset_playback_from(preset: VideoPreset, start_ms: u64) {
        if preset.clip.file_path.trim().is_empty() {
            return;
        }

        stop_active_video_preset_playback();
        let previous_thread = {
            let mut guard = ACTIVE_VIDEO_THREAD.lock();
            guard.take()
        };
        if let Some(handle) = previous_thread {
            let _ = handle.join();
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        {
            let mut guard = ACTIVE_VIDEO_STOP.lock();
            guard.replace(stop_flag.clone());
        }
        *ACTIVE_VIDEO_PRESET_ID.lock() = Some(preset.id);
        let handle = thread::spawn(move || {
            let _ = unsafe { run_video_preset_window(&preset, start_ms, stop_flag.clone()) };
            let mut guard = ACTIVE_VIDEO_STOP.lock();
            if let Some(active) = guard.as_ref() {
                if Arc::ptr_eq(active, &stop_flag) {
                    *guard = None;
                    *ACTIVE_VIDEO_PRESET_ID.lock() = None;
                }
            }
        });
        {
            let mut guard = ACTIVE_VIDEO_THREAD.lock();
            guard.replace(handle);
        }
    }

    unsafe fn run_video_preset_window(
        preset: &VideoPreset,
        start_ms: u64,
        stop_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        let screen_w = GetSystemMetrics(SM_CXSCREEN).max(1);
        let screen_h = GetSystemMetrics(SM_CYSCREEN).max(1);
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            w!("CrosshairOverlay"),
            w!("MacroNestVideoOverlay"),
            WS_POPUP,
            0,
            0,
            screen_w,
            screen_h,
            None,
            None,
            Some(instance),
            None,
        )?;
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: screen_w,
                biHeight: -screen_h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits_ptr: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        )?;
        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        let (mut capture, metadata) =
            media::open_video_capture(&preset.clip.file_path, start_ms)?;
        let chroma_key = if preset.clip.chroma_key_enabled {
            Some((
                preset.clip.chroma_key_color,
                preset.clip.chroma_key_tolerance,
            ))
        } else {
            None
        };
        let clip_end_ms = if preset.clip.end_ms > preset.clip.start_ms {
            preset.clip.end_ms
        } else if metadata.duration_ms > preset.clip.start_ms {
            metadata.duration_ms
        } else {
            u64::MAX
        };
        let mut pt_src = POINT::default();
        let mut pt_dst = POINT { x: 0, y: 0 };
        let mut size_wnd = SIZE {
            cx: screen_w,
            cy: screen_h,
        };
        let mut blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = ShowWindow(hwnd, SW_SHOWNA);
        let _ = audio::play_video_audio_preview(
            &preset.clip.file_path,
            start_ms,
            clip_end_ms,
        );
        let playback_start = Instant::now();
        loop {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let mut frame = Mat::default();
            if !capture.read(&mut frame)? || frame.empty() {
                break;
            }

            let position_ms = capture
                .get(opencv::videoio::CAP_PROP_POS_MSEC)?
                .round()
                .max(0.0) as u64;
            if position_ms > clip_end_ms {
                break;
            }

            let elapsed_video_ms = position_ms.saturating_sub(start_ms);
            let elapsed_real_ms = playback_start.elapsed().as_millis() as u64;
            if elapsed_real_ms < elapsed_video_ms {
                let wait_ms = elapsed_video_ms - elapsed_real_ms;
                // Sleep for the bulk of wait time (avoiding imprecise Windows timer overhead)

                if wait_ms > 10 {
                    thread::sleep(Duration::from_millis(wait_ms - 5));
                }

                // Precise spin-yield loop for the remaining sub-milliseconds to guarantee flawless 60fps pacing

                while (playback_start.elapsed().as_millis() as u64) < elapsed_video_ms {
                    if stop_flag.load(Ordering::Relaxed) {
                        break;
                    }

                    std::thread::yield_now();
                }
            } else if elapsed_real_ms > elapsed_video_ms + 100 {
                // Lagging behind by more than 100ms, skip processing and rendering this frame to catch up.

                // Do NOT use capture.set seek here because seeking is a very expensive keyframe decode operation

                // that causes massive stuttering/frame jumping. Pure frame skipping is extremely fast.

                continue;
            }

            let (pixels, video_w, video_h) =
                media::frame_to_premultiplied_bgra(&frame, chroma_key, &preset.clip.resolution)?;
            let video_bitmap_info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: video_w,
                    biHeight: -video_h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let _ = StretchDIBits(
                mem_dc,
                0,
                0,
                screen_w,
                screen_h,
                0,
                0,
                video_w,
                video_h,
                Some(pixels.as_ptr() as *const c_void),
                &video_bitmap_info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(screen_dc),
                Some(&mut pt_dst),
                Some(&mut size_wnd),
                Some(mem_dc),
                Some(&mut pt_src),
                COLORREF(0),
                Some(&mut blend),
                ULW_ALPHA,
            );
        }

        let _ = ShowWindow(hwnd, SW_HIDE);
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        let _ = DestroyWindow(hwnd);
        audio::stop_video_audio_preview();
        let ui_tx = {
            let state = HOOK_STATE.lock();
            state.ui_tx.clone()
        };
        if let Some(tx) = ui_tx {
            let _ = tx.send(UiCommand::VideoPlaybackFinished(preset.id));
        }

        Ok(())
    }

    fn play_mouse_path_preset(
        spec: &str,
        step: &MacroStep,
        preset_id: Option<u32>,
        stop_immediately_on_retrigger: bool,
    ) -> Result<()> {
        let mouse_path_preset_id = spec
            .trim()
            .parse::<u32>()
            .context("Mouse path preset id is invalid")?;
        let (events, _, replay_relative_motion) = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .mouse_path_presets
                .iter()
                .find(|preset| preset.id == mouse_path_preset_id)
                .map(|preset| (preset.events.clone(), false, preset.replay_relative_motion))
                .context("Mouse path preset was not found")?
        };
        if events.is_empty() {
            return Ok(());
        }

        if step.smooth_mouse_path {
            let speed = step.get_mouse_speed_multiplier();
            let mut last_move_pos: Option<(i32, i32)> = None;
            for event in &events {
                if mouse_path_playback_should_stop(preset_id, stop_immediately_on_retrigger) {
                    return Ok(());
                }

                match event.kind {
                    MousePathEventKind::Move => {
                        if replay_relative_motion {
                            if let Some((from_x, from_y)) = last_move_pos {
                                settle_mouse_path_relative_segment(
                                    from_x,
                                    from_y,
                                    event.x,
                                    event.y,
                                    speed,
                                    preset_id,
                                    stop_immediately_on_retrigger,
                                )?;
                            }

                            last_move_pos = Some((event.x, event.y));
                        } else if let Some((from_x, from_y)) = last_move_pos {
                            let dx = event.x - from_x;
                            let dy = event.y - from_y;
                            let distance = (((dx * dx + dy * dy) as f32).sqrt()).max(1.0);
                            let duration_ms = ((distance / (900.0 * speed)) * 1000.0)
                                .round()
                                .clamp(1.0, 5_000.0)
                                as u64;
                            let steps = ((duration_ms as f32) / 8.0).ceil().max(1.0) as u64;
                            let frame_delay_ms =
                                ((duration_ms as f32) / steps as f32).round().max(1.0) as u64;
                            for index in 1..=steps {
                                if mouse_path_playback_should_stop(
                                    preset_id,
                                    stop_immediately_on_retrigger,
                                ) {
                                    return Ok(());
                                }

                                let t = index as f32 / steps as f32;
                                let x = from_x as f32 + dx as f32 * t;
                                let y = from_y as f32 + dy as f32 * t;
                                send_mouse_move_absolute(x.round() as i32, y.round() as i32)?;
                                if sleep_for_mouse_path_delay(
                                    preset_id,
                                    frame_delay_ms,
                                    stop_immediately_on_retrigger,
                                ) {
                                    return Ok(());
                                }
                            }

                            last_move_pos = Some((event.x, event.y));
                        } else {
                            send_mouse_move_absolute(event.x, event.y)?;
                            last_move_pos = Some((event.x, event.y));
                        }
                    }

                    _ => {
                        if sleep_for_mouse_path_delay(
                            preset_id,
                            event.delay_ms,
                            stop_immediately_on_retrigger,
                        ) {
                            return Ok(());
                        }

                        let pseudo_step = MacroStep {
                            action: match event.kind {
                                MousePathEventKind::LeftDown => MacroAction::MouseLeftDown,
                                MousePathEventKind::LeftUp => MacroAction::MouseLeftUp,
                                MousePathEventKind::RightDown => MacroAction::MouseRightDown,
                                MousePathEventKind::RightUp => MacroAction::MouseRightUp,
                                MousePathEventKind::MiddleDown => MacroAction::MouseMiddleDown,
                                MousePathEventKind::MiddleUp => MacroAction::MouseMiddleUp,
                                MousePathEventKind::WheelUp => MacroAction::MouseWheelUp,
                                MousePathEventKind::WheelDown => MacroAction::MouseWheelDown,
                                MousePathEventKind::Move => MacroAction::MouseMoveAbsolute,
                            },
                            x: event.x,
                            y: event.y,
                            ..MacroStep::default()
                        };
                        send_mouse_event(&pseudo_step)?;
                    }
                }
            }
        } else {
            let mut last_move_pos: Option<(i32, i32)> = None;
            for event in &events {
                if sleep_for_mouse_path_delay(
                    preset_id,
                    event.delay_ms,
                    stop_immediately_on_retrigger,
                ) {
                    return Ok(());
                }

                match event.kind {
                    MousePathEventKind::Move if replay_relative_motion => {
                        if let Some((from_x, from_y)) = last_move_pos {
                            send_mouse_move_relative(event.x - from_x, event.y - from_y)?;
                        }

                        last_move_pos = Some((event.x, event.y));
                    }

                    MousePathEventKind::Move => {
                        let pseudo_step = MacroStep {
                            action: MacroAction::MouseMoveAbsolute,
                            x: event.x,
                            y: event.y,
                            ..MacroStep::default()
                        };
                        send_mouse_event(&pseudo_step)?;
                    }

                    _ => {
                        let pseudo_step = MacroStep {
                            action: match event.kind {
                                MousePathEventKind::LeftDown => MacroAction::MouseLeftDown,
                                MousePathEventKind::LeftUp => MacroAction::MouseLeftUp,
                                MousePathEventKind::RightDown => MacroAction::MouseRightDown,
                                MousePathEventKind::RightUp => MacroAction::MouseRightUp,
                                MousePathEventKind::MiddleDown => MacroAction::MouseMiddleDown,
                                MousePathEventKind::MiddleUp => MacroAction::MouseMiddleUp,
                                MousePathEventKind::WheelUp => MacroAction::MouseWheelUp,
                                MousePathEventKind::WheelDown => MacroAction::MouseWheelDown,
                                MousePathEventKind::Move => MacroAction::MouseMoveAbsolute,
                            },
                            x: event.x,
                            y: event.y,
                            ..MacroStep::default()
                        };
                        send_mouse_event(&pseudo_step)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn start_mouse_path_preset_playback(
        spec: &str,
        step: &MacroStep,
        preset_id: Option<u32>,
        stop_immediately_on_retrigger: bool,
    ) {
        if step.wait_for_completion {
            let _ = play_mouse_path_preset(spec, step, preset_id, stop_immediately_on_retrigger);
            return;
        }

        let spec = spec.trim().to_owned();
        let step = step.clone();
        thread::spawn(move || {
            if let Err(error) =
                play_mouse_path_preset(&spec, &step, preset_id, stop_immediately_on_retrigger)
            {
                eprintln!("Mouse path playback failed: {error}");
            }
        });
    }

    fn apply_mouse_sensitivity_preset_by_id(spec: &str) -> Result<()> {
        let preset_id = parse_mouse_sensitivity_preset_id(spec)
            .context("Mouse sensitivity preset id is invalid")?;
        let preset = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .mouse_sensitivity_presets
                .iter()
                .find(|preset| preset.id == preset_id)
                .cloned()
                .context("Mouse sensitivity preset was not found")?
        };
        apply_mouse_sensitivity_preset(&preset)
    }

    fn apply_manual_mouse_sensitivity(key: &str) -> Result<()> {
        let interpolated = interpolate_variables(key);
        let evaluated = evaluate_math_expression(&interpolated);
        let speed = evaluated.clamp(1, 20) as u32;
        let mut hook_state = HOOK_STATE.lock();
        if hook_state.mouse_sensitivity_restore_speed.is_none() {
            hook_state.mouse_sensitivity_restore_speed = Some(current_mouse_speed()?);
        }

        hook_state.active_mouse_sensitivity_preset_id = None;
        drop(hook_state);
        set_mouse_speed(speed)
    }

    fn enable_zoom_preset(_spec: &str) -> Result<()> {
        bail!("Zoom was removed")
    }

    fn disable_zoom_overlay() {}

    fn set_macro_preset_enabled(spec: &str, enabled: bool) -> Result<()> {
        let preset_id = spec
            .trim()
            .parse::<u32>()
            .context("Macro preset id is invalid")?;
        let mut hook_state = HOOK_STATE.lock();
        for group in &mut hook_state.macro_groups {
            if let Some(preset) = group
                .presets
                .iter_mut()
                .find(|preset| preset.id == preset_id)
            {
                preset.enabled = enabled;
                if !enabled {
                    STOP_REQUESTED_MACRO_PRESETS.lock().insert(preset_id);
                }

                let updated_groups = hook_state.macro_groups.clone();
                let status = format!(
                    "{} macro preset {}.",
                    if enabled { "Enabled" } else { "Disabled" },
                    preset_id
                );
                if let Some(tx) = hook_state.ui_tx.clone() {
                    let _ = tx.send(UiCommand::SyncMacroGroups(updated_groups, status));
                }

                drop(hook_state);
                if !enabled {
                    deactivate_hold_macro(preset_id);
                }

                return Ok(());
            }
        }

        bail!("Macro preset was not found")
    }

    fn parse_macro_trigger_preset_ids(spec: &str) -> Vec<u32> {
        spec.split(',')
            .filter_map(|part| part.trim().parse::<u32>().ok())
            .collect()
    }

    fn collect_trigger_macro_target_ids(spec: &str, bypass_enabled: bool) -> Vec<u32> {
        let target_ids = parse_macro_trigger_preset_ids(spec);
        if bypass_enabled {
            return target_ids;
        }

        let hook_state = HOOK_STATE.lock();
        target_ids
            .into_iter()
            .filter(|preset_id| is_macro_preset_enabled_with_guard(*preset_id, &hook_state))
            .collect()
    }

    fn execute_trigger_macro_step(
        step: &MacroStep,
        bypass_enabled: bool,
        no_locked_keys: &mut Vec<String>,
        no_locked_mouse: &mut Vec<MouseMoveLockMask>,
    ) {
        let target_ids = collect_trigger_macro_target_ids(&step.key, bypass_enabled);
        if step.wait_for_completion {
            for preset_id in target_ids {
                let _ = trigger_nested_macro_preset(
                    &preset_id.to_string(),
                    no_locked_keys,
                    no_locked_mouse,
                    false,
                    None,
                    &[],
                    false,
                    bypass_enabled,
                );
            }
        } else {
            for preset_id in target_ids {
                spawn_macro_by_preset_id(preset_id, bypass_enabled);
            }
        }
    }

    fn stop_macro_preset_by_id(preset_id: u32) {
        FORCE_STOP_REQUESTED_MACRO_PRESETS.lock().insert(preset_id);
        let is_active_hold = {
            let hook_state = HOOK_STATE.lock();
            hook_state.active_hold_macros.contains_key(&preset_id)
        };

        if is_active_hold {
            deactivate_hold_macro(preset_id);
        } else {
            STOP_REQUESTED_MACRO_PRESETS.lock().insert(preset_id);
        }
    }

    fn execute_stop_macro_step(step: &MacroStep) {
        for preset_id in parse_macro_trigger_preset_ids(&step.key) {
            stop_macro_preset_by_id(preset_id);
        }
    }

    fn set_macro_steps_enabled(spec: &str, enabled: bool) -> Result<()> {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.is_empty() {
            bail!("Invalid step enable/disable spec format");
        }

        let preset_id = parts[0]
            .trim()
            .parse::<u32>()
            .context("Macro preset id is invalid")?;
        let mut steps_to_change = Vec::new();
        if parts.len() > 1 {
            for step_str in parts[1].split(',') {
                if let Ok(idx) = step_str.trim().parse::<usize>() {
                    if idx > 0 {
                        steps_to_change.push(idx - 1);
                    }
                }
            }
        }

        let mut hook_state = HOOK_STATE.lock();
        for group in &mut hook_state.macro_groups {
            if let Some(preset) = group
                .presets
                .iter_mut()
                .find(|preset| preset.id == preset_id)
            {
                for &idx in &steps_to_change {
                    if idx < preset.steps.len() {
                        preset.steps[idx].enabled = enabled;
                    }
                }

                let updated_groups = hook_state.macro_groups.clone();
                let status = format!(
                    "{} steps {:?} in macro preset {}.",
                    if enabled { "Enabled" } else { "Disabled" },
                    steps_to_change.iter().map(|x| x + 1).collect::<Vec<_>>(),
                    preset_id
                );
                if let Some(tx) = hook_state.ui_tx.clone() {
                    let _ = tx.send(UiCommand::SyncMacroGroups(updated_groups, status));
                }

                return Ok(());
            }
        }

        bail!("Macro preset was not found")
    }

    fn execute_hold_abort_step(preset_id: u32, step: &MacroStep) {
        if !step.enabled {
            return;
        }

        match step.action {
            MacroAction::LoopStart
            | MacroAction::LoopEnd
            | MacroAction::StopIfTriggerPressedAgain
            | MacroAction::StopIfKeyPressed => {}

            MacroAction::ApplyWindowPreset => {
                let _ = apply_window_preset_by_id(&step.key);
            }

            MacroAction::OcrSearch => {
                execute_ocr_action_step(step);
            }

            MacroAction::FocusWindowPreset => {
                let _ = focus_window_by_preset_id(&step.key);
            }

            MacroAction::TriggerMacroPreset => {
                let mut no_locked_keys = Vec::new();
                let mut no_locked_mouse: Vec<MouseMoveLockMask> = Vec::new();
                execute_trigger_macro_step(step, true, &mut no_locked_keys, &mut no_locked_mouse);
            }

            MacroAction::TriggerMacroPresetIfEnabled => {
                let mut no_locked_keys = Vec::new();
                let mut no_locked_mouse: Vec<MouseMoveLockMask> = Vec::new();
                execute_trigger_macro_step(step, false, &mut no_locked_keys, &mut no_locked_mouse);
            }

            MacroAction::StopMacroPreset => {
                execute_stop_macro_step(step);
            }

            MacroAction::TriggerCommandPreset => {
                let _ = trigger_command_preset_step(step);
            }

            MacroAction::FunnyMemeReply => {
                let _ = trigger_funny_meme_reply_step(preset_id, None, step);
            }

            MacroAction::EnableCrosshairProfile => {
                let _ = enable_crosshair_profile(&step.key);
                let duration = step.get_duration_ms();
                let mut state = HOOK_STATE.lock();
                if duration > 0 {
                    state.active_crosshair_expires = Some(Instant::now() + Duration::from_millis(duration));
                } else {
                    state.active_crosshair_expires = None;
                }
            }

            MacroAction::DisableCrosshair => {
                if step.lock_mouse_left {
                    disable_crosshair_overlay();
                } else {
                    disable_crosshair_profile(&step.key);
                }
            }

            MacroAction::EnablePinPreset => {
                let _ = enable_pin_preset(&step.key);
                let duration = step.get_duration_ms();
                let mut state = HOOK_STATE.lock();
                if duration > 0 {
                    state.active_pin_expires = Some(Instant::now() + Duration::from_millis(duration));
                } else {
                    state.active_pin_expires = None;
                }
            }

            MacroAction::DisablePin => {
                if step.lock_mouse_left {
                    disable_pin_overlay();
                } else {
                    disable_pin_preset(&step.key);
                }
            }

            MacroAction::PlayMousePathPreset => {
                start_mouse_path_preset_playback(&step.key, step, Some(preset_id), false);
            }

            MacroAction::EnableZoomPreset => {
                let _ = enable_zoom_preset(&step.key);
            }

            MacroAction::DisableZoom => {
                disable_zoom_overlay();
            }

            MacroAction::PlaySoundPreset => {
                let _ = play_sound_preset(&step.key);
            }

            MacroAction::PlayVideoPreset => {
                let _ = play_video_preset(&step.key);
                let duration = step.get_duration_ms();
                let mut expires_guard = ACTIVE_VIDEO_EXPIRES.lock();
                if duration > 0 {
                    *expires_guard = Some(Instant::now() + Duration::from_millis(duration));
                } else {
                    *expires_guard = None;
                }
            }

            MacroAction::StartVisionSearch => {
                let _ = start_vision_following(&step.key, Some(&step.if_variable_name));
            }

            MacroAction::StartAudioSensePreset => {
                start_audio_sense_from_step(step, preset_id, 0, false, true, false);
            }

            MacroAction::ScanVisionOnce => {
                if let Ok(preset) = vision_preset_by_id(&step.key) {
                    let outcome = match run_vision_once_with_options(
                        &preset,
                        step.vision_move_cursor_on_match,
                        false,
                        Some(&step.if_variable_name),
                        Some(&step.vision_pos_var_x),
                        Some(&step.vision_pos_var_y),
                        Some(&step.vision_found_var),
                    ) {
                        Ok(outcome) => outcome,
                        Err(error) => {
                            eprintln!("ScanVisionOnce failed: {error}");
                            return;
                        }
                    };
                    if let Some(tx) = HOOK_STATE.lock().ui_tx.clone() {
                        let _ = tx.send(UiCommand::VisionFinished(format!(
                            "{}: {}",
                            preset.name, outcome.status
                        )));
                    }
                }
            }

            MacroAction::StopVisionWait => {
                let _ = stop_vision_waiting(&step.key);
            }

            MacroAction::StopVision => {
                let _ = stop_vision_following(&step.key);
            }

            MacroAction::StopAudioSense => {
                stop_audio_sense_from_step(step, preset_id, 0, false);
            }

            MacroAction::ShowHud => {
                trigger_hud_display(preset_id, step);
            }

            MacroAction::HideHud => {
                hide_hud_now();
            }

            MacroAction::HideTaskbar => {
                let _ = crate::platform::hide_taskbar();
            }

            MacroAction::ShowTaskbar => {
                let _ = crate::platform::show_taskbar();
            }

            MacroAction::StartTimerPreset
            | MacroAction::PauseTimerPreset
            | MacroAction::StopTimerPreset => {
                let t_id = step
                    .timer_preset_id
                    .or_else(|| step.key.trim().parse::<u32>().ok());
                execute_timer_preset_action(
                    step.action,
                    t_id,
                    step.timer_on_complete_macro_preset_id,
                );
            }

            MacroAction::LockKeys => {
                apply_lock_keys(
                    &parse_locked_keys(&step.key),
                    Some(preset_id),
                    step.unlock_on_exit,
                );
            }

            MacroAction::UnlockKeys => {
                apply_unlock_keys(&parse_locked_keys(&step.key), Some(preset_id));
            }

            MacroAction::LockMouse => {
                apply_lock_mouse(step, Some(preset_id), step.unlock_on_exit);
            }

            MacroAction::UnlockMouse => {
                apply_unlock_mouse(Some(preset_id), mouse_move_lock_mask_from_step(step));
            }

            MacroAction::EnableMacroPreset => {
                let _ = set_macro_preset_enabled(&step.key, true);
            }

            MacroAction::DisableMacroPreset => {
                let _ = set_macro_preset_enabled(&step.key, false);
            }

            MacroAction::EnableStep => {
                let _ = set_macro_steps_enabled(&step.key, true);
            }

            MacroAction::DisableStep => {
                let _ = set_macro_steps_enabled(&step.key, false);
            }

            _ => {
                let _ = send_key_event(step);
            }
        }
    }

    fn is_macro_step_enabled(preset_id: u32, step_index: usize, fallback: bool) -> bool {
        let hook_state = HOOK_STATE.lock();
        for group in &hook_state.macro_groups {
            if let Some(preset) = group.presets.iter().find(|preset| preset.id == preset_id) {
                if step_index < preset.steps.len() {
                    return preset.steps[step_index].enabled;
                }
            }
        }

        fallback
    }

    fn is_macro_preset_enabled_with_guard(preset_id: u32, hook_state: &HookState) -> bool {
        hook_state
            .macro_groups
            .iter()
            .find_map(|group| {
                group.presets.iter().find(|preset| preset.id == preset_id).map(|preset| {
                    group.enabled && preset.enabled
                })
            })
            .unwrap_or(false)
    }

    fn is_macro_preset_enabled(preset_id: u32) -> bool {
        let hook_state = HOOK_STATE.lock();
        is_macro_preset_enabled_with_guard(preset_id, &hook_state)
    }

    fn toggle_macro_step_enabled(preset_id: u32, step_index: usize) -> Option<bool> {
        let mut hook_state = HOOK_STATE.lock();
        for group in &mut hook_state.macro_groups {
            if let Some(preset) = group
                .presets
                .iter_mut()
                .find(|preset| preset.id == preset_id)
            {
                if step_index < preset.steps.len() {
                    preset.steps[step_index].enabled = !preset.steps[step_index].enabled;
                    let new_enabled = preset.steps[step_index].enabled;
                    let updated_groups = hook_state.macro_groups.clone();
                    let status = format!(
                        "Toggled step {} in macro preset {} to {}.",
                        step_index + 1,
                        preset_id,
                        if new_enabled { "Enabled" } else { "Disabled" }
                    );
                    if let Some(tx) = hook_state.ui_tx.clone() {
                        let _ = tx.send(UiCommand::SyncMacroGroups(updated_groups, status));
                    }

                    return Some(new_enabled);
                }
            }
        }

        None
    }

    fn execute_macro_sequence(
        preset_id: u32,
        steps: &[MacroStep],
        step_indices: &[usize],
        press_locked_keys: &mut Vec<String>,
        press_locked_mouse_masks: &mut Vec<MouseMoveLockMask>,
        stop_immediately_on_retrigger: bool,
        target_window_title: Option<&str>,
        extra_target_window_titles: &[String],
        match_duplicate_window_titles: bool,
        bypass_enabled: bool,
    ) -> MacroRunFlow {
        let mut index = 0usize;
        'outer: while index < steps.len() {
            if !bypass_enabled && !is_macro_preset_enabled(preset_id) {
                return MacroRunFlow::StopExecution;
            }

            if !macro_runtime_target_matches(
                target_window_title,
                extra_target_window_titles,
                match_duplicate_window_titles,
            ) {
                return MacroRunFlow::StopExecution;
            }

            if macro_stop_requested(preset_id, stop_immediately_on_retrigger) {
                return MacroRunFlow::StopExecution;
            }

            let step = &steps[index];
            let absolute_index = step_indices[index];
            let is_enabled = is_macro_step_enabled(preset_id, absolute_index, step.enabled);
            let mut run_step = is_enabled;
            if step.toggle_enabled_on_run {
                if let Some(new_enabled) = toggle_macro_step_enabled(preset_id, absolute_index) {
                    run_step = !new_enabled;
                }
            }

            if !run_step {
                index += 1;
                continue;
            }

            let _guard = ActiveStepGuard::new(preset_id, absolute_index);
            if sleep_for_macro_delay(
                preset_id,
                step.get_delay_ms(),
                stop_immediately_on_retrigger,
                target_window_title,
                extra_target_window_titles,
                match_duplicate_window_titles,
                bypass_enabled,
            ) {
                return MacroRunFlow::StopExecution;
            }

            match step.action {
                MacroAction::LoopStart => {
                    let Some(loop_end) = find_matching_loop_end(steps, index) else {
                        index += 1;
                        continue;
                    };
                    let loop_body = &steps[index + 1..loop_end];
                    let loop_body_indices = &step_indices[index + 1..loop_end];
                    let loop_end_delay_ms = steps[loop_end].get_delay_ms();
                    if is_infinite_loop_marker(&step.key) {
                        loop {
                            match execute_macro_sequence(
                                preset_id,
                                loop_body,
                                loop_body_indices,
                                press_locked_keys,
                                press_locked_mouse_masks,
                                stop_immediately_on_retrigger,
                                target_window_title,
                                extra_target_window_titles,
                                match_duplicate_window_titles,
                                bypass_enabled,
                            ) {
                                MacroRunFlow::BreakLoop => break,
                                MacroRunFlow::StopExecution => return MacroRunFlow::StopExecution,
                                MacroRunFlow::Continue => {}
                                MacroRunFlow::JumpTo(target) => {
                                    if let Some(pos) = step_indices.iter().position(|&x| x == target) {
                                        index = pos;
                                        continue 'outer;
                                    } else {
                                        return MacroRunFlow::JumpTo(target);
                                    }
                                }
                            }

                            if loop_end_delay_ms > 0
                                && sleep_for_macro_delay(
                                    preset_id,
                                    loop_end_delay_ms,
                                    stop_immediately_on_retrigger,
                                    target_window_title,
                                    extra_target_window_titles,
                                    match_duplicate_window_titles,
                                    bypass_enabled,
                                )
                            {
                                return MacroRunFlow::StopExecution;
                            }
                        }
                    } else {
                        let loop_count_str = interpolate_variables(&step.key);
                        let loop_count = loop_count_str.trim().parse::<u32>().unwrap_or(1).max(1);
                        for _ in 0..loop_count {
                            match execute_macro_sequence(
                                preset_id,
                                loop_body,
                                loop_body_indices,
                                press_locked_keys,
                                press_locked_mouse_masks,
                                stop_immediately_on_retrigger,
                                target_window_title,
                                extra_target_window_titles,
                                match_duplicate_window_titles,
                                bypass_enabled,
                            ) {
                                MacroRunFlow::BreakLoop => break,
                                MacroRunFlow::StopExecution => return MacroRunFlow::StopExecution,
                                MacroRunFlow::Continue => {}
                                MacroRunFlow::JumpTo(target) => {
                                    if let Some(pos) = step_indices.iter().position(|&x| x == target) {
                                        index = pos;
                                        continue 'outer;
                                    } else {
                                        return MacroRunFlow::JumpTo(target);
                                    }
                                }
                            }

                            if loop_end_delay_ms > 0
                                && sleep_for_macro_delay(
                                    preset_id,
                                    loop_end_delay_ms,
                                    stop_immediately_on_retrigger,
                                    target_window_title,
                                    extra_target_window_titles,
                                    match_duplicate_window_titles,
                                    bypass_enabled,
                                )
                            {
                                return MacroRunFlow::StopExecution;
                            }
                        }
                    }

                    index = loop_end + 1;
                    continue;
                }

                MacroAction::LoopEnd => return MacroRunFlow::Continue,
                MacroAction::IfStart => {
                    let (else_index, if_end_index) = find_matching_if_structure(steps, index);
                    let condition_met = evaluate_if_condition(step);
                    if !condition_met {
                        if let Some(else_idx) = else_index {
                            index = else_idx;
                        } else if let Some(end_idx) = if_end_index {
                            index = end_idx;
                        } else {
                            index = steps.len();
                        }
                    }
                }

                MacroAction::Else => {
                    if let Some(end_idx) = find_matching_if_end_from_else(steps, index) {
                        index = end_idx;
                    } else {
                        index = steps.len();
                    }
                }

                MacroAction::IfEnd => {}

                MacroAction::SetVariable => {
                    let target_var = step.if_variable_name.trim().to_string();
                    if !target_var.is_empty() {
                        match step.set_variable_source {
                            crate::model::SetVariableSource::Expression => {
                                smart_set_variable_from_expression(&target_var, &step.key);
                            }
                            _ => {
                                let value = match step.set_variable_source {
                                    crate::model::SetVariableSource::TimeHour => {
                                        use chrono::Timelike;
                                        chrono::Local::now().hour() as i32
                                    }
                                    crate::model::SetVariableSource::TimeMinute => {
                                        use chrono::Timelike;
                                        chrono::Local::now().minute() as i32
                                    }
                                    crate::model::SetVariableSource::TimeSecond => {
                                        use chrono::Timelike;
                                        chrono::Local::now().second() as i32
                                    }
                                    crate::model::SetVariableSource::TimeMillisecond => {
                                        use chrono::Timelike;
                                        chrono::Local::now().nanosecond() as i32 / 1_000_000
                                    }
                                    _ => 0,
                                };
                                set_variable_value(&target_var, value as f64);
                                TEXT_VARIABLES.lock().remove(&target_var);
                            }
                        }
                        send_overlay_command(OverlayCommand::RefreshSearchAreaOverlay);
                    }
                }

                MacroAction::JumpToStep => {
                    let interpolated = interpolate_variables(&step.key);
                    let target_val = if let Ok(val) = interpolated.trim().parse::<f64>() {
                        val
                    } else {
                        evaluate_math_expression_f64(&interpolated)
                    };
                    if target_val.is_nan() || target_val.is_infinite() {
                        return MacroRunFlow::StopExecution;
                    }
                    let target_idx = (target_val.round() as isize) - 1;
                    if target_idx >= 0 {
                        let target_abs = target_idx as usize;
                        if let Some(pos) = step_indices.iter().position(|&x| x == target_abs) {
                            index = pos;
                            continue 'outer;
                        } else {
                            return MacroRunFlow::JumpTo(target_abs);
                        }
                    } else {
                        return MacroRunFlow::StopExecution;
                    }
                }

                MacroAction::StopIfTriggerPressedAgain => {
                    if STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset_id) {
                        return MacroRunFlow::BreakLoop;
                    }
                }

                MacroAction::StopIfKeyPressed => match step.get_break_loop_mode() {
                    "VarCompare" => {
                        if evaluate_if_condition(step) {
                            return MacroRunFlow::BreakLoop;
                        }
                    }

                    "StopKey" => {
                        let keys = parse_stop_keys(&step.key);
                        if keys.iter().any(|key| stop_key_triggered(preset_id, key)) {
                            return MacroRunFlow::BreakLoop;
                        }
                    }

                    _ => {
                        return MacroRunFlow::BreakLoop;
                    }
                },
                MacroAction::ApplyWindowPreset => {
                    let _ = apply_window_preset_by_id(&step.key);
                }

                MacroAction::OcrSearch => {
                    execute_ocr_action_step(step);
                }

                MacroAction::FocusWindowPreset => {
                    let _ = focus_window_by_preset_id(&step.key);
                }

                MacroAction::TriggerMacroPreset => {
                    let target_ids = collect_trigger_macro_target_ids(&step.key, true);
                    if step.wait_for_completion {
                        for preset_id in target_ids {
                            let _ = trigger_nested_macro_preset(
                                &preset_id.to_string(),
                                press_locked_keys,
                                press_locked_mouse_masks,
                                stop_immediately_on_retrigger,
                                target_window_title,
                                extra_target_window_titles,
                                match_duplicate_window_titles,
                                true,
                            );
                        }
                    } else {
                        for preset_id in target_ids {
                            spawn_macro_by_preset_id(preset_id, true);
                        }
                    }
                }

                MacroAction::TriggerMacroPresetIfEnabled => {
                    let target_ids = collect_trigger_macro_target_ids(&step.key, false);
                    if step.wait_for_completion {
                        for preset_id in target_ids {
                            let _ = trigger_nested_macro_preset(
                                &preset_id.to_string(),
                                press_locked_keys,
                                press_locked_mouse_masks,
                                stop_immediately_on_retrigger,
                                target_window_title,
                                extra_target_window_titles,
                                match_duplicate_window_titles,
                                false,
                            );
                        }
                    } else {
                        for preset_id in target_ids {
                            spawn_macro_by_preset_id(preset_id, false);
                        }
                    }
                }

                MacroAction::StopMacroPreset => {
                    execute_stop_macro_step(step);
                }

                MacroAction::TriggerCommandPreset => {
                    let _ = trigger_command_preset_step(step);
                }

                MacroAction::FunnyMemeReply => {
                    let _ = trigger_funny_meme_reply_step(preset_id, Some(absolute_index), step);
                }

                MacroAction::EnableCrosshairProfile => {
                    let _ = enable_crosshair_profile(&step.key);
                    let duration = step.get_duration_ms();
                    let mut state = HOOK_STATE.lock();
                    if duration > 0 {
                        state.active_crosshair_expires = Some(Instant::now() + Duration::from_millis(duration));
                    } else {
                        state.active_crosshair_expires = None;
                    }
                }

                MacroAction::DisableCrosshair => {
                    if step.lock_mouse_left {
                        disable_crosshair_overlay();
                    } else {
                        disable_crosshair_profile(&step.key);
                    }
                }

                MacroAction::EnablePinPreset => {
                    let _ = enable_pin_preset(&step.key);
                    let duration = step.get_duration_ms();
                    let mut state = HOOK_STATE.lock();
                    if duration > 0 {
                        state.active_pin_expires = Some(Instant::now() + Duration::from_millis(duration));
                    } else {
                        state.active_pin_expires = None;
                    }
                }

                MacroAction::DisablePin => {
                    if step.lock_mouse_left {
                        disable_pin_overlay();
                    } else {
                        disable_pin_preset(&step.key);
                    }
                }

                MacroAction::PlayMousePathPreset => {
                    start_mouse_path_preset_playback(
                        &step.key,
                        step,
                        Some(preset_id),
                        stop_immediately_on_retrigger,
                    );
                }

                MacroAction::ApplyMouseSensitivityPreset => {
                    if step.manual_mouse_sensitivity {
                        let _ = apply_manual_mouse_sensitivity(&step.key);
                    } else {
                        let _ = apply_mouse_sensitivity_preset_by_id(&step.key);
                    }
                }

                MacroAction::EnableZoomPreset => {
                    let _ = enable_zoom_preset(&step.key);
                }

                MacroAction::DisableZoom => {
                    disable_zoom_overlay();
                }

                MacroAction::PlaySoundPreset => {
                    let _ = play_sound_preset(&step.key);
                }

                MacroAction::PlayVideoPreset => {
                    let _ = play_video_preset(&step.key);
                    let duration = step.get_duration_ms();
                    let mut expires_guard = ACTIVE_VIDEO_EXPIRES.lock();
                    if duration > 0 {
                        *expires_guard = Some(Instant::now() + Duration::from_millis(duration));
                    } else {
                        *expires_guard = None;
                    }
                }

                MacroAction::StartVisionSearch => {
                    let _ = start_vision_following(&step.key, Some(&step.if_variable_name));
                }

                MacroAction::StartAudioSensePreset => {
                    start_audio_sense_from_step(
                        step,
                        preset_id,
                        absolute_index,
                        false,
                        true,
                        false,
                    );
                }

                MacroAction::ScanVisionOnce => {
                    if let Some(preset) = vision_preset_by_id(&step.key).ok() {
                        let outcome = match run_vision_once_with_options(
                            &preset,
                            step.vision_move_cursor_on_match,
                            false,
                            Some(&step.if_variable_name),
                            Some(&step.vision_pos_var_x),
                            Some(&step.vision_pos_var_y),
                            Some(&step.vision_found_var),
                        ) {
                            Ok(outcome) => outcome,
                            Err(error) => {
                                eprintln!("ScanVisionOnce failed: {error}");
                                return MacroRunFlow::Continue;
                            }
                        };
                        let ui_tx = HOOK_STATE.lock().ui_tx.clone();
                        if let Some(tx) = ui_tx {
                            let _ = tx.send(UiCommand::VisionFinished(format!(
                                "{}: {}",
                                preset.name, outcome.status
                            )));
                        }
                        send_overlay_command(OverlayCommand::RefreshSearchAreaOverlay);
                    }
                }

                MacroAction::DrawGeometry => {
                    set_step_geometry_spec(preset_id, absolute_index, &step.geometry_spec);
                    let duration = step.get_duration_ms();
                    let mut state = HOOK_STATE.lock();
                    if duration > 0 {
                        state.active_geometry_steps_expires.insert((preset_id, absolute_index), Instant::now() + Duration::from_millis(duration));
                    } else {
                        state.active_geometry_steps_expires.remove(&(preset_id, absolute_index));
                    }
                }


                MacroAction::ShowGeometryPreset => {
                    let owner = (preset_id, absolute_index);
                    if let Some(base_preset) = resolve_geometry_preset_from_step(step) {
                        let duration = step.get_duration_ms();
                        if step.geometry_preset_modify_enabled {
                            let instance = build_geometry_preset_instance_from_step(&base_preset, step);
                            activate_geometry_preset_owner(
                                owner,
                                base_preset.id,
                                Some(instance),
                                duration,
                            );
                        } else {
                            activate_geometry_preset_owner(owner, base_preset.id, None, duration);
                        }
                    }
                }

                MacroAction::HideGeometryPreset => {
                    if let Some(geometry_preset_id) = resolve_geometry_preset_id_from_step(step) {
                        hide_geometry_preset_by_id(
                            geometry_preset_id,
                            step.geometry_hide_mode,
                        );
                    } else {
                        clear_geometry_overlay();
                    }
                }

                MacroAction::StopVisionWait => {
                    let _ = stop_vision_waiting(&step.key);
                }

                MacroAction::StopVision => {
                    let _ = stop_vision_following(&step.key);
                }

                MacroAction::StopAudioSense => {
                    stop_audio_sense_from_step(step, preset_id, absolute_index, false);
                }

                MacroAction::ShowHud => {
                    trigger_hud_display(preset_id, step);
                }

                MacroAction::HideHud => {
                    hide_hud_now();
                }

                MacroAction::HideTaskbar => {
                    let _ = crate::platform::hide_taskbar();
                }

                MacroAction::ShowTaskbar => {
                    let _ = crate::platform::show_taskbar();
                }

                MacroAction::StartTimerPreset
                | MacroAction::PauseTimerPreset
                | MacroAction::StopTimerPreset => {
                    let t_id = step
                        .timer_preset_id
                        .or_else(|| step.key.trim().parse::<u32>().ok());
                    execute_timer_preset_action(
                        step.action,
                        t_id,
                        step.timer_on_complete_macro_preset_id,
                    );
                }

                MacroAction::LockKeys => {
                    let keys = parse_locked_keys(&step.key);
                    if step.unlock_on_exit {
                        for key in &keys {
                            if !press_locked_keys
                                .iter()
                                .any(|existing| existing.eq_ignore_ascii_case(key))
                            {
                                press_locked_keys.push(key.clone());
                            }
                        }
                    }

                    apply_lock_keys(&keys, None, step.unlock_on_exit);
                }

                MacroAction::UnlockKeys => {
                    let keys = parse_locked_keys(&step.key);
                    apply_unlock_keys(&keys, None);
                    press_locked_keys
                        .retain(|locked| !keys.iter().any(|key| key.eq_ignore_ascii_case(locked)));
                }

                MacroAction::LockMouse => {
                    let mask = mouse_move_lock_mask_from_step(step);
                    apply_lock_mouse(step, None, step.unlock_on_exit);
                    if step.unlock_on_exit {
                        press_locked_mouse_masks.push(mask);
                    }
                }

                MacroAction::UnlockMouse => {
                    let mask = mouse_move_lock_mask_from_step(step);
                    press_locked_mouse_masks.retain(|entry| *entry != mask);
                    apply_unlock_mouse(None, mask);
                }

                MacroAction::EnableMacroPreset => {
                    let _ = set_macro_preset_enabled(&step.key, true);
                }

                MacroAction::DisableMacroPreset => {
                    let _ = set_macro_preset_enabled(&step.key, false);
                }

                MacroAction::EnableStep => {
                    let _ = set_macro_steps_enabled(&step.key, true);
                }

                MacroAction::DisableStep => {
                    let _ = set_macro_steps_enabled(&step.key, false);
                }

                MacroAction::KeyDown => {
                    let _ = send_key_event(step);
                }

                MacroAction::Legacy => {}

                _ => {
                    let _ = send_key_event(step);
                }
            }

            index += 1;
        }

        MacroRunFlow::Continue
    }

    fn execute_hold_macro_sequence(
        preset_id: u32,
        steps: &[MacroStep],
        step_indices: &[usize],
        stop_immediately_on_retrigger: bool,
        run_token: u64,
        target_window_title: Option<&str>,
        extra_target_window_titles: &[String],
        match_duplicate_window_titles: bool,
        bypass_enabled: bool,
    ) -> MacroRunFlow {
        let mut index = 0usize;
        'outer_hold: while index < steps.len() {
            if !bypass_enabled && !is_macro_preset_enabled(preset_id) {
                return MacroRunFlow::StopExecution;
            }

            if !current_hold_run_matches(preset_id, run_token) {
                return MacroRunFlow::StopExecution;
            }

            if !macro_runtime_target_matches(
                target_window_title,
                extra_target_window_titles,
                match_duplicate_window_titles,
            ) {
                return MacroRunFlow::StopExecution;
            }

            if macro_stop_requested(preset_id, stop_immediately_on_retrigger) {
                return MacroRunFlow::StopExecution;
            }

            let step = &steps[index];
            let absolute_index = step_indices[index];
            let is_enabled = is_macro_step_enabled(preset_id, absolute_index, step.enabled);
            let mut run_step = is_enabled;
            if step.toggle_enabled_on_run {
                if let Some(new_enabled) = toggle_macro_step_enabled(preset_id, absolute_index) {
                    run_step = !new_enabled;
                }
            }

            if !run_step {
                index += 1;
                continue;
            }

            let _guard = ActiveStepGuard::new(preset_id, absolute_index);
            if sleep_for_hold_delay(
                preset_id,
                step.get_delay_ms(),
                stop_immediately_on_retrigger,
                run_token,
                target_window_title,
                extra_target_window_titles,
                match_duplicate_window_titles,
                bypass_enabled,
            ) {
                return MacroRunFlow::StopExecution;
            }

            match step.action {
                MacroAction::LoopStart => {
                    let Some(loop_end) = find_matching_loop_end(steps, index) else {
                        index += 1;
                        continue;
                    };
                    let loop_body = &steps[index + 1..loop_end];
                    let loop_body_indices = &step_indices[index + 1..loop_end];
                    let loop_end_delay_ms = steps[loop_end].get_delay_ms();
                    if is_infinite_loop_marker(&step.key) {
                        loop {
                            match execute_hold_macro_sequence(
                                preset_id,
                                loop_body,
                                loop_body_indices,
                                stop_immediately_on_retrigger,
                                run_token,
                                target_window_title,
                                extra_target_window_titles,
                                match_duplicate_window_titles,
                                bypass_enabled,
                            ) {
                                MacroRunFlow::BreakLoop => break,
                                MacroRunFlow::StopExecution => return MacroRunFlow::StopExecution,
                                MacroRunFlow::Continue => {}
                                MacroRunFlow::JumpTo(target) => {
                                    if let Some(pos) = step_indices.iter().position(|&x| x == target) {
                                        index = pos;
                                        continue 'outer_hold;
                                    } else {
                                        return MacroRunFlow::JumpTo(target);
                                    }
                                }
                            }

                            if loop_end_delay_ms > 0
                                && sleep_for_hold_delay(
                                    preset_id,
                                    loop_end_delay_ms,
                                    stop_immediately_on_retrigger,
                                    run_token,
                                    target_window_title,
                                    extra_target_window_titles,
                                    match_duplicate_window_titles,
                                    bypass_enabled,
                                )
                            {
                                return MacroRunFlow::StopExecution;
                            }
                        }
                    } else {
                        let loop_count_str = interpolate_variables(&step.key);
                        let loop_count = loop_count_str.trim().parse::<u32>().unwrap_or(1).max(1);
                        for _ in 0..loop_count {
                            match execute_hold_macro_sequence(
                                preset_id,
                                loop_body,
                                loop_body_indices,
                                stop_immediately_on_retrigger,
                                run_token,
                                target_window_title,
                                extra_target_window_titles,
                                match_duplicate_window_titles,
                                bypass_enabled,
                            ) {
                                MacroRunFlow::BreakLoop => break,
                                MacroRunFlow::StopExecution => return MacroRunFlow::StopExecution,
                                MacroRunFlow::Continue => {}
                                MacroRunFlow::JumpTo(target) => {
                                    if let Some(pos) = step_indices.iter().position(|&x| x == target) {
                                        index = pos;
                                        continue 'outer_hold;
                                    } else {
                                        return MacroRunFlow::JumpTo(target);
                                    }
                                }
                            }

                            if loop_end_delay_ms > 0
                                && sleep_for_hold_delay(
                                    preset_id,
                                    loop_end_delay_ms,
                                    stop_immediately_on_retrigger,
                                    run_token,
                                    target_window_title,
                                    extra_target_window_titles,
                                    match_duplicate_window_titles,
                                    bypass_enabled,
                                )
                            {
                                return MacroRunFlow::StopExecution;
                            }
                        }
                    }

                    index = loop_end + 1;
                    continue;
                }

                MacroAction::LoopEnd => return MacroRunFlow::Continue,
                MacroAction::IfStart => {
                    let (else_index, if_end_index) = find_matching_if_structure(steps, index);
                    let condition_met = evaluate_if_condition(step);
                    if !condition_met {
                        if let Some(else_idx) = else_index {
                            index = else_idx;
                        } else if let Some(end_idx) = if_end_index {
                            index = end_idx;
                        } else {
                            index = steps.len();
                        }
                    }
                }

                MacroAction::Else => {
                    if let Some(end_idx) = find_matching_if_end_from_else(steps, index) {
                        index = end_idx;
                    } else {
                        index = steps.len();
                    }
                }

                MacroAction::IfEnd => {}

                MacroAction::SetVariable => {
                    let target_var = step.if_variable_name.trim().to_string();
                    if !target_var.is_empty() {
                        match step.set_variable_source {
                            crate::model::SetVariableSource::Expression => {
                                smart_set_variable_from_expression(&target_var, &step.key);
                            }
                            _ => {
                                let value = match step.set_variable_source {
                                    crate::model::SetVariableSource::TimeHour => {
                                        use chrono::Timelike;
                                        chrono::Local::now().hour() as i32
                                    }
                                    crate::model::SetVariableSource::TimeMinute => {
                                        use chrono::Timelike;
                                        chrono::Local::now().minute() as i32
                                    }
                                    crate::model::SetVariableSource::TimeSecond => {
                                        use chrono::Timelike;
                                        chrono::Local::now().second() as i32
                                    }
                                    crate::model::SetVariableSource::TimeMillisecond => {
                                        use chrono::Timelike;
                                        chrono::Local::now().nanosecond() as i32 / 1_000_000
                                    }
                                    _ => 0,
                                };
                                set_variable_value(&target_var, value as f64);
                                TEXT_VARIABLES.lock().remove(&target_var);
                            }
                        }
                        send_overlay_command(OverlayCommand::RefreshSearchAreaOverlay);
                    }
                }

                MacroAction::JumpToStep => {
                    let interpolated = interpolate_variables(&step.key);
                    let target_val = if let Ok(val) = interpolated.trim().parse::<f64>() {
                        val
                    } else {
                        evaluate_math_expression_f64(&interpolated)
                    };
                    if target_val.is_nan() || target_val.is_infinite() {
                        return MacroRunFlow::StopExecution;
                    }
                    let target_idx = (target_val.round() as isize) - 1;
                    if target_idx >= 0 {
                        let target_abs = target_idx as usize;
                        if let Some(pos) = step_indices.iter().position(|&x| x == target_abs) {
                            index = pos;
                            continue 'outer_hold;
                        } else {
                            return MacroRunFlow::JumpTo(target_abs);
                        }
                    } else {
                        return MacroRunFlow::StopExecution;
                    }
                }

                MacroAction::StopIfTriggerPressedAgain => {
                    if STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset_id) {
                        return MacroRunFlow::BreakLoop;
                    }
                }

                MacroAction::StopIfKeyPressed => match step.get_break_loop_mode() {
                    "VarCompare" => {
                        if evaluate_if_condition(step) {
                            return MacroRunFlow::BreakLoop;
                        }
                    }

                    "StopKey" => {
                        let keys = parse_stop_keys(&step.key);
                        if keys.iter().any(|key| stop_key_triggered(preset_id, key)) {
                            return MacroRunFlow::BreakLoop;
                        }
                    }

                    _ => {
                        return MacroRunFlow::BreakLoop;
                    }
                },
                MacroAction::ApplyWindowPreset => {
                    let _ = apply_window_preset_by_id(&step.key);
                }

                MacroAction::OcrSearch => {
                    execute_ocr_action_step(step);
                }

                MacroAction::FocusWindowPreset => {
                    let _ = focus_window_by_preset_id(&step.key);
                }

                MacroAction::TriggerMacroPreset => {
                    let mut no_locked_keys = Vec::new();
                    let mut no_locked_mouse: Vec<MouseMoveLockMask> = Vec::new();
                    execute_trigger_macro_step(step, true, &mut no_locked_keys, &mut no_locked_mouse);
                }

                MacroAction::TriggerMacroPresetIfEnabled => {
                    let mut no_locked_keys = Vec::new();
                    let mut no_locked_mouse: Vec<MouseMoveLockMask> = Vec::new();
                    execute_trigger_macro_step(step, false, &mut no_locked_keys, &mut no_locked_mouse);
                }

                MacroAction::StopMacroPreset => {
                    execute_stop_macro_step(step);
                }

                MacroAction::TriggerCommandPreset => {
                    let _ = trigger_command_preset_step(step);
                }

                MacroAction::FunnyMemeReply => {
                    let _ = trigger_funny_meme_reply_step(preset_id, Some(absolute_index), step);
                }

                MacroAction::EnableCrosshairProfile => {
                    let _ = enable_crosshair_profile(&step.key);
                    let duration = step.get_duration_ms();
                    let mut state = HOOK_STATE.lock();
                    if duration > 0 {
                        state.active_crosshair_expires = Some(Instant::now() + Duration::from_millis(duration));
                    } else {
                        state.active_crosshair_expires = None;
                    }
                }

                MacroAction::DisableCrosshair => {
                    if step.lock_mouse_left {
                        disable_crosshair_overlay();
                    } else {
                        disable_crosshair_profile(&step.key);
                    }
                }

                MacroAction::EnablePinPreset => {
                    let _ = enable_pin_preset(&step.key);
                    let duration = step.get_duration_ms();
                    let mut state = HOOK_STATE.lock();
                    if duration > 0 {
                        state.active_pin_expires = Some(Instant::now() + Duration::from_millis(duration));
                    } else {
                        state.active_pin_expires = None;
                    }
                }

                MacroAction::DisablePin => {
                    if step.lock_mouse_left {
                        disable_pin_overlay();
                    } else {
                        disable_pin_preset(&step.key);
                    }
                }

                MacroAction::PlayMousePathPreset => {
                    start_mouse_path_preset_playback(
                        &step.key,
                        step,
                        Some(preset_id),
                        stop_immediately_on_retrigger,
                    );
                }

                MacroAction::ApplyMouseSensitivityPreset => {
                    if step.manual_mouse_sensitivity {
                        let _ = apply_manual_mouse_sensitivity(&step.key);
                    } else {
                        let _ = apply_mouse_sensitivity_preset_by_id(&step.key);
                    }
                }

                MacroAction::EnableZoomPreset => {
                    let _ = enable_zoom_preset(&step.key);
                }

                MacroAction::DisableZoom => {
                    disable_zoom_overlay();
                }

                MacroAction::PlaySoundPreset => {
                    let _ = play_sound_preset(&step.key);
                }

                MacroAction::PlayVideoPreset => {
                    let _ = play_video_preset(&step.key);
                    let duration = step.get_duration_ms();
                    let mut expires_guard = ACTIVE_VIDEO_EXPIRES.lock();
                    if duration > 0 {
                        *expires_guard = Some(Instant::now() + Duration::from_millis(duration));
                    } else {
                        *expires_guard = None;
                    }
                }

                MacroAction::StartVisionSearch => {
                    let _ = start_vision_following(&step.key, Some(&step.if_variable_name));
                }

                MacroAction::StartAudioSensePreset => {
                    start_audio_sense_from_step(
                        step,
                        preset_id,
                        absolute_index,
                        true,
                        true,
                        false,
                    );
                }

                MacroAction::ScanVisionOnce => {
                    if let Some(preset) = vision_preset_by_id(&step.key).ok() {
                        let outcome = match run_vision_once_with_options(
                            &preset,
                            step.vision_move_cursor_on_match,
                            false,
                            Some(&step.if_variable_name),
                            Some(&step.vision_pos_var_x),
                            Some(&step.vision_pos_var_y),
                            Some(&step.vision_found_var),
                        ) {
                            Ok(outcome) => outcome,
                            Err(error) => {
                                eprintln!("ScanVisionOnce failed: {error}");
                                return MacroRunFlow::Continue;
                            }
                        };
                        let ui_tx = HOOK_STATE.lock().ui_tx.clone();
                        if let Some(tx) = ui_tx {
                            let _ = tx.send(UiCommand::VisionFinished(format!(
                                "{}: {}",
                                preset.name, outcome.status
                            )));
                        }
                        send_overlay_command(OverlayCommand::RefreshSearchAreaOverlay);
                    }
                }

                MacroAction::DrawGeometry => {
                    set_step_geometry_spec(preset_id, absolute_index, &step.geometry_spec);
                    let duration = step.get_duration_ms();
                    let mut state = HOOK_STATE.lock();
                    if duration > 0 {
                        state.active_geometry_steps_expires.insert((preset_id, absolute_index), Instant::now() + Duration::from_millis(duration));
                    } else {
                        state.active_geometry_steps_expires.remove(&(preset_id, absolute_index));
                    }
                }


                MacroAction::ShowGeometryPreset => {
                    let owner = (preset_id, absolute_index);
                    if let Some(base_preset) = resolve_geometry_preset_from_step(step) {
                        let duration = step.get_duration_ms();
                        if step.geometry_preset_modify_enabled {
                            let instance = build_geometry_preset_instance_from_step(&base_preset, step);
                            activate_geometry_preset_owner(
                                owner,
                                base_preset.id,
                                Some(instance),
                                duration,
                            );
                        } else {
                            activate_geometry_preset_owner(owner, base_preset.id, None, duration);
                        }
                    }
                }

                MacroAction::HideGeometryPreset => {
                    if let Some(geometry_preset_id) = resolve_geometry_preset_id_from_step(step) {
                        hide_geometry_preset_by_id(
                            geometry_preset_id,
                            step.geometry_hide_mode,
                        );
                    } else {
                        clear_geometry_overlay();
                    }
                }

                MacroAction::StopVisionWait => {
                    let _ = stop_vision_waiting(&step.key);
                }

                MacroAction::StopVision => {
                    let _ = stop_vision_following(&step.key);
                }

                MacroAction::StopAudioSense => {
                    stop_audio_sense_from_step(step, preset_id, absolute_index, true);
                }

                MacroAction::ShowHud => {
                    trigger_hud_display(preset_id, step);
                }

                MacroAction::HideHud => {
                    hide_hud_now();
                }

                MacroAction::HideTaskbar => {
                    let _ = crate::platform::hide_taskbar();
                }

                MacroAction::ShowTaskbar => {
                    let _ = crate::platform::show_taskbar();
                }

                MacroAction::LockKeys => {
                    apply_lock_keys(
                        &parse_locked_keys(&step.key),
                        Some(preset_id),
                        step.unlock_on_exit,
                    );
                }

                MacroAction::UnlockKeys => {
                    apply_unlock_keys(&parse_locked_keys(&step.key), Some(preset_id));
                }

                MacroAction::LockMouse => {
                    apply_lock_mouse(step, Some(preset_id), step.unlock_on_exit);
                }

                MacroAction::UnlockMouse => {
                    apply_unlock_mouse(Some(preset_id), mouse_move_lock_mask_from_step(step));
                }

                MacroAction::EnableMacroPreset => {
                    let _ = set_macro_preset_enabled(&step.key, true);
                }

                MacroAction::DisableMacroPreset => {
                    let _ = set_macro_preset_enabled(&step.key, false);
                }

                MacroAction::EnableStep => {
                    let _ = set_macro_steps_enabled(&step.key, true);
                }

                MacroAction::DisableStep => {
                    let _ = set_macro_steps_enabled(&step.key, false);
                }

                MacroAction::KeyDown => {
                    let _ = send_key_event(step);
                }

                MacroAction::Legacy => {}

                _ => {
                    let _ = send_key_event(step);
                }
            }

            index += 1;
        }

        MacroRunFlow::Continue
    }

    fn sleep_for_hold_delay(
        preset_id: u32,
        delay_ms: u64,
        stop_immediately_on_retrigger: bool,
        run_token: u64,
        target_window_title: Option<&str>,
        extra_target_window_titles: &[String],
        match_duplicate_window_titles: bool,
        bypass_enabled: bool,
    ) -> bool {
        if delay_ms == 0 {
            return !macro_runtime_target_matches(
                target_window_title,
                extra_target_window_titles,
                match_duplicate_window_titles,
            ) || (!bypass_enabled && !is_macro_preset_enabled(preset_id))
                || !current_hold_run_matches(preset_id, run_token)
                || macro_stop_requested(preset_id, stop_immediately_on_retrigger);
        }

        let mut remaining_ms = delay_ms;
        while remaining_ms > 0 {
            {
                let hook_state = HOOK_STATE.lock();
                if !hook_state.macros_master_enabled {
                    return true;
                }

                if !bypass_enabled && !is_macro_preset_enabled_with_guard(preset_id, &hook_state) {
                    return true;
                }

                if !current_hold_run_matches_with_guard(preset_id, run_token, &hook_state) {
                    return true;
                }

                if !macro_runtime_target_matches_with_guard(
                    target_window_title,
                    extra_target_window_titles,
                    match_duplicate_window_titles,
                    &hook_state,
                ) {
                    return true;
                }
            }

            if macro_stop_requested(preset_id, stop_immediately_on_retrigger) {
                return true;
            }

            let chunk_ms = remaining_ms.min(10);
            thread::sleep(std::time::Duration::from_millis(chunk_ms));
            remaining_ms = remaining_ms.saturating_sub(chunk_ms);
        }

        !macro_runtime_target_matches(
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
        ) || (!bypass_enabled && !is_macro_preset_enabled(preset_id))
            || !current_hold_run_matches(preset_id, run_token)
            || macro_stop_requested(preset_id, stop_immediately_on_retrigger)
    }

    fn sleep_for_macro_delay(
        preset_id: u32,
        delay_ms: u64,
        stop_immediately_on_retrigger: bool,
        target_window_title: Option<&str>,
        extra_target_window_titles: &[String],
        match_duplicate_window_titles: bool,
        bypass_enabled: bool,
    ) -> bool {
        if delay_ms == 0 {
            return !macro_runtime_target_matches(
                target_window_title,
                extra_target_window_titles,
                match_duplicate_window_titles,
            ) || (!bypass_enabled && !is_macro_preset_enabled(preset_id))
                || macro_stop_requested(preset_id, stop_immediately_on_retrigger);
        }

        let mut remaining_ms = delay_ms;
        while remaining_ms > 0 {
            {
                let hook_state = HOOK_STATE.lock();
                if !hook_state.macros_master_enabled {
                    return true;
                }

                if !bypass_enabled && !is_macro_preset_enabled_with_guard(preset_id, &hook_state) {
                    return true;
                }

                if !macro_runtime_target_matches_with_guard(
                    target_window_title,
                    extra_target_window_titles,
                    match_duplicate_window_titles,
                    &hook_state,
                ) {
                    return true;
                }
            }

            if macro_stop_requested(preset_id, stop_immediately_on_retrigger) {
                return true;
            }

            let chunk_ms = remaining_ms.min(10);
            thread::sleep(std::time::Duration::from_millis(chunk_ms));
            remaining_ms = remaining_ms.saturating_sub(chunk_ms);
        }

        !macro_runtime_target_matches(
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
        ) || (!bypass_enabled && !is_macro_preset_enabled(preset_id))
            || macro_stop_requested(preset_id, stop_immediately_on_retrigger)
    }

    fn find_matching_loop_end(steps: &[MacroStep], start_index: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (index, step) in steps.iter().enumerate().skip(start_index) {
            match step.action {
                MacroAction::LoopStart => depth += 1,
                MacroAction::LoopEnd => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(index);
                    }
                }

                _ => {}
            }
        }

        None
    }

    fn find_matching_if_structure(
        steps: &[MacroStep],
        start_index: usize,
    ) -> (Option<usize>, Option<usize>) {
        let mut depth = 0usize;
        let mut else_index = None;
        for i in start_index + 1..steps.len() {
            match steps[i].action {
                MacroAction::IfStart => depth += 1,
                MacroAction::IfEnd => {
                    if depth == 0 {
                        return (else_index, Some(i));
                    } else {
                        depth -= 1;
                    }
                }

                MacroAction::Else => {
                    if depth == 0 {
                        else_index = Some(i);
                    }
                }

                _ => {}
            }
        }

        (else_index, None)
    }

    fn find_matching_if_end_from_else(steps: &[MacroStep], else_index: usize) -> Option<usize> {
        let mut depth = 0usize;
        for i in else_index + 1..steps.len() {
            match steps[i].action {
                MacroAction::IfStart => depth += 1,
                MacroAction::IfEnd => {
                    if depth == 0 {
                        return Some(i);
                    } else {
                        depth -= 1;
                    }
                }

                _ => {}
            }
        }

        None
    }

    fn evaluate_single_condition(
        condition_type: IfConditionType,
        variable_name: &str,
        operator: &str,
        compare_value: i32,
        expression: &str,
        ocr_preset_id: Option<u32>,
        ocr_target_text: &str,
        if_contain_case_sensitive: bool,
        if_contain_isolated: bool,
        key: &str,
        x: i32,
        y: i32,
        target_color: &str,
        tolerance: u8,
        mouse_axis: &str,
        running_preset_id: Option<u32>,
        vision_preset_id: Option<u32>,
    ) -> bool {
        match condition_type {
            IfConditionType::OcrMatch => {
                let preset_id = ocr_preset_id.unwrap_or(0);
                let (x, y, w, h, lang) = {
                    let hook_state = HOOK_STATE.lock();
                    if let Some(preset) = hook_state.ocr_presets.iter().find(|p| p.id == preset_id)
                    {
                        (
                            preset.x,
                            preset.y,
                            preset.width,
                            preset.height,
                            preset.lang.clone(),
                        )
                    } else {
                        return false;
                    }
                };
                let w = w.max(10);
                let h = h.max(10);
                let lang_str = lang.as_deref().unwrap_or("en");
                if let Some(frame) = window_list::capture_virtual_screen_region(x, y, w, h) {
                    if let Ok(res) = crate::ocr::perform_ocr(
                        &frame.rgba,
                        frame.width as u32,
                        frame.height as u32,
                        lang_str,
                    ) {
                        let target_text = ocr_target_text.trim();
                        if target_text.is_empty() {
                            return !res.text.trim().is_empty();
                        }

                        for word in &res.words {
                            if word
                                .text
                                .to_lowercase()
                                .contains(&target_text.to_lowercase())
                            {
                                return true;
                            }
                        }
                    }
                }

                false
            }

            IfConditionType::Variable => {
                let op = operator.trim().to_lowercase();
                let evaluate_contain =
                    |left: &str, right: &str, case_sensitive: bool, isolated: bool| -> bool {
                        let (l, r) = if case_sensitive {
                            (left.to_string(), right.to_string())
                        } else {
                            (left.to_lowercase(), right.to_lowercase())
                        };
                        if r.is_empty() {
                            return true;
                        }

                        if isolated {
                            let mut start = 0;
                            while let Some(pos) = l[start..].find(&r) {
                                let absolute_pos = start + pos;
                                let before_char_ok = if absolute_pos == 0 {
                                    true
                                } else {
                                    let prev_char = l.chars().nth(absolute_pos - 1).unwrap_or(' ');
                                    !prev_char.is_alphanumeric()
                                };
                                let after_char_ok = if absolute_pos + r.len() >= l.len() {
                                    true
                                } else {
                                    let next_char =
                                        l.chars().nth(absolute_pos + r.len()).unwrap_or(' ');
                                    !next_char.is_alphanumeric()
                                };
                                if before_char_ok && after_char_ok {
                                    return true;
                                }

                                start = absolute_pos + 1;
                            }

                            false
                        } else {
                            l.contains(&r)
                        }
                    };
                if op == "contain" || op == "contains" {
                    let left_str = {
                        let vars = RUNTIME_VARIABLES.lock();
                        let trimmed = variable_name.trim();
                        if let Some(val) = vars.get(trimmed) {
                            val.to_string()
                        } else {
                            interpolate_variables(trimmed)
                        }
                    };
                    let right_expr = if expression.is_empty() && !key.is_empty() {
                        key
                    } else {
                        expression
                    };
                    let right_str = interpolate_variables(right_expr.trim());
                    evaluate_contain(
                        &left_str,
                        &right_str,
                        if_contain_case_sensitive,
                        if_contain_isolated,
                    )
                } else {
                    let is_math_expression_or_numeric = |s: &str| -> bool {
                        let s_trimmed = s.trim();
                        if s_trimmed.is_empty() {
                            return false;
                        }
                        if s_trimmed.parse::<f64>().is_ok() {
                            return true;
                        }
                        let allowed_chars = |c: char| {
                            c.is_ascii_digit()
                                || c == '.'
                                || c == '+'
                                || c == '-'
                                || c == '*'
                                || c == '/'
                                || c == '('
                                || c == ')'
                                || c == ','
                                || c.is_whitespace()
                        };
                        if s_trimmed.chars().all(allowed_chars) {
                            return true;
                        }
                        let s_lower = s_trimmed.to_lowercase();
                        let math_funcs = [
                            "choice(", "random(", "min(", "max(", "abs(", "atan(", "atan2(", "sin(", "cos(", "tan(", "sqrt(",
                            "ln(", "log(", "asin(", "acos(", "sinh(", "cosh(", "tanh(", "ceil(", "floor(", "round(",
                            "pow(", "degrees(", "radians(", "gcd(", "lcm(", "isqrt(", "comb(", "perm(", "factorial(",
                        ];
                        for func in &math_funcs {
                            if s_lower.contains(func) {
                                return true;
                            }
                        }
                        false
                    };

                    let left_str = {
                        let trimmed = variable_name.trim();
                        if let Some(val) = RUNTIME_VARIABLES.lock().get(trimmed) {
                            val.to_string()
                        } else if let Some(val) = resolve_text_variable_value(trimmed) {
                            val
                        } else {
                            interpolate_variables(trimmed)
                        }
                    };
                    let right_expr = if expression.is_empty() && !key.is_empty() {
                        key
                    } else {
                        expression
                    };
                    let right_str = interpolate_variables(right_expr.trim());

                    if is_math_expression_or_numeric(&left_str) && is_math_expression_or_numeric(&right_str) {
                        let evaluate_operand = |expr: &str, fallback: f64| -> f64 {
                            if expr.trim().is_empty() {
                                fallback
                            } else {
                                let interpolated = interpolate_variables(expr);
                                evaluate_math_expression_f64(&interpolated)
                            }
                        };
                        let compare_values = |value: f64, operator: &str, comp: f64| match operator {
                            ">" => value > comp,
                            "<" => value < comp,
                            "=" | "==" => (value - comp).abs() < 1e-9,
                            ">=" => value >= comp,
                            "<=" => value <= comp,
                            "!=" => (value - comp).abs() >= 1e-9,
                            _ => false,
                        };
                        let cond_left = evaluate_operand(variable_name, compare_value as f64);
                        let cond_right = evaluate_operand(right_expr, compare_value as f64);
                        compare_values(cond_left, operator, cond_right)
                    } else {
                        match operator.trim() {
                            ">" => left_str > right_str,
                            "<" => left_str < right_str,
                            "=" | "==" => left_str == right_str,
                            ">=" => left_str >= right_str,
                            "<=" => left_str <= right_str,
                            "!=" => left_str != right_str,
                            _ => false,
                        }
                    }
                }
            }

            IfConditionType::PixelColor => {
                let parse_color = |s: &str| -> Option<(u8, u8, u8)> {
                    let parts: Vec<&str> = s.split(',').collect();
                    if parts.len() >= 3 {
                        let r = parts[0].trim().parse::<u8>().ok()?;
                        let g = parts[1].trim().parse::<u8>().ok()?;
                        let b = parts[2].trim().parse::<u8>().ok()?;
                        Some((r, g, b))
                    } else {
                        None
                    }
                };
                if let Some((tr, tg, tb)) = parse_color(target_color) {
                    if let Some(frame) = window_list::capture_virtual_screen_region(x, y, 1, 1) {
                        if frame.rgba.len() >= 4 {
                            let r = frame.rgba[0];
                            let g = frame.rgba[1];
                            let b = frame.rgba[2];
                            let diff_r = (r as i32 - tr as i32).abs();
                            let diff_g = (g as i32 - tg as i32).abs();
                            let diff_b = (b as i32 - tb as i32).abs();
                            return diff_r <= tolerance as i32
                                && diff_g <= tolerance as i32
                                && diff_b <= tolerance as i32;
                        }
                    }
                }

                false
            }

            IfConditionType::PresetRunning => {
                if let Some(pid) = running_preset_id {
                    let active = ACTIVE_MACRO_STEPS.lock();
                    if pid == 0 {
                        !active.is_empty()
                    } else {
                        active.contains_key(&pid)
                    }
                } else {
                    false
                }
            }

            IfConditionType::MousePosition => {
                #[cfg(windows)]
                {
                    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
                    let mut pt = windows::Win32::Foundation::POINT::default();
                    if unsafe { GetCursorPos(&mut pt) }.is_ok() {
                        let val = if mouse_axis.eq_ignore_ascii_case("Y") {
                            pt.y
                        } else {
                            pt.x
                        };
                        let evaluate_operand = |expr: &str, fallback: i32| -> i32 {
                            if expr.trim().is_empty() {
                                fallback
                            } else {
                                evaluate_interpolated_math_expression(expr)
                            }
                        };
                        let compare_values = |value: i32, operator: &str, comp: i32| match operator
                        {
                            ">" => value > comp,
                            "<" => value < comp,
                            "=" | "==" => value == comp,
                            ">=" => value >= comp,
                            "<=" => value <= comp,
                            "!=" => value != comp,
                            _ => false,
                        };
                        let right_expr = if expression.is_empty() && !key.is_empty() {
                            key
                        } else {
                            expression
                        };
                        let right_val = evaluate_operand(right_expr, compare_value);
                        return compare_values(val, operator, right_val);
                    }
                }

                false
            }

            IfConditionType::VisionMatch => {
                if let Some(pid) = vision_preset_id {
                    let preset = {
                        let hook_state = HOOK_STATE.lock();
                        hook_state
                            .vision_presets
                            .iter()
                            .find(|p| p.id == pid)
                            .cloned()
                    };
                    if let Some(preset) = preset {
                        if let Ok(outcome) =
                            run_vision_once_with_options(&preset, false, false, None, None, None, None)
                        {
                            return outcome.matched;
                        }
                    }
                }

                false
            }

            IfConditionType::KeyHeld => {
                let parts: Vec<&str> = key
                    .split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .collect();
                if parts.is_empty() {
                    return false;
                }

                #[cfg(windows)]
                {
                    for part in parts {
                        let is_down = if let Some(vk) = crate::hotkey::key_name_to_vk(part) {
                            (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) != 0
                        } else {
                            false
                        };
                        if !is_down {
                            return false;
                        }
                    }

                    true
                }

                #[cfg(not(windows))]
                {
                    false
                }
            }

            IfConditionType::MouseHeld => {
                let vk = match key.to_ascii_uppercase().as_str() {
                    "MOUSELEFT" | "LEFT" | "LBUTTON" | "MOUSE LEFT" => Some(0x01),
                    "MOUSERIGHT" | "RIGHT" | "RBUTTON" | "MOUSE RIGHT" => Some(0x02),
                    "MOUSEMIDDLE" | "MIDDLE" | "MBUTTON" | "MOUSE MIDDLE" => Some(0x04),
                    "MOUSEX1" | "X1" | "XBUTTON1" | "MOUSE X1" => Some(0x05),
                    "MOUSEX2" | "X2" | "XBUTTON2" | "MOUSE X2" => Some(0x06),
                    _ => None,
                };
                if let Some(vk_code) = vk {
                    #[cfg(windows)]
                    {
                        return (unsafe { GetAsyncKeyState(vk_code as i32) } as u16 & 0x8000) != 0;
                    }
                }

                false
            }

            _ => false,
        }
    }

    fn evaluate_if_condition(step: &MacroStep) -> bool {
        let mut result = evaluate_single_condition(
            step.if_condition_type,
            &step.if_variable_name,
            &step.if_operator,
            step.if_compare_value,
            "",
            step.if_ocr_preset_id,
            &step.ocr_target_text,
            step.if_contain_case_sensitive,
            step.if_contain_isolated,
            &step.key,
            step.x,
            step.y,
            &step.if_target_color,
            step.if_color_tolerance,
            &step.if_mouse_axis,
            step.if_running_preset_id,
            step.if_vision_preset_id,
        );
        for cond in &step.extra_conditions {
            let cond_ok = evaluate_single_condition(
                cond.condition_type,
                &cond.variable_name,
                &cond.operator,
                cond.compare_value,
                &cond.expression,
                cond.ocr_preset_id,
                &cond.ocr_target_text,
                cond.if_contain_case_sensitive,
                cond.if_contain_isolated,
                if cond.condition_type == IfConditionType::KeyHeld {
                    &cond.key_held_name
                } else if cond.condition_type == IfConditionType::MouseHeld {
                    &cond.mouse_button
                } else {
                    ""
                },
                cond.x,
                cond.y,
                &cond.target_color,
                cond.color_tolerance,
                &cond.mouse_axis,
                cond.running_preset_id,
                cond.vision_preset_id,
            );
            let join_operator = cond.join_operator.trim().to_ascii_uppercase();
            result = match join_operator.as_str() {
                "OR" => result || cond_ok,
                _ => result && cond_ok,
            };
        }

        result
    }












    fn normalize_ocr_match_text(text: &str) -> String {
        text.split_whitespace()
            .map(|part| part.trim_matches(|ch: char| !ch.is_alphanumeric()))
            .filter(|part| !part.is_empty())
            .map(|part| part.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn find_ocr_target_bounds(
        words: &[crate::ocr::OcrWord],
        target_text: &str,
    ) -> Option<(f32, f32, f32, f32)> {
        let normalized_target = normalize_ocr_match_text(target_text);
        if normalized_target.is_empty() {
            return None;
        }

        let is_multi_word_target = normalized_target.contains(' ');
        for start in 0..words.len() {
            let mut candidate = String::new();
            let mut left = f32::MAX;
            let mut top = f32::MAX;
            let mut right = f32::MIN;
            let mut bottom = f32::MIN;
            for word in &words[start..] {
                let normalized_word = normalize_ocr_match_text(&word.text);
                if normalized_word.is_empty() {
                    continue;
                }

                if !candidate.is_empty() {
                    candidate.push(' ');
                }
                candidate.push_str(&normalized_word);
                left = left.min(word.x);
                top = top.min(word.y);
                right = right.max(word.x + word.width);
                bottom = bottom.max(word.y + word.height);
                let matched = if is_multi_word_target {
                    candidate == normalized_target || candidate.contains(&normalized_target)
                } else {
                    normalized_word.contains(&normalized_target)
                };
                if matched {
                    return Some((left, top, right, bottom));
                }

                if is_multi_word_target {
                    if candidate.len() >= normalized_target.len()
                        && !normalized_target.starts_with(&candidate)
                        && !candidate.contains(&normalized_target)
                    {
                        break;
                    }
                } else if candidate.len() > normalized_target.len() + 24 {
                    break;
                }
            }
        }

        None
    }

    fn execute_ocr_action_step(step: &crate::model::MacroStep) {
        let preset_id = step.key.trim().parse::<u32>().ok().unwrap_or(0);
        let (x, y, w, h, lang) = {
            let hook_state = HOOK_STATE.lock();
            if let Some(preset) = hook_state.ocr_presets.iter().find(|p| p.id == preset_id) {
                (
                    preset.x,
                    preset.y,
                    preset.width,
                    preset.height,
                    preset.lang.clone(),
                )
            } else {
                (
                    step.x,
                    step.y,
                    step.ocr_width,
                    step.ocr_height,
                    step.ocr_lang.clone(),
                )
            }
        };
        let w = w.max(10);
        let h = h.max(10);
        let lang_str = lang.as_deref().unwrap_or("en");
        let mut success = 0;
        if let Some(frame) = window_list::capture_virtual_screen_region(x, y, w, h) {
            if let Ok(res) = crate::ocr::perform_ocr(
                &frame.rgba,
                frame.width as u32,
                frame.height as u32,
                lang_str,
            ) {
                let full_text = res.text.clone();
                // 0. Store full raw text regardless of target_text

                let text_var = step.ocr_text_var.trim();
                if !text_var.is_empty() {
                    set_text_variable_value(text_var, &full_text);
                }

                // 1. Parse number if ocr_numeric_var is set

                let numeric_var = step.ocr_numeric_var.trim();
                if !numeric_var.is_empty() {
                    let mut number_str = String::new();
                    let mut has_dot = false;
                    for c in full_text.chars() {
                        if c.is_ascii_digit() {
                            number_str.push(c);
                        } else if c == '.' && !has_dot {
                            number_str.push(c);
                            has_dot = true;
                        } else if !number_str.is_empty() {
                            break;
                        }
                    }

                    if !number_str.is_empty() {
                        if let Ok(val) = number_str.parse::<f64>() {
                            set_variable_value(numeric_var, val);
                        }
                    }
                }

                // 2. Search for target_text if ocr_target_text is set

                let target_text = step.ocr_target_text.trim();
                if !target_text.is_empty() {
                    if let Some((left, top, right, bottom)) =
                        find_ocr_target_bounds(&res.words, target_text)
                    {
                        success = 1;
                        // Absolute position of the center of the matched text on screen

                        let abs_x = x + ((left + right) / 2.0).round() as i32;
                        let abs_y = y + ((top + bottom) / 2.0).round() as i32;
                        let pos_x_var = step.ocr_pos_var_x.trim();
                        let pos_y_var = step.ocr_pos_var_y.trim();
                        if !pos_x_var.is_empty() {
                            set_variable_value(pos_x_var, abs_x as f64);
                        }

                        if !pos_y_var.is_empty() {
                            set_variable_value(pos_y_var, abs_y as f64);
                        }
                    }
                } else {
                    // If target text is empty, count it as success if we captured successfully and got text

                    success = 1;
                }
            }
        }

        let success_var = step.ocr_success_var.trim();
        if !success_var.is_empty() {
            set_variable_value(success_var, success as f64);
        }
    }









    #[cfg(test)]
    mod tests {
        use super::*;
        static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

        #[test]
        fn test_evaluate_math_expression() {
            let _guard = TEST_MUTEX.lock().unwrap();
            assert_eq!(evaluate_math_expression(""), 0);
            assert_eq!(evaluate_math_expression("   "), 0);
            assert_eq!(evaluate_math_expression("42"), 42);
            assert_eq!(evaluate_math_expression("1 + 2"), 3);
            assert_eq!(evaluate_math_expression("10 - 4"), 6);
            assert_eq!(evaluate_math_expression("3 * 4"), 12);
            assert_eq!(evaluate_math_expression("12 / 3"), 4);
            assert_eq!(evaluate_math_expression("2 * 3 + 4"), 10);
            assert_eq!(evaluate_math_expression("2 + 3 * 4"), 14);
            assert_eq!(evaluate_math_expression("10 / 2 - 1"), 4);
            assert_eq!(evaluate_math_expression("10 - 4 / 2"), 8);
            // Division by zero protection

            assert_eq!(evaluate_math_expression("5 / 0"), 5);
            // Saturating bounds

            assert_eq!(evaluate_math_expression("2147483647 + 1"), 2147483647);
            // Parentheses support

            assert_eq!(evaluate_math_expression("10 - (10 + 10)"), -10);
            assert_eq!(evaluate_math_expression("(2 + 3) * 4"), 20);
            assert_eq!(evaluate_math_expression("10 - (4 / 2)"), 8);
            assert_eq!(evaluate_math_expression("((2 + 3) * 2) - 5"), 5);
            // Unary minus / negative numbers

            assert_eq!(evaluate_math_expression("10 - -20"), 30);
            assert_eq!(evaluate_math_expression("-5 + 10"), 5);
            assert_eq!(evaluate_math_expression("-5 * -2"), 10);
            // Functions support (min, max, abs, random)

            assert_eq!(evaluate_math_expression("abs(-50)"), 50);
            assert_eq!(evaluate_math_expression("degrees(atan(1))"), 45);
            assert_eq!(evaluate_math_expression("degrees(atan2(1, 1))"), 45);
            assert_eq!(evaluate_math_expression("sin(radians(30)) * 1000"), 500);
            assert_eq!(evaluate_math_expression("cos(radians(60)) * 1000"), 500);
            assert_eq!(evaluate_math_expression("cos(0) * 1000"), 1000);
            assert_eq!(evaluate_math_expression("sqrt(9)"), 3);
            assert_eq!(evaluate_math_expression("pow(2, 3)"), 8);
            assert_eq!(evaluate_math_expression("round(863.6897460727389)"), 864);
            assert!(
                (evaluate_math_expression_f64("round(863.6897460727389, 2)") - 863.69).abs()
                    < 0.000001
            );
            assert!(
                (evaluate_math_expression_f64("round(863.6897460727389, 1)") - 863.7).abs()
                    < 0.000001
            );
            assert_eq!(evaluate_math_expression("ceil(pi)"), 4);
            assert_eq!(evaluate_math_expression("floor(pi)"), 3);
            assert_eq!(evaluate_math_expression("degrees(pi)"), 180);
            assert_eq!(evaluate_math_expression("radians(180)"), 3);
            assert_eq!(evaluate_math_expression("factorial(5)"), 120);
            assert_eq!(evaluate_math_expression("gcd(24, 36, 48)"), 12);
            assert_eq!(evaluate_math_expression("lcm(4, 6, 8)"), 24);
            assert_eq!(evaluate_math_expression("isqrt(17)"), 4);
            assert_eq!(evaluate_math_expression("comb(5, 2)"), 10);
            assert_eq!(evaluate_math_expression("perm(5, 2)"), 20);
            assert_eq!(evaluate_math_expression("pi + 1"), 4);
            assert_eq!(evaluate_math_expression("min(20, 50)"), 20);
            assert_eq!(evaluate_math_expression("max(20, 50)"), 50);
            assert_eq!(evaluate_math_expression("min(max(-10, 0), 100)"), 0);
            let rnd = evaluate_math_expression("random(10, 20)");
            assert!(rnd >= 10 && rnd <= 20);
            let val = evaluate_math_expression("choice(10, 20, 30)");
            assert!(val == 10 || val == 20 || val == 30);
            // Variable resolution

            {
                let mut vars = RUNTIME_VARIABLES.lock();
                vars.insert("x".to_string(), 10.0);
                vars.insert("player_mana".to_string(), 100.0);
            }

            assert_eq!(evaluate_math_expression("x + 1"), 11);
            assert_eq!(evaluate_math_expression("player_mana - 10"), 90);
            assert_eq!(evaluate_math_expression("player_mana * x / 5"), 200);
            // Clean up

            {
                let mut vars = RUNTIME_VARIABLES.lock();
                vars.clear();
            }
        }

        #[test]
        fn test_connected_color_match_requires_adjacent_colors() {
            let red = RgbaColor { r: 255, g: 0, b: 0, a: 255 };
            let blue = RgbaColor { r: 0, g: 0, b: 255, a: 255 };
            let adjacent_screen = window_list::ScreenCaptureFrame {
                screen_x: 0,
                screen_y: 0,
                width: 2,
                height: 1,
                rgba: vec![
                    255, 0, 0, 255,
                    0, 0, 255, 255,
                ],
            };
            let separated_screen = window_list::ScreenCaptureFrame {
                screen_x: 0,
                screen_y: 0,
                width: 3,
                height: 1,
                rgba: vec![
                    255, 0, 0, 255,
                    0, 0, 0, 255,
                    0, 0, 255, 255,
                ],
            };
            let adjacent_hit = find_connected_color_match(
                &adjacent_screen,
                &[red, blue],
                0,
                None,
                None,
            );
            assert!(adjacent_hit.is_some());
            let separated_hit = find_connected_color_match(
                &separated_screen,
                &[red, blue],
                0,
                None,
                None,
            );
            assert!(separated_hit.is_none());
        }

        #[test]
        fn test_interpolate_variables() {
            let _guard = TEST_MUTEX.lock().unwrap();
            // Variable resolution in interpolate_variables

            {
                let mut vars = RUNTIME_VARIABLES.lock();
                vars.insert("A".to_string(), 520.0);
                vars.insert("B".to_string(), 10.0);
            }

            assert_eq!(interpolate_variables("test {A}"), "test 520");
            assert_eq!(interpolate_variables("test {A+A}"), "test 1040");
            assert_eq!(interpolate_variables("test {A + B * 2}"), "test 540");
            assert_eq!(interpolate_variables("test {C}"), "test 0");
        }

        #[test]
        fn test_choice_expression_supports_text_and_numeric_values() {
            let _guard = TEST_MUTEX.lock().unwrap();

            for _ in 0..20 {
                let chosen = resolve_choice_expression_value("choice(hi, hello, bye)").unwrap();
                assert!(matches!(chosen.as_str(), "hi" | "hello" | "bye"));
            }

            for _ in 0..20 {
                let chosen = resolve_choice_expression_value("choice(123, hello123, 456)").unwrap();
                assert!(matches!(chosen.as_str(), "123" | "hello123" | "456"));
            }

            for _ in 0..20 {
                let chosen = resolve_choice_expression_value("choice(random(1, 3), 9)").unwrap();
                let parsed = chosen.parse::<i32>().unwrap();
                assert!((1..=3).contains(&parsed) || parsed == 9);
            }
        }

        #[test]
        fn test_set_variable_round_expression_without_braces() {
            let _guard = TEST_MUTEX.lock().unwrap();
            RUNTIME_VARIABLES.lock().clear();
            TEXT_VARIABLES.lock().clear();

            smart_set_variable_from_expression("pretty", "round(863.6897460727389, 2)");

            let val = RUNTIME_VARIABLES.lock()
                .get("pretty")
                .copied()
                .unwrap_or_default();
            assert!((val - 863.69).abs() < 0.000001);
            assert_eq!(TEXT_VARIABLES.lock().get("pretty"), None);

            RUNTIME_VARIABLES.lock().clear();
        }

        #[test]
        fn test_evaluate_interpolated_math_expression() {
            let _guard = TEST_MUTEX.lock().unwrap();
            {
                let mut vars = RUNTIME_VARIABLES.lock();
                vars.insert("x".to_string(), 1660.0);
                vars.insert("x1".to_string(), 1555.0);
                vars.insert("y".to_string(), 555.0);
                vars.insert("y1".to_string(), 520.0);
            }

            assert_eq!(evaluate_interpolated_math_expression("{x-x1}"), 105);
            assert_eq!(evaluate_interpolated_math_expression("{y-y1}"), 35);
            assert_eq!(evaluate_interpolated_math_expression("{x-x1} + {y-y1}"), 140);
            {
                let mut vars = RUNTIME_VARIABLES.lock();
                vars.clear();
            }
        }

        #[test]
        fn test_tonumber_property() {
            let _guard = TEST_MUTEX.lock().unwrap();
            {
                let mut text_vars = TEXT_VARIABLES.lock();
                text_vars.insert("A".to_string(), "hello123".to_string());
                text_vars.insert("B".to_string(), "hel123lo45".to_string());
            }

            assert_eq!(evaluate_math_expression("A.toNumber"), 123);
            assert_eq!(evaluate_math_expression("B.toNumber + 5"), 12350);
            {
                let text_vars = TEXT_VARIABLES.lock();
                assert!(!text_vars.contains_key("A"));
                assert!(!text_vars.contains_key("B"));
            }

            {
                let vars = RUNTIME_VARIABLES.lock();
                assert_eq!(*vars.get("A").unwrap_or(&0.0), 123.0);
                assert_eq!(*vars.get("B").unwrap_or(&0.0), 12345.0);
            }

            {
                let mut text_vars = TEXT_VARIABLES.lock();
                text_vars.clear();
                let mut vars = RUNTIME_VARIABLES.lock();
                vars.clear();
            }

            // Clean up

            {
                let mut vars = RUNTIME_VARIABLES.lock();
                vars.clear();
            }
        }

        #[test]
        fn test_find_ocr_target_bounds_matches_phrase_across_words() {
            let words = vec![
                crate::ocr::OcrWord {
                    text: "better".to_string(),
                    x: 10.0,
                    y: 20.0,
                    width: 40.0,
                    height: 12.0,
                },
                crate::ocr::OcrWord {
                    text: "prompt".to_string(),
                    x: 56.0,
                    y: 20.0,
                    width: 46.0,
                    height: 12.0,
                },
            ];
            let bounds = find_ocr_target_bounds(&words, "better prompt");
            assert_eq!(bounds, Some((10.0, 20.0, 102.0, 32.0)));
        }

        #[test]
        fn test_find_ocr_target_bounds_trims_punctuation() {
            let words = vec![crate::ocr::OcrWord {
                text: "prompt,".to_string(),
                x: 100.0,
                y: 200.0,
                width: 50.0,
                height: 20.0,
            }];
            let bounds = find_ocr_target_bounds(&words, "prompt");
            assert_eq!(bounds, Some((100.0, 200.0, 150.0, 220.0)));
        }

        #[test]
        fn test_numeric_variable_overrides_stale_text_variable() {
            let _guard = TEST_MUTEX.lock().unwrap();
            set_text_variable_value("u", "better prompt");
            set_variable_value("u", 1.0);
            {
                let text_vars = TEXT_VARIABLES.lock();
                assert!(!text_vars.contains_key("u"));
            }

            assert_eq!(resolve_text_variable_value("u"), Some("1".to_string()));
            {
                let mut vars = RUNTIME_VARIABLES.lock();
                vars.clear();
            }
        }

        #[test]
        fn test_text_variable_overrides_stale_numeric_variable() {
            let _guard = TEST_MUTEX.lock().unwrap();
            set_variable_value("u", 1.0);
            set_text_variable_value("u", "better prompt");
            {
                let vars = RUNTIME_VARIABLES.lock();
                assert!(!vars.contains_key("u"));
            }

            assert_eq!(
                resolve_text_variable_value("u"),
                Some("better prompt".to_string())
            );
            {
                let mut text_vars = TEXT_VARIABLES.lock();
                text_vars.clear();
            }
        }

        #[test]
        fn test_jump_to_step() {
            let _guard = TEST_MUTEX.lock().unwrap();
            let steps = vec![
                MacroStep {
                    action: MacroAction::SetVariable,
                    if_variable_name: "a".to_string(),
                    set_variable_source: crate::model::SetVariableSource::Expression,
                    key: "1".to_string(),
                    ..Default::default()
                },
                MacroStep {
                    action: MacroAction::JumpToStep,
                    key: "4".to_string(),
                    ..Default::default()
                },
                MacroStep {
                    action: MacroAction::SetVariable,
                    if_variable_name: "a".to_string(),
                    set_variable_source: crate::model::SetVariableSource::Expression,
                    key: "2".to_string(),
                    ..Default::default()
                },
                MacroStep {
                    action: MacroAction::SetVariable,
                    if_variable_name: "a".to_string(),
                    set_variable_source: crate::model::SetVariableSource::Expression,
                    key: "3".to_string(),
                    ..Default::default()
                },
            ];
            let step_indices = vec![0, 1, 2, 3];
            let mut locked_keys = vec![];
            let mut locked_mouse = vec![];
            
            // Clear variables first
            RUNTIME_VARIABLES.lock().clear();

            let result = execute_macro_sequence(
                1,
                &steps,
                &step_indices,
                &mut locked_keys,
                &mut locked_mouse,
                false,
                None,
                &[],
                false,
                true, // bypass_enabled = true so we don't check master enable
            );

            assert_eq!(result, MacroRunFlow::Continue);
            // Variable "a" should be 3, because step at index 2 (SetVariable to 2) was skipped!
            let val = RUNTIME_VARIABLES.lock().get("a").copied();
            assert_eq!(val, Some(3.0));
            
            RUNTIME_VARIABLES.lock().clear();
        }

        #[test]
        fn test_jump_to_step_loop_propagation() {
            let _guard = TEST_MUTEX.lock().unwrap();
            
            // We have a loop body, and inside it a JumpToStep back to the start of the macro (index 0).
            // Since index 0 is outside the loop body, it must propagate out.
            // We'll set a counter variable "count" to prevent infinite loop, and check that it actually executed step 0 twice.
            let steps = vec![
                // Step 1 (index 0)
                MacroStep {
                    action: MacroAction::SetVariable,
                    if_variable_name: "count".to_string(),
                    set_variable_source: crate::model::SetVariableSource::Expression,
                    key: "{count + 1}".to_string(),
                    ..Default::default()
                },
                // Step 2 (index 1): LoopStart
                MacroStep {
                    action: MacroAction::LoopStart,
                    key: "1".to_string(), // loop 1 time
                    ..Default::default()
                },
                // Step 3 (index 2): If count < 2, jump to Step 1
                MacroStep {
                    action: MacroAction::IfStart,
                    if_condition_type: IfConditionType::Variable,
                    if_variable_name: "count".to_string(),
                    if_operator: "<".to_string(),
                    key: "2".to_string(),
                    ..Default::default()
                },
                MacroStep {
                    action: MacroAction::JumpToStep,
                    key: "1".to_string(),
                    ..Default::default()
                },
                MacroStep {
                    action: MacroAction::IfEnd,
                    ..Default::default()
                },
                // Step 6 (index 5): LoopEnd
                MacroStep {
                    action: MacroAction::LoopEnd,
                    ..Default::default()
                },
            ];
            let step_indices = vec![0, 1, 2, 3, 4, 5];
            let mut locked_keys = vec![];
            let mut locked_mouse = vec![];
            
            RUNTIME_VARIABLES.lock().clear();
            RUNTIME_VARIABLES.lock().insert("count".to_string(), 0.0);

            let result = execute_macro_sequence(
                1,
                &steps,
                &step_indices,
                &mut locked_keys,
                &mut locked_mouse,
                false,
                None,
                &[],
                false,
                true,
            );

            assert_eq!(result, MacroRunFlow::Continue);
            // Count should be 2, because step 0 executed twice.
            let val = RUNTIME_VARIABLES.lock().get("count").copied();
            assert_eq!(val, Some(2.0));

            RUNTIME_VARIABLES.lock().clear();
        }
    }

    fn is_infinite_loop_marker(value: &str) -> bool {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "infinite" | "inf" | "forever" | "-1"
        )
    }

    fn macro_runtime_target_matches(
        target_window_title: Option<&str>,
        extra_target_window_titles: &[String],
        match_duplicate_window_titles: bool,
    ) -> bool {
        let hook_state = HOOK_STATE.lock();
        macro_runtime_target_matches_with_guard(
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
            &hook_state,
        )
    }

    fn macro_runtime_target_matches_with_guard(
        target_window_title: Option<&str>,
        extra_target_window_titles: &[String],
        match_duplicate_window_titles: bool,
        _hook_state: &HookState,
    ) -> bool {
        window_focus_matches(
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
        )
    }

    fn trigger_nested_macro_preset(
        spec: &str,
        press_locked_keys: &mut Vec<String>,
        press_locked_mouse_masks: &mut Vec<MouseMoveLockMask>,
        stop_immediately_on_retrigger: bool,
        target_window_title: Option<&str>,
        extra_target_window_titles: &[String],
        match_duplicate_window_titles: bool,
        bypass_enabled: bool,
    ) -> Result<()> {
        let preset_id = spec
            .trim()
            .parse::<u32>()
            .context("Macro preset id is invalid")?;
        let preset = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .macro_groups
                .iter()
                .flat_map(|group| group.presets.iter())
                .find(|preset| preset.id == preset_id)
                .cloned()
        }
        .context("Macro preset was not found")?;
        let step_indices: Vec<usize> = (0..preset.steps.len()).collect();
        let _ = execute_macro_sequence(
            preset.id,
            &preset.steps,
            &step_indices,
            press_locked_keys,
            press_locked_mouse_masks,
            stop_immediately_on_retrigger,
            target_window_title,
            extra_target_window_titles,
            match_duplicate_window_titles,
            bypass_enabled,
        );
        Ok(())
    }

    fn parse_locked_keys(spec: &str) -> Vec<String> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }

        let has_separator = trimmed
            .chars()
            .any(|ch| matches!(ch, ',' | ';' | '+' | ' ' | '\t' | '\n'));
        if has_separator {
            return trimmed
                .split(|ch: char| matches!(ch, ',' | ';' | '+' | ' ' | '\t' | '\n'))
                .filter_map(|part| {
                    let key = part.trim();
                    (!key.is_empty()).then(|| normalize_locked_key(key))
                })
                .collect();
        }

        if trimmed.len() > 1 && trimmed.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            return trimmed
                .chars()
                .map(|ch| normalize_locked_key(&ch.to_string()))
                .collect();
        }

        vec![normalize_locked_key(trimmed)]
    }

    fn parse_stop_keys(spec: &str) -> Vec<String> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }

        let has_separator = trimmed
            .chars()
            .any(|ch| matches!(ch, ',' | ';' | '+' | ' ' | '\t' | '\n'));
        if has_separator {
            return trimmed
                .split(|ch: char| matches!(ch, ',' | ';' | '+' | ' ' | '\t' | '\n'))
                .filter_map(|part| {
                    let key = part.trim();
                    (!key.is_empty()).then(|| normalize_locked_key(key))
                })
                .collect();
        }

        vec![normalize_locked_key(trimmed)]
    }

    fn normalize_locked_key(key: &str) -> String {
        let trimmed = key.trim();
        if let Some(vk) = hotkey::key_name_to_vk(trimmed)
            && let Some(name) = hotkey::vk_to_key_name(vk)
        {
            return name.to_owned();
        }

        trimmed.to_owned()
    }

    fn show_hud_preset(owner_preset_id: u32, step: &MacroStep) -> Result<()> {
        let preset_id = step
            .key
            .trim()
            .parse::<u32>()
            .context("Toolbox preset id is invalid")?;
        let preset = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .hud_presets
                .iter()
                .find(|preset| preset.id == preset_id)
                .cloned()
        }
        .context("Toolbox preset was not found")?;
        let text = if step.text_override.trim().is_empty() {
            preset.text.trim().to_owned()
        } else {
            step.text_override.trim().to_owned()
        };
        let text = interpolate_variables(&text);
        if text.is_empty() {
            hide_hud_now();
            return Ok(());
        }

        let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1);
        let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1);
        let scale_x = screen_width as f32 / 1920.0;
        let scale_y = screen_height as f32 / 1080.0;
        let duration = step.get_duration_ms();
        let expires_at = if duration > 0 {
            Some(Instant::now() + Duration::from_millis(duration))
        } else {
            None
        };
        *HUD_DISPLAY.lock() = Some(HudDisplayState {
            owner_preset_id: Some(owner_preset_id),
            preset_id: Some(preset.id),
            text,
            text_color: preset.text_color,
            background_color: preset.background_color,
            background_opacity: preset.background_opacity.clamp(0.0, 1.0),
            rounded_background: preset.rounded_background,
            font_size: preset.font_size.max(1.0),
            x: (preset.x as f32 * scale_x).round() as i32,
            y: (preset.y as f32 * scale_y).round() as i32,
            width: ((preset.width.max(1)) as f32 * scale_x).round().max(1.0) as i32,
            height: ((preset.height.max(1)) as f32 * scale_y).round().max(1.0) as i32,
            auto_hide_on_owner_completion: false,
            expires_at,
        });
        Ok(())
    }

    fn toolbox_preview_display_from_preset(preset: HudPreset) -> HudDisplayState {
        let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1);
        let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1);
        let scale_x = screen_width as f32 / 1920.0;
        let scale_y = screen_height as f32 / 1080.0;
        HudDisplayState {
            owner_preset_id: None,
            preset_id: Some(preset.id),
            text: preset.text,
            text_color: preset.text_color,
            background_color: preset.background_color,
            background_opacity: preset.background_opacity.clamp(0.0, 1.0),
            rounded_background: preset.rounded_background,
            font_size: preset.font_size.max(1.0),
            x: (preset.x as f32 * scale_x).round() as i32,
            y: (preset.y as f32 * scale_y).round() as i32,
            width: ((preset.width.max(1)) as f32 * scale_x).round().max(1.0) as i32,
            height: ((preset.height.max(1)) as f32 * scale_y).round().max(1.0) as i32,
            auto_hide_on_owner_completion: false,
            expires_at: None,
        }
    }

    fn show_legacy_hud_text(owner_preset_id: u32, step: &MacroStep) {
        let text = if step.text_override.trim().is_empty() {
            step.key.trim().to_owned()
        } else {
            step.text_override.trim().to_owned()
        };
        let trimmed = interpolate_variables(text.trim()).to_owned();
        if trimmed.is_empty() {
            hide_hud_now();
            return;
        }

        *HUD_DISPLAY.lock() = Some(HudDisplayState {
            owner_preset_id: Some(owner_preset_id),
            preset_id: None,
            text: trimmed,
            text_color: RgbaColor {
                r: 244,
                g: 244,
                b: 244,
                a: 255,
            },
            background_color: RgbaColor {
                r: 34,
                g: 34,
                b: 34,
                a: 255,
            },
            background_opacity: 0.72,
            rounded_background: true,
            font_size: 28.0,
            x: 660,
            y: 36,
            width: 600,
            height: 80,
            auto_hide_on_owner_completion: false,
            expires_at: {
                let duration = step.get_duration_ms();
                if duration > 0 {
                    Some(Instant::now() + Duration::from_millis(duration))
                } else {
                    None
                }
            },
        });
    }

    fn trigger_hud_display(owner_preset_id: u32, step: &MacroStep) {
        if show_hud_preset(owner_preset_id, step).is_err() {
            show_legacy_hud_text(owner_preset_id, step);
        }

        wake_command_queue();
    }

    pub(crate) fn hide_hud_now() {
        *HUD_DISPLAY.lock() = None;
        *HUD_PREVIEW_DISPLAY.lock() = None;
        send_overlay_command(OverlayCommand::PreviewHudPreset(Vec::new()));
    }

    fn hide_toolbox_for_owner(owner_preset_id: u32) {
        let mut guard = HUD_DISPLAY.lock();
        if let Some(active) = guard.as_ref()
            && active.owner_preset_id == Some(owner_preset_id)
            && active.auto_hide_on_owner_completion
        {
            *guard = None;
        }
    }

    fn apply_lock_keys(keys: &[String], preset_id: Option<u32>, unlock_on_exit: bool) {
        let keys_to_release = {
            let mut to_release = Vec::new();
            let mut hook_state = HOOK_STATE.lock();
            for key in keys {
                let already_locked = hook_state
                    .locked_inputs
                    .get(key)
                    .copied()
                    .unwrap_or_default()
                    > 0;
                if !already_locked && hook_state.held_inputs.contains(key.as_str()) {
                    to_release.push(key.clone());
                }

                *hook_state.locked_inputs.entry(key.clone()).or_insert(0) += 1;
                if unlock_on_exit
                    && let Some(preset_id) = preset_id
                    && let Some(active) = hook_state.active_hold_macros.get_mut(&preset_id)
                    && !active
                        .locked_keys
                        .iter()
                        .any(|existing| existing.eq_ignore_ascii_case(key))
                {
                    active.locked_keys.push(key.clone());
                }
            }

            to_release
        };
        for key in keys_to_release {
            let _ = send_key_event(&MacroStep {
                key,
                action: MacroAction::KeyUp,
                delay_ms: 0,
                x: 0,
                y: 0,
                ..MacroStep::default()
            });
        }
    }

    fn apply_unlock_keys(keys: &[String], preset_id: Option<u32>) {
        let keys_to_restore = {
            let mut to_restore = Vec::new();
            let mut hook_state = HOOK_STATE.lock();
            for key in keys {
                let mut should_restore = false;
                if let Some(preset_id) = preset_id
                    && let Some(active) = hook_state.active_hold_macros.get_mut(&preset_id)
                {
                    active
                        .locked_keys
                        .retain(|locked| !locked.eq_ignore_ascii_case(key));
                }

                if let Some(count) = hook_state.locked_inputs.get_mut(key) {
                    if *count > 1 {
                        *count -= 1;
                    } else {
                        hook_state.locked_inputs.remove(key);
                        should_restore = hook_state.held_inputs.contains(key.as_str());
                    }
                }

                if should_restore {
                    to_restore.push(key.clone());
                }
            }

            to_restore
        };
        for key in keys_to_restore {
            let _ = send_key_event(&MacroStep {
                key,
                action: MacroAction::KeyDown,
                delay_ms: 0,
                x: 0,
                y: 0,
                ..MacroStep::default()
            });
        }
    }

    fn mouse_move_lock_mask_from_step(step: &MacroStep) -> MouseMoveLockMask {
        MouseMoveLockMask {
            left: step.lock_mouse_left,
            right: step.lock_mouse_right,
            up: step.lock_mouse_middle,
            down: step.lock_mouse_scroll,
        }
    }

    fn apply_lock_mouse(step: &MacroStep, preset_id: Option<u32>, unlock_on_exit: bool) {
        let mask = mouse_move_lock_mask_from_step(step);
        if !mask.any() {
            return;
        }

        let mut hook_state = HOOK_STATE.lock();
        hook_state.mouse_move_locks.add(mask);
        if hook_state.mouse_move_lock_anchor.is_none() {
            let mut point = POINT::default();
            if unsafe { GetCursorPos(&mut point) }.is_ok() {
                hook_state.mouse_move_lock_anchor = Some(point);
            }
        }

        if unlock_on_exit
            && let Some(preset_id) = preset_id
            && let Some(active) = hook_state.active_hold_macros.get_mut(&preset_id)
        {
            active.locked_mouse_masks.push(mask);
        }
    }

    fn apply_unlock_mouse(preset_id: Option<u32>, mask: MouseMoveLockMask) {
        if !mask.any() {
            return;
        }

        let mut hook_state = HOOK_STATE.lock();
        if let Some(preset_id) = preset_id
            && let Some(active) = hook_state.active_hold_macros.get_mut(&preset_id)
            && let Some(index) = active.locked_mouse_masks.iter().position(|entry| *entry == mask)
        {
            active.locked_mouse_masks.remove(index);
        }

        hook_state.mouse_move_locks.remove(mask);
        if !hook_state.mouse_move_locks.any() {
            hook_state.mouse_move_lock_anchor = None;
        }
    }

    fn collect_macro_release_steps(steps: &[MacroStep]) -> Vec<MacroStep> {
        let mut held_keys = HashSet::new();
        let mut held_mouse = HashSet::new();
        for step in steps {
            if !step.enabled {
                continue;
            }

            match step.action {
                MacroAction::KeyDown => {
                    held_keys.insert(step.key.clone());
                }

                MacroAction::KeyUp | MacroAction::KeyPress => {
                    held_keys.remove(&step.key);
                }

                MacroAction::TypeText
                | MacroAction::Wait
                | MacroAction::ApplyWindowPreset
                | MacroAction::FocusWindowPreset
                | MacroAction::TriggerMacroPreset
                | MacroAction::TriggerMacroPresetIfEnabled
                | MacroAction::StopMacroPreset
                | MacroAction::TriggerCommandPreset
                | MacroAction::EnableCrosshairProfile
                | MacroAction::DisableCrosshair
                | MacroAction::EnablePinPreset
                | MacroAction::DisablePin
                | MacroAction::PlayMousePathPreset
                | MacroAction::ApplyMouseSensitivityPreset
                | MacroAction::EnableZoomPreset
                | MacroAction::DisableZoom
                | MacroAction::PlaySoundPreset
                | MacroAction::PlayVideoPreset
                | MacroAction::StartVisionSearch
                | MacroAction::ScanVisionOnce
                | MacroAction::StopVisionWait
                | MacroAction::StopVision => {}

                MacroAction::LoopStart
                | MacroAction::LoopEnd
                | MacroAction::StopIfTriggerPressedAgain
                | MacroAction::StopIfKeyPressed
                | MacroAction::ShowHud
                | MacroAction::HideHud
                | MacroAction::HideTaskbar
                | MacroAction::ShowTaskbar
                | MacroAction::LockKeys
                | MacroAction::UnlockKeys
                | MacroAction::LockMouse
                | MacroAction::UnlockMouse
                | MacroAction::EnableMacroPreset
                | MacroAction::DisableMacroPreset
                | MacroAction::EnableStep
                | MacroAction::DisableStep
                | MacroAction::StartTimerPreset
                | MacroAction::PauseTimerPreset
                | MacroAction::StopTimerPreset => {}

                MacroAction::MouseLeftDown => {
                    held_mouse.insert(MacroAction::MouseLeftUp);
                }

                MacroAction::MouseLeftUp | MacroAction::MouseLeftClick => {
                    held_mouse.remove(&MacroAction::MouseLeftUp);
                }

                MacroAction::MouseRightDown => {
                    held_mouse.insert(MacroAction::MouseRightUp);
                }

                MacroAction::MouseRightUp | MacroAction::MouseRightClick => {
                    held_mouse.remove(&MacroAction::MouseRightUp);
                }

                MacroAction::MouseMiddleDown => {
                    held_mouse.insert(MacroAction::MouseMiddleUp);
                }

                MacroAction::MouseMiddleUp | MacroAction::MouseMiddleClick => {
                    held_mouse.remove(&MacroAction::MouseMiddleUp);
                }

                MacroAction::MouseX1Down => {
                    held_mouse.insert(MacroAction::MouseX1Up);
                }

                MacroAction::MouseX1Up | MacroAction::MouseX1Click => {
                    held_mouse.remove(&MacroAction::MouseX1Up);
                }

                MacroAction::MouseX2Down => {
                    held_mouse.insert(MacroAction::MouseX2Up);
                }

                MacroAction::MouseX2Up | MacroAction::MouseX2Click => {
                    held_mouse.remove(&MacroAction::MouseX2Up);
                }

                MacroAction::MouseWheelUp
                | MacroAction::MouseWheelDown
                | MacroAction::MouseMoveAbsolute
                | MacroAction::MouseMoveRelative => {}

                _ => {}
            }
        }

        let mut cleanup_steps = Vec::new();
        for key in held_keys {
            cleanup_steps.push(MacroStep {
                key,
                action: MacroAction::KeyUp,
                delay_ms: 0,
                x: 0,
                y: 0,
                ..MacroStep::default()
            });
        }

        for action in held_mouse {
            cleanup_steps.push(MacroStep {
                key: String::new(),
                action,
                delay_ms: 0,
                x: 0,
                y: 0,
                ..MacroStep::default()
            });
        }

        cleanup_steps
    }

    fn collect_macro_image_search_start_ids(steps: &[MacroStep]) -> Vec<u32> {
        let mut ids = HashSet::new();
        for step in steps {
            if !step.enabled {
                continue;
            }

            if step.action == MacroAction::StartVisionSearch
                && let Ok(preset_id) = step.key.trim().parse::<u32>()
            {
                ids.insert(preset_id);
            }
        }

        ids.into_iter().collect()
    }

    fn send_key_event(step: &MacroStep) -> Result<()> {
        match step.action {
            MacroAction::MouseLeftClick
            | MacroAction::MouseLeftDown
            | MacroAction::MouseLeftUp
            | MacroAction::MouseRightClick
            | MacroAction::MouseRightDown
            | MacroAction::MouseRightUp
            | MacroAction::MouseMiddleClick
            | MacroAction::MouseMiddleDown
            | MacroAction::MouseMiddleUp
            | MacroAction::MouseX1Click
            | MacroAction::MouseX1Down
            | MacroAction::MouseX1Up
            | MacroAction::MouseX2Click
            | MacroAction::MouseX2Down
            | MacroAction::MouseX2Up
            | MacroAction::MouseWheelUp
            | MacroAction::MouseWheelDown
            | MacroAction::MouseMoveAbsolute
            | MacroAction::MouseMoveRelative => return send_mouse_event(step),
            MacroAction::TypeText => return send_text_input(&interpolate_variables(&step.key)),
            MacroAction::Wait => return Ok(()),
            MacroAction::ApplyWindowPreset
            | MacroAction::FocusWindowPreset
            | MacroAction::TriggerMacroPreset
            | MacroAction::TriggerMacroPresetIfEnabled
            | MacroAction::StopMacroPreset
            | MacroAction::TriggerCommandPreset
            | MacroAction::EnableCrosshairProfile
            | MacroAction::DisableCrosshair
            | MacroAction::EnablePinPreset
            | MacroAction::DisablePin
            | MacroAction::PlayMousePathPreset
            | MacroAction::ApplyMouseSensitivityPreset
            | MacroAction::EnableZoomPreset
            | MacroAction::DisableZoom
            | MacroAction::PlaySoundPreset
            | MacroAction::PlayVideoPreset
            | MacroAction::StartVisionSearch
            | MacroAction::ScanVisionOnce
            | MacroAction::StopVisionWait
            | MacroAction::StopVision => return Ok(()),
            MacroAction::LoopStart
            | MacroAction::LoopEnd
            | MacroAction::StopIfTriggerPressedAgain
            | MacroAction::StopIfKeyPressed
            | MacroAction::ShowHud
            | MacroAction::HideHud
            | MacroAction::HideTaskbar
            | MacroAction::ShowTaskbar
            | MacroAction::LockKeys
            | MacroAction::UnlockKeys
            | MacroAction::LockMouse
            | MacroAction::UnlockMouse
            | MacroAction::EnableMacroPreset
            | MacroAction::DisableMacroPreset
            | MacroAction::EnableStep
            | MacroAction::DisableStep
            | MacroAction::StartTimerPreset
            | MacroAction::PauseTimerPreset
            | MacroAction::StopTimerPreset => return Ok(()),
            MacroAction::KeyPress | MacroAction::KeyDown | MacroAction::KeyUp => {}

            _ => return Ok(()),
        }

        let Some(vk) = hotkey::key_name_to_vk(&step.key) else {
            bail!("Unsupported macro key: {}", step.key);
        };
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };
        if scan == 0 {
            bail!("Unsupported macro key scan code: {}", step.key);
        }

        let base_flags = KEYEVENTF_SCANCODE
            | if is_extended_key(vk) {
                KEYEVENTF_EXTENDEDKEY
            } else {
                Default::default()
            };
        let key_down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: scan as u16,
                    dwFlags: base_flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let key_up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: scan as u16,
                    dwFlags: base_flags | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let delay_ms = HOOK_STATE.lock().macro_keyboard_key_press_delay_ms;
        unsafe {
            if step.action == MacroAction::KeyPress && delay_ms > 0 {
                let sent = SendInput(&[key_down], size_of::<INPUT>() as i32);
                if sent == 0 {
                    bail!("SendInput key down failed");
                }

                thread::sleep(Duration::from_millis(delay_ms as u64));
                let sent = SendInput(&[key_up], size_of::<INPUT>() as i32);
                if sent == 0 {
                    bail!("SendInput key up failed");
                }
            } else {
                let inputs: Vec<INPUT> = match step.action {
                    MacroAction::KeyPress => vec![key_down, key_up],
                    MacroAction::KeyDown => vec![key_down],
                    MacroAction::KeyUp => vec![key_up],
                    _ => unreachable!("mouse actions are handled earlier"),
                };
                let sent = SendInput(&inputs, size_of::<INPUT>() as i32);
                if sent == 0 {
                    bail!("SendInput failed");
                }
            }
        }

        Ok(())
    }

    fn send_text_input(text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        let mut inputs = Vec::with_capacity(text.encode_utf16().count() * 2);
        for unit in text.encode_utf16() {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        unsafe {
            let sent = SendInput(&inputs, size_of::<INPUT>() as i32);
            if sent == 0 {
                bail!("SendInput failed");
            }
        }

        Ok(())
    }

    fn write_arduino_data(bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        let (use_arduino, transport, com_port, vid, pid, flash_in_progress) = {
            let state = HOOK_STATE.lock();
            (
                state.use_arduino_mouse,
                state.arduino_transport,
                state.arduino_com_port.clone(),
                state.arduino_vid.clone(),
                state.arduino_pid.clone(),
                state.arduino_flash_in_progress,
            )
        };
        if flash_in_progress {
            anyhow::bail!("Arduino flash is in progress");
        }
        if !use_arduino {
            anyhow::bail!("Arduino emulation not enabled");
        }

        match transport {
            ArduinoTransport::Serial => {
                if com_port.is_empty() {
                    anyhow::bail!("Arduino serial mode selected but COM port is empty");
                }

                let mut name_guard = CURRENT_ARDUINO_PORT_NAME.lock();
                let mut port_guard = ARDUINO_PORT.lock();
                if *name_guard != com_port || port_guard.is_none() {
                    *port_guard = None;

                    // Check cooldown to prevent resetting Arduino Leonardo in an infinite loop
                    let mut last_attempt = LAST_ARDUINO_OPEN_ATTEMPT.lock();
                    if let Some(instant) = *last_attempt {
                        if instant.elapsed() < Duration::from_secs(3) {
                            anyhow::bail!("Arduino reconnect cooldown active");
                        }
                    }
                    *last_attempt = Some(Instant::now());

                    match serialport::new(&com_port, 115200)
                        .timeout(Duration::from_millis(10))
                        .open()
                    {
                        Ok(p) => {
                            *port_guard = Some(p);
                            *name_guard = com_port.clone();
                        }
                        Err(e) => {
                            anyhow::bail!("Failed to open serial port: {}", e);
                        }
                    }
                }

                if let Some(ref mut port) = *port_guard {
                    if let Err(e) = port.write_all(bytes).and_then(|_| port.flush()) {
                        *port_guard = None;
                        anyhow::bail!("Failed to write to serial port: {}", e);
                    }
                    Ok(())
                } else {
                    anyhow::bail!("Serial port not open")
                }
            }
            ArduinoTransport::Hid => {
                let target_vid = parse_hex_u16_runtime(&vid, 0x2341);
                let target_pid = parse_hex_u16_runtime(&pid, 0x8036);
                let mut hid_name_guard = CURRENT_ARDUINO_HID_NAME.lock();
                let mut hid_guard = ARDUINO_HID_DEVICE.lock();
                let mut hid_write_guard = LAST_ARDUINO_HID_WRITE_AT.lock();

                if hid_guard.is_none() {
                    let runtime = open_arduino_hid_device(target_vid, target_pid)?;
                    *hid_name_guard = runtime.path.clone();
                    *hid_guard = Some(runtime);
                    *hid_write_guard = None;
                }

                let min_gap = Duration::from_millis(8);
                if let Some(last_write_at) = *hid_write_guard {
                    let elapsed = last_write_at.elapsed();
                    if elapsed < min_gap {
                        thread::sleep(min_gap - elapsed);
                    }
                }

                let mut report = [0u8; 65];
                report[0] = 0;
                report[1] = 0xA5;
                report[2] = bytes.get(1).copied().unwrap_or(0);
                report[3] = bytes.get(2).copied().unwrap_or(0);
                report[4] = bytes.get(3).copied().unwrap_or(0);
                report[5] = bytes.get(4).copied().unwrap_or(0);
                report[6] = bytes.get(5).copied().unwrap_or(0);
                report[7] = 0x5A;

                if let Some(runtime) = hid_guard.as_mut() {
                    let report_ok = unsafe {
                        HidD_SetOutputReport(
                            runtime.handle,
                            report.as_ptr() as *mut c_void,
                            report.len() as u32,
                        )
                    };

                    if !report_ok {
                        let mut bytes_written = 0u32;
                        let write_ok = unsafe {
                            WriteFile(
                                runtime.handle,
                                Some(&report),
                                Some(&mut bytes_written as *mut u32),
                                None,
                            )
                        }
                        .is_ok();
                        if !write_ok || bytes_written == 0 {
                            *hid_guard = None;
                            *hid_name_guard = String::new();
                            *hid_write_guard = None;
                            anyhow::bail!("Failed to write RawHID report");
                        }
                    }
                    *hid_write_guard = Some(Instant::now());
                    Ok(())
                } else {
                    anyhow::bail!("HID device not open")
                }
            }
        }
    }

    fn send_mouse_input(dw_flags: MOUSE_EVENT_FLAGS, mouse_data: u32) -> Result<()> {
        let (use_arduino, transport, com_port) = {
            let state = HOOK_STATE.lock();
            (
                state.use_arduino_mouse,
                state.arduino_transport,
                state.arduino_com_port.clone(),
            )
        };
        let arduino_ready = use_arduino
            && match transport {
                ArduinoTransport::Serial => !com_port.is_empty(),
                ArduinoTransport::Hid => true,
            };
        if arduino_ready {
            let mut send_btn = |btn: u8, state: u8| -> Result<()> {
                let packet = [0xAA, 2, btn, state, 0, 0];
                write_arduino_data(&packet)
            };

            let mut arduino_success = true;

            if dw_flags.contains(MOUSEEVENTF_LEFTDOWN) {
                if send_btn(1, 1).is_err() { arduino_success = false; }
            }
            if dw_flags.contains(MOUSEEVENTF_LEFTUP) {
                if send_btn(1, 0).is_err() { arduino_success = false; }
            }
            if dw_flags.contains(MOUSEEVENTF_RIGHTDOWN) {
                if send_btn(2, 1).is_err() { arduino_success = false; }
            }
            if dw_flags.contains(MOUSEEVENTF_RIGHTUP) {
                if send_btn(2, 0).is_err() { arduino_success = false; }
            }
            if dw_flags.contains(MOUSEEVENTF_MIDDLEDOWN) {
                if send_btn(3, 1).is_err() { arduino_success = false; }
            }
            if dw_flags.contains(MOUSEEVENTF_MIDDLEUP) {
                if send_btn(3, 0).is_err() { arduino_success = false; }
            }
            if dw_flags.contains(MOUSEEVENTF_WHEEL) {
                let val = (mouse_data as i32) / 120;
                let val_byte = (val.clamp(-127, 127) as i8) as u8;
                let packet = [0xAA, 3, val_byte, 0, 0, 0];
                if write_arduino_data(&packet).is_err() { arduino_success = false; }
            }
            if dw_flags.contains(MOUSEEVENTF_XDOWN) || dw_flags.contains(MOUSEEVENTF_XUP) {
                arduino_success = false;
            }

            if arduino_success {
                return Ok(());
            }
        }
        let suppressed_mouse_name =
            if dw_flags == MOUSEEVENTF_LEFTDOWN || dw_flags == MOUSEEVENTF_LEFTUP {
                Some("MouseLeft")
            } else if dw_flags == MOUSEEVENTF_RIGHTDOWN || dw_flags == MOUSEEVENTF_RIGHTUP {
                Some("MouseRight")
            } else if dw_flags == MOUSEEVENTF_MIDDLEDOWN || dw_flags == MOUSEEVENTF_MIDDLEUP {
                Some("MouseMiddle")
            } else if dw_flags == MOUSEEVENTF_XDOWN || dw_flags == MOUSEEVENTF_XUP {
                if mouse_data == XBUTTON1_DATA as u32 {
                    Some("MouseX1")
                } else if mouse_data == XBUTTON2_DATA as u32 {
                    Some("MouseX2")
                } else {
                    None
                }
            } else if dw_flags == MOUSEEVENTF_WHEEL {
                if mouse_data == 120u32 {
                    Some("MouseWheelUp")
                } else {
                    Some("MouseWheelDown")
                }
            } else {
                None
            };
        let use_interception = {
            let state = HOOK_STATE.lock();
            state.use_interception
                && state.interception_dll_path.exists()
                && crate::platform::is_interception_driver_installed()
        };
        if use_interception {
            if let Some(key_name) = suppressed_mouse_name {
                suppress_next_mouse_trigger(key_name);
            }
        }

        if use_interception {
            let interception_dll = { HOOK_STATE.lock().interception_dll_path.clone() };
            unsafe {
                if let Ok(lib) = libloading::Library::new(&interception_dll) {
                    let create_context: Result<
                        libloading::Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_void>,
                        _,
                    > = lib.get(b"interception_create_context");
                    let send: Result<
                        libloading::Symbol<
                            unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const u8, u32) -> i32,
                        >,
                        _,
                    > = lib.get(b"interception_send");
                    let destroy_context: Result<
                        libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void)>,
                        _,
                    > = lib.get(b"interception_destroy_context");
                    if let (Ok(create_fn), Ok(send_fn), Ok(destroy_fn)) =
                        (create_context, send, destroy_context)
                    {
                        let context = create_fn();
                        if !context.is_null() {
                            #[repr(C)]
                            struct InterceptionMouseStroke {
                                state: u16,
                                flags: u16,
                                rolling: i16,
                                x: i32,
                                y: i32,
                                information: u32,
                            }

                            let mut state_val = 0u16;
                            // Map win32 MOUSEEVENTF flags to Interception mouse state bits

                            if dw_flags.contains(MOUSEEVENTF_LEFTDOWN) {
                                state_val |= 0x0001;
                            }

                            if dw_flags.contains(MOUSEEVENTF_LEFTUP) {
                                state_val |= 0x0002;
                            }

                            if dw_flags.contains(MOUSEEVENTF_RIGHTDOWN) {
                                state_val |= 0x0004;
                            }

                            if dw_flags.contains(MOUSEEVENTF_RIGHTUP) {
                                state_val |= 0x0008;
                            }

                            if dw_flags.contains(MOUSEEVENTF_MIDDLEDOWN) {
                                state_val |= 0x0010;
                            }

                            if dw_flags.contains(MOUSEEVENTF_MIDDLEUP) {
                                state_val |= 0x0020;
                            }

                            if dw_flags.contains(MOUSEEVENTF_XDOWN) {
                                if mouse_data == XBUTTON1_DATA as u32 {
                                    state_val |= 0x0040;
                                } else if mouse_data == XBUTTON2_DATA as u32 {
                                    state_val |= 0x0100;
                                }
                            }

                            if dw_flags.contains(MOUSEEVENTF_XUP) {
                                if mouse_data == XBUTTON1_DATA as u32 {
                                    state_val |= 0x0080;
                                } else if mouse_data == XBUTTON2_DATA as u32 {
                                    state_val |= 0x0200;
                                }
                            }

                            if dw_flags.contains(MOUSEEVENTF_WHEEL) {
                                state_val |= 0x0400;
                            }

                            let rolling_val = if dw_flags.contains(MOUSEEVENTF_WHEEL) {
                                mouse_data as i16
                            } else {
                                0
                            };
                            let stroke = InterceptionMouseStroke {
                                state: state_val,
                                flags: 0,
                                rolling: rolling_val,
                                x: 0,
                                y: 0,
                                information: 0,
                            };
                            // Send to mouse device 12 (standard first mouse handle INTERCEPTION_MOUSE(0))

                            let stroke_ptr = &stroke as *const InterceptionMouseStroke as *const u8;
                            let sent = send_fn(context, 12, stroke_ptr, 1);
                            destroy_fn(context);
                            if sent > 0 {
                                set_interception_runtime_status(InterceptionRuntimeStatus::Active);
                                return Ok(());
                            }
                        }
                    }
                }
            }

            set_interception_runtime_status(InterceptionRuntimeStatus::FallbackToSendInput);
        } else {
            set_interception_runtime_status(InterceptionRuntimeStatus::Unavailable);
        }

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: mouse_data,
                    dwFlags: dw_flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            let sent = SendInput(&[input], size_of::<INPUT>() as i32);
            if sent == 0 {
                bail!("SendInput failed");
            }
        }

        Ok(())
    }

    fn send_mouse_event(step: &MacroStep) -> Result<()> {
        let delay_ms = HOOK_STATE.lock().macro_mouse_click_delay_ms;
        match step.action {
            MacroAction::MouseMoveAbsolute => {
                return send_mouse_move_absolute(step.get_x(), step.get_y());
            }

            MacroAction::MouseMoveRelative => {
                return send_mouse_move_relative(step.get_x(), step.get_y());
            }

            MacroAction::MouseLeftClick => {
                send_mouse_input(MOUSEEVENTF_LEFTDOWN, 0)?;
                if delay_ms > 0 {
                    thread::sleep(Duration::from_millis(delay_ms as u64));
                }

                return send_mouse_input(MOUSEEVENTF_LEFTUP, 0);
            }

            MacroAction::MouseRightClick => {
                send_mouse_input(MOUSEEVENTF_RIGHTDOWN, 0)?;
                if delay_ms > 0 {
                    thread::sleep(Duration::from_millis(delay_ms as u64));
                }

                return send_mouse_input(MOUSEEVENTF_RIGHTUP, 0);
            }

            MacroAction::MouseMiddleClick => {
                send_mouse_input(MOUSEEVENTF_MIDDLEDOWN, 0)?;
                if delay_ms > 0 {
                    thread::sleep(Duration::from_millis(delay_ms as u64));
                }

                return send_mouse_input(MOUSEEVENTF_MIDDLEUP, 0);
            }

            MacroAction::MouseX1Click => {
                send_mouse_input(MOUSEEVENTF_XDOWN, XBUTTON1_DATA as u32)?;
                if delay_ms > 0 {
                    thread::sleep(Duration::from_millis(delay_ms as u64));
                }

                return send_mouse_input(MOUSEEVENTF_XUP, XBUTTON1_DATA as u32);
            }

            MacroAction::MouseX2Click => {
                send_mouse_input(MOUSEEVENTF_XDOWN, XBUTTON2_DATA as u32)?;
                if delay_ms > 0 {
                    thread::sleep(Duration::from_millis(delay_ms as u64));
                }

                return send_mouse_input(MOUSEEVENTF_XUP, XBUTTON2_DATA as u32);
            }

            _ => {}
        }

        let (flags, mouse_data) = match step.action {
            MacroAction::MouseLeftDown => (MOUSEEVENTF_LEFTDOWN, 0),
            MacroAction::MouseLeftUp => (MOUSEEVENTF_LEFTUP, 0),
            MacroAction::MouseRightDown => (MOUSEEVENTF_RIGHTDOWN, 0),
            MacroAction::MouseRightUp => (MOUSEEVENTF_RIGHTUP, 0),
            MacroAction::MouseMiddleDown => (MOUSEEVENTF_MIDDLEDOWN, 0),
            MacroAction::MouseMiddleUp => (MOUSEEVENTF_MIDDLEUP, 0),
            MacroAction::MouseX1Down => (MOUSEEVENTF_XDOWN, XBUTTON1_DATA as u32),
            MacroAction::MouseX1Up => (MOUSEEVENTF_XUP, XBUTTON1_DATA as u32),
            MacroAction::MouseX2Down => (MOUSEEVENTF_XDOWN, XBUTTON2_DATA as u32),
            MacroAction::MouseX2Up => (MOUSEEVENTF_XUP, XBUTTON2_DATA as u32),
            MacroAction::MouseWheelUp => (MOUSEEVENTF_WHEEL, 120u32),
            MacroAction::MouseWheelDown => (MOUSEEVENTF_WHEEL, (-120i32) as u32),
            _ => bail!("Unsupported mouse action"),
        };
        send_mouse_input(flags, mouse_data)
    }

    fn send_arduino_relative_move_packet(dx: i32, dy: i32) -> Result<()> {
        let packet = [
            0xAA,
            1,
            ((dx >> 8) & 0xFF) as u8,
            (dx & 0xFF) as u8,
            ((dy >> 8) & 0xFF) as u8,
            (dy & 0xFF) as u8,
        ];
        write_arduino_data(&packet)
    }

    fn send_arduino_relative_move_sequence(dx: i32, dy: i32) -> Result<()> {
        let mut rem_x = dx;
        let mut rem_y = dy;

        while rem_x != 0 || rem_y != 0 {
            let step_x = rem_x.clamp(-96, 96);
            let step_y = rem_y.clamp(-96, 96);
            send_arduino_relative_move_packet(step_x, step_y)?;
            rem_x -= step_x;
            rem_y -= step_y;
        }

        Ok(())
    }

    fn send_mouse_move_absolute(x: i32, y: i32) -> Result<()> {
        let (use_arduino, transport, com_port) = {
            let state = HOOK_STATE.lock();
            (
                state.use_arduino_mouse,
                state.arduino_transport,
                state.arduino_com_port.clone(),
            )
        };
        let arduino_ready = use_arduino
            && match transport {
                ArduinoTransport::Serial => !com_port.is_empty(),
                ArduinoTransport::Hid => true,
            };
        if arduino_ready {
            let mut pos = POINT { x: 0, y: 0 };
            unsafe {
                let _ = GetCursorPos(&mut pos);
            }
            let dx = x - pos.x;
            let dy = y - pos.y;
            if send_arduino_relative_move_sequence(dx, dy).is_ok() {
                return Ok(());
            }
        }

        let use_interception = {
            let state = HOOK_STATE.lock();
            state.use_interception
                && state.interception_dll_path.exists()
                && crate::platform::is_interception_driver_installed()
        };
        if use_interception {
            let interception_dll = { HOOK_STATE.lock().interception_dll_path.clone() };
            unsafe {
                if let Ok(lib) = libloading::Library::new(&interception_dll) {
                    let create_context: Result<
                        libloading::Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_void>,
                        _,
                    > = lib.get(b"interception_create_context");
                    let send: Result<
                        libloading::Symbol<
                            unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const u8, u32) -> i32,
                        >,
                        _,
                    > = lib.get(b"interception_send");
                    let destroy_context: Result<
                        libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void)>,
                        _,
                    > = lib.get(b"interception_destroy_context");
                    if let (Ok(create_fn), Ok(send_fn), Ok(destroy_fn)) =
                        (create_context, send, destroy_context)
                    {
                        let context = create_fn();
                        if !context.is_null() {
                            #[repr(C)]
                            struct InterceptionMouseStroke {
                                state: u16,
                                flags: u16,
                                rolling: i16,
                                x: i32,
                                y: i32,
                                information: u32,
                            }

                            let screen_w = GetSystemMetrics(SM_CXSCREEN).max(1);
                            let screen_h = GetSystemMetrics(SM_CYSCREEN).max(1);
                            let normalized_x = ((x.clamp(0, screen_w - 1) as i64) * 65535
                                / (screen_w - 1).max(1) as i64)
                                as i32;
                            let normalized_y = ((y.clamp(0, screen_h - 1) as i64) * 65535
                                / (screen_h - 1).max(1) as i64)
                                as i32;
                            let stroke = InterceptionMouseStroke {
                                state: 0,
                                flags: 0x001 | 0x002, // absolute movement + virtual desktop

                                rolling: 0,
                                x: normalized_x,
                                y: normalized_y,
                                information: 0,
                            };
                            let stroke_ptr = &stroke as *const InterceptionMouseStroke as *const u8;
                            let sent = send_fn(context, 12, stroke_ptr, 1);
                            destroy_fn(context);
                            if sent > 0 {
                                set_interception_runtime_status(InterceptionRuntimeStatus::Active);
                                return Ok(());
                            }
                        }
                    }
                }
            }

            set_interception_runtime_status(InterceptionRuntimeStatus::FallbackToSendInput);
        } else {
            set_interception_runtime_status(InterceptionRuntimeStatus::Unavailable);
        }

        let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1);
        let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1);
        let normalized_x =
            ((x.clamp(0, screen_w - 1) as i64) * 65535 / (screen_w - 1).max(1) as i64) as i32;
        let normalized_y =
            ((y.clamp(0, screen_h - 1) as i64) * 65535 / (screen_h - 1).max(1) as i64) as i32;
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: normalized_x,
                    dy: normalized_y,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            let sent = SendInput(&[input], size_of::<INPUT>() as i32);
            if sent == 0 {
                let _ = SetCursorPos(x, y);
            }
        }

        Ok(())
    }

    fn settle_image_search_mouse_move(
        x: i32,
        y: i32,
        move_passes: u8,
        move_delay_ms: u64,
    ) -> Result<()> {
        let attempts = move_passes.max(1) as usize;
        for attempt in 0..attempts {
            send_mouse_move_absolute(x, y)?;
            if attempt + 1 < attempts && move_delay_ms > 0 {
                thread::sleep(Duration::from_millis(move_delay_ms));
            }
        }

        Ok(())
    }

    fn settle_mouse_path_relative_segment(
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        speed: f32,
        preset_id: Option<u32>,
        stop_immediately_on_retrigger: bool,
    ) -> Result<()> {
        let dx = to_x - from_x;
        let dy = to_y - from_y;
        let distance = (((dx * dx + dy * dy) as f32).sqrt()).max(1.0);
        let duration_ms = ((distance / (900.0 * speed)) * 1000.0)
            .round()
            .clamp(1.0, 5_000.0) as u64;
        let steps = ((duration_ms as f32) / 8.0).ceil().max(1.0) as u64;
        let frame_delay_ms =
            ((duration_ms as f32) / steps as f32).round().max(1.0) as u64;
        let mut prev_x = from_x;
        let mut prev_y = from_y;
        for index in 1..=steps {
            if preset_id.is_some_and(|id| macro_stop_requested(id, stop_immediately_on_retrigger)) {
                return Ok(());
            }

            let t = index as f32 / steps as f32;
            let next_x = (from_x as f32 + dx as f32 * t).round() as i32;
            let next_y = (from_y as f32 + dy as f32 * t).round() as i32;
            send_mouse_move_relative(next_x - prev_x, next_y - prev_y)?;
            prev_x = next_x;
            prev_y = next_y;
            if sleep_for_mouse_path_delay(
                preset_id,
                frame_delay_ms,
                stop_immediately_on_retrigger,
            ) {
                return Ok(());
            }
        }

        Ok(())
    }

    fn send_mouse_move_relative(dx: i32, dy: i32) -> Result<()> {
        let (use_arduino, transport, com_port) = {
            let state = HOOK_STATE.lock();
            (
                state.use_arduino_mouse,
                state.arduino_transport,
                state.arduino_com_port.clone(),
            )
        };
        let arduino_ready = use_arduino
            && match transport {
                ArduinoTransport::Serial => !com_port.is_empty(),
                ArduinoTransport::Hid => true,
            };
        if arduino_ready {
            if send_arduino_relative_move_sequence(dx, dy).is_ok() {
                return Ok(());
            }
        }

        let use_interception = {
            let state = HOOK_STATE.lock();
            state.use_interception
                && state.interception_dll_path.exists()
                && crate::platform::is_interception_driver_installed()
        };
        if use_interception {
            let interception_dll = { HOOK_STATE.lock().interception_dll_path.clone() };
            unsafe {
                if let Ok(lib) = libloading::Library::new(&interception_dll) {
                    let create_context: Result<
                        libloading::Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_void>,
                        _,
                    > = lib.get(b"interception_create_context");
                    let send: Result<
                        libloading::Symbol<
                            unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const u8, u32) -> i32,
                        >,
                        _,
                    > = lib.get(b"interception_send");
                    let destroy_context: Result<
                        libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void)>,
                        _,
                    > = lib.get(b"interception_destroy_context");
                    if let (Ok(create_fn), Ok(send_fn), Ok(destroy_fn)) =
                        (create_context, send, destroy_context)
                    {
                        let context = create_fn();
                        if !context.is_null() {
                            #[repr(C)]
                            struct InterceptionMouseStroke {
                                state: u16,
                                flags: u16,
                                rolling: i16,
                                x: i32,
                                y: i32,
                                information: u32,
                            }

                            let stroke = InterceptionMouseStroke {
                                state: 0,
                                flags: 0x000, // relative movement

                                rolling: 0,
                                x: dx,
                                y: dy,
                                information: 0,
                            };
                            let stroke_ptr = &stroke as *const InterceptionMouseStroke as *const u8;
                            let sent = send_fn(context, 12, stroke_ptr, 1);
                            destroy_fn(context);
                            if sent > 0 {
                                set_interception_runtime_status(InterceptionRuntimeStatus::Active);
                                return Ok(());
                            }
                        }
                    }
                }
            }

            set_interception_runtime_status(InterceptionRuntimeStatus::FallbackToSendInput);
        } else {
            set_interception_runtime_status(InterceptionRuntimeStatus::Unavailable);
        }

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            let sent = SendInput(&[input], size_of::<INPUT>() as i32);
            if sent == 0 {
                let mut point = POINT::default();
                let _ = GetCursorPos(&mut point);
                let _ = SetCursorPos(point.x + dx, point.y + dy);
            }
        }

        Ok(())
    }

    fn send_mouse_left_click() -> Result<()> {
        send_mouse_input(MOUSEEVENTF_LEFTDOWN, 0)?;
        thread::sleep(Duration::from_millis(16));
        send_mouse_input(MOUSEEVENTF_LEFTUP, 0)
    }

    fn send_mouse_left_click_backend() -> Result<()> {
        send_mouse_left_click()
    }

    #[derive(Clone, Debug, PartialEq)]
    struct GeometryRenderText {
        x: i32,
        y: i32,
        font_size: i32,
        color: [u8; 4],
        rotation_deg: f32,
        text: String,
    }

    #[derive(Clone, Debug)]
    struct ActiveGeometryPresetInstance {
        base_preset_id: u32,
        preset: crate::model::GeometryPreset,
    }

    fn geometry_label_bounds(
        x: i32,
        y: i32,
        font_size: i32,
        text: &str,
        rotation_deg: f32,
    ) -> (i32, i32, i32, i32) {
        let text_len = text.chars().count().max(1) as i32;
        let width_est = ((font_size as f32) * (text_len as f32 * 0.72 + 0.8)).ceil() as i32 + 12;
        let height_est = ((font_size as f32) * 1.45).ceil() as i32 + 12;
        let (virtual_left, virtual_top, virtual_width, virtual_height) =
            window_list::virtual_screen_bounds();
        let virtual_right = virtual_left + virtual_width;
        let virtual_bottom = virtual_top + virtual_height;
        let pad = font_size.clamp(16, 64);
        let (left, top, right, bottom) = if rotation_deg.abs() < f32::EPSILON {
            let half_w = (width_est.max(1) + 1) / 2;
            let half_h = (height_est.max(1) + 1) / 2;
            (x - half_w, y - half_h, x + half_w, y + half_h)
        } else {
            let radius = width_est.max(height_est).max(font_size).max(1);
            (x - radius, y - radius, x + radius, y + radius)
        };
        let clamped_left = left.clamp(virtual_left, virtual_right.saturating_sub(1));
        let clamped_top = top.clamp(virtual_top, virtual_bottom.saturating_sub(1));
        let clamped_right = right.clamp(clamped_left + 1, virtual_right + pad);
        let clamped_bottom = bottom.clamp(clamped_top + 1, virtual_bottom + pad);
        (clamped_left, clamped_top, clamped_right, clamped_bottom)
    }

    #[derive(Clone, Debug, PartialEq)]
    struct GeometryRenderShape {
        bounds: (i32, i32, i32, i32),
        draw: GeometryRenderDraw,
    }



    #[derive(Clone, Debug, PartialEq)]
    enum GeometryRenderDraw {
        Point {
            x: i32,
            y: i32,
            radius: i32,
            fill: [u8; 4],
        },
        Line {
            x1: i32,
            y1: i32,
            x2: i32,
            y2: i32,
            stroke: [u8; 4],
            thickness: i32,
        },
        Circle {
            cx: i32,
            cy: i32,
            radius: i32,
            stroke: [u8; 4],
            fill: Option<[u8; 4]>,
            thickness: i32,
        },
        Arrow {
            x1: i32,
            y1: i32,
            x2: i32,
            y2: i32,
            stroke: [u8; 4],
            thickness: i32,
            head_size: i32,
        },
        Polyline {
            points: Vec<(i32, i32)>,
            stroke: [u8; 4],
            thickness: i32,
        },
        Polygon {
            points: Vec<(i32, i32)>,
            stroke: [u8; 4],
            fill: Option<[u8; 4]>,
            thickness: i32,
        },
        Label(GeometryRenderText),
        Svg {
            x: i32,
            y: i32,
            width: u32,
            height: u32,
            opacity: f32,
            rotation: f32,
            code: String,
        },
    }

    fn geometry_eval_i32(expr: &str, fallback: i32) -> i32 {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return fallback;
        }
        evaluate_interpolated_math_expression(trimmed)
    }

    fn geometry_eval_f32(expr: &str, fallback: f32) -> f32 {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return fallback;
        }
        let interpolated = interpolate_variables(trimmed);
        evaluate_math_expression_f64(&interpolated) as f32
    }

    fn geometry_rotate_point(x: f32, y: f32, cx: f32, cy: f32, rotation_deg: f32) -> (i32, i32) {
        if rotation_deg.abs() < f32::EPSILON {
            return (x.round() as i32, y.round() as i32);
        }
        let rad = rotation_deg.to_radians();
        let cos = rad.cos();
        let sin = rad.sin();
        let dx = x - cx;
        let dy = y - cy;
        let rx = cx + dx * cos - dy * sin;
        let ry = cy + dx * sin + dy * cos;
        (rx.round() as i32, ry.round() as i32)
    }

    fn geometry_points_centroid(points: &[(i32, i32)]) -> Option<(f32, f32)> {
        if points.is_empty() {
            return None;
        }
        let (sum_x, sum_y) = points.iter().fold((0.0_f32, 0.0_f32), |(sx, sy), (x, y)| {
            (sx + *x as f32, sy + *y as f32)
        });
        let count = points.len() as f32;
        Some((sum_x / count, sum_y / count))
    }

    fn geometry_rotate_points(points: &[(i32, i32)], cx: f32, cy: f32, rotation_deg: f32) -> Vec<(i32, i32)> {
        points
            .iter()
            .map(|(x, y)| geometry_rotate_point(*x as f32, *y as f32, cx, cy, rotation_deg))
            .collect()
    }

    fn geometry_sample_ellipse_points(
        cx: i32,
        cy: i32,
        rx: i32,
        ry: i32,
        start_angle_deg: f32,
        end_angle_deg: f32,
        rotation_deg: f32,
        closed: bool,
    ) -> Vec<(i32, i32)> {
        let delta = (end_angle_deg - start_angle_deg).abs().max(1.0);
        let steps = ((rx.max(ry) as f32) * delta.to_radians())
            .round()
            .clamp(24.0, 480.0) as i32;
        let start = start_angle_deg;
        let span = end_angle_deg - start_angle_deg;
        let mut points = Vec::with_capacity((steps.max(1) + 1) as usize);
        for i in 0..=steps.max(1) {
            let t = i as f32 / steps.max(1) as f32;
            let angle_deg = start + span * t;
            let rad = angle_deg.to_radians();
            let px = cx as f32 + rx as f32 * rad.cos();
            let py = cy as f32 + ry as f32 * rad.sin();
            points.push(geometry_rotate_point(
                px,
                py,
                cx as f32,
                cy as f32,
                rotation_deg,
            ));
        }
        if closed && points.len() > 2 && points.first() != points.last() {
            if let Some(first) = points.first().copied() {
                points.push(first);
            }
        }
        points
    }

    fn parse_geometry_color_literal(value: &str) -> Option<[u8; 4]> {
        let trimmed = value.trim().trim_start_matches('#');
        match trimmed.len() {
            6 => {
                let rgb = u32::from_str_radix(trimmed, 16).ok()?;
                Some([
                    ((rgb >> 16) & 0xFF) as u8,
                    ((rgb >> 8) & 0xFF) as u8,
                    (rgb & 0xFF) as u8,
                    255,
                ])
            }
            8 => {
                let rgba = u32::from_str_radix(trimmed, 16).ok()?;
                Some([
                    ((rgba >> 24) & 0xFF) as u8,
                    ((rgba >> 16) & 0xFF) as u8,
                    ((rgba >> 8) & 0xFF) as u8,
                    (rgba & 0xFF) as u8,
                ])
            }
            _ => None,
        }
    }

    fn geometry_resolve_color(expr: &str, fallback: RgbaColor, opacity: f32) -> [u8; 4] {
        let mut base = [fallback.r, fallback.g, fallback.b, 255];
        let interpolated = interpolate_variables(expr.trim());
        if !interpolated.trim().is_empty() {
            if let Some(parsed) = parse_geometry_color_literal(&interpolated) {
                base = parsed;
            }
        }
        let alpha_scale = opacity.clamp(0.0, 1.0);
        let scaled_alpha = ((base[3] as f32) * alpha_scale).round().clamp(0.0, 255.0) as u8;
        [base[0], base[1], base[2], scaled_alpha]
    }

    fn geometry_parse_points(points_expr: &str) -> Vec<(i32, i32)> {
        points_expr
            .split(';')
            .filter_map(|pair| {
                let (x_expr, y_expr) = pair.split_once(',')?;
                Some((
                    geometry_eval_i32(x_expr, 0),
                    geometry_eval_i32(y_expr, 0),
                ))
            })
            .collect()
    }

    fn geometry_bounds_from_points(points: &[(i32, i32)], pad: i32) -> Option<(i32, i32, i32, i32)> {
        let first = points.first()?;
        let mut min_x = first.0;
        let mut max_x = first.0;
        let mut min_y = first.1;
        let mut max_y = first.1;
        for &(x, y) in points.iter().skip(1) {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        Some((min_x - pad, min_y - pad, max_x + pad, max_y + pad))
    }

    fn geometry_render_shape_from_spec(spec: &GeometrySpec) -> Option<GeometryRenderShape> {
        if !spec.visible {
            return None;
        }

        let thickness = geometry_eval_i32(&spec.thickness_expr, spec.thickness.round() as i32).max(1).min(50);
        let stroke_opacity = geometry_eval_f32(&spec.opacity_expr, spec.opacity).clamp(0.0, 1.0);
        let fill_opacity = geometry_eval_f32(&spec.fill_opacity_expr, spec.fill_opacity).clamp(0.0, 1.0);
        let rotation_deg = geometry_eval_f32(&spec.rotation_expr, 0.0);
        let stroke = geometry_resolve_color(&spec.stroke_color_expr, spec.stroke_color, stroke_opacity);
        let fill = geometry_resolve_color(&spec.fill_color_expr, spec.fill_color, fill_opacity);
        let fill_option = spec.filled.then_some(fill);
        match spec.shape {
            GeometryShapeKind::Point => {
                let x = geometry_eval_i32(&spec.x1_expr, 0);
                let y = geometry_eval_i32(&spec.y1_expr, 0);
                let radius = geometry_eval_i32(&spec.radius_expr, spec.point_radius.round() as i32).max(1).min(1000);
                Some(GeometryRenderShape {
                    bounds: (x - radius, y - radius, x + radius, y + radius),
                    draw: GeometryRenderDraw::Point {
                        x,
                        y,
                        radius,
                        fill: fill_option.unwrap_or(stroke),
                    },
                })
            }
            GeometryShapeKind::Line => {
                let mut x1 = geometry_eval_i32(&spec.x1_expr, 0);
                let mut y1 = geometry_eval_i32(&spec.y1_expr, 0);
                let mut x2 = geometry_eval_i32(&spec.x2_expr, 0);
                let mut y2 = geometry_eval_i32(&spec.y2_expr, 0);
                if rotation_deg.abs() >= f32::EPSILON {
                    let cx = (x1 + x2) as f32 * 0.5;
                    let cy = (y1 + y2) as f32 * 0.5;
                    (x1, y1) = geometry_rotate_point(x1 as f32, y1 as f32, cx, cy, rotation_deg);
                    (x2, y2) = geometry_rotate_point(x2 as f32, y2 as f32, cx, cy, rotation_deg);
                }
                Some(GeometryRenderShape {
                    bounds: (
                        x1.min(x2) - thickness,
                        y1.min(y2) - thickness,
                        x1.max(x2) + thickness,
                        y1.max(y2) + thickness,
                    ),
                    draw: GeometryRenderDraw::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        stroke,
                        thickness,
                    },
                })
            }
            GeometryShapeKind::Circle => {
                let cx = geometry_eval_i32(&spec.x1_expr, 0);
                let cy = geometry_eval_i32(&spec.y1_expr, 0);
                let radius = geometry_eval_i32(&spec.radius_expr, spec.point_radius.round() as i32).max(1).min(1000);
                Some(GeometryRenderShape {
                    bounds: (
                        cx - radius - thickness,
                        cy - radius - thickness,
                        cx + radius + thickness,
                        cy + radius + thickness,
                    ),
                    draw: GeometryRenderDraw::Circle {
                        cx,
                        cy,
                        radius,
                        stroke,
                        fill: fill_option,
                        thickness,
                    },
                })
            }
            GeometryShapeKind::Rectangle => {
                let x = geometry_eval_i32(&spec.x1_expr, 0);
                let y = geometry_eval_i32(&spec.y1_expr, 0);
                let width = geometry_eval_i32(&spec.width_expr, 1).max(1).min(2000);
                let height = geometry_eval_i32(&spec.height_expr, 1).max(1).min(2000);
                let cx = x as f32 + width as f32 * 0.5;
                let cy = y as f32 + height as f32 * 0.5;
                let points = geometry_rotate_points(
                    &[
                        (x, y),
                        (x + width, y),
                        (x + width, y + height),
                        (x, y + height),
                    ],
                    cx,
                    cy,
                    rotation_deg,
                );
                let bounds = geometry_bounds_from_points(&points, thickness)?;
                Some(GeometryRenderShape {
                    bounds,
                    draw: GeometryRenderDraw::Polygon {
                        points,
                        stroke,
                        fill: fill_option,
                        thickness,
                    },
                })
            }
            GeometryShapeKind::Label => {
                let x = geometry_eval_i32(&spec.x1_expr, 0);
                let y = geometry_eval_i32(&spec.y1_expr, 0);
                let font_size = geometry_eval_i32(&spec.font_size_expr, spec.font_size.round() as i32)
                    .max(10)
                    .min(256);
                let text = interpolate_variables(&spec.text);
                let bounds = geometry_label_bounds(x, y, font_size, &text, rotation_deg);
                Some(GeometryRenderShape {
                    bounds,
                    draw: GeometryRenderDraw::Label(GeometryRenderText {
                        x,
                        y,
                        font_size,
                        color: stroke,
                        rotation_deg,
                        text,
                    }),
                })
            }
            GeometryShapeKind::Ellipse => {
                let cx = geometry_eval_i32(&spec.x1_expr, 0);
                let cy = geometry_eval_i32(&spec.y1_expr, 0);
                let rx = geometry_eval_i32(&spec.radius_x_expr, 1).max(1).min(1000);
                let ry = geometry_eval_i32(&spec.radius_y_expr, 1).max(1).min(1000);
                let points = geometry_sample_ellipse_points(cx, cy, rx, ry, 0.0, 360.0, rotation_deg, true);
                let bounds = geometry_bounds_from_points(&points, thickness)?;
                Some(GeometryRenderShape {
                    bounds,
                    draw: GeometryRenderDraw::Polygon {
                        points,
                        stroke,
                        fill: fill_option,
                        thickness,
                    },
                })
            }
            GeometryShapeKind::Arrow => {
                let mut x1 = geometry_eval_i32(&spec.x1_expr, 0);
                let mut y1 = geometry_eval_i32(&spec.y1_expr, 0);
                let mut x2 = geometry_eval_i32(&spec.x2_expr, 0);
                let mut y2 = geometry_eval_i32(&spec.y2_expr, 0);
                let head_size = geometry_eval_i32(&spec.arrow_head_size_expr, spec.arrow_head_size.round() as i32).max(4).min(200);
                if rotation_deg.abs() >= f32::EPSILON {
                    let cx = (x1 + x2) as f32 * 0.5;
                    let cy = (y1 + y2) as f32 * 0.5;
                    (x1, y1) = geometry_rotate_point(x1 as f32, y1 as f32, cx, cy, rotation_deg);
                    (x2, y2) = geometry_rotate_point(x2 as f32, y2 as f32, cx, cy, rotation_deg);
                }
                Some(GeometryRenderShape {
                    bounds: (
                        x1.min(x2) - head_size - thickness,
                        y1.min(y2) - head_size - thickness,
                        x1.max(x2) + head_size + thickness,
                        y1.max(y2) + head_size + thickness,
                    ),
                    draw: GeometryRenderDraw::Arrow {
                        x1,
                        y1,
                        x2,
                        y2,
                        stroke,
                        thickness,
                        head_size,
                    },
                })
            }
            GeometryShapeKind::Polyline => {
                let mut points = geometry_parse_points(&spec.points_expr);
                if rotation_deg.abs() >= f32::EPSILON {
                    let (cx, cy) = geometry_points_centroid(&points)?;
                    points = geometry_rotate_points(&points, cx, cy, rotation_deg);
                }
                let bounds = geometry_bounds_from_points(&points, thickness)?;
                Some(GeometryRenderShape {
                    bounds,
                    draw: GeometryRenderDraw::Polyline {
                        points,
                        stroke,
                        thickness,
                    },
                })
            }
            GeometryShapeKind::Polygon => {
                let mut points = geometry_parse_points(&spec.points_expr);
                if rotation_deg.abs() >= f32::EPSILON {
                    let (cx, cy) = geometry_points_centroid(&points)?;
                    points = geometry_rotate_points(&points, cx, cy, rotation_deg);
                }
                let bounds = geometry_bounds_from_points(&points, thickness)?;
                Some(GeometryRenderShape {
                    bounds,
                    draw: GeometryRenderDraw::Polygon {
                        points,
                        stroke,
                        fill: fill_option,
                        thickness,
                    },
                })
            }
            GeometryShapeKind::Arc => {
                let cx = geometry_eval_i32(&spec.x1_expr, 0);
                let cy = geometry_eval_i32(&spec.y1_expr, 0);
                let rx = geometry_eval_i32(&spec.radius_x_expr, 1).max(1).min(1000);
                let ry = geometry_eval_i32(&spec.radius_y_expr, 1).max(1).min(1000);
                let start_angle_deg = geometry_eval_f32(&spec.start_angle_expr, 0.0);
                let end_angle_deg = geometry_eval_f32(&spec.end_angle_expr, 180.0);
                let points = geometry_sample_ellipse_points(
                    cx,
                    cy,
                    rx,
                    ry,
                    start_angle_deg,
                    end_angle_deg,
                    rotation_deg,
                    false,
                );
                let bounds = geometry_bounds_from_points(&points, thickness)?;
                Some(GeometryRenderShape {
                    bounds,
                    draw: GeometryRenderDraw::Polyline {
                        points,
                        stroke,
                        thickness,
                    },
                })
            }
            GeometryShapeKind::Svg => {
                let x = geometry_eval_i32(&spec.x1_expr, 960);
                let y = geometry_eval_i32(&spec.y1_expr, 540);
                let width = geometry_eval_i32(&spec.width_expr, 0).max(0) as u32;
                let height = geometry_eval_i32(&spec.height_expr, 0).max(0) as u32;
                let opacity = (geometry_eval_f32(&spec.opacity_expr, 100.0) / 100.0).clamp(0.0, 1.0);
                let rotation = geometry_eval_f32(&spec.rotation_expr, 0.0);
                let code = spec.text.clone();
                
                let w = if width > 0 { width as i32 } else { 1000 };
                let h = if height > 0 { height as i32 } else { 1000 };
                
                Some(GeometryRenderShape {
                    bounds: (x - w, y - h, x + w * 2, y + h * 2),
                    draw: GeometryRenderDraw::Svg {
                        x,
                        y,
                        width,
                        height,
                        opacity,
                        rotation,
                        code,
                    },
                })
            }
        }
    }



    fn geometry_overlay_static_shapes(hook_state: &mut HookState) -> Vec<GeometryRenderShape> {
        let mut shapes = Vec::new();
        let overridden_preset_ids: HashSet<u32> = hook_state
            .active_geometry_preset_instances
            .values()
            .map(|instance| instance.base_preset_id)
            .collect();
        for preset_id in &hook_state.active_geometry_preset_ids {
            if overridden_preset_ids.contains(preset_id) {
                continue;
            }
            if let Some(preset) = hook_state
                .geometry_presets
                .iter()
                .find(|preset| preset.id == *preset_id && preset.enabled)
            {
                for object in &preset.objects {
                    if let Some(shape) = geometry_render_shape_from_spec(&object.spec) {
                        shapes.push(shape);
                    }
                }
            }
        }
        for instance in hook_state.active_geometry_preset_instances.values() {
            if !instance.preset.enabled {
                continue;
            }
            for object in &instance.preset.objects {
                if let Some(shape) = geometry_render_shape_from_spec(&object.spec) {
                    shapes.push(shape);
                }
            }
        }
        if let Some(preview_preset_id) = hook_state.preview_geometry_preset_id {
            let is_active = hook_state.active_geometry_preset_ids.contains(&preview_preset_id)
                || overridden_preset_ids.contains(&preview_preset_id);
            if let Some(preset) = hook_state
                .geometry_presets
                .iter()
                .find(|preset| preset.id == preview_preset_id)
            {
                for object in &preset.objects {
                    if !is_active {
                        if let Some(shape) = geometry_render_shape_from_spec(&object.spec) {
                            shapes.push(shape);
                        }
                    }
                }
            }
        }
        shapes
    }

    fn rebuild_active_geometry_preset_ids(hook_state: &mut HookState) {
        hook_state.active_geometry_preset_ids.clear();
        hook_state
            .active_geometry_preset_ids
            .extend(hook_state.active_geometry_preset_owner_ids.values().copied());
    }

    fn remove_active_geometry_preset_owner(hook_state: &mut HookState, owner: (u32, usize)) {
        hook_state.active_geometry_preset_owner_ids.remove(&owner);
        hook_state.active_geometry_preset_owner_expires.remove(&owner);
        hook_state.active_geometry_preset_instances.remove(&owner);
        hook_state
            .active_geometry_preset_activation_order
            .retain(|active_owner| *active_owner != owner);
    }

    fn geometry_overlay_dynamic_shapes(hook_state: &mut HookState) -> Vec<GeometryRenderShape> {
        let mut shapes = Vec::new();
        for spec in hook_state.active_geometry_steps.values() {
            if let Some(shape) = geometry_render_shape_from_spec(spec) {
                shapes.push(shape);
            }
        }
        if let Some(spec) = &hook_state.preview_geometry_spec
            && let Some(shape) = geometry_render_shape_from_spec(spec)
        {
            shapes.push(shape);
        }
        shapes
    }

    fn geometry_shape_refresh_interval(shape: &GeometryRenderShape) -> Duration {
        let (left, top, right, bottom) = shape.bounds;
        let width = (right - left).max(1) as i64;
        let height = (bottom - top).max(1) as i64;
        let area = width.saturating_mul(height);
        if area > 600_000 {
            Duration::from_millis(66)
        } else if area > 300_000 {
            Duration::from_millis(50)
        } else if area > 120_000 {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(16)
        }
    }

    fn geometry_shape_motion_threshold(shape: &GeometryRenderShape) -> i32 {
        match &shape.draw {
            GeometryRenderDraw::Line { x1, y1, x2, y2, .. }
            | GeometryRenderDraw::Arrow { x1, y1, x2, y2, .. } => {
                let dx = (*x2 - *x1) as f32;
                let dy = (*y2 - *y1) as f32;
                let length = (dx * dx + dy * dy).sqrt();
                if length > 900.0 {
                    10
                } else if length > 600.0 {
                    8
                } else if length > 350.0 {
                    6
                } else if length > 180.0 {
                    4
                } else {
                    2
                }
            }
            _ => 2,
        }
    }

    fn geometry_shape_motion_delta(previous: &GeometryRenderShape, current: &GeometryRenderShape) -> i32 {
        match (&previous.draw, &current.draw) {
            (
                GeometryRenderDraw::Line { x1: px1, y1: py1, x2: px2, y2: py2, .. },
                GeometryRenderDraw::Line { x1: cx1, y1: cy1, x2: cx2, y2: cy2, .. },
            )
            | (
                GeometryRenderDraw::Arrow { x1: px1, y1: py1, x2: px2, y2: py2, .. },
                GeometryRenderDraw::Arrow { x1: cx1, y1: cy1, x2: cx2, y2: cy2, .. },
            ) => [
                (cx1 - px1).abs(),
                (cy1 - py1).abs(),
                (cx2 - px2).abs(),
                (cy2 - py2).abs(),
            ]
            .into_iter()
            .max()
            .unwrap_or(0),
            _ => {
                let (pl, pt, pr, pb) = previous.bounds;
                let (cl, ct, cr, cb) = current.bounds;
                [
                    (cl - pl).abs(),
                    (ct - pt).abs(),
                    (cr - pr).abs(),
                    (cb - pb).abs(),
                ]
                .into_iter()
                .max()
                .unwrap_or(0)
            }
        }
    }













    fn find_color_match_in_range(
        screen: &window_list::ScreenCaptureFrame,
        targets: &[RgbaColor],
        tolerance: u8,
        x_start: usize,
        x_end: usize,
        region: Option<&VisionRegion>,
    ) -> Option<ColorMatchHit> {
        let width = screen.width as i32;
        let height = screen.height as i32;
        if width <= 0 || height <= 0 || targets.is_empty() {
            return None;
        }

        let x_start = x_start.min(screen.width);
        let x_end = x_end.min(screen.width);
        if x_start >= x_end {
            return None;
        }

        let center_x = width / 2;
        let center_y = height / 2;
        let tolerance = tolerance as i16;
        let mut best_hit: Option<ColorMatchHit> = None;
        for y in 0..height {
            for x in x_start as i32..x_end as i32 {
                let candidate = color_match_candidate_for_pixel(
                    screen, targets, tolerance, x, y, center_x, center_y, region,
                );
                if let Some(candidate) = candidate {
                    let replace = match best_hit {
                        None => true,
                        Some(current) if candidate.score < current.score => true,
                        Some(current) if candidate.score == current.score => {
                            candidate.distance_sq < current.distance_sq
                        }

                        _ => false,
                    };
                    if replace {
                        best_hit = Some(candidate);
                    }
                }
            }
        }

        best_hit
    }


    fn find_color_match_from_anchor(
        screen: &window_list::ScreenCaptureFrame,
        targets: &[RgbaColor],
        tolerance: u8,
        anchor_x: i32,
        anchor_y: i32,
        region: Option<&VisionRegion>,
    ) -> Option<ColorMatchHit> {
        let width = screen.width as i32;
        let height = screen.height as i32;
        if width <= 0 || height <= 0 || targets.is_empty() {
            return None;
        }

        if anchor_x < 0 || anchor_y < 0 || anchor_x >= width || anchor_y >= height {
            return None;
        }

        let tolerance = tolerance as i16;
        let max_radius = (anchor_x)
            .max(width - 1 - anchor_x)
            .max(anchor_y)
            .max(height - 1 - anchor_y);
        for radius in 0..=max_radius {
            let left = (anchor_x - radius).max(0);
            let right = (anchor_x + radius).min(width - 1);
            let top = (anchor_y - radius).max(0);
            let bottom = (anchor_y + radius).min(height - 1);
            let mut best_in_radius: Option<ColorMatchHit> = None;
            for x in left..=right {
                for y in [top, bottom] {
                    if let Some(candidate) = color_match_candidate_for_pixel(
                        screen, targets, tolerance, x, y, anchor_x, anchor_y, region,
                    ) {
                        let replace = match best_in_radius {
                            None => true,
                            Some(current) if candidate.score < current.score => true,
                            Some(current) if candidate.score == current.score => {
                                candidate.distance_sq < current.distance_sq
                            }

                            _ => false,
                        };
                        if replace {
                            best_in_radius = Some(candidate);
                        }
                    }
                }
            }

            if top + 1 <= bottom.saturating_sub(1) {
                for y in (top + 1)..bottom {
                    for x in [left, right] {
                        if let Some(candidate) = color_match_candidate_for_pixel(
                            screen, targets, tolerance, x, y, anchor_x, anchor_y, region,
                        ) {
                            let replace = match best_in_radius {
                                None => true,
                                Some(current) if candidate.score < current.score => true,
                                Some(current) if candidate.score == current.score => {
                                    candidate.distance_sq < current.distance_sq
                                }

                                _ => false,
                            };
                            if replace {
                                best_in_radius = Some(candidate);
                            }
                        }
                    }
                }
            }

            if best_in_radius.is_some() {
                return best_in_radius;
            }
        }

        None
    }

    fn find_color_match(
        screen: &window_list::ScreenCaptureFrame,
        targets: &[RgbaColor],
        tolerance: u8,
        region: Option<&VisionRegion>,
    ) -> Option<ColorMatchHit> {
        find_color_match_in_range(screen, targets, tolerance, 0, screen.width, region)
    }

    fn find_dual_color_midpoint_match(
        screen: &window_list::ScreenCaptureFrame,
        targets: &[RgbaColor],
        tolerance: u8,
        region: Option<&VisionRegion>,
    ) -> Option<ColorMatchHit> {
        let mid = (screen.width / 2).max(1);
        let (left_hit, right_hit) = thread::scope(|scope| {
            let left = scope
                .spawn(|| find_color_match_in_range(screen, targets, tolerance, 0, mid, region));
            let right = scope.spawn(|| {
                find_color_match_in_range(screen, targets, tolerance, mid, screen.width, region)
            });
            (left.join().ok().flatten(), right.join().ok().flatten())
        });
        match (left_hit, right_hit) {
            (Some(left), Some(right)) => Some(ColorMatchHit {
                x: ((left.x + right.x) / 2).max(0),
                y: ((left.y + right.y) / 2).max(0),
                score: left.score.min(right.score),
                distance_sq: left.distance_sq.min(right.distance_sq),
                matched_color: left.matched_color,
            }),
            (Some(hit), None) | (None, Some(hit)) => Some(hit),
            (None, None) => None,
        }
    }

    fn resolve_variables_in_text(text: &str) -> String {
        let mut result = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '{' {
                let mut var_name = String::new();
                let mut found_close = false;
                while let Some(&next_ch) = chars.peek() {
                    if next_ch == '}' {
                        chars.next(); // consume '}'

                        found_close = true;
                        break;
                    } else {
                        var_name.push(chars.next().unwrap());
                    }
                }

                if found_close {
                    let trimmed = var_name.trim();
                    if let Some(text_val) = resolve_text_variable_value(trimmed) {
                        result.push_str(&text_val);
                    } else {
                        result.push_str("0");
                    }
                } else {
                    result.push('{');
                    result.push_str(&var_name);
                }
            } else {
                result.push(ch);
            }
        }

        result
    }



























    fn stop_vision_waiting(spec: &str) -> Result<()> {
        let preset = vision_preset_by_id(spec)?;
        bump_image_search_wait_generation(preset.id);
        Ok(())
    }

    fn is_extended_key(vk: u32) -> bool {
        matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C)
    }

    fn internal_app_window_class(hwnd: HWND) -> Option<String> {
        unsafe {
            let mut buffer = [0u16; 256];
            let copied = GetClassNameW(hwnd, &mut buffer);
            if copied <= 0 {
                return None;
            }

            Some(String::from_utf16_lossy(&buffer[..copied as usize]))
        }
    }

    fn is_internal_app_window(hwnd: HWND) -> bool {
        internal_app_window_class(hwnd).is_some_and(|class_name| {
            matches!(
                class_name.as_str(),
                "CrosshairController" | "CrosshairOverlay" | "CrosshairToolbox" | "Magnifier"
            )
        })
    }

    fn window_belongs_to_current_process(hwnd: HWND) -> bool {
        unsafe {
            let mut pid = 0u32;
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
            pid != 0 && pid == GetCurrentProcessId()
        }
    }

    fn looks_like_main_ui_window(hwnd: HWND) -> bool {
        unsafe {
            if hwnd.0.is_null()
                || !window_belongs_to_current_process(hwnd)
                || is_internal_app_window(hwnd)
            {
                return false;
            }

            if GetAncestor(hwnd, GA_ROOT) != hwnd {
                return false;
            }

            if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.0.is_null()) {
                return false;
            }

            let style = windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
            ) as u32;
            (style & WS_OVERLAPPEDWINDOW.0) != 0 || (style & WS_CAPTION.0) != 0
        }
    }

    #[derive(Default)]
    struct AppUiWindowSearch {
        visible: Option<HWND>,
        hidden: Option<HWND>,
    }

    unsafe fn find_app_ui_window() -> Option<HWND> {
        let cached = CACHED_APP_UI_HWND.load(Ordering::Relaxed);
        if cached != 0 {
            let hwnd = HWND(cached as *mut std::ffi::c_void);
            if windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() {
                return Some(hwnd);
            }
        }

        let mut found = AppUiWindowSearch::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(find_app_ui_window_proc),
            LPARAM((&mut found) as *mut _ as isize),
        );
        let res = found.visible.or(found.hidden);
        if let Some(hwnd) = res {
            CACHED_APP_UI_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        }

        res
    }

    unsafe extern "system" fn find_app_ui_window_proc(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> windows::core::BOOL {
        let found = &mut *(lparam.0 as *mut AppUiWindowSearch);
        if !looks_like_main_ui_window(hwnd) {
            return true.into();
        }

        if windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool() {
            found.visible = Some(hwnd);
            false.into()
        } else {
            if found.hidden.is_none() {
                found.hidden = Some(hwnd);
            }

            true.into()
        }
    }

    fn is_ui_in_foreground() -> bool {
        UI_WINDOW_FOREGROUND.load(Ordering::Relaxed)
    }

    pub fn find_app_ui_window_for_ui_thread() -> Option<windows::Win32::Foundation::HWND> {
        unsafe { find_app_ui_window() }
    }

    pub fn update_ui_window_metrics(
        visible: bool,
        is_foreground: bool,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    ) {
        UI_WINDOW_VISIBLE.store(visible, Ordering::Relaxed);
        UI_WINDOW_FOREGROUND.store(is_foreground, Ordering::Relaxed);
        if visible {
            UI_WINDOW_RECT_LEFT.store(left, Ordering::Relaxed);
            UI_WINDOW_RECT_TOP.store(top, Ordering::Relaxed);
            UI_WINDOW_RECT_RIGHT.store(right, Ordering::Relaxed);
            UI_WINDOW_RECT_BOTTOM.store(bottom, Ordering::Relaxed);
        }
    }

    fn schedule_window_focus_trigger(hwnd: HWND) {
        let mut hook_state = HOOK_STATE.lock();
        if hwnd.0.is_null() {
            hook_state.pending_window_focus_trigger = None;
            hook_state.pending_window_focus_stable_polls = 0;
            return;
        }

        let hwnd_value = hwnd.0 as isize;
        if hook_state.pending_window_focus_trigger == Some(hwnd_value) {
            return;
        }

        hook_state.pending_window_focus_trigger = Some(hwnd_value);
        hook_state.pending_window_focus_stable_polls = 0;
    }

    fn clear_pending_window_focus_trigger() {
        let mut hook_state = HOOK_STATE.lock();
        hook_state.pending_window_focus_trigger = None;
        hook_state.pending_window_focus_stable_polls = 0;
    }

    fn reset_window_focus_dispatch_guard() {
        let mut hook_state = HOOK_STATE.lock();
        hook_state.pending_window_focus_trigger = None;
        hook_state.pending_window_focus_stable_polls = 0;
        hook_state.last_dispatched_window_focus_hwnd = None;
    }

    fn normalize_focus_window(hwnd: HWND) -> HWND {
        if hwnd.0.is_null() {
            return hwnd;
        }

        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        if root.0.is_null() { hwnd } else { root }
    }

    fn handle_window_focus_event(controller_hwnd: HWND, hwnd: HWND) {
        let hwnd = normalize_focus_window(hwnd);
        if !update_foreground_window(hwnd) {
            return;
        }

        {
            let mut hook_state = HOOK_STATE.lock();
            if hook_state.last_dispatched_window_focus_hwnd != Some(hwnd.0 as isize) {
                hook_state.last_dispatched_window_focus_hwnd = None;
            }
        }

        let is_candidate = is_focus_trigger_candidate_window(hwnd);
        if is_candidate {
            schedule_window_focus_trigger(hwnd);
            if process_pending_window_focus_trigger() {
                unsafe {
                    let _ = SetTimer(Some(controller_hwnd), FOCUS_TRIGGER_TIMER_ID, 10, None);
                }
            }
        } else {
            reset_window_focus_dispatch_guard();
            unsafe {
                let _ = KillTimer(Some(controller_hwnd), FOCUS_TRIGGER_TIMER_ID);
            }
        }
    }

    fn has_pending_window_focus_trigger() -> bool {
        HOOK_STATE.lock().pending_window_focus_trigger.is_some()
    }

    fn process_pending_window_focus_trigger() -> bool {
        let pending = {
            let hook_state = HOOK_STATE.lock();
            hook_state.pending_window_focus_trigger
        };
        let Some(pending) = pending else {
            return false;
        };

        let current_hwnd = FOREGROUND_WINDOW_HWND.load(Ordering::Relaxed);
        if current_hwnd != pending {
            clear_pending_window_focus_trigger();
            return false;
        }

        {
            let mut hook_state = HOOK_STATE.lock();
            hook_state.pending_window_focus_stable_polls =
                hook_state.pending_window_focus_stable_polls.saturating_add(1);
            if hook_state.pending_window_focus_stable_polls < 2 {
                return true;
            }
            if hook_state.pending_window_focus_trigger == Some(pending) {
                hook_state.pending_window_focus_trigger = None;
                hook_state.pending_window_focus_stable_polls = 0;
            }
            if hook_state.last_dispatched_window_focus_hwnd == Some(pending) {
                return false;
            }
            hook_state.last_dispatched_window_focus_hwnd = Some(pending);
        }

        trigger_macros_on_window_focus_change();
        has_pending_window_focus_trigger()
    }

    fn trigger_macros_on_window_focus_change() {
        let matches = {
            let hook_state = HOOK_STATE.lock();
            if !hook_state.macros_master_enabled {
                return;
            }

            let mut matches = Vec::new();
            for group in &hook_state.macro_groups {
                if !group.enabled || !macro_target_matches(group) {
                    continue;
                }

                for preset in &group.presets {
                    if !preset.enabled || preset.trigger_mode != MacroTriggerMode::WindowFocus {
                        continue;
                    }

                    if !window_focus_matches(
                        preset.event_target_window_title.as_deref(),
                        &preset.event_extra_target_window_titles,
                        preset.event_match_duplicate_window_titles,
                    ) {
                        continue;
                    }

                    matches.push((
                        preset.clone(),
                        group.target_window_title.clone(),
                        group.extra_target_window_titles.clone(),
                        group.match_duplicate_window_titles,
                    ));
                }
            }

            matches
        };

        for (preset, target_window_title, extra_target_window_titles, match_duplicate_window_titles) in
            matches
        {
            let hotkey_id = MACRO_PRESET_BASE_ID + preset.id as i32;
            if !SUPPRESSED_MACRO_HOTKEYS.lock().contains(&hotkey_id) {
                let _ = play_macro_preset(
                    hotkey_id,
                    preset,
                    target_window_title,
                    extra_target_window_titles,
                    match_duplicate_window_titles,
                    "WindowFocus".to_owned(),
                );
            }
        }
    }

    pub fn update_foreground_window(hwnd: HWND) -> bool {
        let current_hwnd = FOREGROUND_WINDOW_HWND.load(Ordering::Relaxed);
        if hwnd.0 as isize != current_hwnd {
            FOREGROUND_WINDOW_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
            let title = if hwnd.0.is_null() {
                None
            } else {
                unsafe { window_title(hwnd) }
            };
            let mut guard = FOREGROUND_WINDOW_TITLE.lock();
            *guard = title;
            true
        } else {
            false
        }
    }

    fn hide_ui_window_native() {
        unsafe {
            let Some(app) = find_app_ui_window() else {
                return;
            };
            if app.0.is_null() {
                return;
            }

            let _ = ShowWindow(app, SW_HIDE);
        }
    }

    fn show_ui_window_native() {
        unsafe {
            let Some(app) = find_app_ui_window() else {
                return;
            };
            if app.0.is_null() {
                return;
            }

            let _ = ShowWindow(app, SW_SHOWNA);
        }
    }

    fn restore_ui_window_native() {
        unsafe {
            let Some(app) = find_app_ui_window() else {
                return;
            };
            if app.0.is_null() {
                return;
            }

            let _ = ShowWindow(app, SW_SHOWNA);
        }
    }

    fn apply_window_preset_for_macro(preset: &WindowPreset) -> Result<()> {
        window_preset::apply_window_preset_for_macro(preset)
    }

    fn apply_window_preset(preset: &WindowPreset) -> Result<()> {
        window_preset::apply_window_preset(preset)
    }

    fn apply_window_preset_impl(preset: &WindowPreset, require_enabled: bool) -> Result<()> {
        if require_enabled && !preset.enabled {
            return Ok(());
        }

        unsafe {
            let target = resolve_window_target(
                preset.target_window_title.as_deref(),
                &preset.extra_target_window_titles,
                preset.match_duplicate_window_titles,
                false,
            );
            if target.0.is_null() {
                bail!("No foreground window is available");
            }

            let target_root = GetAncestor(target, GA_ROOT);
            if !target_root.0.is_null()
                && window_belongs_to_current_process(target_root)
                && !is_internal_app_window(target_root)
            {
                return Ok(());
            }

            let _ = ShowWindow(target, SW_RESTORE);
            if preset.remove_title_bar {
                let _ = remove_window_title_bar(target);
            } else {
                let _ = restore_window_title_bar(target);
            }

            let bounds = calculate_window_bounds(target, preset)?;
            let _ = SetWindowPos(
                target,
                None,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                windows::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED
                    | SWP_NOACTIVATE
                    | SWP_NOZORDER,
            );
        }

        Ok(())
    }

    fn apply_window_preset_animated(preset: &WindowPreset) -> Result<()> {
        window_preset::apply_window_preset_animated(preset)
    }

    fn restore_window_title_bar_for_preset(preset: &WindowPreset) -> Result<()> {
        window_preset::restore_window_title_bar_for_preset(preset)
    }

    #[allow(dead_code)]
    fn expand_window_edge(direction: WindowExpandDirection, amount_px: i32) -> Result<()> {
        unsafe {
            let target = resolve_window_target(None, &[], false, false);
            if target.0.is_null() {
                bail!("No foreground window is available");
            }

            let target_root = GetAncestor(target, GA_ROOT);
            if !target_root.0.is_null()
                && window_belongs_to_current_process(target_root)
                && !is_internal_app_window(target_root)
            {
                return Ok(());
            }

            ensure_window_restored(target);
            let mut rect = RECT::default();
            GetWindowRect(target, &mut rect)?;
            match direction {
                WindowExpandDirection::Up => rect.top -= amount_px,
                WindowExpandDirection::Down => rect.bottom += amount_px,
                WindowExpandDirection::Left => rect.left -= amount_px,
                WindowExpandDirection::Right => rect.right += amount_px,
            }

            let _ = SetWindowPos(
                target,
                None,
                rect.left,
                rect.top,
                (rect.right - rect.left).max(1),
                (rect.bottom - rect.top).max(1),
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }

        Ok(())
    }

    fn animate_window_rect(target: HWND, start: RECT, end: RECT, duration_ms: u64) -> Result<()> {
        let start_width = (start.right - start.left).max(1);
        let start_height = (start.bottom - start.top).max(1);
        let end_width = (end.right - end.left).max(1);
        let end_height = (end.bottom - end.top).max(1);
        let resizing = start_width != end_width || start_height != end_height;
        let duration = Duration::from_millis(duration_ms.max(if resizing { 160 } else { 120 }));
        let frame_sleep = if resizing {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(8)
        };
        let start_time = Instant::now();
        let mut last_rect = start;
        loop {
            let elapsed = start_time.elapsed();
            let t = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
            let eased = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
            let left = lerp_i32(start.left, end.left, eased);
            let top = lerp_i32(start.top, end.top, eased);
            let right = lerp_i32(start.right, end.right, eased);
            let bottom = lerp_i32(start.bottom, end.bottom, eased);
            let next_rect = RECT {
                left,
                top,
                right,
                bottom,
            };
            if next_rect.left == last_rect.left
                && next_rect.top == last_rect.top
                && next_rect.right == last_rect.right
                && next_rect.bottom == last_rect.bottom
                && t < 1.0
            {
                thread::sleep(frame_sleep);
                continue;
            }

            unsafe {
                let _ = SetWindowPos(
                    target,
                    None,
                    left,
                    top,
                    (right - left).max(1),
                    (bottom - top).max(1),
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }

            last_rect = next_rect;
            if t >= 1.0 {
                break;
            }

            thread::sleep(frame_sleep);
        }

        Ok(())
    }

    fn lerp_i32(start: i32, end: i32, t: f32) -> i32 {
        start + ((end - start) as f32 * t).round() as i32
    }

    fn remove_window_title_bar(target: HWND) -> Result<()> {
        unsafe {
            let style = windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(
                target,
                windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
            ) as u32;
            let caption = windows::Win32::UI::WindowsAndMessaging::WS_CAPTION.0;
            let thickframe = windows::Win32::UI::WindowsAndMessaging::WS_THICKFRAME.0;
            let new_style = style & !caption & !thickframe;
            if new_style != style {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowLongW(
                    target,
                    windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
                    new_style as i32,
                );
                let _ = SetWindowPos(
                    target,
                    None,
                    0,
                    0,
                    0,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED
                        | SWP_NOACTIVATE
                        | SWP_NOZORDER
                        | SWP_NOMOVE
                        | SWP_NOSIZE,
                );
            }
        }

        Ok(())
    }

    fn restore_window_title_bar(target: HWND) -> Result<()> {
        unsafe {
            let style = windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(
                target,
                windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
            ) as u32;
            let new_style = style | WS_OVERLAPPEDWINDOW.0;
            if new_style != style {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowLongW(
                    target,
                    windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
                    new_style as i32,
                );
            }

            let mut rect = RECT::default();
            GetWindowRect(target, &mut rect)?;
            let _ = SetWindowPos(
                target,
                None,
                rect.left,
                rect.top,
                (rect.right - rect.left).max(1),
                (rect.bottom - rect.top).max(1),
                windows::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED
                    | SWP_NOACTIVATE
                    | SWP_NOZORDER,
            );
        }

        Ok(())
    }

    fn ensure_window_restored(target: HWND) {
        unsafe {
            if IsZoomed(target).as_bool() {
                let _ = ShowWindow(target, SW_RESTORE);
                for _ in 0..18 {
                    if !IsZoomed(target).as_bool() {
                        break;
                    }

                    thread::sleep(Duration::from_millis(10));
                }
            } else {
                let _ = ShowWindow(target, SW_RESTORE);
            }
        }
    }

    fn wait_for_window_frame_to_settle(target: HWND) {
        unsafe {
            let mut previous = RECT::default();
            let _ = GetWindowRect(target, &mut previous);
            for _ in 0..8 {
                thread::sleep(Duration::from_millis(12));
                let mut current = RECT::default();
                if GetWindowRect(target, &mut current).is_ok()
                    && current.left == previous.left
                    && current.top == previous.top
                    && current.right == previous.right
                    && current.bottom == previous.bottom
                {
                    break;
                }

                previous = current;
            }
        }
    }

    fn calculate_window_bounds(hwnd: HWND, preset: &WindowPreset) -> Result<RECT> {
        unsafe {
            let mut window_rect = RECT::default();
            GetWindowRect(hwnd, &mut window_rect)?;
            let mut client_rect = RECT::default();
            GetClientRect(hwnd, &mut client_rect)?;
            let frame_extra_width =
                (window_rect.right - window_rect.left) - (client_rect.right - client_rect.left);
            let frame_extra_height =
                (window_rect.bottom - window_rect.top) - (client_rect.bottom - client_rect.top);
            let mut frame_rect = RECT::default();
            let frame_result = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut frame_rect as *mut _ as *mut c_void,
                size_of::<RECT>() as u32,
            );
            let (left_invisible, top_invisible) = if frame_result.is_ok() {
                (
                    frame_rect.left - window_rect.left,
                    frame_rect.top - window_rect.top,
                )
            } else {
                (0, 0)
            };
            let (right_invisible, bottom_invisible) = if frame_result.is_ok() {
                (
                    window_rect.right - frame_rect.right,
                    window_rect.bottom - frame_rect.bottom,
                )
            } else {
                (0, 0)
            };
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut monitor_info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let monitor_rect = if GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
                monitor_info.rcMonitor
            } else {
                RECT {
                    left: 0,
                    top: 0,
                    right: GetSystemMetrics(SM_CXSCREEN),
                    bottom: GetSystemMetrics(SM_CYSCREEN),
                }
            };
            let screen_width = monitor_rect.right - monitor_rect.left;
            let screen_height = monitor_rect.bottom - monitor_rect.top;
            let client_width = preset.width.max(1);
            let client_height = preset.height.max(1);
            let outer_width = client_width + frame_extra_width;
            let outer_height = client_height + frame_extra_height;
            let visible_width = (outer_width - left_invisible - right_invisible).max(1);
            let visible_height = (outer_height - top_invisible - bottom_invisible).max(1);
            let (target_x, target_y) = match preset.anchor {
                WindowAnchor::Manual => (preset.x, preset.y),
                WindowAnchor::Center => (
                    monitor_rect.left + ((screen_width - visible_width) / 2),
                    monitor_rect.top + ((screen_height - visible_height) / 2),
                ),
                WindowAnchor::TopLeft => (monitor_rect.left, monitor_rect.top),
                WindowAnchor::Top => (
                    monitor_rect.left + ((screen_width - visible_width) / 2),
                    monitor_rect.top,
                ),
                WindowAnchor::TopRight => (
                    monitor_rect.left + (screen_width - visible_width),
                    monitor_rect.top,
                ),
                WindowAnchor::Left => (
                    monitor_rect.left,
                    monitor_rect.top + ((screen_height - visible_height) / 2),
                ),
                WindowAnchor::Right => (
                    monitor_rect.left + (screen_width - visible_width),
                    monitor_rect.top + ((screen_height - visible_height) / 2),
                ),
                WindowAnchor::BottomLeft => (
                    monitor_rect.left,
                    monitor_rect.top + (screen_height - visible_height),
                ),
                WindowAnchor::Bottom => (
                    monitor_rect.left + ((screen_width - visible_width) / 2),
                    monitor_rect.top + (screen_height - visible_height),
                ),
                WindowAnchor::BottomRight => (
                    monitor_rect.left + (screen_width - visible_width),
                    monitor_rect.top + (screen_height - visible_height),
                ),
            };
            let left = target_x - left_invisible;
            let top = target_y - top_invisible;
            Ok(RECT {
                left,
                top,
                right: left + client_width + frame_extra_width,
                bottom: top + client_height + frame_extra_height,
            })
        }
    }

    fn macro_target_matches(group: &MacroGroup) -> bool {
        if group.target_window_title.is_none() && group.extra_target_window_titles.is_empty() {
            return true;
        }

        let foreground =
            HWND(FOREGROUND_WINDOW_HWND.load(Ordering::Relaxed) as *mut std::ffi::c_void);
        if foreground.0.is_null() {
            return false;
        }

        let title_guard = FOREGROUND_WINDOW_TITLE.lock();
        let Some(ref title) = *title_guard else {
            return false;
        };
        if let Some(target) = group.target_window_title.as_deref() {
            if title == target || format!("{title} (0x{:X})", foreground.0 as usize) == target {
                return true;
            }

            let base_title = selector_base_title(target);
            if base_title != target && title == base_title {
                return true;
            }

            if group.match_duplicate_window_titles && title == selector_base_title(target) {
                return true;
            }

            if matches_browser_suffix(target, title) {
                return true;
            }
        }

        group.extra_target_window_titles.iter().any(|target| {
            let target_str = target.as_str();
            if title.as_str() == target_str
                || format!("{title} (0x{:X})", foreground.0 as usize) == target_str
            {
                return true;
            }

            let base_title = selector_base_title(target_str);
            if base_title != target_str && title.as_str() == base_title {
                return true;
            }

            if group.match_duplicate_window_titles
                && title.as_str() == selector_base_title(target_str)
            {
                return true;
            }

            if matches_browser_suffix(target_str, title) {
                return true;
            }

            false
        })
    }

    fn activate_geometry_preset_owner(
        owner: (u32, usize),
        preset_id: u32,
        instance: Option<crate::model::GeometryPreset>,
        duration_ms: u64,
    ) {
        {
            let mut hook_state = HOOK_STATE.lock();
            hook_state
                .active_geometry_preset_activation_order
                .retain(|active_owner| *active_owner != owner);
            hook_state.active_geometry_preset_activation_order.push(owner);
            hook_state.active_geometry_preset_owner_ids.insert(owner, preset_id);
            if duration_ms > 0 {
                hook_state.active_geometry_preset_owner_expires.insert(
                    owner,
                    Instant::now() + Duration::from_millis(duration_ms),
                );
            } else {
                hook_state.active_geometry_preset_owner_expires.remove(&owner);
            }
            if let Some(preset) = instance {
                hook_state.active_geometry_preset_instances.insert(
                    owner,
                    ActiveGeometryPresetInstance {
                        base_preset_id: preset_id,
                        preset,
                    },
                );
            } else {
                hook_state.active_geometry_preset_instances.remove(&owner);
            }
            rebuild_active_geometry_preset_ids(&mut hook_state);
        }
        send_overlay_command(OverlayCommand::RefreshSearchAreaOverlay);
    }

    fn apply_geometry_spec_overrides(target: &mut GeometrySpec, source: &GeometrySpec, step: &MacroStep) {
        if !source.x1_expr.trim().is_empty() {
            target.x1_expr = source.x1_expr.clone();
        }
        if !source.y1_expr.trim().is_empty() {
            target.y1_expr = source.y1_expr.clone();
        }
        if !source.x2_expr.trim().is_empty() {
            target.x2_expr = source.x2_expr.clone();
        }
        if !source.y2_expr.trim().is_empty() {
            target.y2_expr = source.y2_expr.clone();
        }
        if !source.x3_expr.trim().is_empty() {
            target.x3_expr = source.x3_expr.clone();
        }
        if !source.y3_expr.trim().is_empty() {
            target.y3_expr = source.y3_expr.clone();
        }
        if !source.x4_expr.trim().is_empty() {
            target.x4_expr = source.x4_expr.clone();
        }
        if !source.y4_expr.trim().is_empty() {
            target.y4_expr = source.y4_expr.clone();
        }
        if !source.width_expr.trim().is_empty() {
            target.width_expr = source.width_expr.clone();
        }
        if !source.height_expr.trim().is_empty() {
            target.height_expr = source.height_expr.clone();
        }
        if !source.radius_expr.trim().is_empty() {
            target.radius_expr = source.radius_expr.clone();
        }
        if !source.radius_x_expr.trim().is_empty() {
            target.radius_x_expr = source.radius_x_expr.clone();
        }
        if !source.radius_y_expr.trim().is_empty() {
            target.radius_y_expr = source.radius_y_expr.clone();
        }
        if !source.start_angle_expr.trim().is_empty() {
            target.start_angle_expr = source.start_angle_expr.clone();
        }
        if !source.end_angle_expr.trim().is_empty() {
            target.end_angle_expr = source.end_angle_expr.clone();
        }
        if !source.rotation_expr.trim().is_empty() {
            target.rotation_expr = source.rotation_expr.clone();
        }
        if !source.arrow_head_size_expr.trim().is_empty() {
            target.arrow_head_size_expr = source.arrow_head_size_expr.clone();
        }
        if !source.font_size_expr.trim().is_empty() {
            target.font_size_expr = source.font_size_expr.clone();
        }
        if !source.thickness_expr.trim().is_empty() {
            target.thickness_expr = source.thickness_expr.clone();
        }
        if !source.opacity_expr.trim().is_empty() {
            target.opacity_expr = source.opacity_expr.clone();
        }
        if !source.fill_opacity_expr.trim().is_empty() {
            target.fill_opacity_expr = source.fill_opacity_expr.clone();
        }
        if !source.points_expr.trim().is_empty() {
            target.points_expr = source.points_expr.clone();
        }
        if !source.text.is_empty() {
            target.text = source.text.clone();
        }
        let style_override_requested = !source.stroke_color_expr.trim().is_empty()
            || !source.fill_color_expr.trim().is_empty()
            || !source.thickness_expr.trim().is_empty()
            || !source.opacity_expr.trim().is_empty()
            || !source.fill_opacity_expr.trim().is_empty()
            || source.filled;

        if !source.stroke_color_expr.trim().is_empty() {
            target.stroke_color_expr = source.stroke_color_expr.clone();
            target.stroke_color = source.stroke_color;
        }
        if !source.fill_color_expr.trim().is_empty() {
            target.fill_color_expr = source.fill_color_expr.clone();
            target.fill_color = source.fill_color;
        }
        if style_override_requested {
            target.filled = source.filled;
        }
    }

    fn build_geometry_preset_instance_from_step(
        base_preset: &crate::model::GeometryPreset,
        step: &MacroStep,
    ) -> crate::model::GeometryPreset {
        let mut preset = base_preset.clone();
        if !step.geometry_preset_modify_enabled {
            return preset;
        }

        for object in &mut preset.objects {
            apply_geometry_spec_overrides(&mut object.spec, &step.geometry_spec, step);
        }
        preset
    }

    fn hide_geometry_preset_by_id(preset_id: u32, hide_mode: crate::model::HideGeometryMode) {
        {
            let mut hook_state = HOOK_STATE.lock();
            if hide_mode == crate::model::HideGeometryMode::AllShown {
                hook_state
                    .active_geometry_preset_owner_ids
                    .retain(|_, active_id| *active_id != preset_id);
                let remaining_owner_keys = hook_state
                    .active_geometry_preset_owner_ids
                    .keys()
                    .copied()
                    .collect::<HashSet<_>>();
                hook_state
                    .active_geometry_preset_owner_expires
                    .retain(|owner, _| remaining_owner_keys.contains(owner));
                hook_state
                    .active_geometry_preset_instances
                    .retain(|_, instance| instance.base_preset_id != preset_id);
                hook_state
                    .active_geometry_preset_activation_order
                    .retain(|owner| remaining_owner_keys.contains(owner));
            } else {
                let owner = match hide_mode {
                    crate::model::HideGeometryMode::Newest => hook_state
                        .active_geometry_preset_activation_order
                        .iter()
                        .rev()
                        .copied()
                        .find(|owner| {
                            hook_state
                                .active_geometry_preset_owner_ids
                                .get(owner)
                                .is_some_and(|active_id| *active_id == preset_id)
                        }),
                    crate::model::HideGeometryMode::Oldest => hook_state
                        .active_geometry_preset_activation_order
                        .iter()
                        .copied()
                        .find(|owner| {
                            hook_state
                                .active_geometry_preset_owner_ids
                                .get(owner)
                                .is_some_and(|active_id| *active_id == preset_id)
                        }),
                    crate::model::HideGeometryMode::AllShown => None,
                };
                if let Some(owner) = owner {
                    remove_active_geometry_preset_owner(&mut hook_state, owner);
                }
            }
            rebuild_active_geometry_preset_ids(&mut hook_state);
        }
        send_overlay_command(OverlayCommand::RefreshSearchAreaOverlay);
    }

    fn resolve_geometry_preset_id_from_step(step: &MacroStep) -> Option<u32> {
        if !step.geometry_preset_use_custom_ref {
            if step.geometry_preset_id.is_some() {
                return step.geometry_preset_id;
            }
            if step.key.trim().is_empty() {
                return None;
            }
        }

        let spec = interpolate_variables(step.key.trim());
        let spec = spec.trim();
        if spec.is_empty() {
            return step.geometry_preset_id;
        }

        let hook_state = HOOK_STATE.lock();
        if let Some(preset_id) = spec
            .parse::<u32>()
            .ok()
            .filter(|preset_id| hook_state.geometry_presets.iter().any(|preset| preset.id == *preset_id))
        {
            return Some(preset_id);
        }

        hook_state
            .geometry_presets
            .iter()
            .find(|preset| preset.name.trim().eq_ignore_ascii_case(spec))
            .or_else(|| {
                let normalized = spec.replace(' ', "").to_ascii_lowercase();
                hook_state
                    .geometry_presets
                    .iter()
                    .find(|preset| preset.name.replace(' ', "").to_ascii_lowercase() == normalized)
            })
            .map(|preset| preset.id)
            .or(step.geometry_preset_id)
    }

    fn resolve_geometry_preset_from_step(step: &MacroStep) -> Option<crate::model::GeometryPreset> {
        let preset_id = resolve_geometry_preset_id_from_step(step)?;
        HOOK_STATE
            .lock()
            .geometry_presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .cloned()
    }

    fn set_step_geometry_spec(preset_id: u32, absolute_step_index: usize, spec: &GeometrySpec) {
        let mut should_refresh = false;
        {
            let mut hook_state = HOOK_STATE.lock();
            let key = (preset_id, absolute_step_index);
            let spec_changed = hook_state
                .active_geometry_steps
                .get(&key)
                .is_none_or(|existing| existing != spec);

            if spec_changed {
                hook_state.active_geometry_steps.insert(key, spec.clone());
                let current_shape = geometry_render_shape_from_spec(spec);
                let now = Instant::now();
                if let Some(shape) = current_shape {
                    let refresh_interval = geometry_shape_refresh_interval(&shape);
                    let movement_threshold = geometry_shape_motion_threshold(&shape);
                    let motion_delta = hook_state
                        .rendered_geometry_steps
                        .get(&key)
                        .map(|previous| geometry_shape_motion_delta(previous, &shape))
                        .unwrap_or(i32::MAX);
                    let interval_elapsed = hook_state
                        .last_geometry_overlay_refresh_at
                        .is_none_or(|last| now.duration_since(last) >= refresh_interval);
                    should_refresh = interval_elapsed || motion_delta >= movement_threshold;
                    if should_refresh {
                        hook_state.last_geometry_overlay_refresh_at = Some(now);
                        hook_state.rendered_geometry_steps.insert(key, shape);
                    }
                } else {
                    should_refresh = hook_state
                        .last_geometry_overlay_refresh_at
                        .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(16));
                    if should_refresh {
                        hook_state.last_geometry_overlay_refresh_at = Some(now);
                    }
                }
            }
        }
        if should_refresh {
            send_overlay_command(OverlayCommand::RefreshSearchAreaOverlay);
        }
    }

    fn clear_geometry_overlay() {
        {
            let mut hook_state = HOOK_STATE.lock();
            hook_state.active_geometry_preset_ids.clear();
            hook_state.active_geometry_preset_owner_ids.clear();
            hook_state.active_geometry_preset_owner_expires.clear();
            hook_state.active_geometry_preset_instances.clear();
            hook_state.active_geometry_preset_activation_order.clear();
            hook_state.active_geometry_steps.clear();
            hook_state.rendered_geometry_steps.clear();
            hook_state.active_geometry_steps_expires.clear();
            hook_state.last_geometry_overlay_refresh_at = None;
        }
        send_overlay_command(OverlayCommand::RefreshSearchAreaOverlay);
    }


    fn macro_preset_trigger_matches(preset: &MacroPreset, binding: &HotkeyBinding) -> bool {
        if preset
            .hotkey
            .as_ref()
            .is_some_and(|hotkey| trigger_binding_matches(hotkey, binding))
        {
            return true;
        }

        let trigger_keys = preset.trigger_keys.trim();
        if trigger_keys.is_empty() {
            return false;
        }

        hotkey::split_binding_list(trigger_keys)
            .iter()
            .filter_map(|entry| hotkey::parse_binding(entry))
            .any(|expected| trigger_binding_matches(&expected, binding))
    }

    fn preset_trigger_matches(
        hotkey: Option<&HotkeyBinding>,
        trigger_keys: &str,
        binding: &HotkeyBinding,
    ) -> bool {
        if hotkey.is_some_and(|h| trigger_binding_matches(h, binding)) {
            return true;
        }

        let trigger_keys = trigger_keys.trim();
        if trigger_keys.is_empty() {
            return false;
        }

        hotkey::split_binding_list(trigger_keys)
            .iter()
            .filter_map(|entry| hotkey::parse_binding(entry))
            .any(|expected| trigger_binding_matches(&expected, binding))
    }

    fn window_focus_matches(
        target_title: Option<&str>,
        extra_target_titles: &[String],
        match_duplicate_window_titles: bool,
    ) -> bool {
        if target_title.is_none() && extra_target_titles.is_empty() {
            let foreground =
                HWND(FOREGROUND_WINDOW_HWND.load(Ordering::Relaxed) as *mut std::ffi::c_void);
            return is_focus_trigger_candidate_window(foreground);
        }

        let foreground =
            HWND(FOREGROUND_WINDOW_HWND.load(Ordering::Relaxed) as *mut std::ffi::c_void);
        if foreground.0.is_null() {
            return false;
        }

        let title_guard = FOREGROUND_WINDOW_TITLE.lock();
        let Some(ref title) = *title_guard else {
            return false;
        };
        if let Some(target) = target_title {
            if title == target || format!("{title} (0x{:X})", foreground.0 as usize) == target {
                return true;
            }

            let base_title = selector_base_title(target);
            if base_title != target && title == base_title {
                return true;
            }

            if match_duplicate_window_titles && title == selector_base_title(target) {
                return true;
            }

            if matches_browser_suffix(target, title) {
                return true;
            }
        }

        extra_target_titles.iter().any(|target| {
            let target_str = target.as_str();
            if title.as_str() == target_str
                || format!("{title} (0x{:X})", foreground.0 as usize) == target_str
            {
                return true;
            }

            let base_title = selector_base_title(target_str);
            if base_title != target_str && title.as_str() == base_title {
                return true;
            }

            if match_duplicate_window_titles && title.as_str() == selector_base_title(target_str) {
                return true;
            }

            if matches_browser_suffix(target_str, title) {
                return true;
            }

            false
        })
    }

    fn is_focus_trigger_candidate_window(hwnd: HWND) -> bool {
        unsafe {
            if hwnd.0.is_null()
                || !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool()
            {
                return false;
            }

            let root = GetAncestor(hwnd, GA_ROOT);
            if root.0.is_null() || root != hwnd {
                return false;
            }

            if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.0.is_null()) {
                return false;
            }

            if window_belongs_to_current_process(hwnd) || is_internal_app_window(hwnd) {
                return false;
            }

            window_title(hwnd)
                .map(|title| !title.trim().is_empty())
                .unwrap_or(false)
        }
    }

    thread_local! {
        pub static MACRO_TARGETED_WINDOWS: std::cell::RefCell<HashSet<isize>> = std::cell::RefCell::new(HashSet::new());
    }

    unsafe fn resolve_duplicate_by_rule(
        base_title: &str,
        match_duplicate_window_titles: bool,
        exclude: Option<HWND>,
        targeted: Option<&HashSet<isize>>,
        rule: &str,
    ) -> Option<HWND> {
        let mut candidates = Vec::new();
        struct EnumPayload<'a> {
            base_title: &'a str,
            match_duplicate_window_titles: bool,
            exclude: Option<HWND>,
            targeted: Option<&'a HashSet<isize>>,
            candidates: &'a mut Vec<HWND>,
        }

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
            let payload = &mut *(lparam.0 as *mut EnumPayload);
            if !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool() {
                return true.into();
            }
            if payload.exclude.is_some_and(|ex| ex == hwnd) {
                return true.into();
            }
            if let Some(targeted_set) = payload.targeted {
                if targeted_set.contains(&(hwnd.0 as isize)) {
                    return true.into();
                }
            }

            if window_matches_selector_with_duplicate_titles(
                hwnd,
                payload.base_title,
                payload.match_duplicate_window_titles,
            ) {
                payload.candidates.push(hwnd);
            }
            true.into()
        }

        {
            let mut payload = EnumPayload {
                base_title,
                match_duplicate_window_titles,
                exclude,
                targeted,
                candidates: &mut candidates,
            };

            let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
                Some(enum_proc),
                LPARAM((&mut payload) as *mut _ as isize),
            );
        }

        if candidates.is_empty() {
            return None;
        }

        let mut best_hwnd = None;
        let mut best_val = match rule {
            "Lowest" | "Rightmost" => i32::MIN,
            "Highest" | "Leftmost" => i32::MAX,
            _ => 0,
        };

        for hwnd in &candidates {
            let mut rect = windows::Win32::Foundation::RECT::default();
            if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(*hwnd, &mut rect).is_ok() {
                match rule {
                    "Lowest" => {
                        let y = rect.top;
                        if y > best_val {
                            best_val = y;
                            best_hwnd = Some(*hwnd);
                        }
                    }
                    "Highest" => {
                        let y = rect.top;
                        if y < best_val {
                            best_val = y;
                            best_hwnd = Some(*hwnd);
                        }
                    }
                    "Leftmost" => {
                        let x = rect.left;
                        if x < best_val {
                            best_val = x;
                            best_hwnd = Some(*hwnd);
                        }
                    }
                    "Rightmost" => {
                        let x = rect.left;
                        if x > best_val {
                            best_val = x;
                            best_hwnd = Some(*hwnd);
                        }
                    }
                    _ => {}
                }
            }
        }

        best_hwnd.or_else(|| candidates.first().cloned())
    }

    unsafe fn find_window_by_selector_excluding_targeted(
        title: &str,
        match_duplicate_window_titles: bool,
        exclude: Option<HWND>,
        targeted: &HashSet<isize>,
    ) -> Option<HWND> {
        let (base_title, rule) = if title.ends_with(" [Lowest]") {
            (title.strip_suffix(" [Lowest]").unwrap(), Some("Lowest"))
        } else if title.ends_with(" [Highest]") {
            (title.strip_suffix(" [Highest]").unwrap(), Some("Highest"))
        } else if title.ends_with(" [Leftmost]") {
            (title.strip_suffix(" [Leftmost]").unwrap(), Some("Leftmost"))
        } else if title.ends_with(" [Rightmost]") {
            (title.strip_suffix(" [Rightmost]").unwrap(), Some("Rightmost"))
        } else {
            (title, None)
        };

        if let Some(rule) = rule {
            return resolve_duplicate_by_rule(
                base_title,
                match_duplicate_window_titles,
                exclude,
                Some(targeted),
                rule,
            );
        }

        let mut found = None;
        let mut payload = (title, match_duplicate_window_titles, exclude, targeted, &mut found);
        let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(find_window_by_selector_excluding_targeted_proc),
            LPARAM((&mut payload) as *mut _ as isize),
        );
        found
    }

    unsafe extern "system" fn find_window_by_selector_excluding_targeted_proc(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> windows::core::BOOL {
        let (target, match_duplicate_window_titles, exclude, targeted, found) =
            &mut *(lparam.0 as *mut (&str, bool, Option<HWND>, &HashSet<isize>, &mut Option<HWND>));
        let clean_target = strip_rule_suffix(*target);
        if !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }

        if exclude.is_some_and(|excluded| excluded == hwnd) {
            return true.into();
        }

        if targeted.contains(&(hwnd.0 as isize)) {
            return true.into();
        }

        if window_matches_selector_with_duplicate_titles(
            hwnd,
            clean_target,
            *match_duplicate_window_titles,
        ) {
            **found = Some(hwnd);
            return false.into();
        }

        true.into()
    }

    fn resolve_window_target(
        target_title: Option<&str>,
        extra_target_titles: &[String],
        match_duplicate_window_titles: bool,
        prefer_other_if_foreground_matches: bool,
    ) -> HWND {
        unsafe {
            let target_uses_position_rule = target_title.is_some_and(has_position_rule_suffix)
                || extra_target_titles
                    .iter()
                    .any(|title| has_position_rule_suffix(title));
            let foreground = GetForegroundWindow();
            let targeted = MACRO_TARGETED_WINDOWS.with(|set| set.borrow().clone());

            // 1. Try to find a matching window that is NOT yet targeted in this macro execution
            if !target_uses_position_rule
                && !foreground.0.is_null()
                && !targeted.contains(&(foreground.0 as isize))
                && window_matches_any_selector(
                    foreground,
                    target_title,
                    extra_target_titles,
                    match_duplicate_window_titles,
                )
            {
                if prefer_other_if_foreground_matches {
                    if let Some(target) = target_title
                        && let Some(hwnd) = find_window_by_selector_excluding_targeted(
                            target,
                            match_duplicate_window_titles,
                            Some(foreground),
                            &targeted,
                        )
                    {
                        MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(hwnd.0 as isize));
                        return hwnd;
                    }

                    for title in extra_target_titles {
                        if let Some(hwnd) = find_window_by_selector_excluding_targeted(
                            title,
                            match_duplicate_window_titles,
                            Some(foreground),
                            &targeted,
                        ) {
                            MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(hwnd.0 as isize));
                            return hwnd;
                        }
                    }
                }

                MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(foreground.0 as isize));
                return foreground;
            }

            if let Some(title) = target_title
                && let Some(hwnd) = find_window_by_selector_excluding_targeted(
                    title,
                    match_duplicate_window_titles,
                    None,
                    &targeted,
                )
            {
                MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(hwnd.0 as isize));
                return hwnd;
            }

            for title in extra_target_titles {
                if let Some(hwnd) = find_window_by_selector_excluding_targeted(
                    title,
                    match_duplicate_window_titles,
                    None,
                    &targeted,
                ) {
                    MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(hwnd.0 as isize));
                    return hwnd;
                }
            }

            // 2. Fallback: If no untargeted matching window is found, search using the original logic
            if !target_uses_position_rule
                && !foreground.0.is_null()
                && window_matches_any_selector(
                    foreground,
                    target_title,
                    extra_target_titles,
                    match_duplicate_window_titles,
                )
            {
                if prefer_other_if_foreground_matches {
                    if let Some(target) = target_title
                        && let Some(hwnd) = find_window_by_selector_excluding(
                            target,
                            match_duplicate_window_titles,
                            Some(foreground),
                        )
                    {
                        MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(hwnd.0 as isize));
                        return hwnd;
                    }

                    for title in extra_target_titles {
                        if let Some(hwnd) = find_window_by_selector_excluding(
                            title,
                            match_duplicate_window_titles,
                            Some(foreground),
                        ) {
                            MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(hwnd.0 as isize));
                            return hwnd;
                        }
                    }
                }

                MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(foreground.0 as isize));
                return foreground;
            }

            if let Some(title) = target_title
                && let Some(hwnd) =
                    find_window_by_selector_excluding(title, match_duplicate_window_titles, None)
            {
                MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(hwnd.0 as isize));
                return hwnd;
            }

            for title in extra_target_titles {
                if let Some(hwnd) =
                    find_window_by_selector_excluding(title, match_duplicate_window_titles, None)
                {
                    MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(hwnd.0 as isize));
                    return hwnd;
                }
            }

            if !foreground.0.is_null() {
                MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().insert(foreground.0 as isize));
            }
            foreground
        }
    }

    fn find_target_window_hwnd(
        target_title: Option<&str>,
        extra_target_titles: &[String],
        match_duplicate_window_titles: bool,
        prefer_other_if_foreground_matches: bool,
    ) -> Option<HWND> {
        let hwnd = resolve_window_target(
            target_title,
            extra_target_titles,
            match_duplicate_window_titles,
            prefer_other_if_foreground_matches,
        );
        if hwnd.0.is_null() { None } else { Some(hwnd) }
    }

    fn shutdown_application(hwnd: HWND, runtime: &Runtime) -> Result<()> {
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &notify_icon(hwnd)) };
        let _ = crate::platform::show_taskbar();
        if let Some(highlighted_hwnd) = runtime.active_focus_highlight_hwnd {
            let _ = unsafe { set_native_border_color(highlighted_hwnd, DWM_COLOR_DEFAULT) };
        }
        let _ = restore_mouse_sensitivity_on_exit();
        let _ = unsafe { ShowWindow(runtime.overlay_hwnd, SW_HIDE) };
        let _ = unsafe { ShowWindow(runtime.hud_hwnd, SW_HIDE) };
        let _ = unsafe { ShowWindow(runtime.pin_hwnd, SW_HIDE) };
        let _ = unsafe { ShowWindow(runtime.focus_highlight_hwnd, SW_HIDE) };
        if let Some(active) = &runtime.active_pin_thumbnail {
            if let Some(thumbnail_id) = active.thumbnail_id {
                let _ = unsafe { DwmUnregisterThumbnail(thumbnail_id) };
            }
        }

        if !runtime.keyboard_hook.0.is_null() {
            let _ = unsafe { UnhookWindowsHookEx(runtime.keyboard_hook) };
        }

        if !runtime.mouse_hook.0.is_null() {
            let _ = unsafe { UnhookWindowsHookEx(runtime.mouse_hook) };
        }

        if !runtime.window_focus_event_hook.0.is_null() {
            let _ = unsafe { UnhookWinEvent(runtime.window_focus_event_hook) };
        }

        if !runtime.window_location_event_hook.0.is_null() {
            let _ = unsafe { UnhookWinEvent(runtime.window_location_event_hook) };
        }

        {
            let mut hook_state = HOOK_STATE.lock();
            hook_state.window_presets.clear();
            hook_state.window_expand_controls = WindowExpandControls::default();
            hook_state.pin_presets.clear();
            hook_state.active_pin_preset_id = None;
            hook_state.macro_groups.clear();
            hook_state.locked_inputs.clear();
            hook_state.mouse_move_locks = MouseMoveLockCounts::default();
            hook_state.mouse_move_lock_anchor = None;
            hook_state.active_hold_macros.clear();
            hook_state.held_mouse_buttons.clear();
        }

        std::process::exit(0);
    }

    unsafe fn find_window_by_title(title: &str) -> Option<HWND> {
        let mut found = None;
        let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(find_window_by_selector_proc),
            LPARAM((&mut (title, &mut found)) as *mut _ as isize),
        );
        found
    }

    unsafe fn find_window_by_selector_excluding(
        title: &str,
        match_duplicate_window_titles: bool,
        exclude: Option<HWND>,
    ) -> Option<HWND> {
        let (base_title, rule) = if title.ends_with(" [Lowest]") {
            (title.strip_suffix(" [Lowest]").unwrap(), Some("Lowest"))
        } else if title.ends_with(" [Highest]") {
            (title.strip_suffix(" [Highest]").unwrap(), Some("Highest"))
        } else if title.ends_with(" [Leftmost]") {
            (title.strip_suffix(" [Leftmost]").unwrap(), Some("Leftmost"))
        } else if title.ends_with(" [Rightmost]") {
            (title.strip_suffix(" [Rightmost]").unwrap(), Some("Rightmost"))
        } else {
            (title, None)
        };

        if let Some(rule) = rule {
            return resolve_duplicate_by_rule(
                base_title,
                match_duplicate_window_titles,
                exclude,
                None,
                rule,
            );
        }

        let mut found = None;
        let mut payload = (title, match_duplicate_window_titles, exclude, &mut found);
        let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(find_window_by_selector_excluding_proc),
            LPARAM((&mut payload) as *mut _ as isize),
        );
        found
    }

    unsafe extern "system" fn find_window_by_selector_proc(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> windows::core::BOOL {
        let (target, found) = &mut *(lparam.0 as *mut (&str, &mut Option<HWND>));
        let clean_target = strip_rule_suffix(*target);
        if !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }

        if window_matches_selector(hwnd, clean_target) {
            **found = Some(hwnd);
            return false.into();
        }

        true.into()
    }

    unsafe extern "system" fn find_window_by_selector_excluding_proc(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> windows::core::BOOL {
        let (target, match_duplicate_window_titles, exclude, found) =
            &mut *(lparam.0 as *mut (&str, bool, Option<HWND>, &mut Option<HWND>));
        let clean_target = strip_rule_suffix(*target);
        if !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }

        if exclude.is_some_and(|excluded| excluded == hwnd) {
            return true.into();
        }

        if window_matches_selector_with_duplicate_titles(
            hwnd,
            clean_target,
            *match_duplicate_window_titles,
        ) {
            **found = Some(hwnd);
            return false.into();
        }

        true.into()
    }

    fn selector_base_title(target: &str) -> &str {
        if let Some(prefix) = target.strip_suffix(')')
            && let Some((base, _)) = prefix.rsplit_once(" (0x")
        {
            return base;
        }

        target
    }

    fn clean_invisible_chars(s: &str) -> String {
        s.chars()
            .filter(|&c| c != '\u{200B}' && c != '\u{200C}' && c != '\u{200D}' && c != '\u{FEFF}')
            .collect()
    }

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
    fn matches_browser_suffix(target: &str, candidate: &str) -> bool {
        let clean_target = clean_invisible_chars(target);
        let clean_candidate = clean_invisible_chars(candidate);
        let target_base = selector_base_title(&clean_target);
        let candidate_base = selector_base_title(&clean_candidate);
        
        let is_target_anti = target_base.contains(" - Antigravity IDE - ") || target_base.ends_with(" - Antigravity IDE");
        let is_cand_anti = candidate_base.contains(" - Antigravity IDE - ") || candidate_base.ends_with(" - Antigravity IDE");
        if is_target_anti && is_cand_anti {
            return true;
        }

        for suffix in BROWSER_SUFFIXES {
            if target_base.ends_with(suffix) && candidate_base.ends_with(suffix) {
                return true;
            }
        }

        false
    }

    fn strip_rule_suffix(target: &str) -> &str {
        if let Some(s) = target.strip_suffix(" [Lowest]") {
            s
        } else if let Some(s) = target.strip_suffix(" [Highest]") {
            s
        } else if let Some(s) = target.strip_suffix(" [Leftmost]") {
            s
        } else if let Some(s) = target.strip_suffix(" [Rightmost]") {
            s
        } else {
            target
        }
    }

    fn has_position_rule_suffix(target: &str) -> bool {
        target.ends_with(" [Lowest]")
            || target.ends_with(" [Highest]")
            || target.ends_with(" [Leftmost]")
            || target.ends_with(" [Rightmost]")
    }

    unsafe fn window_matches_selector(hwnd: HWND, target: &str) -> bool {
        let target = strip_rule_suffix(target);
        let Some(title) = window_title(hwnd) else {
            return false;
        };
        title == target || format!("{title} (0x{:X})", hwnd.0 as usize) == target
    }

    unsafe fn window_matches_selector_with_duplicate_titles(
        hwnd: HWND,
        target: &str,
        match_duplicate_window_titles: bool,
    ) -> bool {
        let target = strip_rule_suffix(target);
        let base_title = selector_base_title(target);
        if base_title != target {
            return window_matches_selector(hwnd, target);
        }

        if window_matches_selector(hwnd, target) {
            return true;
        }

        if match_duplicate_window_titles {
            let Some(title) = window_title(hwnd) else {
                return false;
            };
            if title == base_title {
                return true;
            }
        }

        if let Some(title) = window_title(hwnd) {
            if matches_browser_suffix(target, &title) {
                return true;
            }
        }

        false
    }

    unsafe fn window_matches_any_selector(
        hwnd: HWND,
        target_title: Option<&str>,
        extra_target_titles: &[String],
        match_duplicate_window_titles: bool,
    ) -> bool {
        if let Some(target) = target_title
            && window_matches_selector_with_duplicate_titles(
                hwnd,
                target,
                match_duplicate_window_titles,
            )
        {
            return true;
        }

        extra_target_titles.iter().any(|target| {
            window_matches_selector_with_duplicate_titles(
                hwnd,
                target,
                match_duplicate_window_titles,
            )
        })
    }

    unsafe fn window_title(hwnd: HWND) -> Option<String> {
        let length = windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return None;
        }

        let mut buffer = vec![0u16; length as usize + 1];
        let copied = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut buffer);
        if copied <= 0 {
            None
        } else {
            Some(String::from_utf16_lossy(&buffer[..copied as usize]))
        }
    }

    unsafe fn paint_overlay(
        hwnd: HWND,
        style: &CrosshairStyle,
        rendered: RenderedCrosshair,
    ) -> Result<()> {
        let window_x = style.x_offset - rendered.center_x;
        let window_y = style.y_offset - rendered.center_y;
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            window_x,
            window_y,
            rendered.width as i32,
            rendered.height as i32,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            bail!("Failed to acquire the screen DC");
        }

        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create a memory DC");
        }

        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: rendered.width as i32,
            biHeight: -(rendered.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?;
        if bitmap.0.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create the DIB surface");
        }

        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        let bgra = rgba_to_bgra(&rendered.rgba);
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
        let destination = POINT {
            x: window_x,
            y: window_y,
        };
        let source = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: rendered.width as i32,
            cy: rendered.height as i32,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&destination),
            Some(&size),
            Some(mem_dc),
            Some(&source),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        Ok(())
    }

    unsafe fn paint_mouse_trail(hwnd: HWND, points: &[POINT], marker: Option<POINT>) -> Result<()> {
        let screen_width = GetSystemMetrics(SM_CXSCREEN).max(1);
        let screen_height = GetSystemMetrics(SM_CYSCREEN).max(1);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            screen_width,
            screen_height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            bail!("Failed to acquire the screen DC");
        }

        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create a memory DC");
        }

        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: screen_width,
            biHeight: -screen_height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?;
        if bitmap.0.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create mouse trail DIB");
        }

        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        let pixel_len = (screen_width as usize) * (screen_height as usize) * 4;
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, pixel_len);
        pixels.fill(0);
        for segment in points.windows(2) {
            if let [from, to] = segment {
                draw_line_rgba(
                    pixels,
                    screen_width as usize,
                    screen_height as usize,
                    from.x,
                    from.y,
                    to.x,
                    to.y,
                    [255, 40, 40, 180],
                );
            }
        }

        if let (Some(start), Some(end)) = (points.first().copied(), points.last().copied()) {
            let width_usize = screen_width as usize;
            let height_usize = screen_height as usize;
            let start_fill = [90, 235, 150, 220];
            let start_stroke = [180, 255, 210, 255];
            let end_fill = [90, 140, 255, 220];
            let end_stroke = [210, 225, 255, 255];
            fill_ellipse_rgba(
                pixels,
                width_usize,
                height_usize,
                start.x - 7,
                start.y - 7,
                14,
                14,
                start_fill,
            );
            draw_ellipse_outline_rgba(
                pixels,
                width_usize,
                height_usize,
                start.x - 9,
                start.y - 9,
                18,
                18,
                start_stroke,
            );
            fill_ellipse_rgba(
                pixels,
                width_usize,
                height_usize,
                end.x - 7,
                end.y - 7,
                14,
                14,
                end_fill,
            );
            draw_ellipse_outline_rgba(
                pixels,
                width_usize,
                height_usize,
                end.x - 9,
                end.y - 9,
                18,
                18,
                end_stroke,
            );
            let font_name = "Segoe UI"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let font = CreateFontW(
                -14,
                0,
                0,
                0,
                FW_MEDIUM.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                FF_DONTCARE.0 as u32,
                PCWSTR(font_name.as_ptr()),
            );
            let old_font = SelectObject(mem_dc, HGDIOBJ(font.0));
            let _ = SetBkMode(mem_dc, TRANSPARENT);
            let draw_anchor_label =
                |mem_dc: HDC, pixels: &mut [u8], anchor: POINT, text: String, color: [u8; 4], y_bias: i32| {
                    let label_width = 144;
                    let label_height = 26;
                    let desired_left = if anchor.x + 20 + label_width > screen_width {
                        anchor.x - label_width - 20
                    } else {
                        anchor.x + 20
                    };
                    let desired_top = (anchor.y + y_bias)
                        .clamp(6, screen_height.saturating_sub(label_height + 6));
                    let label_left = desired_left.clamp(6, screen_width.saturating_sub(label_width + 6));
                    fill_rect_rgba(
                        pixels,
                        width_usize,
                        height_usize,
                        label_left,
                        desired_top,
                        label_width,
                        label_height,
                        [18, 26, 22, 210],
                    );
                    draw_rect_outline_rgba(
                        pixels,
                        width_usize,
                        height_usize,
                        label_left,
                        desired_top,
                        label_width,
                        label_height,
                        color,
                    );
                    let _ = SetTextColor(
                        mem_dc,
                        COLORREF(
                            ((color[0] as u32) << 16)
                                | ((color[1] as u32) << 8)
                                | color[2] as u32,
                        ),
                    );
                    let mut rect = RECT {
                        left: label_left + 8,
                        top: desired_top + 4,
                        right: label_left + label_width - 8,
                        bottom: desired_top + label_height - 4,
                    };
                    let mut wide = text
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect::<Vec<_>>();
                    let _ = DrawTextW(
                        mem_dc,
                        &mut wide,
                        &mut rect,
                        DT_VCENTER | DT_SINGLELINE,
                    );
                };
            draw_anchor_label(
                mem_dc,
                pixels,
                start,
                format!("Start {} , {}", start.x, start.y),
                start_stroke,
                -34,
            );
            draw_anchor_label(
                mem_dc,
                pixels,
                end,
                format!("End {} , {}", end.x, end.y),
                end_stroke,
                10,
            );
            if let Some(marker) = marker {
                fill_ellipse_rgba(
                    pixels,
                    width_usize,
                    height_usize,
                    marker.x - 6,
                    marker.y - 6,
                    12,
                    12,
                    [255, 232, 96, 235],
                );
                draw_ellipse_outline_rgba(
                    pixels,
                    width_usize,
                    height_usize,
                    marker.x - 10,
                    marker.y - 10,
                    20,
                    20,
                    [255, 255, 255, 255],
                );
            }
            let _ = SelectObject(mem_dc, old_font);
            let _ = DeleteObject(HGDIOBJ(font.0));
        }

        let destination = POINT { x: 0, y: 0 };
        let source = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: screen_width,
            cy: screen_height,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&destination),
            Some(&size),
            Some(mem_dc),
            Some(&source),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        Ok(())
    }

    unsafe fn paint_search_area_overlay(
        hwnd: HWND,
        regions: &[VisionRegion],
        preview_regions: &[VisionRegion],
        static_geometry_shapes: &[GeometryRenderShape],
        dynamic_geometry_shapes: &[GeometryRenderShape],
    ) -> Result<()> {
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        for region in regions {
            let r_left = region.left - 2;
            let r_top = region.top - 2;
            let r_right = region.left + region.width + 2;
            let r_bottom = region.top + region.height + 2;
            min_x = min_x.min(r_left);
            min_y = min_y.min(r_top);
            max_x = max_x.max(r_right);
            max_y = max_y.max(r_bottom);
        }

        for region in preview_regions {
            let r_left = region.left - 2;
            let r_top = region.top - 2;
            let r_right = region.left + region.width + 2;
            let r_bottom = region.top + region.height + 2;
            min_x = min_x.min(r_left);
            min_y = min_y.min(r_top);
            max_x = max_x.max(r_right);
            max_y = max_y.max(r_bottom);
        }

        for shape in static_geometry_shapes.iter().chain(dynamic_geometry_shapes.iter()) {
            let (left, top, right, bottom) = shape.bounds;
            min_x = min_x.min(left);
            min_y = min_y.min(top);
            max_x = max_x.max(right);
            max_y = max_y.max(bottom);
        }

        if min_x == i32::MAX {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return Ok(());
        }

        let width = (max_x - min_x).max(1);
        let height = (max_y - min_y).max(1);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            min_x,
            min_y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let screen_dc = GetDC(None);
        if screen_dc.0.is_null() {
            bail!("Failed to acquire the screen DC");
        }

        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        if mem_dc.0.is_null() {
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create a memory DC");
        }

        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?;
        if bitmap.0.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            bail!("Failed to create search area DIB");
        }
        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        let pixel_len = (width as usize) * (height as usize) * 4;
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, pixel_len);
        pixels.fill(0);

        let mut pixmap = tiny_skia::Pixmap::new(width as u32, height as u32).unwrap();

        for region in regions {
            let rel_left = region.left - min_x;
            let rel_top = region.top - min_y;
            let outline = [92, 220, 255, 210];
            if region.is_circle {
                let rect = tiny_skia::Rect::from_xywh(
                    rel_left as f32,
                    rel_top as f32,
                    region.width as f32,
                    region.height as f32,
                ).unwrap();
                let mut pb = tiny_skia::PathBuilder::new();
                pb.push_oval(rect);
                if let Some(path) = pb.finish() {
                    let mut paint = tiny_skia::Paint::default();
                    paint.set_color(tiny_skia::Color::from_rgba8(outline[0], outline[1], outline[2], outline[3]));
                    paint.anti_alias = true;
                    let stroke = tiny_skia::Stroke {
                        width: 1.0,
                        ..Default::default()
                    };
                    pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
                }

                let center_x = rel_left + region.width / 2;
                let center_y = rel_top + region.height / 2;
                let rx = region.width as f32 / 2.0;
                let ry = region.height as f32 / 2.0;
                if let Some(angle_deg) = region.angle_offset_deg {
                    // 1. Draw START ANGLE (0% - Orange Line)
                    let rad0 = angle_deg.to_radians();
                    let x0 = center_x as f32 + rx * rad0.sin();
                    let y0 = center_y as f32 - ry * rad0.cos();
                    let mut pb_line = tiny_skia::PathBuilder::new();
                    pb_line.move_to(center_x as f32, center_y as f32);
                    pb_line.line_to(x0, y0);
                    if let Some(path) = pb_line.finish() {
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color(tiny_skia::Color::from_rgba8(255, 120, 0, 255));
                        paint.anti_alias = true;
                        let stroke = tiny_skia::Stroke {
                            width: 1.0,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
                    }

                    // 2. Draw END ANGLE (100% - Bright Green Line) based on SPAN!
                    if let Some(span) = region.angle_span_deg {
                        if span < 360.0 {
                            let end_deg = (angle_deg + span) % 360.0;
                            let rad1 = end_deg.to_radians();
                            let x1 = center_x as f32 + rx * rad1.sin();
                            let y1 = center_y as f32 - ry * rad1.cos();
                            let mut pb_line = tiny_skia::PathBuilder::new();
                            pb_line.move_to(center_x as f32, center_y as f32);
                            pb_line.line_to(x1, y1);
                            if let Some(path) = pb_line.finish() {
                                let mut paint = tiny_skia::Paint::default();
                                paint.set_color(tiny_skia::Color::from_rgba8(50, 255, 50, 255));
                                paint.anti_alias = true;
                                let stroke = tiny_skia::Stroke {
                                    width: 1.0,
                                    ..Default::default()
                                };
                                pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
                            }
                        }
                    }
                }
            } else {
                let rect = tiny_skia::Rect::from_xywh(
                    rel_left as f32,
                    rel_top as f32,
                    region.width as f32,
                    region.height as f32,
                ).unwrap();
                let path = tiny_skia::PathBuilder::from_rect(rect);
                let mut paint = tiny_skia::Paint::default();
                paint.set_color(tiny_skia::Color::from_rgba8(outline[0], outline[1], outline[2], outline[3]));
                paint.anti_alias = true;
                let stroke = tiny_skia::Stroke {
                    width: 1.0,
                    ..Default::default()
                };
                pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
            }
        }

        for region in preview_regions {
            let rel_left = region.left - min_x;
            let rel_top = region.top - min_y;
            let outline = [255, 216, 96, 230];
            let rect = tiny_skia::Rect::from_xywh(
                rel_left as f32,
                rel_top as f32,
                region.width as f32,
                region.height as f32,
            ).unwrap();
            let path = tiny_skia::PathBuilder::from_rect(rect);
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(outline[0], outline[1], outline[2], outline[3]));
            paint.anti_alias = true;
            let stroke = tiny_skia::Stroke {
                width: 1.0,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
        }

        let mut geometry_texts = Vec::new();
        for shape in static_geometry_shapes.iter().chain(dynamic_geometry_shapes.iter()) {
            match &shape.draw {
                GeometryRenderDraw::Point {
                    x,
                    y,
                    radius,
                    fill,
                } => {
                    let left = x - min_x - radius;
                    let top = y - min_y - radius;
                    let size = radius.saturating_mul(2).max(1);
                    let mut pb = tiny_skia::PathBuilder::new();
                    if let Some(rect) = tiny_skia::Rect::from_xywh(left as f32, top as f32, size as f32, size as f32) {
                        pb.push_oval(rect);
                        if let Some(path) = pb.finish() {
                            let mut paint = tiny_skia::Paint::default();
                            paint.set_color(tiny_skia::Color::from_rgba8(fill[0], fill[1], fill[2], fill[3]));
                            paint.anti_alias = true;
                            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
                        }
                    }
                }
                GeometryRenderDraw::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    stroke,
                    thickness,
                } => {
                    let mut pb = tiny_skia::PathBuilder::new();
                    pb.move_to((x1 - min_x) as f32, (y1 - min_y) as f32);
                    pb.line_to((x2 - min_x) as f32, (y2 - min_y) as f32);
                    if let Some(path) = pb.finish() {
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color(tiny_skia::Color::from_rgba8(stroke[0], stroke[1], stroke[2], stroke[3]));
                        paint.anti_alias = true;
                        let skia_stroke = tiny_skia::Stroke {
                            width: *thickness as f32,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &paint, &skia_stroke, tiny_skia::Transform::identity(), None);
                    }
                }
                GeometryRenderDraw::Circle {
                    cx,
                    cy,
                    radius,
                    stroke,
                    fill,
                    thickness,
                } => {
                    let left = cx - min_x - radius;
                    let top = cy - min_y - radius;
                    let size = radius.saturating_mul(2).max(1);
                    let mut pb = tiny_skia::PathBuilder::new();
                    if let Some(rect) = tiny_skia::Rect::from_xywh(left as f32, top as f32, size as f32, size as f32) {
                        pb.push_oval(rect);
                        if let Some(path) = pb.finish() {
                            let mut paint = tiny_skia::Paint::default();
                            paint.anti_alias = true;
                            if let Some(fill_color) = fill {
                                paint.set_color(tiny_skia::Color::from_rgba8(fill_color[0], fill_color[1], fill_color[2], fill_color[3]));
                                pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
                            }
                            paint.set_color(tiny_skia::Color::from_rgba8(stroke[0], stroke[1], stroke[2], stroke[3]));
                            let skia_stroke = tiny_skia::Stroke {
                                width: *thickness as f32,
                                ..Default::default()
                            };
                            pixmap.stroke_path(&path, &paint, &skia_stroke, tiny_skia::Transform::identity(), None);
                        }
                    }
                }
                GeometryRenderDraw::Arrow {
                    x1,
                    y1,
                    x2,
                    y2,
                    stroke,
                    thickness,
                    head_size,
                } => {
                    let rel_x1 = (x1 - min_x) as f32;
                    let rel_y1 = (y1 - min_y) as f32;
                    let rel_x2 = (x2 - min_x) as f32;
                    let rel_y2 = (y2 - min_y) as f32;
                    let mut pb = tiny_skia::PathBuilder::new();
                    pb.move_to(rel_x1, rel_y1);
                    pb.line_to(rel_x2, rel_y2);

                    let dx = rel_x2 - rel_x1;
                    let dy = rel_y2 - rel_y1;
                    let len = (dx * dx + dy * dy).sqrt().max(1.0);
                    let ux = dx / len;
                    let uy = dy / len;
                    let angle = 28.0_f32.to_radians();
                    let sin_a = angle.sin();
                    let cos_a = angle.cos();
                    for side in [-1.0_f32, 1.0_f32] {
                        let rx = ux * cos_a - side * uy * sin_a;
                        let ry = uy * cos_a + side * ux * sin_a;
                        let hx = rel_x2 - rx * *head_size as f32;
                        let hy = rel_y2 - ry * *head_size as f32;
                        pb.move_to(rel_x2, rel_y2);
                        pb.line_to(hx, hy);
                    }
                    if let Some(path) = pb.finish() {
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color(tiny_skia::Color::from_rgba8(stroke[0], stroke[1], stroke[2], stroke[3]));
                        paint.anti_alias = true;
                        let skia_stroke = tiny_skia::Stroke {
                            width: *thickness as f32,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &paint, &skia_stroke, tiny_skia::Transform::identity(), None);
                    }
                }
                GeometryRenderDraw::Polyline {
                    points,
                    stroke,
                    thickness,
                } => {
                    let mut pb = tiny_skia::PathBuilder::new();
                    let mut first = true;
                    for pt in points {
                        let px = (pt.0 - min_x) as f32;
                        let py = (pt.1 - min_y) as f32;
                        if first {
                            pb.move_to(px, py);
                            first = false;
                        } else {
                            pb.line_to(px, py);
                        }
                    }
                    if let Some(path) = pb.finish() {
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color(tiny_skia::Color::from_rgba8(stroke[0], stroke[1], stroke[2], stroke[3]));
                        paint.anti_alias = true;
                        let skia_stroke = tiny_skia::Stroke {
                            width: *thickness as f32,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &paint, &skia_stroke, tiny_skia::Transform::identity(), None);
                    }
                }
                GeometryRenderDraw::Polygon {
                    points,
                    stroke,
                    thickness,
                    fill,
                } => {
                    let mut pb = tiny_skia::PathBuilder::new();
                    let mut first = true;
                    for pt in points {
                        let px = (pt.0 - min_x) as f32;
                        let py = (pt.1 - min_y) as f32;
                        if first {
                            pb.move_to(px, py);
                            first = false;
                        } else {
                            pb.line_to(px, py);
                        }
                    }
                    pb.close();
                    if let Some(path) = pb.finish() {
                        let mut paint = tiny_skia::Paint::default();
                        paint.anti_alias = true;
                        if let Some(fill_color) = fill {
                            paint.set_color(tiny_skia::Color::from_rgba8(fill_color[0], fill_color[1], fill_color[2], fill_color[3]));
                            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
                        }
                        paint.set_color(tiny_skia::Color::from_rgba8(stroke[0], stroke[1], stroke[2], stroke[3]));
                        let skia_stroke = tiny_skia::Stroke {
                            width: *thickness as f32,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &paint, &skia_stroke, tiny_skia::Transform::identity(), None);
                    }
                }
                GeometryRenderDraw::Label(text) => geometry_texts.push(text.clone()),
                GeometryRenderDraw::Svg { .. } => {}
            }
        }

        // Copy Skia pixmap to DIB section pixels buffer
        let pixmap_data = pixmap.data();
        let total_pixels = width as usize * height as usize;
        for i in 0..total_pixels {
            let offset = i * 4;
            let r = pixmap_data[offset];
            let g = pixmap_data[offset + 1];
            let b = pixmap_data[offset + 2];
            let a = pixmap_data[offset + 3];
            pixels[offset] = b;
            pixels[offset + 1] = g;
            pixels[offset + 2] = r;
            pixels[offset + 3] = a;
        }

        // Draw SVG images directly on top of pixels
        for shape in static_geometry_shapes.iter().chain(dynamic_geometry_shapes.iter()) {
            if let GeometryRenderDraw::Svg {
                x,
                y,
                width: target_w,
                height: target_h,
                opacity,
                rotation,
                code,
            } = &shape.draw
            {
                if !code.trim().is_empty() {
                    let opacity_key = (opacity * 1000.0).round() as u32;
                    let rotation_key = (rotation * 1000.0).round() as i32;
                    let cache_key = (code.clone(), *target_w, *target_h, opacity_key, rotation_key);
                    let mut cache = GEOMETRY_SVG_CACHE.lock();
                    let rendered = if let Some(cached) = cache.get(&cache_key) {
                        Some(cached)
                    } else {
                        match crate::render::render_svg_image(code, *target_w, *target_h, *opacity, *rotation) {
                            Ok(img) => {
                                cache.insert(cache_key.clone(), img);
                                cache.get(&cache_key)
                            }
                            Err(e) => {
                                eprintln!("Overlay Svg paint: failed to render inline SVG: {e}");
                                None
                            }
                        }
                    };
                    
                    if let Some(img) = rendered {
                        let img_w = img.width as usize;
                        let img_h = img.height as usize;
                        let offset_x = (img.orig_width as i32 - img.width as i32) / 2;
                        let offset_y = (img.orig_height as i32 - img.height as i32) / 2;
                        let rel_x = x + offset_x - min_x;
                        let rel_y = y + offset_y - min_y;
                        for py in 0..img_h {
                            let screen_y = rel_y + py as i32;
                            if screen_y < 0 || screen_y >= height as i32 {
                                continue;
                            }
                            for px in 0..img_w {
                                let screen_x = rel_x + px as i32;
                                if screen_x < 0 || screen_x >= width as i32 {
                                    continue;
                                }
                                let img_idx = (py * img_w + px) * 4;
                                if img_idx + 3 >= img.rgba.len() {
                                    continue;
                                }
                                let alpha = img.rgba[img_idx + 3] as u32;
                                if alpha > 0 {
                                    let dest_idx = ((screen_y as usize) * (width as usize) + (screen_x as usize)) * 4;
                                    if dest_idx + 3 < pixels.len() {
                                        let src_r = img.rgba[img_idx] as u32;
                                        let src_g = img.rgba[img_idx + 1] as u32;
                                        let src_b = img.rgba[img_idx + 2] as u32;
                                        
                                        let dest_b = pixels[dest_idx] as u32;
                                        let dest_g = pixels[dest_idx + 1] as u32;
                                        let dest_r = pixels[dest_idx + 2] as u32;
                                        
                                        let out_r = (src_r * alpha + dest_r * (255 - alpha)) / 255;
                                        let out_g = (src_g * alpha + dest_g * (255 - alpha)) / 255;
                                        let out_b = (src_b * alpha + dest_b * (255 - alpha)) / 255;
                                        
                                        pixels[dest_idx] = out_b as u8;
                                        pixels[dest_idx + 1] = out_g as u8;
                                        pixels[dest_idx + 2] = out_r as u8;
                                        pixels[dest_idx + 3] = pixels[dest_idx + 3].max(alpha as u8);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        use windows::Win32::Graphics::Gdi::{
            DT_LEFT, DT_SINGLELINE, DT_VCENTER, DrawTextW, GetTextExtentPoint32W, GetTextMetricsW,
            SetBkMode, SetTextAlign, SetTextColor, TextOutW, TRANSPARENT, TA_BASELINE, TA_CENTER,
            TEXTMETRICW,
        };
        use windows::Win32::Foundation::RECT;
        unsafe {
            let _ = SetTextColor(mem_dc, COLORREF(0xFFFFFF));
            let _ = SetBkMode(mem_dc, TRANSPARENT);
        }

        let mut occupied_label_rects: Vec<RECT> = Vec::new();
        for region in preview_regions {
            let rel_left = region.left - min_x;
            let rel_top = region.top - min_y;
            let mut text_rect = RECT {
                left: rel_left,
                top: rel_top - 18,
                right: rel_left + 300,
                bottom: rel_top,
            };
            if text_rect.top < 0 {
                text_rect.top = rel_top + region.height + 2;
                text_rect.bottom = text_rect.top + 18;
            }

            loop {
                let overlaps_existing = occupied_label_rects.iter().any(|occupied| {
                    text_rect.left < occupied.right
                        && text_rect.right > occupied.left
                        && text_rect.top < occupied.bottom
                        && text_rect.bottom > occupied.top
                });
                if !overlaps_existing {
                    break;
                }

                text_rect.top += 20;
                text_rect.bottom += 20;
                if text_rect.bottom > height {
                    text_rect.top = (rel_top - 38).max(0);
                    text_rect.bottom = (text_rect.top + 18).min(height);
                    break;
                }
            }

            let text_str = format!(
                "{}x{} @ {},{}",
                region.width, region.height, region.left, region.top
            );
            let mut wide_text = text_str
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            unsafe {
                let _ = DrawTextW(
                    mem_dc,
                    &mut wide_text,
                    &mut text_rect,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
            }

            occupied_label_rects.push(text_rect);
        }

        let rects_to_fix = occupied_label_rects.clone();
        let font_name = "Segoe UI"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut label_font_cache: HashMap<(i32, i32), HGDIOBJ> = HashMap::new();
        for text in &geometry_texts {
            let label_bg = [1_u8, 2_u8, 3_u8];
            let font_height = text.font_size.max(10);
            let rotation_tenths = (-(text.rotation_deg) * 10.0).round() as i32;
            let font_key = (font_height, rotation_tenths);
            let font = *label_font_cache.entry(font_key).or_insert_with(|| {
                HGDIOBJ(
                    CreateFontW(
                        -font_height,
                        0,
                        rotation_tenths,
                        rotation_tenths,
                        FW_MEDIUM.0 as i32,
                        0,
                        0,
                        0,
                        DEFAULT_CHARSET,
                        OUT_DEFAULT_PRECIS,
                        CLIP_DEFAULT_PRECIS,
                        ANTIALIASED_QUALITY,
                        FF_DONTCARE.0 as u32,
                        PCWSTR(font_name.as_ptr()),
                    )
                    .0,
                )
            });
            let old_font = SelectObject(mem_dc, font);
            let _ = SetBkMode(mem_dc, TRANSPARENT);
            let _ = SetTextColor(
                mem_dc,
                COLORREF(
                    (text.color[2] as u32) << 16
                        | (text.color[1] as u32) << 8
                        | text.color[0] as u32,
                ),
            );
            let mut wide_text = text
                .text
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let text_utf16 = &wide_text[..wide_text.len().saturating_sub(1)];
            let mut text_size = SIZE { cx: 0, cy: 0 };
            let _ = GetTextExtentPoint32W(mem_dc, text_utf16, &mut text_size);
            let mut metrics = TEXTMETRICW::default();
            let _ = GetTextMetricsW(mem_dc, &mut metrics);
            let pad = (text.font_size / 5).max(4);
            let center_x = text.x - min_x;
            let center_y = text.y - min_y;
            let baseline_y = center_y + ((metrics.tmAscent - metrics.tmDescent) / 2);
            let marker_rect = if text.rotation_deg.abs() < f32::EPSILON {
                let half_w = (text_size.cx + 1) / 2;
                let half_h = (text_size.cy + 1) / 2;
                RECT {
                    left: center_x - half_w - pad,
                    top: center_y - half_h - pad,
                    right: center_x + half_w + pad,
                    bottom: center_y + half_h + pad,
                }
            } else {
                let radius = ((((text_size.cx * text_size.cx + text_size.cy * text_size.cy) as f32)
                    .sqrt()
                    .ceil() as i32)
                    + pad)
                    .max(text.font_size + pad);
                RECT {
                    left: center_x - radius,
                    top: center_y - radius,
                    right: center_x + radius,
                    bottom: center_y + radius,
                }
            };
            let marker_start_y = marker_rect.top.max(0).min(height as i32);
            let marker_end_y = marker_rect.bottom.max(0).min(height as i32);
            let marker_start_x = marker_rect.left.max(0).min(width as i32);
            let marker_end_x = marker_rect.right.max(0).min(width as i32);
            let backup_width = (marker_end_x - marker_start_x).max(0) as usize;
            let backup_height = (marker_end_y - marker_start_y).max(0) as usize;
            let mut background_backup = vec![0_u8; backup_width * backup_height * 4];
            for py in marker_start_y..marker_end_y {
                for px in marker_start_x..marker_end_x {
                    let index = ((py as usize) * (width as usize) + (px as usize)) * 4;
                    if index + 3 >= pixels.len() {
                        continue;
                    }
                    let backup_index = (((py - marker_start_y) as usize) * backup_width
                        + (px - marker_start_x) as usize)
                        * 4;
                    background_backup[backup_index..backup_index + 4]
                        .copy_from_slice(&pixels[index..index + 4]);
                    pixels[index] = label_bg[2];
                    pixels[index + 1] = label_bg[1];
                    pixels[index + 2] = label_bg[0];
                    pixels[index + 3] = 0;
                }
            }
            let old_align = SetTextAlign(mem_dc, TA_CENTER | TA_BASELINE);
            let _ = TextOutW(mem_dc, center_x, baseline_y, text_utf16);
            let _ = SetTextAlign(mem_dc, windows::Win32::Graphics::Gdi::TEXT_ALIGN_OPTIONS(old_align));
            let text_alpha = text.color[3].max(1);
            let start_y = marker_rect.top.max(0).min(height as i32);
            let end_y = marker_rect.bottom.max(0).min(height as i32);
            let start_x = marker_rect.left.max(0).min(width as i32);
            let end_x = marker_rect.right.max(0).min(width as i32);
            for py in start_y..end_y {
                for px in start_x..end_x {
                    let index = ((py as usize) * (width as usize) + (px as usize)) * 4;
                    if index + 3 >= pixels.len() {
                        continue;
                    }
                    let backup_index = (((py - marker_start_y) as usize) * backup_width
                        + (px - marker_start_x) as usize)
                        * 4;

                    let chunk = &mut pixels[index..index + 4];
                    let is_background = chunk[0] == label_bg[2]
                        && chunk[1] == label_bg[1]
                        && chunk[2] == label_bg[0];
                    if is_background {
                        chunk.copy_from_slice(&background_backup[backup_index..backup_index + 4]);
                    } else {
                        chunk[3] = chunk[3].max(text_alpha);
                    }
                }
            }
            let _ = SelectObject(mem_dc, old_font);
        }

        for font in label_font_cache.into_values() {
            let _ = DeleteObject(font);
        }

        for rect in rects_to_fix {
            let start_y = (rect.top).max(0).min(height as i32);
            let end_y = (rect.bottom).max(0).min(height as i32);
            let start_x = (rect.left).max(0).min(width as i32);
            let end_x = (rect.right).max(0).min(width as i32);
            for py in start_y..end_y {
                for px in start_x..end_x {
                    let index = ((py as usize) * (width as usize) + (px as usize)) * 4;
                    if index + 3 < pixels.len() {
                        let chunk = &mut pixels[index..index + 4];
                        if chunk[3] == 0
                            && (chunk[0] != 0 || chunk[1] != 0 || chunk[2] != 0)
                        {
                            chunk[3] = 255;
                        }
                    }
                }
            }
        }

        let source = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: width,
            cy: height,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            None,
            Some(&size),
            Some(mem_dc),
            Some(&source),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        Ok(())
    }





















    unsafe fn paint_timer_hwnd(hwnd: HWND, preset: &TimerPreset, text: &str) -> Result<()> {
        let window_x = preset.x.max(0);
        let window_y = preset.y.max(0);
        let width = preset.width.max(1);
        let height = preset.height.max(1);
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits_ptr: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            Some(mem_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        )?;
        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        let bg_opacity = preset.background_opacity;
        let bg_alpha = (bg_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let bytes_len = (width as usize) * (height as usize) * 4;
        let pixels = std::slice::from_raw_parts_mut(bits_ptr as *mut u8, bytes_len);
        let radius = if preset.rounded_background { 16.0 } else { 0.0 };
        let bg_color = &preset.background_color;
        let bg_b = ((bg_color.b as u32 * bg_alpha as u32) / 255) as u8;
        let bg_g = ((bg_color.g as u32 * bg_alpha as u32) / 255) as u8;
        let bg_r = ((bg_color.r as u32 * bg_alpha as u32) / 255) as u8;
        for py in 0..height {
            for px in 0..width {
                let index = ((py as usize) * (width as usize) + (px as usize)) * 4;
                let inside = if radius <= 0.0 {
                    true
                } else {
                    let px_f = px as f32 + 0.5;
                    let py_f = py as f32 + 0.5;
                    let inner_left = radius;
                    let inner_right = width as f32 - radius;
                    let inner_top = radius;
                    let inner_bottom = height as f32 - radius;
                    if (px_f >= inner_left && px_f <= inner_right)
                        || (py_f >= inner_top && py_f <= inner_bottom)
                    {
                        true
                    } else {
                        let corner_x = if px_f < inner_left {
                            inner_left
                        } else {
                            inner_right
                        };
                        let corner_y = if py_f < inner_top {
                            inner_top
                        } else {
                            inner_bottom
                        };
                        let dx = px_f - corner_x;
                        let dy = py_f - corner_y;
                        (dx * dx) + (dy * dy) <= radius * radius
                    }
                };
                if inside && bg_alpha > 0 {
                    pixels[index] = bg_b;
                    pixels[index + 1] = bg_g;
                    pixels[index + 2] = bg_r;
                    pixels[index + 3] = bg_alpha;
                } else {
                    pixels[index] = 0;
                    pixels[index + 1] = 0;
                    pixels[index + 2] = 0;
                    pixels[index + 3] = 0;
                }
            }
        }

        let font_name = "Segoe UI"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let font_size = preset.font_size;
        let font = CreateFontW(
            -(font_size.round() as i32).max(1),
            0,
            0,
            0,
            FW_MEDIUM.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            FF_DONTCARE.0 as u32,
            PCWSTR(font_name.as_ptr()),
        );
        let old_font = SelectObject(mem_dc, HGDIOBJ(font.0));
        let _ = SetBkMode(mem_dc, TRANSPARENT);
        let text_color = &preset.text_color;
        let _ = SetTextColor(
            mem_dc,
            COLORREF(
                ((text_color.b as u32) << 16)
                    | ((text_color.g as u32) << 8)
                    | (text_color.r as u32),
            ),
        );
        if preset.show_text {
            let mut text_rect = RECT {
                left: 12,
                top: 4,
                right: width - 12,
                bottom: height - 4,
            };
            let mut wide = text
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let _ = DrawTextW(
                mem_dc,
                &mut wide,
                &mut text_rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }

        let text_alpha = text_color.a.max(1);
        for py in 0..height {
            for px in 0..width {
                let index = ((py as usize) * (width as usize) + (px as usize)) * 4;
                let chunk = &mut pixels[index..index + 4];
                let looks_like_bg = chunk[0] == bg_b
                    && chunk[1] == bg_g
                    && chunk[2] == bg_r
                    && chunk[3] == bg_alpha;
                let alpha = if looks_like_bg {
                    bg_alpha
                } else if chunk[0] == 0 && chunk[1] == 0 && chunk[2] == 0 && chunk[3] == 0 {
                    0
                } else {
                    text_alpha
                };
                chunk[3] = alpha;
                chunk[0] = ((chunk[0] as u32 * alpha as u32) / 255) as u8;
                chunk[1] = ((chunk[1] as u32 * alpha as u32) / 255) as u8;
                chunk[2] = ((chunk[2] as u32 * alpha as u32) / 255) as u8;
            }
        }

        let mut pt_src = POINT::default();
        let mut pt_dst = POINT {
            x: window_x,
            y: window_y,
        };
        let mut size_wnd = SIZE {
            cx: width,
            cy: height,
        };
        let mut blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&mut pt_dst),
            Some(&mut size_wnd),
            Some(mem_dc),
            Some(&mut pt_src),
            COLORREF(0),
            Some(&mut blend),
            ULW_ALPHA,
        );
        let _ = SelectObject(mem_dc, old_font);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        let _ = ShowWindow(hwnd, SW_SHOWNA);
        Ok(())
    }

    fn refresh_timer_overlays(runtime: &mut Runtime) -> Result<()> {
        let mut active_timer_ids = HashSet::new();
        let mut presets_to_render = Vec::new();
        if let Some(preview) = runtime.preview_timer_preset.clone() {
            presets_to_render.push(preview);
        }

        let hook_guard = HOOK_STATE.lock();
        let active_timers = hook_guard.active_timers.clone();
        let timer_presets = hook_guard.timer_presets.clone();
        drop(hook_guard);
        for preset in &timer_presets {
            if let Some(state) = active_timers.get(&preset.id) {
                if state.running || state.elapsed_ms > 0 {
                    if !presets_to_render.iter().any(|p| p.id == preset.id) {
                        presets_to_render.push(preset.clone());
                    }
                }
            }
        }

        for preset in presets_to_render {
            active_timer_ids.insert(preset.id);
            let mut just_finished = false;
            let text = if let Some(state) = active_timers.get(&preset.id) {
                let elapsed = state.get_elapsed_ms();
                if preset.is_countdown {
                    let total_ms = (preset.duration_secs as u64) * 1000;
                    let remaining = if elapsed >= total_ms {
                        if state.running {
                            let mut lock = HOOK_STATE.lock();
                            let removed_state = lock.active_timers.remove(&preset.id);
                            drop(lock);
                            request_ui_repaint();
                            just_finished = true;
                            if let Some(t_state) = removed_state {
                                if let Some(macro_id) = t_state.on_complete_macro_preset_id {
                                    spawn_macro_by_preset_id(macro_id, true);
                                }
                            }
                        }

                        0
                    } else {
                        total_ms - elapsed
                    };
                    let display_ms = remaining;
                    format_stopwatch_time(
                        display_ms,
                        preset.show_minutes,
                        preset.show_seconds,
                        preset.show_ms,
                    )
                } else {
                    format_stopwatch_time(
                        elapsed,
                        preset.show_minutes,
                        preset.show_seconds,
                        preset.show_ms,
                    )
                }
            } else {
                if preset.is_countdown {
                    let total_ms = (preset.duration_secs as u64) * 1000;
                    format_stopwatch_time(
                        total_ms,
                        preset.show_minutes,
                        preset.show_seconds,
                        preset.show_ms,
                    )
                } else {
                    format_stopwatch_time(
                        0,
                        preset.show_minutes,
                        preset.show_seconds,
                        preset.show_ms,
                    )
                }
            };
            if just_finished {
                if let Some(&hwnd) = runtime.timer_hwnds.get(&preset.id) {
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                    }
                }

                runtime.timer_hwnds.remove(&preset.id);
                continue;
            }

            let hwnd = match runtime.timer_hwnds.get(&preset.id) {
                Some(&hwnd) => hwnd,
                None => {
                    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }?.0);
                    let hwnd = unsafe {
                        CreateWindowExW(
                            WS_EX_LAYERED
                                | WS_EX_TOOLWINDOW
                                | WS_EX_TOPMOST
                                | WS_EX_NOACTIVATE
                                | WS_EX_TRANSPARENT,
                            w!("CrosshairOverlay"),
                            w!("CrosshairTimer"),
                            WS_POPUP,
                            0,
                            0,
                            preset.width.max(1),
                            preset.height.max(1),
                            None,
                            None,
                            Some(instance),
                            None,
                        )?
                    };
                    runtime.timer_hwnds.insert(preset.id, hwnd);
                    hwnd
                }
            };
            unsafe {
                let _ = paint_timer_hwnd(hwnd, &preset, &text);
            }
        }

        let mut keys_to_remove = Vec::new();
        for (&preset_id, &hwnd) in &runtime.timer_hwnds {
            if !active_timer_ids.contains(&preset_id) {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                }

                keys_to_remove.push(preset_id);
            }
        }

        for key in keys_to_remove {
            runtime.timer_hwnds.remove(&key);
        }

        Ok(())
    }

    fn execute_timer_preset_action(
        action: MacroAction,
        timer_preset_id: Option<u32>,
        on_complete_macro_preset_id: Option<u32>,
    ) {
        let Some(preset_id) = timer_preset_id else {
            return;
        };
        let mut hook_state = HOOK_STATE.lock();
        match action {
            MacroAction::StartTimerPreset => {
                let state = hook_state
                    .active_timers
                    .entry(preset_id)
                    .or_insert_with(|| ActiveTimerState {
                        running: false,
                        start_time: None,
                        elapsed_ms: 0,
                        on_complete_macro_preset_id: None,
                    });
                state.on_complete_macro_preset_id = on_complete_macro_preset_id;
                if !state.running {
                    state.running = true;
                    state.start_time = Some(Instant::now());
                }
            }

            MacroAction::PauseTimerPreset => {
                if let Some(state) = hook_state.active_timers.get_mut(&preset_id) {
                    if state.running {
                        state.running = false;
                        if let Some(start) = state.start_time {
                            state.elapsed_ms += start.elapsed().as_millis() as u64;
                        }

                        state.start_time = None;
                    }
                }
            }

            MacroAction::StopTimerPreset => {
                hook_state.active_timers.remove(&preset_id);
            }

            _ => {}
        }

        drop(hook_state);
        wake_command_queue();
        request_ui_repaint();
    }

    fn spawn_macro_by_preset_id(preset_id: u32, bypass_enabled: bool) {
        let preset = {
            let hook_state = HOOK_STATE.lock();
            hook_state
                .macro_groups
                .iter()
                .flat_map(|group| group.presets.iter())
                .find(|preset| preset.id == preset_id)
                .cloned()
        };
        if let Some(preset) = preset {
            let hotkey_id = preset.id as i32;
            SUPPRESSED_MACRO_HOTKEYS.lock().insert(hotkey_id);
            STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
            FORCE_STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
            thread::spawn(move || {
                MACRO_TARGETED_WINDOWS.with(|set| set.borrow_mut().clear());
                let cleanup_steps = collect_macro_release_steps(&preset.steps);
                let mut press_locked_keys: Vec<String> = Vec::new();
                let mut press_locked_mouse_masks: Vec<MouseMoveLockMask> = Vec::new();
                let step_indices: Vec<usize> = (0..preset.steps.len()).collect();
                let _ = execute_macro_sequence(
                    preset.id,
                    &preset.steps,
                    &step_indices,
                    &mut press_locked_keys,
                    &mut press_locked_mouse_masks,
                    preset.stop_on_retrigger_immediate,
                    None,
                    &[],
                    false,
                    bypass_enabled,
                );
                for step in cleanup_steps {
                    let _ = send_key_event(&step);
                }
                STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
                FORCE_STOP_REQUESTED_MACRO_PRESETS.lock().remove(&preset.id);
                SUPPRESSED_MACRO_HOTKEYS.lock().remove(&hotkey_id);
            });
        }
    }

    fn widestring(value: &str) -> Vec<u16> {
        let mut wide: Vec<u16> = value.encode_utf16().collect();
        wide.push(0);
        wide
    }

    unsafe fn runtime_icon_path(hwnd: HWND, enabled: bool) -> Result<Vec<u16>> {
        let runtime = runtime_mut(hwnd).context("Runtime was not available for tray icon")?;
        let path = if enabled {
            &runtime.paths.icon_file
        } else {
            &runtime.paths.icon_file_disabled
        };
        Ok(widestring(&path.to_string_lossy()))
    }

    pub(crate) fn is_geometry_active(preset_id: u32, step_index: usize) -> bool {
        let hook_state = HOOK_STATE.lock();
        hook_state.active_geometry_steps.contains_key(&(preset_id, step_index))
    }

    pub(crate) fn stop_geometry(preset_id: u32, step_index: usize) {
        let mut hook_state = HOOK_STATE.lock();
        hook_state.active_geometry_steps.remove(&(preset_id, step_index));
        hook_state.rendered_geometry_steps.remove(&(preset_id, step_index));
        hook_state.active_geometry_steps_expires.remove(&(preset_id, step_index));
        drop(hook_state);
        send_overlay_command(OverlayCommand::RefreshSearchAreaOverlay);
    }






    pub(crate) fn is_crosshair_active(profile_name: &str) -> bool {
        let name = profile_name.trim();
        if name.is_empty() {
            return false;
        }
        let hook_state = HOOK_STATE.lock();
        hook_state.active_crosshair_profile_name.as_deref() == Some(name)
    }

    pub(crate) fn is_pin_active(preset_id_str: &str) -> bool {
        let preset_id = match preset_id_str.trim().parse::<u32>() {
            Ok(id) => id,
            Err(_) => return false,
        };
        let hook_state = HOOK_STATE.lock();
        hook_state.active_pin_preset_id == Some(preset_id)
    }

    pub(crate) fn is_hud_active(preset_id_str: &str) -> bool {
        let preset_id = match preset_id_str.trim().parse::<u32>() {
            Ok(id) => id,
            Err(_) => return false,
        };
        let hud_state = HUD_DISPLAY.lock();
        if hud_state.as_ref().and_then(|h| h.preset_id) == Some(preset_id) {
            return true;
        }
        let preview_state = HUD_PREVIEW_DISPLAY.lock();
        preview_state.as_ref().and_then(|h| h.preset_id) == Some(preset_id)
    }

    pub(crate) fn is_video_active(preset_id_str: &str) -> bool {
        let preset_id = match preset_id_str.trim().parse::<u32>() {
            Ok(id) => id,
            Err(_) => return false,
        };
        let video_id = ACTIVE_VIDEO_PRESET_ID.lock();
        *video_id == Some(preset_id)
    }
}

#[cfg(windows)]
pub use windows_overlay::*;
#[cfg(not(windows))]
mod fallback {
    use anyhow::{Result, bail};
    use crate::{
        model::{
            AudioSettings, CrosshairStyle, MacroGroup, ProfileRecord, RgbaColor, VisionPreset,
            HotkeyBinding, WindowExpandControls, WindowFocusPreset, WindowLayout, WindowPreset,
        },
        storage::AppPaths,
    };
    #[derive(Debug, Clone)]
    pub enum OverlayCommand {
        Update(CrosshairStyle),
        UpdateProfiles(Vec<ProfileRecord>),
        UpdateCrosshairProfile {
            index: usize,
            profile: ProfileRecord,
        },
        UpdateWindowPresets(Vec<WindowPreset>),
        UpdateWindowFocusPresets(Vec<WindowFocusPreset>),
        UpdateWindowLayouts(Vec<WindowLayout>),
        ApplyWindowLayout(WindowLayout),
        UpdateWindowExpandControls(WindowExpandControls),
        UpdateMacroPresets(Vec<MacroGroup>),
        UpdateAudioSettings(AudioSettings),
        PlayVideoPreset(u32),
        PlayVideoPresetFrom(u32, u64),
        StopVideoPlayback,
        UpdateKeyboardArrowMouseSettings {
            enabled: bool,
            step_px: u32,
        },
        UpdateMacroDelays {
            mouse_click_delay_ms: u32,
            keyboard_key_press_delay_ms: u32,
        },
        UpdateVisionPresets(Vec<VisionPreset>),
        SetArduinoFlashInProgress(bool),
        SetMacrosMasterEnabled(bool),
        SetNativeFocusHighlightEnabled(bool),
        UpdateQuickKeyDisplayConfig {
            enabled: bool,
            center_x: i32,
            center_y: i32,
            size: f32,
        },
        ShowQuickKeyDisplay(Option<String>),
        UpdateScreenDrawConfig {
            enabled: bool,
            trigger: Option<HotkeyBinding>,
            color: RgbaColor,
            brush_size: f32,
            smoothing: bool,
            smoothing_amount: f32,
        },
        SetUiVisible(bool),
        SetTrayIconVisible(bool),
        Exit,
        ToggleMacroRecording(u32, u32, String),
        UpdateTimerPresets(Vec<TimerPreset>),
        PreviewTimerPreset(Option<TimerPreset>),
    }

    #[derive(Debug, Clone)]
    pub enum UiCommand {
        ShowWindow,
        Exit,
        StartupIconLoaded(std::sync::Arc<eframe::egui::IconData>),
        StartupStateLoaded {
            state: crate::model::AppState,
            startup_state_dirty: bool,
        },
        VisionFinished(String),
        MacroStepInlineFeedback {
            preset_id: u32,
            step_index: usize,
            message: String,
            open_groq_settings: bool,
        },
        VisionPointCaptureCancelled(String),
        MacroRealtimeStepRemoved(u32, u32),
        CustomCommandResult { preset_id: u32, output: String },
        OpenWindowsLoaded {
            windows: Vec<String>,
            status: Option<String>,
        },
        AudioSenseDevicesLoaded {
            devices: Vec<String>,
        },
        VideoPlaybackFinished(u32),
    }

    pub struct OverlayHandle;
    impl OverlayHandle {
        pub fn send(&self, _command: OverlayCommand) {}
    }

    pub fn wake_command_queue() {}

    pub fn spawn_custom_command(
        _preset_id: Option<u32>,
        _use_powershell: bool,
        _command_text: String,
    ) {
    }

    pub fn start(
        _paths: AppPaths,
        _initial_style: CrosshairStyle,
        _ui_tx: crossbeam_channel::Sender<UiCommand>,
    ) -> Result<OverlayHandle> {
        bail!("This application currently supports Windows only")
    }

    pub static ACTIVE_MACRO_STEPS: once_cell::sync::Lazy<
        parking_lot::Mutex<std::collections::HashMap<u32, std::collections::HashSet<usize>>>,
    > = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));
    pub fn is_vision_following_active_by_spec(_spec: &str) -> bool {
        false
    }

    pub fn is_timer_preset_active(_t_id: Option<u32>) -> bool {
        false
    }

    pub(crate) fn is_geometry_active(_preset_id: u32, _step_index: usize) -> bool {
        false
    }

    pub(crate) fn stop_geometry(_preset_id: u32, _step_index: usize) {}






    pub(crate) fn is_crosshair_active(_profile_name: &str) -> bool {
        false
    }

    pub(crate) fn is_pin_active(_preset_id_str: &str) -> bool {
        false
    }

    pub(crate) fn is_hud_active(_preset_id_str: &str) -> bool {
        false
    }

    pub(crate) fn is_video_active(_preset_id_str: &str) -> bool {
        false
    }

    pub(crate) fn enable_crosshair_profile(_spec: &str) -> Result<()> {
        Ok(())
    }

    pub(crate) fn disable_crosshair_profile(_spec: &str) {}

    pub(crate) fn enable_pin_preset(_spec: &str) -> Result<()> {
        Ok(())
    }

    pub(crate) fn disable_pin_preset(_spec: &str) {}

    pub(crate) fn hide_hud_now() {}
}

#[cfg(not(windows))]
pub use fallback::*;
