use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

use super::audio_model::AudioSenseSpec;
use super::geometry_model::{GeometrySpec, HideGeometryMode, SetVariableSource};
use super::window_model::HotkeyBinding;
use super::{
    default_condition_join_operator, default_false, default_if_color_tolerance,
    default_if_operator, default_image_search_color_scan_rate_hz,
    default_image_search_color_tolerance, default_image_search_move_delay_ms,
    default_image_search_move_passes, default_macro_step_ocr_language, default_ocr_height,
    default_ocr_width, default_true,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum MacroAction {
    #[default]
    KeyPress,
    KeyDown,
    KeyUp,
    Wait,
    TypeText,
    ApplyWindowPreset,
    FocusWindowPreset,
    TriggerMacroPreset,
    TriggerMacroPresetIfEnabled,
    StopMacroPreset,
    #[serde(alias = "TriggerCustomPreset")]
    TriggerCommandPreset,
    DisableNetworkAdapter,
    EnableNetworkAdapter,
    CutInternetRoute,
    RestoreInternetRoute,
    SetWifiRadioOff,
    SetWifiRadioOn,
    EnableCrosshairProfile,
    DisableCrosshair,
    EnablePinPreset,
    DisablePin,
    PlayMousePathPreset,
    ApplyMouseSensitivityPreset,
    EnableZoomPreset,
    DisableZoom,
    PlaySoundPreset,
    #[serde(alias = "StartImageSearch")]
    StartVisionSearch,
    #[serde(alias = "ScanImageOnce", alias = "ScanVisionOnce")]
    ScanVisionOnce,
    #[serde(alias = "StartPitchDetect", alias = "StartSpatialAudioDetect")]
    StartAudioSensePreset,
    #[serde(alias = "StopAudioSensePreset")]
    StopAudioSense,

    #[serde(alias = "StopImageSearchWait")]
    StopVisionWait,
    #[serde(alias = "StopImageSearch")]
    StopVision,
    LoopStart,
    LoopEnd,
    StopIfTriggerPressedAgain,
    StopIfKeyPressed,
    #[serde(alias = "ShowToolbox")]
    ShowHud,
    #[serde(alias = "HideToolbox")]
    HideHud,
    HideTaskbar,
    ShowTaskbar,
    LockKeys,
    UnlockKeys,
    LockMouse,
    UnlockMouse,
    EnableMacroPreset,
    DisableMacroPreset,
    MouseLeftClick,
    MouseLeftDown,
    MouseLeftUp,
    MouseRightClick,
    MouseRightDown,
    MouseRightUp,
    MouseMiddleClick,
    MouseMiddleDown,
    MouseMiddleUp,
    MouseX1Click,
    MouseX1Down,
    MouseX1Up,
    MouseX2Click,
    MouseX2Down,
    MouseX2Up,
    MouseWheelUp,
    MouseWheelDown,
    MouseMoveAbsolute,
    MouseMoveRelative,
    TriggerVisionTiming,
    StartVisionTiming,
    StopVisionTiming,
    IfStart,
    Else,
    IfEnd,
    SetVariable,
    StartTimerPreset,
    PauseTimerPreset,
    StopTimerPreset,
    ReadTimerPreset,
    EnableStep,
    DisableStep,
    OcrSearch,
    DrawGeometry,
    ShowGeometryPreset,
    HideGeometryPreset,
    FunnyMemeReply,
    JumpToStep,
    #[serde(other)]
    Legacy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum IfConditionType {
    #[default]
    Variable,
    PixelColor,
    VisionMatch,
    KeyHeld,
    MouseHeld,
    MousePosition,
    PresetRunning,
    OcrMatch,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum VisionMoveAxisLock {
    #[default]
    None,
    HorizontalOnly,
    VerticalOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ExtraCondition {
    #[serde(default = "default_condition_join_operator")]
    pub join_operator: String,
    #[serde(default)]
    pub condition_type: IfConditionType,
    pub variable_name: String,
    #[serde(default = "default_if_operator")]
    pub operator: String,
    pub compare_value: i32,
    pub expression: String,
    #[serde(default)]
    pub ocr_preset_id: Option<u32>,
    #[serde(default)]
    pub ocr_target_text: String,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub target_color: String,
    #[serde(default = "default_if_color_tolerance")]
    pub color_tolerance: u8,
    #[serde(default)]
    pub vision_preset_id: Option<u32>,
    #[serde(default)]
    pub key_held_name: String,
    #[serde(default)]
    pub mouse_button: String,
    #[serde(default)]
    pub mouse_axis: String,
    #[serde(default)]
    pub running_preset_id: Option<u32>,
    #[serde(skip)]
    pub running_preset_group_id: Option<u32>,
}

impl Default for ExtraCondition {
    fn default() -> Self {
        Self {
            join_operator: "AND".to_string(),
            condition_type: IfConditionType::Variable,
            variable_name: String::new(),
            operator: "==".to_string(),
            compare_value: 0,
            expression: String::new(),
            ocr_preset_id: None,
            ocr_target_text: String::new(),
            x: 0,
            y: 0,
            target_color: String::new(),
            color_tolerance: 5,
            vision_preset_id: None,
            key_held_name: String::new(),
            mouse_button: "Left".to_string(),
            mouse_axis: "X".to_string(),
            running_preset_id: None,
            running_preset_group_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MacroStep {
    pub key: String,
    pub action: MacroAction,
    pub delay_ms: u64,
    #[serde(default)]
    pub delay_expr: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub x_expr: String,
    #[serde(default)]
    pub y_expr: String,
    pub text_override: String,
    #[serde(default, alias = "custom_preset_command")]
    pub command_preset_command: String,
    #[serde(default = "default_false", alias = "custom_preset_use_powershell")]
    pub command_preset_use_powershell: bool,
    pub timed_override: bool,
    pub duration_override_ms: u64,
    #[serde(default)]
    pub duration_expr: String,
    pub smooth_mouse_path: bool,
    #[serde(default)]
    pub mouse_speed_expr: String,
    pub mouse_speed_percent: u32,
    #[serde(default = "default_false", alias = "image_search_move_cursor_on_match")]
    pub vision_move_cursor_on_match: bool,
    #[serde(default)]
    pub vision_move_axis_lock: VisionMoveAxisLock,
    #[serde(default)]
    pub vision_move_relative: bool,
    #[serde(default)]
    pub vision_move_offset_x: i32,
    #[serde(default)]
    pub vision_move_offset_y: i32,
    #[serde(default = "default_image_search_move_passes")]
    pub vision_move_passes: u8,
    #[serde(default = "default_image_search_move_delay_ms")]
    pub vision_move_delay_ms: u64,
    #[serde(default = "default_image_search_color_tolerance")]
    pub vision_color_tolerance: u8,
    #[serde(default = "default_image_search_color_scan_rate_hz")]
    pub vision_color_scan_rate_hz: u32,
    #[serde(default, alias = "image_search_wait_until_found")]
    pub vision_wait_until_found: bool,
    #[serde(default, alias = "image_search_trigger_macro_enabled")]
    pub vision_trigger_macro_enabled: bool,
    #[serde(default, alias = "image_search_trigger_macro_preset_id")]
    pub vision_trigger_macro_preset_id: Option<u32>,
    #[serde(default)]
    pub if_condition_type: IfConditionType,
    #[serde(default)]
    pub if_target_color: String,
    #[serde(default = "default_if_color_tolerance")]
    pub if_color_tolerance: u8,
    #[serde(default)]
    pub if_vision_preset_id: Option<u32>,
    #[serde(default)]
    pub if_ocr_preset_id: Option<u32>,
    #[serde(default)]
    pub if_key_held_name: String,
    #[serde(default)]
    pub if_mouse_button: String,
    #[serde(default)]
    pub if_mouse_axis: String,
    #[serde(default)]
    pub if_running_preset_id: Option<u32>,
    #[serde(skip)]
    pub if_running_preset_group_id: Option<u32>,
    #[serde(default)]
    pub timer_preset_id: Option<u32>,
    #[serde(default)]
    pub timer_on_complete_macro_preset_id: Option<u32>,
    #[serde(default = "default_true")]
    pub lock_mouse_left: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_right: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_middle: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_scroll: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_x1: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_x2: bool,
    #[serde(default = "default_true")]
    pub lock_mouse_move: bool,
    #[serde(default = "default_false")]
    pub toggle_enabled_on_run: bool,
    #[serde(default = "default_false")]
    pub loop_finish_iteration_on_stop: bool,
    #[serde(default)]
    pub if_variable_name: String,
    #[serde(default = "default_if_operator")]
    pub if_operator: String,
    #[serde(default)]
    pub manual_mouse_sensitivity: bool,
    #[serde(default)]
    pub break_loop_by_variable: bool,
    #[serde(default)]
    pub break_loop_mode: String,
    #[serde(default)]
    pub if_compare_value: i32,
    #[serde(default)]
    pub if_compare_by_expression: bool,
    #[serde(default)]
    pub extra_conditions: Vec<ExtraCondition>,
    #[serde(default)]
    pub wait_time_unit: String,
    #[serde(default = "default_true")]
    pub unlock_on_exit: bool,
    #[serde(default)]
    pub set_variable_source: SetVariableSource,
    #[serde(default = "default_false")]
    pub wait_for_completion: bool,
    #[serde(default)]
    pub auto_key_up_on_macro_end: bool,
    #[serde(default = "default_ocr_width")]
    pub ocr_width: i32,
    #[serde(default = "default_ocr_height")]
    pub ocr_height: i32,
    #[serde(default)]
    pub ocr_target_text: String,
    #[serde(default)]
    pub ocr_success_var: String,
    #[serde(default)]
    pub ocr_pos_var_x: String,
    #[serde(default)]
    pub ocr_pos_var_y: String,
    #[serde(default)]
    pub ocr_numeric_var: String,
    #[serde(default)]
    pub ocr_text_var: String,
    #[serde(default = "default_macro_step_ocr_language")]
    pub ocr_language: String,
    #[serde(default)]
    pub vision_pos_var_x: String,
    #[serde(default)]
    pub vision_pos_var_y: String,
    #[serde(default)]
    pub vision_found_var: String,
    #[serde(default)]
    pub audio_sense_preset_id: Option<u32>,
    #[serde(default)]
    pub audio_sense_spec: AudioSenseSpec,
    #[serde(default = "default_true")]
    pub audio_sense_collapsed: bool,
    #[serde(default)]
    pub audio_sense_stop_all: bool,
    #[serde(default)]
    pub geometry_preset_id: Option<u32>,
    #[serde(default)]
    pub geometry_preset_use_custom_ref: bool,
    #[serde(default)]
    pub geometry_preset_modify_enabled: bool,
    #[serde(default)]
    pub geometry_hide_mode: HideGeometryMode,
    #[serde(default)]
    pub geometry_preset_modify_initialized: bool,
    #[serde(default)]
    pub geometry_spec: GeometrySpec,
    #[serde(default = "default_true")]
    pub geometry_collapsed: bool,
    #[serde(default)]
    pub trigger_macro_group_id: Option<u32>,
}

impl Default for MacroStep {
    fn default() -> Self {
        Self {
            key: String::new(),
            action: MacroAction::KeyPress,
            delay_ms: 0,
            delay_expr: String::new(),
            enabled: true,
            x: 0,
            y: 0,
            x_expr: String::new(),
            y_expr: String::new(),
            text_override: String::new(),
            command_preset_command: String::new(),
            command_preset_use_powershell: false,
            timed_override: false,
            duration_override_ms: 1500,
            duration_expr: String::new(),
            smooth_mouse_path: false,
            mouse_speed_expr: String::new(),
            mouse_speed_percent: 100,
            vision_move_cursor_on_match: false,
            vision_move_axis_lock: VisionMoveAxisLock::None,
            vision_move_relative: false,
            vision_move_offset_x: 0,
            vision_move_offset_y: 0,
            vision_move_passes: 1,
            vision_move_delay_ms: 0,
            vision_color_tolerance: default_image_search_color_tolerance(),
            vision_color_scan_rate_hz: 100,
            vision_wait_until_found: false,
            vision_trigger_macro_enabled: false,
            vision_trigger_macro_preset_id: None,
            if_condition_type: IfConditionType::default(),
            if_target_color: String::new(),
            if_color_tolerance: 10,
            if_vision_preset_id: None,
            if_ocr_preset_id: None,
            if_key_held_name: String::new(),
            if_mouse_button: "MouseLeft".to_string(),
            if_mouse_axis: "X".to_string(),
            if_running_preset_id: None,
            if_running_preset_group_id: None,
            timer_preset_id: None,
            timer_on_complete_macro_preset_id: None,
            lock_mouse_left: true,
            lock_mouse_right: true,
            lock_mouse_middle: true,
            lock_mouse_scroll: true,
            lock_mouse_x1: true,
            lock_mouse_x2: true,
            lock_mouse_move: true,
            toggle_enabled_on_run: false,
            loop_finish_iteration_on_stop: false,
            if_variable_name: String::new(),
            if_operator: "==".to_string(),
            manual_mouse_sensitivity: false,
            break_loop_by_variable: false,
            break_loop_mode: String::new(),
            if_compare_value: 0,
            if_compare_by_expression: false,
            extra_conditions: Vec::new(),
            wait_time_unit: String::new(),
            unlock_on_exit: true,
            set_variable_source: SetVariableSource::Expression,
            wait_for_completion: false,
            auto_key_up_on_macro_end: false,
            ocr_width: 320,
            ocr_height: 180,
            ocr_target_text: String::new(),
            ocr_success_var: String::new(),
            ocr_pos_var_x: String::new(),
            ocr_pos_var_y: String::new(),
            ocr_numeric_var: String::new(),
            ocr_text_var: String::new(),
            ocr_language: default_macro_step_ocr_language(),
            vision_pos_var_x: String::new(),
            vision_pos_var_y: String::new(),
            vision_found_var: String::new(),
            audio_sense_preset_id: None,
            audio_sense_spec: AudioSenseSpec::default(),
            audio_sense_collapsed: true,
            audio_sense_stop_all: false,
            geometry_preset_id: None,
            geometry_preset_use_custom_ref: false,
            geometry_preset_modify_enabled: false,
            geometry_hide_mode: HideGeometryMode::Newest,
            geometry_preset_modify_initialized: false,
            geometry_spec: GeometrySpec::default(),
            geometry_collapsed: true,
            trigger_macro_group_id: None,
        }
    }
}

static DEFAULT_LAZY_MACRO_STEP: Lazy<MacroStep> = Lazy::new(MacroStep::default);

#[derive(Debug, Clone, Default)]
pub struct LazyMacroStep(Option<Box<MacroStep>>);

impl LazyMacroStep {
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }
}

impl From<MacroStep> for LazyMacroStep {
    fn from(step: MacroStep) -> Self {
        if step == MacroStep::default() {
            Self::default()
        } else {
            Self(Some(Box::new(step)))
        }
    }
}

impl PartialEq for LazyMacroStep {
    fn eq(&self, other: &Self) -> bool {
        self.deref() == other.deref()
    }
}

impl Serialize for LazyMacroStep {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.deref().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LazyMacroStep {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let step = Option::<MacroStep>::deserialize(deserializer)?.unwrap_or_default();
        Ok(step.into())
    }
}

impl Deref for LazyMacroStep {
    type Target = MacroStep;

    fn deref(&self) -> &Self::Target {
        self.0.as_deref().unwrap_or(&DEFAULT_LAZY_MACRO_STEP)
    }
}

impl DerefMut for LazyMacroStep {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
            .get_or_insert_with(|| Box::new(MacroStep::default()))
            .as_mut()
    }
}

impl MacroStep {
    pub fn get_break_loop_mode(&self) -> &str {
        if self.break_loop_mode.is_empty() {
            if self.break_loop_by_variable {
                "VarCompare"
            } else if !self.key.trim().is_empty() {
                "StopKey"
            } else {
                "Immediate"
            }
        } else {
            &self.break_loop_mode
        }
    }

    pub fn get_delay_ms(&self) -> u64 {
        if !self.delay_expr.trim().is_empty() {
            let interpolated = crate::overlay::interpolate_variables(&self.delay_expr);
            let base_val = crate::overlay::evaluate_math_expression(&interpolated);
            let multiplier = match self.wait_time_unit.as_str() {
                "s" => 1000,
                "m" => 60000,
                "h" => 3600000,
                _ => 1,
            };
            (base_val.max(0) as u64) * multiplier
        } else {
            self.delay_ms
        }
    }

    pub fn get_duration_ms(&self) -> u64 {
        match self.action {
            MacroAction::EnableCrosshairProfile
            | MacroAction::EnablePinPreset
            | MacroAction::ShowHud
            | MacroAction::DrawGeometry => {
                let is_timed = self.timed_override
                    || (!self.duration_expr.trim().is_empty() && self.duration_expr.trim() != "0");
                if is_timed && !self.duration_expr.trim().is_empty() {
                    let trimmed = self.duration_expr.trim();
                    let interpolated = crate::overlay::interpolate_variables(trimmed);
                    let val = crate::overlay::evaluate_math_expression(&interpolated);
                    val.max(0) as u64
                } else {
                    0
                }
            }
            _ => {
                if !self.duration_expr.trim().is_empty() {
                    let trimmed = self.duration_expr.trim();
                    let interpolated = crate::overlay::interpolate_variables(trimmed);
                    let val = crate::overlay::evaluate_math_expression(&interpolated);
                    val.max(0) as u64
                } else {
                    self.duration_override_ms
                }
            }
        }
    }

    pub fn get_x(&self) -> i32 {
        Self::resolve_i32_expression(&self.x_expr).unwrap_or(self.x)
    }

    pub fn get_y(&self) -> i32 {
        Self::resolve_i32_expression(&self.y_expr).unwrap_or(self.y)
    }

    pub fn duration_is_permanent(&self) -> bool {
        !self.timed_override
    }

    pub fn set_duration_permanent(&mut self, permanent: bool) {
        self.timed_override = !permanent;
        if !permanent {
            let trimmed = self.duration_expr.trim();
            if trimmed.is_empty() || trimmed == "0" {
                self.duration_expr = self.duration_override_ms.max(1).to_string();
            }
        }
    }

    pub fn remember_duration_input(&mut self) {
        let trimmed = self.duration_expr.trim();
        if trimmed.is_empty() || trimmed == "0" {
            return;
        }

        let interpolated = crate::overlay::interpolate_variables(trimmed);
        let value = crate::overlay::evaluate_math_expression(&interpolated).max(0) as u64;
        if value > 0 {
            self.duration_override_ms = value;
        }
    }

    pub fn get_mouse_speed_multiplier(&self) -> f32 {
        Self::resolve_mouse_speed_multiplier(&self.mouse_speed_expr)
            .unwrap_or_else(|| self.legacy_mouse_speed_multiplier())
    }

    pub fn format_mouse_speed_multiplier(multiplier: f32) -> String {
        let clamped = multiplier.clamp(0.1, 100.0);
        let mut number = format!("{clamped:.2}");
        while number.contains('.') && number.ends_with('0') {
            number.pop();
        }
        if number.ends_with('.') {
            number.pop();
        }
        format!("x{number}")
    }

    pub fn resolve_mouse_speed_multiplier(expr: &str) -> Option<f32> {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return None;
        }

        let interpolated = crate::overlay::interpolate_variables(trimmed);
        let normalized = interpolated
            .trim()
            .trim_start_matches('x')
            .trim_start_matches('X')
            .trim();

        if normalized.is_empty() {
            return None;
        }

        if let Ok(parsed) = normalized.parse::<f32>() {
            if parsed.is_finite() && parsed > 0.0 {
                return Some(parsed.clamp(0.1, 100.0));
            }
        }

        let evaluated = crate::overlay::evaluate_math_expression(normalized);
        if evaluated > 0 {
            Some((evaluated as f32).clamp(0.1, 100.0))
        } else {
            None
        }
    }

    fn resolve_i32_expression(expr: &str) -> Option<i32> {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return None;
        }

        let interpolated = crate::overlay::interpolate_variables(trimmed);
        Some(crate::overlay::evaluate_math_expression(&interpolated))
    }

    pub fn is_infinite_loop(&self) -> bool {
        self.action == MacroAction::LoopStart
            && matches!(
                self.key.trim().to_ascii_lowercase().as_str(),
                "infinite" | "inf" | "forever" | "-1"
            )
    }

    fn legacy_mouse_speed_multiplier(&self) -> f32 {
        self.mouse_speed_percent.max(10) as f32 / 100.0
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MacroTriggerMode {
    #[default]
    Press,
    Hold,
    Release,
    WindowFocus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterWindowPresetState {
    pub id: u32,
    pub enabled: bool,
    pub animate_enabled: bool,
    pub restore_titlebar_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterWindowFocusPresetState {
    pub id: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterZoomPresetState {
    pub id: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterMacroPresetState {
    pub id: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct MasterMacroGroupState {
    pub id: u32,
    pub enabled: bool,
    pub presets: Vec<MasterMacroPresetState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MasterPreset {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub macros_master_enabled: bool,
    pub window_expand_controls_enabled: bool,
    pub window_presets: Vec<MasterWindowPresetState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub window_focus_presets: Vec<MasterWindowFocusPresetState>,
    pub zoom_presets: Vec<MasterZoomPresetState>,
    pub macro_groups: Vec<MasterMacroGroupState>,
}

impl MasterPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Mode {id}"),
            collapsed: true,
            macros_master_enabled: true,
            window_expand_controls_enabled: false,
            window_presets: Vec::new(),
            window_focus_presets: Vec::new(),
            zoom_presets: Vec::new(),
            macro_groups: Vec::new(),
        }
    }
}

impl Default for MasterPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MacroPreset {
    pub id: u32,
    pub enabled: bool,
    pub collapsed: bool,
    pub trigger_mode: MacroTriggerMode,
    pub pass_through_press: bool,
    pub pass_through_hold: bool,
    pub stop_on_retrigger_immediate: bool,
    pub release_requires_all_inputs_released: bool,
    pub release_wait_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trigger_keys: String,
    pub hotkey: Option<HotkeyBinding>,
    pub event_target_window_title: Option<String>,
    pub event_extra_target_window_titles: Vec<String>,
    #[serde(default = "default_true")]
    pub event_match_duplicate_window_titles: bool,
    pub hold_stop_step_enabled: bool,
    #[serde(default, skip_serializing_if = "LazyMacroStep::is_empty")]
    pub hold_stop_step: LazyMacroStep,
    pub press_stop_step_enabled: bool,
    #[serde(default, skip_serializing_if = "LazyMacroStep::is_empty")]
    pub press_stop_step: LazyMacroStep,
    pub steps: Vec<MacroStep>,
    pub record_hotkey: Option<HotkeyBinding>,
    #[serde(skip)]
    pub acknowledged_infinite_loop: bool,
}

impl MacroPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            enabled: true,
            collapsed: true,
            trigger_mode: MacroTriggerMode::Press,
            pass_through_press: false,
            pass_through_hold: false,
            stop_on_retrigger_immediate: false,
            release_requires_all_inputs_released: false,
            release_wait_key: String::new(),
            trigger_keys: String::new(),
            hotkey: None,
            event_target_window_title: None,
            event_extra_target_window_titles: Vec::new(),
            event_match_duplicate_window_titles: true,
            hold_stop_step_enabled: false,
            hold_stop_step: LazyMacroStep::default(),
            press_stop_step_enabled: false,
            press_stop_step: LazyMacroStep::default(),
            steps: vec![MacroStep::default()],
            record_hotkey: None,
            acknowledged_infinite_loop: false,
        }
    }
}

impl Default for MacroPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MacroFolder {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
}

impl MacroFolder {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Folder {id}"),
            enabled: true,
            collapsed: false,
        }
    }
}

impl Default for MacroFolder {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MacroGroup {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    #[serde(default)]
    pub favorite: bool,
    pub folder_id: Option<u32>,
    pub target_window_title: Option<String>,
    pub extra_target_window_titles: Vec<String>,
    #[serde(default = "default_true")]
    pub match_duplicate_window_titles: bool,
    pub presets: Vec<MacroPreset>,
}

impl MacroGroup {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: "Macro Group".to_owned(),
            enabled: true,
            collapsed: false,
            favorite: false,
            folder_id: None,
            target_window_title: None,
            extra_target_window_titles: Vec::new(),
            match_duplicate_window_titles: true,
            presets: vec![MacroPreset::new(1)],
        }
    }
}

impl Default for MacroGroup {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{LazyMacroStep, MacroPreset, MacroStep};

    #[test]
    fn default_stop_steps_are_omitted_from_macro_preset_json() {
        let preset = MacroPreset::default();
        let value = serde_json::to_value(&preset).expect("serialize preset");

        let object = value.as_object().expect("macro preset json object");
        assert!(!object.contains_key("hold_stop_step"));
        assert!(!object.contains_key("press_stop_step"));
    }

    #[test]
    fn non_default_stop_step_round_trips_through_macro_preset_json() {
        let mut preset = MacroPreset::default();
        let mut stop_step = MacroStep::default();
        stop_step.delay_ms = 42;
        preset.hold_stop_step = LazyMacroStep::from(stop_step.clone());

        let json = serde_json::to_string(&preset).expect("serialize preset");
        let restored: MacroPreset = serde_json::from_str(&json).expect("deserialize preset");

        assert_eq!(*restored.hold_stop_step, stop_step);
        assert!(restored.press_stop_step.is_empty());
    }

    #[test]
    fn duration_toggle_restores_previous_value_instead_of_resetting_default() {
        let mut step = MacroStep::default();
        step.set_duration_permanent(false);
        step.duration_expr = "4321".to_string();
        step.remember_duration_input();

        step.set_duration_permanent(true);
        step.set_duration_permanent(false);

        assert_eq!(step.duration_expr, "4321");
        assert_eq!(step.duration_override_ms, 4321);
    }
}
