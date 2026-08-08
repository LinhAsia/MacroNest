use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{self, Button, Color32, Frame, RichText, Sense, vec2};

use crate::{
    hotkey,
    model::{
        EspPreset, HotkeyBinding, MemoryCodeEntry, MemoryDebuggerArchitecture, MemoryDebuggerMethod,
        MemoryPointerEntry,
    },
    process_memory::{
        MemoryRegionInfo, MemoryScanOptions, PausedProcess, PointerMap, PointerPath,
        PointerScanLimits, ScanCandidate, ScanComparison, ScanValue, ScanValueType, TextEncoding,
        TextScanCandidate, ViewProjectionCandidate, capture_pointer_map_with_budget,
        filter_aob_scan_candidates, filter_scan_candidates, filter_text_scan_candidates,
        query_memory_region, read_memory_bytes, read_scan_value, read_text_memory,
        refresh_scan_candidates, scan_aob_memory_with_progress,
        scan_memory_range_with_progress, scan_pointer_paths_with_budget,
        scan_text_memory_with_progress, scan_view_projection_candidates, write_code_bytes,
        write_scan_value, write_text_memory,
    },
    window_list,
};

#[cfg(windows)]
use crate::memory_debugger::debugger::{
    AccessWatch, AddressAccessWatch, ProcessInfo, WatchEvent, WriteWatch, disassemble_from,
    get_instruction_bytes, instruction_writes_memory, is_instruction_compatible,
    list_process_details, module_offset_for_address, normalize_instruction, process_modules,
    process_pointer_width, resolve_module_offset,
};

use super::CrosshairApp;

#[cfg(windows)]
use super::{GetCursorPos, POINT};

const DEFAULT_SCAN_LIMIT: usize = usize::MAX;
// ponytail: keep live polling bounded; add paged candidate refresh before raising this ceiling.
const MAX_VISIBLE_RESULTS: usize = 1_000;
const MAX_VISIBLE_INSTRUCTIONS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MemoryScanAction {
    FirstScan,
    Unknown,
    Exact,
    Increased,
    Decreased,
    Changed,
    Unchanged,
    Less,
    Greater,
    Between,
}

impl MemoryScanAction {
    fn label(self) -> &'static str {
        match self {
            Self::FirstScan => "First scan",
            Self::Unknown => "Unknown",
            Self::Exact => "New value",
            Self::Increased => "Increased",
            Self::Decreased => "Decreased",
            Self::Changed => "Changed",
            Self::Unchanged => "Unchanged",
            Self::Less => "Less than",
            Self::Greater => "Greater than",
            Self::Between => "Between",
        }
    }

    fn comparison(self) -> Option<ScanComparison> {
        Some(match self {
            Self::Exact => ScanComparison::Exact,
            Self::Increased => ScanComparison::Increased,
            Self::Decreased => ScanComparison::Decreased,
            Self::Changed => ScanComparison::Changed,
            Self::Unchanged => ScanComparison::Unchanged,
            Self::Less => ScanComparison::Less,
            Self::Greater => ScanComparison::Greater,
            Self::Between => ScanComparison::Between,
            Self::FirstScan | Self::Unknown => return None,
        })
    }

    fn config_key(self) -> &'static str {
        match self {
            Self::FirstScan => "first_scan",
            Self::Unknown => "unknown",
            Self::Exact => "exact",
            Self::Increased => "increased",
            Self::Decreased => "decreased",
            Self::Changed => "changed",
            Self::Unchanged => "unchanged",
            Self::Less => "less",
            Self::Greater => "greater",
            Self::Between => "between",
        }
    }

    fn from_config_key(key: &str) -> Option<Self> {
        Some(match key {
            "first_scan" => Self::FirstScan,
            "unknown" => Self::Unknown,
            "exact" => Self::Exact,
            "increased" => Self::Increased,
            "decreased" => Self::Decreased,
            "changed" => Self::Changed,
            "unchanged" => Self::Unchanged,
            "less" => Self::Less,
            "greater" => Self::Greater,
            "between" => Self::Between,
            _ => return None,
        })
    }
}

#[derive(Clone)]
struct SavedMemoryAddress {
    address: usize,
    value_type: ScanValueType,
    current: Option<ScanValue>,
    text_encoding: Option<TextEncoding>,
    text_byte_len: usize,
    current_text: Option<String>,
    description: String,
    pointer: Option<PointerSpec>,
    frozen: Option<ScanValue>,
    saved_to_library: bool,
}

#[derive(Clone)]
struct PointerSpec {
    base: usize,
    module: Option<(String, usize)>,
    offsets: Vec<usize>,
}

struct StablePointerCandidate {
    path: PointerPath,
    valid: Option<bool>,
    resolved_base: Option<usize>,
    resolved_address: Option<usize>,
    observed_value: Option<ScanValue>,
    live_value: Option<ScanValue>,
    filter_value: Option<ScanValue>,
}

struct StablePointerJobResult {
    pid: u32,
    result: Result<Vec<PointerPath>, String>,
}

struct StablePointerFilterResult {
    pid: u32,
    action: MemoryScanAction,
    input_count: usize,
    result: Result<Vec<ScanCandidate>, String>,
}

struct StablePointerDialog {
    source_address: usize,
    source_pid: u32,
    value_type: ScanValueType,
    expected_value: ScanValue,
    status: String,
    candidates: Vec<StablePointerCandidate>,
    selected: Option<usize>,
    rx: Option<Receiver<StablePointerJobResult>>,
    progress: Arc<AtomicUsize>,
    limits: PointerScanLimits,
    filter: String,
    exe_only: bool,
    last_live_refresh: Instant,
    validation_pid: Option<u32>,
    validation_cursor: usize,
    filter_rx: Option<Receiver<StablePointerFilterResult>>,
}

enum DeepPointerJobResult {
    MapA(Result<PointerMap, String>),
    Compared(Result<Vec<PointerPath>, String>),
}

struct DeepPointerDialog {
    map_a: Option<Arc<PointerMap>>,
    source_pid: u32,
    source_address: usize,
    value_type: ScanValueType,
    status: String,
    rx: Option<Receiver<DeepPointerJobResult>>,
    progress: Arc<AtomicUsize>,
    candidates: Vec<PointerPath>,
    selected: HashSet<usize>,
    selection_anchor: Option<usize>,
    filter: String,
    exe_only: bool,
    display_type: ScanValueType,
    resolved_rows: HashMap<usize, DeepPointerResolvedRow>,
}

struct DeepPointerResolvedRow {
    address: Option<usize>,
    value: Option<ScanValue>,
    updated_at: Instant,
}

struct CameraMatrixJobResult {
    pid: u32,
    result: Result<Vec<ViewProjectionCandidate>, String>,
}

struct CameraMatrixDialog {
    x: String,
    y: String,
    z: String,
    viewport_width: String,
    viewport_height: String,
    status: String,
    candidates: Vec<ViewProjectionCandidate>,
    selected: Option<usize>,
    rx: Option<Receiver<CameraMatrixJobResult>>,
    progress: Arc<AtomicUsize>,
    baseline: HashMap<usize, [f32; 16]>,
    world: Option<[f32; 3]>,
    projection_variant: usize,
    last_preview_refresh: Instant,
    stability_sample: Option<(Instant, HashMap<usize, [f32; 16]>)>,
    auto_pick_started: Option<Instant>,
}

struct AddressDialog {
    index: usize,
    address: String,
    offsets: String,
    pointer: bool,
    position: egui::Pos2,
    rect: Option<egui::Rect>,
}

#[derive(Clone, Copy)]
enum MemoryViewKind {
    Bytes,
    Structure,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MemoryDisplayType {
    ByteHex,
    ByteDecimal,
    I16Hex,
    I16Decimal,
    I32Hex,
    I32Decimal,
    I64Hex,
    I64Decimal,
    Float,
    Double,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StructureElementType {
    Byte,
    I16,
    I32,
    I64,
    Float,
    Double,
    Pointer,
}

impl StructureElementType {
    const ALL: [(Self, &'static str); 7] = [
        (Self::Byte, "Byte"),
        (Self::I16, "2 Bytes"),
        (Self::I32, "4 Bytes"),
        (Self::I64, "8 Bytes"),
        (Self::Float, "Float"),
        (Self::Double, "Double"),
        (Self::Pointer, "Pointer"),
    ];

    fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find_map(|(kind, label)| (*kind == self).then_some(*label))
            .unwrap()
    }

    fn width(self, pointer_width: usize) -> usize {
        match self {
            Self::Byte => 1,
            Self::I16 => 2,
            Self::I32 | Self::Float => 4,
            Self::I64 | Self::Double => 8,
            Self::Pointer => pointer_width,
        }
    }

    fn scan_type(self, pointer_width: usize) -> ScanValueType {
        match self {
            Self::Float => ScanValueType::F32,
            Self::Double => ScanValueType::F64,
            Self::I64 => ScanValueType::I64,
            Self::Pointer if pointer_width == 4 => ScanValueType::I32,
            Self::Pointer => ScanValueType::I64,
            Self::Byte => ScanValueType::I8,
            Self::I16 => ScanValueType::I16,
            Self::I32 => ScanValueType::I32,
        }
    }
}

#[derive(Clone)]
struct StructureElement {
    offset: usize,
    value_type: StructureElementType,
    name: String,
    /// Cached RTTI class name for Pointer fields (None = not yet resolved, Some("") = not found)
    detected_class: Option<String>,
    expanded: bool,
    child_elements: Vec<StructureElement>,
}

#[derive(Clone)]
struct StructureClass {
    name: String,
    address: usize,
    elements: Vec<StructureElement>,
}

struct MemoryViewDialog {
    address: usize,
    tracked_base: Option<usize>,
    kind: MemoryViewKind,
    display_type: MemoryDisplayType,
    relative_addresses: bool,
    pinned: bool,
    elements: Vec<StructureElement>,
    pending_add: Option<(usize, ScanValueType)>,
    pending_track: Option<usize>,
    pointer_width: usize,
    previous_bytes: Vec<u8>,
    /// Maps byte offset -> time (seconds, from egui) when that byte last changed.
    /// Used to render the Cheat Engine-style red-fade highlight.
    byte_change_times: HashMap<usize, f64>,
    classes: Vec<StructureClass>,
    selected_class: usize,
    class_detection_status: String,
    class_detection_attempted: bool,
    /// True once auto_structure_elements has run for this view
    auto_dissected: bool,
    /// Navigation history stack (previous addresses)
    history: Vec<usize>,
}

#[cfg(windows)]
struct ModuleListDialog {
    modules: Vec<(String, usize, usize)>,
    filter: String,
    status: String,
}

#[cfg(windows)]
enum ActiveInstructionWatch {
    Accesses(AddressAccessWatch),
    Writes(WriteWatch),
}

#[cfg(windows)]
impl ActiveInstructionWatch {
    fn stop(&mut self) {
        match self {
            Self::Accesses(watch) => watch.stop(),
            Self::Writes(watch) => watch.stop(),
        }
    }
}

#[cfg(windows)]
struct InstructionHit {
    address: usize,
    instruction: String,
    details: String,
    count: usize,
}

#[cfg(windows)]
struct InstructionWatchDialog {
    address: usize,
    writes_only: bool,
    status: String,
    hits: Vec<InstructionHit>,
    selected: Option<usize>,
    rx: Receiver<WatchEvent>,
    active: Option<ActiveInstructionWatch>,
    pinned: bool,
    pending_code_add: Option<usize>,
    pending_disassembler: Option<usize>,
    auto_stop_on_hit: bool,
    hits_sort: u8,
}

#[cfg(windows)]
struct DisassemblerDialog {
    address: usize,
    lines: Vec<(usize, String, String)>,
    status: String,
}

#[cfg(windows)]
struct CodeAccessDialog {
    code_index: usize,
    instruction_address: usize,
    status: String,
    addresses: Vec<(usize, usize)>,
    rx: Receiver<WatchEvent>,
    active: Option<AccessWatch>,
    pinned: bool,
    selected: Option<usize>,
    value_type: ScanValueType,
    values: HashMap<usize, String>,
    tracked_name: String,
    tracked_offset: String,
    save_tracked: bool,
    auto_stop_on_hit: bool,
    hits_sort: u8,
}

struct ScanJobResult {
    pid: u32,
    action: MemoryScanAction,
    result: Result<ScanJobCandidates, String>,
}

enum ScanJobCandidates {
    Numeric(Vec<ScanCandidate>),
    Text(Vec<TextScanCandidate>),
}

#[derive(Clone)]
struct FreezeTarget {
    address: usize,
    value: ScanValue,
    pointer: Option<PointerSpec>,
}

struct PendingWriteCheck {
    due: Instant,
    address: usize,
    value_type: ScanValueType,
    expected: ScanValue,
}

struct MemoryFreezeWorker {
    config: Arc<Mutex<(Option<u32>, Vec<FreezeTarget>)>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Default for MemoryFreezeWorker {
    fn default() -> Self {
        let config = Arc::new(Mutex::new((None, Vec::<FreezeTarget>::new())));
        let stop = Arc::new(AtomicBool::new(false));
        Self {
            config,
            stop,
            worker: None,
        }
    }
}

impl MemoryFreezeWorker {
    fn ensure_started(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let worker_config = Arc::clone(&self.config);
        let worker_stop = Arc::clone(&self.stop);
        self.worker = Some(thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                let (pid, targets) = worker_config
                    .lock()
                    .map(|config| (config.0, config.1.clone()))
                    .unwrap_or_default();
                if let Some(pid) = pid {
                    for target in targets {
                        let address =
                            resolve_memory_address(pid, target.address, target.pointer.as_ref())
                                .unwrap_or(target.address);
                        let _ = write_scan_value(pid, address, target.value);
                    }
                }
                thread::sleep(Duration::from_millis(if pid.is_some() { 25 } else { 100 }));
            }
        }));
    }
}

impl Drop for MemoryFreezeWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) struct MemoryPanelState {
    process_selector: String,
    process_pid: Option<u32>,
    last_process_liveness_check: Instant,
    #[cfg(windows)]
    process_choices: Vec<ProcessInfo>,
    value_type: ScanValueType,
    text_encoding: Option<TextEncoding>,
    is_aob_scan: bool,
    text_case_sensitive: bool,
    text_null_terminated: bool,
    value_input: String,
    between_min_input: String,
    between_max_input: String,
    between_open: bool,
    scan_modules: Vec<(String, usize, usize)>,
    hex: bool,
    result_limit_input: String,
    scan_writable: bool,
    scan_executable: bool,
    scan_copy_on_write: bool,
    scan_active_memory_only: bool,
    scan_mem_private: bool,
    scan_mem_image: bool,
    scan_mem_mapped: bool,
    fast_scan: bool,
    fast_scan_alignment: String,
    pause_while_scanning: bool,
    candidates: Vec<ScanCandidate>,
    live_candidate_values: HashMap<usize, ScanValue>,
    text_candidates: Vec<TextScanCandidate>,
    selected_results: HashSet<usize>,
    marked_result_addresses: HashSet<usize>,
    selection_anchor: Option<usize>,
    saved: Vec<SavedMemoryAddress>,
    selected_saved: HashSet<usize>,
    saved_selection_anchor: Option<usize>,
    saved_list_active: bool,
    last_saved_cell_click: Option<(usize, isize, Instant)>,
    manual_address: String,
    status: String,
    last_action: String,
    job_rx: Option<Receiver<ScanJobResult>>,
    scanning: bool,
    scan_progress: Arc<AtomicUsize>,
    scan_input_count: usize,
    has_scan_session: bool,
    pinned: bool,
    address_list_pinned: bool,
    show_scan_previous: bool,
    hotkeys: HashMap<MemoryScanAction, HotkeyBinding>,
    capturing_hotkey: Option<MemoryScanAction>,
    edit_value_index: Option<usize>,
    edit_value_input: String,
    edit_value_position: Option<egui::Pos2>,
    edit_description_index: Option<usize>,
    edit_code_name_index: Option<usize>,
    edit_code_name_input: String,
    address_dialog: Option<AddressDialog>,
    memory_view_dialog: Option<MemoryViewDialog>,
    #[cfg(windows)]
    module_list_dialog: Option<ModuleListDialog>,
    memory_settings_open: bool,
    code_list_open: bool,
    stable_pointer_dialog: Option<StablePointerDialog>,
    deep_pointer_dialog: Option<DeepPointerDialog>,
    camera_matrix_dialog: Option<CameraMatrixDialog>,
    saved_library_open: bool,
    #[cfg(windows)]
    instruction_watch_dialog: Option<InstructionWatchDialog>,
    #[cfg(windows)]
    disassembler_dialog: Option<DisassemblerDialog>,
    #[cfg(windows)]
    code_access_dialog: Option<CodeAccessDialog>,
    last_refresh: Instant,
    last_saved_refresh: Instant,
    visible_scan_ranges: [Option<(usize, usize, Instant)>; 2],
    pending_write_checks: Vec<PendingWriteCheck>,
    freeze_worker: MemoryFreezeWorker,
    pub(crate) dll_config: crate::dll_generator::DllProjectConfig,
    pub(crate) show_dll_studio: bool,
    pub(crate) dll_status_msg: String,
    pub(crate) inject_dll_file_path: String,
}

impl Default for MemoryPanelState {
    fn default() -> Self {
        Self {
            process_selector: String::new(),
            process_pid: None,
            last_process_liveness_check: Instant::now() - Duration::from_secs(2),
            #[cfg(windows)]
            process_choices: Vec::new(),
            value_type: ScanValueType::I32,
            text_encoding: None,
            is_aob_scan: false,
            text_case_sensitive: true,
            text_null_terminated: false,
            value_input: "0".to_owned(),
            between_min_input: "0".to_owned(),
            between_max_input: "100".to_owned(),
            between_open: false,
            scan_modules: Vec::new(),
            hex: false,
            result_limit_input: "Unlimited".to_owned(),
            scan_writable: true,
            scan_executable: false,
            scan_copy_on_write: false,
            scan_active_memory_only: true,
            scan_mem_private: true,
            scan_mem_image: false,
            scan_mem_mapped: false,
            fast_scan: true,
            fast_scan_alignment: "4".to_owned(),
            pause_while_scanning: false,
            candidates: Vec::new(),
            live_candidate_values: HashMap::new(),
            text_candidates: Vec::new(),
            selected_results: HashSet::new(),
            marked_result_addresses: HashSet::new(),
            selection_anchor: None,
            saved: Vec::new(),
            selected_saved: HashSet::new(),
            saved_selection_anchor: None,
            saved_list_active: false,
            last_saved_cell_click: None,
            manual_address: String::new(),
            status: "Ready".to_owned(),
            last_action: "Ready".to_owned(),
            job_rx: None,
            scanning: false,
            scan_progress: Arc::new(AtomicUsize::new(0)),
            scan_input_count: 0,
            has_scan_session: false,
            pinned: false,
            address_list_pinned: false,
            show_scan_previous: true,
            hotkeys: HashMap::new(),
            capturing_hotkey: None,
            edit_value_index: None,
            edit_value_input: String::new(),
            edit_value_position: None,
            edit_description_index: None,
            edit_code_name_index: None,
            edit_code_name_input: String::new(),
            address_dialog: None,
            memory_view_dialog: None,
            #[cfg(windows)]
            module_list_dialog: None,
            memory_settings_open: false,
            code_list_open: false,
            stable_pointer_dialog: None,
            deep_pointer_dialog: None,
            camera_matrix_dialog: None,
            saved_library_open: false,
            #[cfg(windows)]
            instruction_watch_dialog: None,
            #[cfg(windows)]
            disassembler_dialog: None,
            #[cfg(windows)]
            code_access_dialog: None,
            last_refresh: Instant::now(),
            last_saved_refresh: Instant::now(),
            visible_scan_ranges: [None, None],
            pending_write_checks: Vec::new(),
            freeze_worker: MemoryFreezeWorker::default(),
            dll_config: crate::dll_generator::DllProjectConfig::default(),
            show_dll_studio: false,
            dll_status_msg: String::new(),
            inject_dll_file_path: String::new(),
        }
    }
}

impl MemoryPanelState {
    pub(crate) fn with_hotkeys(
        stored: &[(String, HotkeyBinding)],
        _pointers: &[MemoryPointerEntry],
    ) -> Self {
        let mut state = Self::default();
        state.hotkeys = stored
            .iter()
            .filter_map(|(key, binding)| {
                MemoryScanAction::from_config_key(key).map(|action| (action, binding.clone()))
            })
            .collect();
        state
    }
}

impl CrosshairApp {
    fn select_memory_process(&mut self, selector: String, pid: Option<u32>, ctx: &egui::Context) {
        self.memory_panel.process_selector = selector;
        if self.memory_panel.process_pid != pid {
            self.reset_memory_scan("Process changed");
            for saved in &mut self.memory_panel.saved {
                saved.current = None;
                saved.frozen = None;
            }
            self.memory_panel.selected_saved.clear();
            self.memory_panel.saved_selection_anchor = None;
            self.memory_panel.edit_value_index = None;
            self.memory_panel.edit_value_position = None;
            self.memory_panel.edit_description_index = None;
            self.memory_panel.address_dialog = None;
        }
        self.memory_panel.process_pid = pid;
        #[cfg(windows)]
        {
            self.memory_panel.scan_modules = pid
                .and_then(|pid| process_modules(pid).ok())
                .unwrap_or_default();
        }
        self.memory_panel.status = pid.map_or_else(
            || "Unable to open selected process".to_owned(),
            |pid| format!("Process selected — PID {pid}"),
        );
        ctx.request_repaint();
    }

    pub(crate) fn render_memory_panel(&mut self, ui: &mut egui::Ui) {
        let close_address_dialog = self
            .memory_panel
            .address_dialog
            .as_ref()
            .and_then(|dialog| dialog.rect)
            .is_some_and(|rect| {
                ui.input(|input| input.pointer.any_pressed())
                    && ui
                        .ctx()
                        .pointer_latest_pos()
                        .is_some_and(|pointer| !rect.contains(pointer))
            });
        if close_address_dialog {
            self.memory_panel.address_dialog = None;
        }
        self.poll_memory_job();
        self.capture_memory_hotkey(ui.ctx());
        self.poll_memory_hotkeys(ui.ctx());
        self.refresh_memory_values();

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(self.tr("Memory Scanner", "Memory Scanner"))
                    .strong()
                    .size(17.0),
            );
            ui.separator();
            ui.label(RichText::new(&self.memory_panel.status).small().color(
                if self.memory_panel.scanning {
                    Color32::from_rgb(230, 170, 70)
                } else {
                    ui.visuals().weak_text_color()
                },
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let pin_label = if self.memory_panel.pinned {
                    self.tr("Unpin results", "Unpin results")
                } else {
                    self.tr("Pin results", "Pin results")
                };
                if ui.button(pin_label).clicked() {
                    self.memory_panel.pinned = !self.memory_panel.pinned;
                }
                if ui
                    .button(self.tr("Memory settings", "Memory settings"))
                    .clicked()
                {
                    self.memory_panel.memory_settings_open = true;
                }
                if ui.button("Find camera matrix").clicked() {
                    self.open_camera_matrix_dialog();
                }
                if ui
                    .button(self.tr("Advanced options", "Advanced options"))
                    .clicked()
                {
                    self.memory_panel.code_list_open = true;
                }
                #[cfg(windows)]
                ui.menu_button(self.tr("Memory view", "Memory view"), |ui| {
                    if ui
                        .button(self.tr("Enumerate modules / DLLs", "Enumerate modules / DLLs"))
                        .clicked()
                    {
                        self.open_memory_module_list();
                        ui.ctx().request_repaint();
                        ui.close();
                    }
                });
                if ui
                    .button(self.tr("Saved addresses", "Saved addresses"))
                    .clicked()
                {
                    self.memory_panel.saved_library_open = true;
                }
                if ui
                    .button(self.tr("Auto DLL Studio", "Auto DLL Studio"))
                    .clicked()
                {
                    self.memory_panel.show_dll_studio = true;
                }
            });
        });
        ui.add_space(6.0);

        let available = ui.available_size();
        let gap = 8.0;
        let upper_height = (available.y * 0.57).clamp(280.0, (available.y - 170.0).max(280.0));
        let left_width = ((available.x - gap) * 0.58).max(360.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                vec2(left_width, upper_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.render_memory_scan_results(ui, false),
            );
            ui.add_space(gap);
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), upper_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.render_memory_scan_controls(ui),
            );
        });
        ui.add_space(gap);
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| self.render_saved_memory_addresses(ui),
        );

        let address_open = self.memory_panel.address_dialog.is_some();
        if !self.render_detached_memory_popup(
            ui.ctx(),
            "memory-change-address-host",
            "Change address",
            address_open,
            Self::render_memory_address_dialog,
        ) {
            self.memory_panel.address_dialog = None;
        }
        self.render_memory_view_dialog(ui.ctx());
        #[cfg(windows)]
        {
            let active = self.memory_panel.module_list_dialog.is_some();
            if !self.render_detached_memory_popup(
                ui.ctx(),
                "memory-modules-host",
                "Enumerate modules / DLLs",
                active,
                Self::render_memory_module_list,
            ) {
                self.memory_panel.module_list_dialog = None;
            }
        }
        if !self.render_detached_memory_popup(
            ui.ctx(),
            "memory-settings-host",
            "Memory settings",
            self.memory_panel.memory_settings_open,
            Self::render_memory_settings,
        ) {
            self.memory_panel.memory_settings_open = false;
        }
        if !self.render_detached_memory_popup(
            ui.ctx(),
            "memory-code-list-host",
            "Advanced options — Code list",
            self.memory_panel.code_list_open,
            Self::render_memory_code_list,
        ) {
            self.memory_panel.code_list_open = false;
        }
        if !self.render_detached_memory_popup(
            ui.ctx(),
            "memory-saved-host",
            "Saved addresses",
            self.memory_panel.saved_library_open,
            Self::render_saved_address_library,
        ) {
            self.memory_panel.saved_library_open = false;
        }
        let stable_active = self.memory_panel.stable_pointer_dialog.is_some();
        if !self.render_detached_memory_popup(
            ui.ctx(),
            "memory-stable-pointer-host",
            "Find stable pointer",
            stable_active,
            Self::render_stable_pointer_dialog,
        ) {
            self.memory_panel.stable_pointer_dialog = None;
        }
        self.render_deep_pointer_dialog(ui.ctx());
        let camera_active = self.memory_panel.camera_matrix_dialog.is_some();
        if !self.render_detached_memory_popup(
            ui.ctx(),
            "memory-camera-matrix-host",
            "Find camera matrix",
            camera_active,
            Self::render_camera_matrix_dialog,
        ) {
            self.memory_panel.camera_matrix_dialog = None;
        }
        #[cfg(windows)]
        self.render_instruction_watch_dialog(ui.ctx());
        #[cfg(windows)]
        self.render_disassembler_dialog(ui.ctx());
        #[cfg(windows)]
        self.render_code_access_dialog(ui.ctx());
        self.render_dll_studio_window(ui.ctx());
        self.sync_memory_freeze_targets();
    }

    fn render_detached_memory_popup(
        &mut self,
        ctx: &egui::Context,
        id: &'static str,
        title: &'static str,
        active: bool,
        render: fn(&mut Self, &egui::Context),
    ) -> bool {
        if !active {
            return true;
        }
        let mut open = true;
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_position(egui::pos2(0.0, 0.0))
            .with_inner_size(vec2(760.0, 560.0))
            .with_min_inner_size(vec2(480.0, 280.0))
            .with_clamp_size_to_monitor_size(true)
            .with_decorations(false)
            .with_resizable(true)
            .with_always_on_top();
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(id), builder, |ctx, _| {
            Self::constrain_memory_popup_to_monitor(ctx);
            if ctx.input(|input| input.viewport().close_requested()) {
                open = false;
            }
            let mut unpin = false;
            Self::render_memory_popup_titlebar(
                ctx,
                self.state.ui_language,
                title,
                &mut unpin,
                &mut open,
            );
            render(self, ctx);
            Self::render_memory_popup_resize_handles(ctx);
        });
        open
    }

    pub(crate) fn render_memory_pinned_viewport(&mut self, ctx: &egui::Context) {
        self.poll_memory_hotkeys(ctx);
        self.render_pinned_scan_results(ctx);
        self.render_pinned_address_list(ctx);
    }

    fn render_pinned_scan_results(&mut self, ctx: &egui::Context) {
        if !self.memory_panel.pinned {
            return;
        }
        self.poll_memory_job();
        self.poll_memory_hotkeys(ctx);
        self.refresh_memory_values();
        self.sync_memory_freeze_targets();
        let builder = egui::ViewportBuilder::default()
            .with_title("MacroNest — Scan results")
            .with_position(egui::pos2(0.0, 0.0))
            .with_inner_size(vec2(560.0, 430.0))
            .with_min_inner_size(vec2(400.0, 260.0))
            .with_clamp_size_to_monitor_size(true)
            .with_decorations(false)
            .with_resizable(true)
            .with_always_on_top();
        let mut unpin = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("memory-scan-results"),
            builder,
            |ctx, _| {
                Self::constrain_memory_popup_to_monitor(ctx);
                if ctx.input(|input| input.viewport().close_requested()) {
                    unpin = true;
                }
                egui::TopBottomPanel::top("memory-pinned-titlebar")
                    .exact_height(38.0)
                    .frame(
                        Frame::new()
                            .fill(Color32::from_rgb(16, 20, 26))
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(34, 42, 56)))
                            .inner_margin(egui::Margin::symmetric(8, 4)),
                    )
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(Self::material_icon_text(0xe30c, 17.0));
                            ui.label(RichText::new("MacroNest").strong());
                            ui.label(RichText::new("Scan results").weak());
                            let drag = ui.allocate_response(
                                vec2((ui.available_width() - 36.0).max(0.0), 28.0),
                                Sense::click_and_drag(),
                            );
                            if drag.drag_started() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                            }
                            if ui
                                .add_sized(
                                    [32.0, 28.0],
                                    Button::new(Self::material_icon_text(0xe5cd, 17.0)),
                                )
                                .on_hover_text("Unpin")
                                .clicked()
                            {
                                unpin = true;
                            }
                        });
                    });
                egui::CentralPanel::default()
                    .frame(Self::memory_popup_frame(ctx))
                    .show(ctx, |ui| {
                        let count = if self.memory_panel.scanning {
                            self.memory_panel
                                .scan_progress
                                .load(Ordering::Relaxed)
                                .max(self.memory_panel.scan_input_count)
                        } else {
                            self.memory_panel
                                .candidates
                                .len()
                                .max(self.memory_panel.text_candidates.len())
                        };
                        ui.label(format!(
                            "{}  •  {count} address(es){}",
                            self.memory_panel.last_action,
                            if self.memory_panel.scanning {
                                "  •  Loading…"
                            } else {
                                ""
                            }
                        ));
                        ui.separator();
                        self.render_memory_scan_results(ui, true);
                    });
                Self::render_memory_popup_resize_handles(ctx);
                ctx.request_repaint_after(Duration::from_millis(50));
            },
        );
        if unpin {
            self.memory_panel.pinned = false;
        }
    }

    fn render_pinned_address_list(&mut self, ctx: &egui::Context) {
        if !self.memory_panel.address_list_pinned {
            return;
        }
        self.refresh_memory_values();
        self.sync_memory_freeze_targets();
        let builder = egui::ViewportBuilder::default()
            .with_title("MacroNest — Address list")
            .with_position(egui::pos2(0.0, 0.0))
            .with_inner_size(vec2(760.0, 430.0))
            .with_min_inner_size(vec2(520.0, 260.0))
            .with_clamp_size_to_monitor_size(true)
            .with_decorations(false)
            .with_resizable(true)
            .with_always_on_top();
        let mut unpin = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("memory-address-list"),
            builder,
            |ctx, _| {
                Self::constrain_memory_popup_to_monitor(ctx);
                if ctx.input(|input| input.viewport().close_requested()) {
                    unpin = true;
                }
                let title = self.tr("Address list", "Danh sách địa chỉ");
                let mut open = true;
                Self::render_memory_popup_titlebar(
                    ctx,
                    self.state.ui_language,
                    title,
                    &mut unpin,
                    &mut open,
                );
                if !open {
                    unpin = true;
                }
                egui::CentralPanel::default()
                    .frame(Self::memory_popup_frame(ctx))
                    .show(ctx, |ui| self.render_saved_memory_addresses(ui));
                Self::render_memory_popup_resize_handles(ctx);
                ctx.request_repaint_after(Duration::from_millis(50));
            },
        );
        if unpin {
            self.memory_panel.address_list_pinned = false;
        }
    }

    fn render_memory_popup_resize_handles(ctx: &egui::Context) {
        let rect = ctx.content_rect();
        // Keep resize hitboxes clear of egui scrollbars along the viewport edges.
        let edge = 3.0;
        let corner = 18.0;
        let handles = [
            (
                "n",
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + edge)),
                egui::viewport::ResizeDirection::North,
                egui::CursorIcon::ResizeVertical,
            ),
            (
                "s",
                egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - edge), rect.max),
                egui::viewport::ResizeDirection::South,
                egui::CursorIcon::ResizeVertical,
            ),
            (
                "w",
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + edge, rect.max.y)),
                egui::viewport::ResizeDirection::West,
                egui::CursorIcon::ResizeHorizontal,
            ),
            (
                "e",
                egui::Rect::from_min_max(egui::pos2(rect.max.x - edge, rect.min.y), rect.max),
                egui::viewport::ResizeDirection::East,
                egui::CursorIcon::ResizeHorizontal,
            ),
            (
                "nw",
                egui::Rect::from_min_size(rect.min, vec2(corner, corner)),
                egui::viewport::ResizeDirection::NorthWest,
                egui::CursorIcon::ResizeNwSe,
            ),
            (
                "ne",
                egui::Rect::from_min_max(
                    egui::pos2(rect.max.x - corner, rect.min.y),
                    egui::pos2(rect.max.x, rect.min.y + corner),
                ),
                egui::viewport::ResizeDirection::NorthEast,
                egui::CursorIcon::ResizeNeSw,
            ),
            (
                "sw",
                egui::Rect::from_min_max(
                    egui::pos2(rect.min.x, rect.max.y - corner),
                    egui::pos2(rect.min.x + corner, rect.max.y),
                ),
                egui::viewport::ResizeDirection::SouthWest,
                egui::CursorIcon::ResizeNeSw,
            ),
            (
                "se",
                egui::Rect::from_min_max(
                    egui::pos2(rect.max.x - corner, rect.max.y - corner),
                    rect.max,
                ),
                egui::viewport::ResizeDirection::SouthEast,
                egui::CursorIcon::ResizeNwSe,
            ),
        ];

        for (id, handle_rect, direction, cursor) in handles {
            egui::Area::new(egui::Id::new(("memory-popup-resize", id)))
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

    fn memory_popup_frame(ctx: &egui::Context) -> Frame {
        Frame::new()
            .fill(ctx.style().visuals.panel_fill)
            .stroke(egui::Stroke::new(1.5, Color32::from_rgb(78, 92, 112)))
            .inner_margin(egui::Margin::same(7))
    }

    fn constrain_memory_popup_to_monitor(ctx: &egui::Context) {
        let Some((rect, monitor)) =
            ctx.input(|input| Some((input.viewport().outer_rect?, input.viewport().monitor_size?)))
        else {
            return;
        };
        let position = egui::pos2(
            rect.min.x.clamp(0.0, (monitor.x - rect.width()).max(0.0)),
            rect.min.y.clamp(0.0, (monitor.y - rect.height()).max(0.0)),
        );
        if position != rect.min {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
        }
    }

    fn render_memory_popup_titlebar(
        ctx: &egui::Context,
        language: crate::model::UiLanguage,
        title: &str,
        unpin: &mut bool,
        open: &mut bool,
    ) {
        egui::TopBottomPanel::top("memory-tool-pinned-titlebar")
            .exact_height(38.0)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(16, 20, 26))
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(34, 42, 56)))
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(Self::material_icon_text(0xe30c, 17.0));
                    ui.label(RichText::new("MacroNest").strong());
                    ui.label(RichText::new(title).weak().small());
                    let drag = ui.allocate_response(
                        vec2((ui.available_width() - 98.0).max(0.0), 28.0),
                        Sense::click_and_drag(),
                    );
                    if drag.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    let unpin_label = Self::tr_lang(language, "Unpin", "Bỏ ghim");
                    if ui
                        .add_sized([58.0, 28.0], Button::new(unpin_label))
                        .clicked()
                    {
                        *unpin = true;
                    }
                    if ui
                        .add_sized(
                            [32.0, 28.0],
                            Button::new(Self::material_icon_text(0xe5cd, 17.0)),
                        )
                        .on_hover_text("Close")
                        .clicked()
                    {
                        *open = false;
                    }
                });
            });
    }

    fn render_memory_scan_controls(&mut self, ui: &mut egui::Ui) {
        if self.memory_panel.last_process_liveness_check.elapsed() >= Duration::from_secs(1) {
            self.memory_panel.last_process_liveness_check = Instant::now();
            if self
                .memory_panel
                .process_pid
                .is_some_and(|pid| process_pointer_width(pid).is_err())
            {
                self.select_memory_process(String::new(), None, ui.ctx());
                self.memory_panel.status = "Target process exited".to_owned();
            }
        }
        let size = ui.available_size();
        Frame::group(ui.style())
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_min_size(size - vec2(18.0, 18.0));
                ui.label(RichText::new(self.tr("Scan", "Scan")).strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let select_process_str = self.tr("Select process", "Select process");
                    let process_label = if self.memory_panel.process_pid.is_none() {
                        select_process_str.to_owned()
                    } else if let Some(label) = self
                        .memory_panel
                        .process_selector
                        .strip_prefix("pid:")
                        .and_then(|value| value.split_once(':').map(|(_, name)| name))
                    {
                        format!(
                            "{label} — PID {}",
                            self.memory_panel.process_pid.unwrap_or_default()
                        )
                    } else {
                        self.open_window_infos
                            .iter()
                            .find(|window| window.selector == self.memory_panel.process_selector)
                            .map(|window| Self::simplify_window_title(&window.title))
                            .unwrap_or_else(|| select_process_str.to_owned())
                    };
                    let missing_process = self.memory_panel.process_pid.is_none();
                    let process_combo = ui
                        .scope(|ui| {
                            if missing_process {
                                let stroke = egui::Stroke::new(1.0, Color32::from_rgb(185, 82, 82));
                                ui.style_mut().visuals.widgets.inactive.bg_stroke = stroke;
                                ui.style_mut().visuals.widgets.hovered.bg_stroke = stroke;
                            }
                            egui::ComboBox::from_id_salt("memory-process")
                                .width(ui.available_width())
                                .height(720.0)
                                .selected_text(Self::truncate_window_title(&process_label, 52))
                                .show_ui(ui, |ui| {
                                    ui.label(RichText::new(self.tr("Window processes (grouped)", "Window processes (grouped)")).strong());
                                    for window in self.open_window_infos.clone() {
                                        let selected =
                                            window.selector == self.memory_panel.process_selector;
                                        let title_with_pid = format!("{} (PID: {})", Self::simplify_window_title(&window.title), window.process_id);
                                        ui.horizontal(|ui| {
                                            if ui.small_button("Focus")
                                                .on_hover_text(self.tr("Bring this window to front to check", "Bật nổi cửa sổ này lên màn hình để kiểm tra"))
                                                .clicked()
                                            {
                                                window_list::focus_window(&window.selector);
                                            }
                                            if Self::selectable_process_row(
                                                    ui,
                                                    selected,
                                                    Self::truncate_window_title(
                                                        &title_with_pid,
                                                        60,
                                                    ),
                                                    window.process_id,
                                                    &window.process_path,
                                                )
                                                .clicked()
                                            {
                                                let selector = window.selector;
                                                self.memory_panel.process_selector = selector.clone();
                                                let pid =
                                                    window_list::process_id_for_window(Some(&selector));
                                                if self.memory_panel.process_pid != pid {
                                                    self.reset_memory_scan("Process changed");
                                                    for saved in &mut self.memory_panel.saved {
                                                        saved.current = None;
                                                        saved.frozen = None;
                                                    }
                                                    self.memory_panel.selected_saved.clear();
                                                    self.memory_panel.saved_selection_anchor = None;
                                                    self.memory_panel.edit_value_index = None;
                                                    self.memory_panel.edit_description_index = None;
                                                    self.memory_panel.address_dialog = None;
                                                }
                                                self.memory_panel.process_pid = pid;
                                                self.memory_panel.status = pid.map_or_else(
                                                    || "Unable to open selected process".to_owned(),
                                                    |pid| format!("Process selected — PID {pid}"),
                                                );
                                                ui.ctx().request_repaint();
                                            }
                                        });
                                    }
                                    #[cfg(windows)]
                                    if !self.memory_panel.process_choices.is_empty() {
                                        ui.separator();
                                        ui.label(
                                            RichText::new(self.tr("All processes (individual PID)", "All processes (individual PID)"))
                                                .strong(),
                                        );
                                        ui.horizontal(|ui| {
                                            ui.add_space(24.0);
                                            ui.add_sized([190.0, 18.0], egui::Label::new(RichText::new(self.tr("Name", "Name")).strong()));
                                            ui.add_sized([70.0, 18.0], egui::Label::new(RichText::new("PID").strong()));
                                            ui.label(RichText::new(self.tr("Path", "Path")).strong());
                                        });
                                        let count = self.memory_panel.process_choices.len();
                                        egui::ScrollArea::vertical().max_height(620.0).show_rows(ui, 22.0, count, |ui, rows| {
                                            for index in rows {
                                                let process = &mut self.memory_panel.process_choices[index];
                                                if process.path.is_empty() {
                                                    process.path = crate::memory_debugger::debugger::process_path(process.pid);
                                                }
                                                let process = process.clone();
                                                if Self::selectable_process_detail_row(ui, self.memory_panel.process_pid == Some(process.pid), &process.name, process.pid, &process.path).clicked() {
                                                    self.select_memory_process(format!("pid:{}:{}", process.pid, process.name), Some(process.pid), ui.ctx());
                                                }
                                            }
                                        });
                                    }
                                })
                        })
                        .inner;
                    if process_combo.response.clicked() {
                        self.ensure_open_windows_ready(true);
                        #[cfg(windows)]
                        if let Ok(processes) = list_process_details() {
                            self.memory_panel.process_choices = processes;
                        }
                    }
                });
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("memory-value-type")
                        .width(110.0)
                        .selected_text(if self.memory_panel.is_aob_scan {
                            self.tr("Array of Bytes (AOB)", "Array of Bytes (AOB)")
                        } else {
                            match self.memory_panel.text_encoding {
                                Some(TextEncoding::Utf8) => self.tr("Text (UTF-8)", "Text (UTF-8)"),
                                Some(TextEncoding::Utf16) => self.tr("Text (UTF-16)", "Text (UTF-16)"),
                                None => self.tr(memory_type_label(self.memory_panel.value_type), memory_type_label(self.memory_panel.value_type)),
                            }
                        })
                        .show_ui(ui, |ui| {
                            for value_type in [
                                ScanValueType::I8,
                                ScanValueType::I16,
                                ScanValueType::I32,
                                ScanValueType::F32,
                                ScanValueType::I64,
                                ScanValueType::F64,
                            ] {
                                if ui
                                    .selectable_label(
                                        !self.memory_panel.is_aob_scan
                                            && self.memory_panel.text_encoding.is_none()
                                            && self.memory_panel.value_type == value_type,
                                        self.tr(memory_type_label(value_type), memory_type_label(value_type)),
                                    )
                                    .clicked()
                                {
                                    self.memory_panel.value_type = value_type;
                                    self.memory_panel.text_encoding = None;
                                    self.memory_panel.is_aob_scan = false;
                                    self.reset_memory_scan("Value type changed");
                                }
                            }
                            ui.separator();
                            for (encoding, label) in [
                                (TextEncoding::Utf8, "Text (UTF-8)"),
                                (TextEncoding::Utf16, "Text (UTF-16)"),
                            ] {
                                if ui
                                    .selectable_label(
                                        !self.memory_panel.is_aob_scan
                                            && self.memory_panel.text_encoding == Some(encoding),
                                        self.tr(label, label),
                                    )
                                    .clicked()
                                {
                                    self.memory_panel.text_encoding = Some(encoding);
                                    self.memory_panel.is_aob_scan = false;
                                    self.memory_panel.hex = false;
                                    self.reset_memory_scan("Value type changed");
                                }
                            }
                            ui.separator();
                            if ui
                                .selectable_label(
                                    self.memory_panel.is_aob_scan,
                                    self.tr("Array of Bytes (AOB)", "Array of Bytes (AOB)"),
                                )
                                .clicked()
                            {
                                self.memory_panel.is_aob_scan = true;
                                self.memory_panel.text_encoding = None;
                                self.memory_panel.hex = true;
                                self.reset_memory_scan("Value type changed");
                            }
                        });
                    let val_hint = if self.memory_panel.is_aob_scan {
                        "F3 41 ?? 10 50 10"
                    } else {
                        self.tr("value", "value")
                    };
                    let value_response = ui.add(
                        egui::TextEdit::singleline(&mut self.memory_panel.value_input)
                            .desired_width(120.0)
                            .hint_text(val_hint),
                    );
                    Self::apply_vietnamese_input_if_changed(
                        &value_response,
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
                        &mut self.memory_panel.value_input,
                    );
                    if value_response.gained_focus() {
                        Self::select_all_text(
                            ui.ctx(),
                            &value_response,
                            self.memory_panel.value_input.chars().count(),
                        );
                    }
                    if (value_response.has_focus() || value_response.lost_focus())
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        let action = if self.memory_panel.has_scan_session {
                            MemoryScanAction::Exact
                        } else {
                            MemoryScanAction::FirstScan
                        };
                        self.start_memory_action(action);
                    }
                    if self.memory_panel.text_encoding.is_some() {
                        let case_label = self.tr("Case", "Case");
                        ui.checkbox(&mut self.memory_panel.text_case_sensitive, case_label);
                    } else if !self.memory_panel.is_aob_scan {
                        let hex_label = self.tr("Hex", "Hex");
                        ui.checkbox(&mut self.memory_panel.hex, hex_label);
                    }
                });
                if self.memory_panel.text_encoding.is_some() {
                    ui.horizontal(|ui| {
                        let null_label = self.tr("Null terminated", "Null terminated");
                        ui.checkbox(
                            &mut self.memory_panel.text_null_terminated,
                            null_label,
                        );
                        ui.label(RichText::new(self.tr("Exact text", "Exact text")).weak().small());
                    });
                }
                ui.add_space(8.0);
                self.memory_action_row(
                    ui,
                    [
                        Some(MemoryScanAction::FirstScan),
                        Some(MemoryScanAction::Unknown),
                        None,
                    ],
                    true,
                );
                ui.add_space(5.0);
                for actions in [
                    [
                        Some(MemoryScanAction::Exact),
                        Some(MemoryScanAction::Increased),
                        Some(MemoryScanAction::Decreased),
                    ],
                    [
                        Some(MemoryScanAction::Changed),
                        Some(MemoryScanAction::Unchanged),
                        Some(MemoryScanAction::Less),
                    ],
                    [Some(MemoryScanAction::Greater), None, None],
                ] {
                    self.memory_action_row(ui, actions, false);
                    ui.add_space(5.0);
                }
                ui.horizontal(|ui| {
                    if ui.button(self.tr("Between", "Between")).clicked() {
                        self.memory_panel.between_open = !self.memory_panel.between_open;
                    }
                    if self.memory_panel.between_open {
                        let min_resp = ui.add(egui::TextEdit::singleline(&mut self.memory_panel.between_min_input).desired_width(80.0).hint_text("Min"));
                        Self::apply_vietnamese_input_if_changed(
                            &min_resp,
                            self.state.vietnamese_input_enabled,
                            self.state.vietnamese_input_mode,
                            &mut self.memory_panel.between_min_input,
                        );
                        ui.label(self.tr("to", "to"));
                        let max_resp = ui.add(egui::TextEdit::singleline(&mut self.memory_panel.between_max_input).desired_width(80.0).hint_text("Max"));
                        Self::apply_vietnamese_input_if_changed(
                            &max_resp,
                            self.state.vietnamese_input_enabled,
                            self.state.vietnamese_input_mode,
                            &mut self.memory_panel.between_max_input,
                        );
                        if ui.button(self.tr("Scan", "Scan")).clicked() {
                            self.start_memory_action(MemoryScanAction::Between);
                        }
                    }
                });
                ui.add_space(5.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(self.tr("Limit", "Limit"));
                    let limit_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.memory_panel.result_limit_input)
                            .desired_width(110.0),
                    );
                    Self::apply_vietnamese_input_if_changed(
                        &limit_resp,
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
                        &mut self.memory_panel.result_limit_input,
                    );
                });
                let writable_label = self.tr("Writable", "Writable");
                let executable_label = self.tr("Executable", "Executable");
                let copy_label = self.tr("CopyOnWrite", "CopyOnWrite");
                let active_label = self.tr("Active memory only", "Active memory only");
                let private_label = self.tr("Heap/Stack (MEM_PRIVATE)", "Bộ nhớ động (MEM_PRIVATE)");
                let image_label = self.tr("DLLs / Mapped memory", "DLL & Mapped file");
                ui.columns(2, |columns| {
                    columns[0].checkbox(&mut self.memory_panel.scan_writable, writable_label);
                    columns[1].checkbox(&mut self.memory_panel.scan_executable, executable_label);
                    columns[0].checkbox(
                        &mut self.memory_panel.scan_copy_on_write,
                        copy_label,
                    );
                    columns[1].checkbox(
                        &mut self.memory_panel.scan_active_memory_only,
                        active_label,
                    );
                    columns[0].checkbox(
                        &mut self.memory_panel.scan_mem_private,
                        private_label,
                    );
                    columns[1].checkbox(
                        &mut self.memory_panel.scan_mem_image,
                        image_label,
                    );
                });
                ui.horizontal(|ui| {
                    let fast_scan_label = self.tr("Fast scan", "Fast scan");
                    ui.checkbox(&mut self.memory_panel.fast_scan, fast_scan_label);
                    let align_resp = ui.add_enabled(
                        self.memory_panel.fast_scan,
                        egui::TextEdit::singleline(
                            &mut self.memory_panel.fast_scan_alignment,
                        )
                        .desired_width(42.0),
                    );
                    Self::apply_vietnamese_input_if_changed(
                        &align_resp,
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
                        &mut self.memory_panel.fast_scan_alignment,
                    );
                    ui.label(self.tr("Alignment", "Alignment"));
                });
                let pause_label = self.tr("Pause while scanning", "Pause while scanning");
                ui.checkbox(
                    &mut self.memory_panel.pause_while_scanning,
                    pause_label,
                );
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    let addr_hint = self.tr("address / module+offset [offsets]", "address / module+offset [offsets]");
                    let addr_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.memory_panel.manual_address)
                            .desired_width(180.0)
                            .hint_text(addr_hint),
                    );
                    Self::apply_vietnamese_input_if_changed(
                        &addr_resp,
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
                        &mut self.memory_panel.manual_address,
                    );
                    if ui.button(self.tr("Add address", "Add address")).clicked() {
                        self.add_manual_memory_address();
                    }
                    if ui.button(self.tr("View class", "View class")).clicked() {
                        self.open_manual_structure_view();
                    }
                });
                if let Some(action) = self.memory_panel.capturing_hotkey {
                    ui.label(
                        RichText::new(format!("Press a key for {}…", action.label()))
                            .color(Color32::from_rgb(86, 190, 238)),
                    );
                }
            });
    }

    fn memory_action_row(
        &mut self,
        ui: &mut egui::Ui,
        actions: [Option<MemoryScanAction>; 3],
        reset_last: bool,
    ) {
        const GAP: f32 = 5.0;
        let cell_width = ((ui.available_width() - GAP * 2.0) / 3.0).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (index, action) in actions.into_iter().enumerate() {
                ui.allocate_ui_with_layout(
                    vec2(cell_width, 26.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(cell_width);
                        if let Some(action) = action {
                            self.memory_action_button(ui, action, true);
                        } else if reset_last && index == 2 {
                            let width = ui.available_width();
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if ui
                                    .add_enabled(
                                        !self.memory_panel.scanning,
                                        Button::new(self.tr("Reset", "Reset"))
                                            .min_size(vec2((width - 34.0).max(52.0), 26.0)),
                                    )
                                    .clicked()
                                {
                                    self.reset_memory_scan("New scan");
                                }
                                ui.allocate_space(vec2(26.0, 26.0));
                            });
                        }
                    },
                );
            }
        });
    }

    fn memory_action_button(&mut self, ui: &mut egui::Ui, action: MemoryScanAction, hotkey: bool) {
        let width = ui.available_width();
        let action_btn_text = self.tr(action.label(), action.label());
        ui.horizontal(|ui| {
            let stable_filter_enabled = self
                .memory_panel
                .stable_pointer_dialog
                .as_ref()
                .is_some_and(|dialog| {
                    !dialog.candidates.is_empty()
                        && dialog.rx.is_none()
                        && dialog.validation_pid.is_none()
                        && dialog.filter_rx.is_none()
                });
            let enabled = self.memory_panel.process_pid.is_some()
                && (stable_filter_enabled
                    || (!self.memory_panel.scanning
                        && (self.memory_panel.text_encoding.is_none()
                            || matches!(
                                action,
                                MemoryScanAction::FirstScan | MemoryScanAction::Exact
                            ))
                        && (matches!(
                            action,
                            MemoryScanAction::FirstScan | MemoryScanAction::Unknown
                        ) || !self.memory_panel.candidates.is_empty()
                            || !self.memory_panel.text_candidates.is_empty())));
            if ui
                .add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [(width - if hotkey { 34.0 } else { 0.0 }).max(52.0), 26.0],
                        Button::new(action_btn_text),
                    )
                })
                .inner
                .clicked()
            {
                self.start_memory_action(action);
            }
            if hotkey {
                let assigned_label = self
                    .memory_panel
                    .hotkeys
                    .get(&action)
                    .map(|binding| hotkey::format_binding(Some(binding)));
                let capturing = self.memory_panel.capturing_hotkey == Some(action);
                let content = assigned_label
                    .as_ref()
                    .map(|_| RichText::new(""))
                    .unwrap_or_else(|| Self::material_icon_text(0xe312, 15.0));
                let mut button = Button::new(content);
                if capturing {
                    button = button
                        .fill(Color32::from_rgb(41, 112, 142))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(105, 211, 255)));
                }
                let response =
                    ui.add_sized([26.0, 26.0], button)
                        .on_hover_text(if assigned_label.is_some() {
                            "Click to clear hotkey"
                        } else {
                            "Click to assign hotkey"
                        });
                if let Some(label) = assigned_label.as_deref() {
                    ui.painter().with_clip_rect(response.rect).text(
                        response.rect.center(),
                        egui::Align2::CENTER_CENTER,
                        compact_hotkey_label(label),
                        egui::FontId::proportional(9.5),
                        ui.visuals().strong_text_color(),
                    );
                    Self::paint_expanded_hotkey(ui, &response, label);
                }
                if response.clicked() {
                    if assigned_label.is_some() {
                        self.memory_panel.hotkeys.remove(&action);
                        self.memory_panel.capturing_hotkey = None;
                        self.capture_hotkey_combo_keys = None;
                        self.capture_hotkey_combo_vks.clear();
                        self.persist_memory_hotkeys();
                    } else {
                        self.memory_panel.capturing_hotkey = Some(action);
                        self.capture_ignored_keys = self.snapshot_pressed_capture_keys();
                        self.capture_hotkey_combo_keys = None;
                        self.capture_hotkey_combo_vks.clear();
                    }
                }
            }
        });
    }

    fn paint_expanded_hotkey(ui: &egui::Ui, response: &egui::Response, label: &str) {
        if !response.hovered() {
            return;
        }
        let font_id = egui::FontId::proportional(11.0);
        let text_color = ui.visuals().strong_text_color();
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_owned(), font_id, text_color);
        let width = galley.size().x + 14.0;
        if width <= response.rect.width() {
            return;
        }
        let rect = egui::Rect::from_min_max(
            egui::pos2(response.rect.right() - width, response.rect.top()),
            response.rect.right_bottom(),
        );
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            response.id.with("expanded-hotkey"),
        ));
        let visuals = &ui.visuals().widgets.hovered;
        painter.rect_filled(rect, visuals.corner_radius, visuals.bg_fill);
        painter.rect_stroke(
            rect,
            visuals.corner_radius,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.center() - galley.size() * 0.5, galley, text_color);
    }

    fn select_all_text(ctx: &egui::Context, response: &egui::Response, char_count: usize) {
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, response.id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(char_count),
                )));
            state.store(ctx, response.id);
        }
    }

    fn render_memory_scan_result_item(
        &mut self,
        ui: &mut egui::Ui,
        pinned: bool,
        index: usize,
        pane_width: f32,
        address_ratio: f32,
        value_ratio: f32,
        show_previous: bool,
    ) {
        let (address_value, current_value, previous_value) =
            if let Some(candidate) = self.memory_panel.text_candidates.get(index) {
                (
                    candidate.address,
                    candidate.current.clone(),
                    candidate.previous.clone(),
                )
            } else {
                let candidate = self.memory_panel.candidates[index];
                let current = self
                    .memory_panel
                    .live_candidate_values
                    .get(&index)
                    .copied()
                    .unwrap_or_else(|| candidate.current(self.memory_panel.value_type));
                (
                    candidate.address,
                    format_scan_value(current, self.memory_panel.hex),
                    "-".to_owned(),
                )
            };
        let selected = self.memory_panel.selected_results.contains(&index);
        let marked = self
            .memory_panel
            .marked_result_addresses
            .contains(&address_value);
        let (full_row_rect, mut response) =
            ui.allocate_exact_size(vec2(pane_width, 22.0), Sense::click());
        response = response.on_hover_cursor(egui::CursorIcon::Default);
        if let Some(pid) = self.memory_panel.process_pid {
            let read_f32 = |addr: usize| -> Option<f32> {
                let bytes = crate::process_memory::read_memory_bytes(pid, addr, 4).ok()?;
                Some(f32::from_le_bytes(bytes.try_into().ok()?))
            };
            if let (Some(y_val), Some(z_val)) = (
                read_f32(address_value.wrapping_add(4)),
                read_f32(address_value.wrapping_add(8)),
            ) {
                let prev_y = read_f32(address_value.wrapping_sub(4)).unwrap_or(0.0);
                response = response.on_hover_text(format!(
                    "Tọa độ 3D liên kết xung quanh 0x{:X}:\n• [+0x04]: Y = {:.3}\n• [+0x08]: Z = {:.3}\n• [-0x04]: Y = {:.3}",
                    address_value, y_val, z_val, prev_y
                ));
            }
        }
        if marked {
            ui.painter().rect_filled(
                full_row_rect,
                3.0,
                Color32::from_rgba_premultiplied(196, 82, 82, 72),
            );
        }
        if response.hovered() || selected {
            ui.painter().rect_filled(
                full_row_rect,
                3.0,
                Color32::from_rgba_premultiplied(84, 178, 222, if selected { 58 } else { 42 }),
            );
        }
        ui.allocate_ui_at_rect(full_row_rect, |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                Self::memory_table_cell(
                    ui,
                    pane_width * address_ratio,
                    RichText::new(format_memory_address(address_value)).monospace(),
                );
                Self::memory_table_cell(
                    ui,
                    pane_width * value_ratio,
                    RichText::new(&current_value).monospace(),
                );
                if show_previous {
                    Self::memory_table_cell(
                        ui,
                        pane_width * value_ratio,
                        RichText::new(&previous_value).monospace(),
                    );
                }
            });
        });
        response.context_menu(|ui| {
            let label = if marked {
                "Remove not-relevant mark"
            } else {
                "Mark as not relevant"
            };
            if ui.button(label).clicked() {
                if marked {
                    self.memory_panel
                        .marked_result_addresses
                        .remove(&address_value);
                } else {
                    self.memory_panel
                        .marked_result_addresses
                        .insert(address_value);
                }
                ui.close();
            }
            if ui.button("Copy Address (Hex)").clicked() {
                ui.ctx().copy_text(format!("0x{:X}", address_value));
                ui.close();
            }
            if ui.button("Copy 3D Target (X, X+0x04, X+0x08)").clicked() {
                let text = format!(
                    "X: 0x{:X}\nY: 0x{:X} + 0x04\nZ: 0x{:X} + 0x08",
                    address_value, address_value, address_value
                );
                ui.ctx().copy_text(text);
                ui.close();
            }
        });
        if response.double_clicked() {
            self.memory_panel.selected_results.clear();
            self.memory_panel.selected_results.insert(index);
            self.add_selected_memory_results();
        } else if response.clicked() {
            let toggle = ui.input(|input| input.modifiers.ctrl || input.modifiers.command);
            self.select_memory_result(index, if toggle { !selected } else { true }, ui);
        }
    }

    fn render_memory_scan_results(&mut self, ui: &mut egui::Ui, pinned: bool) {
        let size = ui.available_size();
        let frame = Frame::group(ui.style()).inner_margin(egui::Margin::same(5));
        frame.show(ui, |ui| {
            ui.set_min_size(size - vec2(12.0, 12.0));
            let result_count = self
                .memory_panel
                .candidates
                .len()
                .max(self.memory_panel.text_candidates.len());
            let visible_count = result_count.min(MAX_VISIBLE_RESULTS);
            if !pinned {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.tr("Scan results", "Scan results")).strong());
                    ui.label(result_count.to_string());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(self.tr("Add ↓", "Thêm ↓")).clicked() {
                            self.add_selected_memory_results();
                        }
                    });
                });
            }
            let previous_label = self.tr("Previous", "Previous");
            ui.checkbox(&mut self.memory_panel.show_scan_previous, previous_label);
            if !pinned
                && !self.memory_panel.saved_list_active
                && ui.ctx().memory(|memory| memory.focused().is_none())
                && ui.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::A))
            {
                self.memory_panel.selected_results = (0..visible_count).collect();
            }
            if result_count == 0 && !self.memory_panel.scanning {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(self.tr("No scan results", "No scan results")).weak());
                });
                return;
            }
            // Widening the pinned window creates compact parallel result panes.
            let show_previous = self.memory_panel.show_scan_previous;
            let minimum_pane_width = if show_previous { 360.0 } else { 250.0 };
            let pane_count = if pinned {
                (ui.available_width() / minimum_pane_width).floor().max(1.0) as usize
            } else {
                1
            };
            let pane_width = ui.available_width() / pane_count as f32;
            let address_ratio = if show_previous { 0.40 } else { 0.56 };
            let value_ratio = if show_previous { 0.30 } else { 0.44 };
            let grid_row_count = visible_count.div_ceil(pane_count);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for _ in 0..pane_count {
                    ui.allocate_ui_with_layout(
                        vec2(pane_width, 20.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            Self::memory_table_cell(
                                ui,
                                pane_width * address_ratio,
                                RichText::new(self.tr("Address", "Address")).strong(),
                            );
                            Self::memory_table_cell(
                                ui,
                                pane_width * value_ratio,
                                RichText::new(self.tr("Current", "Current")).strong(),
                            );
                            if show_previous {
                                Self::memory_table_cell(
                                    ui,
                                    pane_width * value_ratio,
                                    RichText::new(previous_label).strong(),
                                );
                            }
                        },
                    );
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt(if pinned {
                    "pinned-memory-results"
                } else {
                    "memory-results"
                })
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show_rows(ui, 22.0, grid_row_count, |ui, rows| {
                    self.memory_panel.visible_scan_ranges[usize::from(pinned)] = Some((
                        rows.start * pane_count,
                        (rows.end * pane_count).min(visible_count),
                        Instant::now(),
                    ));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for row in rows {
                        let start = row * pane_count;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for index in start..(start + pane_count).min(visible_count) {
                                self.render_memory_scan_result_item(
                                    ui,
                                    pinned,
                                    index,
                                    pane_width,
                                    address_ratio,
                                    value_ratio,
                                    show_previous,
                                );
                            }
                            if start + pane_count > visible_count {
                                ui.allocate_space(vec2(
                                    pane_width * (start + pane_count - visible_count) as f32,
                                    22.0,
                                ));
                            }
                        });
                    }
                });
            if result_count > MAX_VISIBLE_RESULTS {
                ui.label(
                    RichText::new(format!(
                        "Showing first {MAX_VISIBLE_RESULTS} of {}",
                        result_count
                    ))
                    .small()
                    .weak(),
                );
            }
        });
    }

    #[cfg(any())]
    fn render_memory_scan_results_legacy(&mut self, ui: &mut egui::Ui, pinned: bool) {
        let size = ui.available_size();
        let frame = Frame::group(ui.style()).inner_margin(egui::Margin::same(5));
        frame.show(ui, |ui| {
            ui.set_min_size(size - vec2(12.0, 12.0));
            if !pinned {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.tr("Scan results", "Scan results")).strong());
                    ui.label(format!(
                        "{}",
                        self.memory_panel
                            .candidates
                            .len()
                            .max(self.memory_panel.text_candidates.len())
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(self.tr("Add ↓", "Add ↓")).clicked() {
                            self.add_selected_memory_results();
                        }
                    });
                });
            }
            let available_width = ui.available_width();
            let result_column_width = (available_width / 3.0).max(80.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                Self::memory_table_cell(
                    ui,
                    result_column_width,
                    RichText::new(self.tr("Address", "Address")).strong(),
                );
                Self::memory_table_cell(
                    ui,
                    result_column_width,
                    RichText::new(self.tr("Current", "Current")).strong(),
                );
                Self::memory_table_cell(
                    ui,
                    result_column_width,
                    RichText::new(self.tr("Previous", "Previous")).strong(),
                );
            });
            ui.separator();
            let result_count = self
                .memory_panel
                .candidates
                .len()
                .max(self.memory_panel.text_candidates.len());
            let visible_count = result_count.min(MAX_VISIBLE_RESULTS);
            if !pinned
                && !self.memory_panel.saved_list_active
                && ui.ctx().memory(|memory| memory.focused().is_none())
                && ui.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::A))
            {
                self.memory_panel.selected_results = (0..visible_count).collect();
            }
            if result_count == 0 && !self.memory_panel.scanning {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(self.tr("No scan results", "No scan results")).weak());
                });
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt(if pinned {
                    "pinned-memory-results"
                } else {
                    "memory-results"
                })
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show_rows(ui, 22.0, visible_count, |ui, rows| {
                    self.memory_panel.visible_scan_ranges[usize::from(pinned)] =
                        Some((rows.start, rows.end, Instant::now()));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for index in rows {
                        let (address_value, current_value, previous_value) =
                            if let Some(candidate) = self.memory_panel.text_candidates.get(index) {
                                (
                                    candidate.address,
                                    candidate.current.clone(),
                                    candidate.previous.clone(),
                                )
                            } else {
                                let candidate = self.memory_panel.candidates[index];
                                let current = self
                                    .memory_panel
                                    .live_candidate_values
                                    .get(&index)
                                    .copied()
                                    .unwrap_or_else(|| {
                                        candidate.current(self.memory_panel.value_type)
                                    });
                                (
                                    candidate.address,
                                    format_scan_value(current, self.memory_panel.hex),
                                    "-".to_owned(),
                                )
                            };
                        let selected = self.memory_panel.selected_results.contains(&index);
                        let static_address =
                            self.memory_panel
                                .scan_modules
                                .iter()
                                .find_map(|(name, base, size)| {
                                    (*base..base.saturating_add(*size))
                                        .contains(&address_value)
                                        .then(|| format!("{}+{:X}", name, address_value - *base))
                                });
                        let address_text = static_address
                            .clone()
                            .unwrap_or_else(|| format_memory_address(address_value));
                        let marked = self
                            .memory_panel
                            .marked_result_addresses
                            .contains(&address_value);
                        let row_width = ui.available_width();
                        let full_row_rect = egui::Rect::from_min_size(
                            ui.next_widget_position(),
                            vec2(row_width, 22.0),
                        );
                        let response = ui
                            .interact(
                                full_row_rect,
                                ui.id().with(("memory-result-row", pinned, address_value)),
                                Sense::click(),
                            )
                            .on_hover_cursor(egui::CursorIcon::Default);
                        ui.allocate_ui_with_layout(
                            vec2(row_width, 22.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                Self::memory_table_cell(
                                    ui,
                                    result_column_width,
                                    RichText::new(address_text).monospace().color(
                                        if static_address.is_some() {
                                            Color32::from_rgb(80, 210, 120)
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                );
                                Self::memory_table_cell(
                                    ui,
                                    result_column_width,
                                    RichText::new(&current_value).monospace(),
                                );
                                Self::memory_table_cell(
                                    ui,
                                    result_column_width,
                                    RichText::new(&previous_value).monospace(),
                                );
                            },
                        );
                        if marked {
                            ui.painter().rect_filled(
                                response.rect,
                                3.0,
                                Color32::from_rgba_premultiplied(196, 82, 82, 72),
                            );
                        }
                        if response.hovered() || selected {
                            ui.painter().rect_filled(
                                response.rect,
                                3.0,
                                Color32::from_rgba_premultiplied(
                                    84,
                                    178,
                                    222,
                                    if selected { 58 } else { 42 },
                                ),
                            );
                        }
                        response.context_menu(|ui| {
                            let label = if marked {
                                "Remove not-relevant mark"
                            } else {
                                "Mark as not relevant"
                            };
                            if ui.button(label).clicked() {
                                if marked {
                                    self.memory_panel
                                        .marked_result_addresses
                                        .remove(&address_value);
                                } else {
                                    self.memory_panel
                                        .marked_result_addresses
                                        .insert(address_value);
                                }
                                ui.close();
                            }
                        });
                        if response.double_clicked() {
                            self.memory_panel.selected_results.clear();
                            self.memory_panel.selected_results.insert(index);
                            self.add_selected_memory_results();
                        } else if response.clicked() {
                            let toggle =
                                ui.input(|input| input.modifiers.ctrl || input.modifiers.command);
                            self.select_memory_result(
                                index,
                                if toggle { !selected } else { true },
                                ui,
                            );
                        }
                    }
                });
            if result_count > MAX_VISIBLE_RESULTS {
                ui.label(
                    RichText::new(format!(
                        "Showing first {MAX_VISIBLE_RESULTS} of {}",
                        result_count
                    ))
                    .small()
                    .weak(),
                );
            }
        });
    }

    fn memory_table_cell(ui: &mut egui::Ui, width: f32, text: RichText) {
        Self::memory_label_cell(
            ui,
            width,
            18.0,
            egui::Label::new(text).selectable(false).truncate(),
        )
        .on_hover_cursor(egui::CursorIcon::Default);
    }

    fn memory_label_cell(
        ui: &mut egui::Ui,
        width: f32,
        height: f32,
        label: egui::Label,
    ) -> egui::Response {
        let (rect, cell_response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let mut cell = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        cell_response
            .union(cell.add(label.selectable(false)))
            .on_hover_cursor(egui::CursorIcon::Default)
    }

    fn select_memory_result(&mut self, index: usize, selected: bool, ui: &egui::Ui) {
        self.memory_panel.saved_list_active = false;
        let (shift, additive) = ui.input(|input| {
            (
                input.modifiers.shift,
                input.modifiers.ctrl || input.modifiers.command,
            )
        });
        if shift && let Some(anchor) = self.memory_panel.selection_anchor {
            if !additive {
                self.memory_panel.selected_results.clear();
            }
            let (start, end) = if anchor <= index {
                (anchor, index)
            } else {
                (index, anchor)
            };
            for row in start..=end.min(MAX_VISIBLE_RESULTS - 1) {
                if selected {
                    self.memory_panel.selected_results.insert(row);
                } else {
                    self.memory_panel.selected_results.remove(&row);
                }
            }
        } else {
            if !additive {
                self.memory_panel.selected_results.clear();
            }
            if selected {
                self.memory_panel.selected_results.insert(index);
            } else {
                self.memory_panel.selected_results.remove(&index);
            }
        }
        if !shift {
            self.memory_panel.selection_anchor = Some(index);
        }
    }

    fn select_saved_memory_row(&mut self, index: usize, selected: bool, ui: &egui::Ui) {
        self.memory_panel.saved_list_active = true;
        let (shift, additive) = ui.input(|input| {
            (
                input.modifiers.shift,
                input.modifiers.ctrl || input.modifiers.command,
            )
        });
        if shift && let Some(anchor) = self.memory_panel.saved_selection_anchor {
            if !additive {
                self.memory_panel.selected_saved.clear();
            }
            let (start, end) = if anchor <= index {
                (anchor, index)
            } else {
                (index, anchor)
            };
            self.memory_panel.selected_saved.extend(start..=end);
        } else {
            if !additive {
                self.memory_panel.selected_saved.clear();
            }
            if additive && selected {
                self.memory_panel.selected_saved.remove(&index);
            } else {
                self.memory_panel.selected_saved.insert(index);
            }
        }
        if !shift {
            self.memory_panel.saved_selection_anchor = Some(index);
        }
    }

    fn render_saved_memory_addresses(&mut self, ui: &mut egui::Ui) {
        let size = ui.available_size();
        Frame::group(ui.style())
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.set_min_size(size - vec2(14.0, 14.0));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.tr("Address list", "Address list")).strong());
                    let selected = self.memory_panel.selected_saved.len();
                    if selected > 0 {
                        if ui.button(self.tr("Delete", "Delete")).clicked() {
                            self.delete_selected_saved_memory();
                        }
                        if selected < self.memory_panel.saved.len()
                            && ui
                                .button(self.tr("Delete unselected", "Delete unselected"))
                                .clicked()
                        {
                            self.delete_unselected_saved_memory();
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if self.memory_panel.address_list_pinned {
                            self.tr("Unpin address list", "Unpin address list")
                        } else {
                            self.tr("Pin address list", "Pin address list")
                        };
                        if ui.button(label).clicked() {
                            self.memory_panel.address_list_pinned =
                                !self.memory_panel.address_list_pinned;
                        }
                    });
                });
                ui.separator();
                if ui.rect_contains_pointer(ui.max_rect())
                    && ui.input(|input| input.pointer.primary_pressed())
                {
                    self.memory_panel.saved_list_active = true;
                }
                let editing = self.memory_panel.edit_value_index.is_some()
                    || self.memory_panel.edit_description_index.is_some();
                if self.memory_panel.saved_list_active
                    && !editing
                    && ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::A))
                {
                    self.memory_panel.selected_saved = (0..self.memory_panel.saved.len()).collect();
                }
                if self.memory_panel.saved_list_active && !editing {
                    let (shift_w, shift_s, delete, shift_delete, edit) = ui.input(|input| {
                        (
                            input.modifiers.shift && input.key_pressed(egui::Key::W),
                            input.modifiers.shift && input.key_pressed(egui::Key::S),
                            !input.modifiers.shift && input.key_pressed(egui::Key::Delete),
                            input.modifiers.shift && input.key_pressed(egui::Key::Delete),
                            !input.modifiers.ctrl
                                && !input.modifiers.command
                                && !input.modifiers.alt
                                && input.key_pressed(egui::Key::C),
                        )
                    });
                    if shift_w && !self.memory_panel.selected_saved.is_empty() {
                        let end = self
                            .memory_panel
                            .selected_saved
                            .iter()
                            .copied()
                            .max()
                            .unwrap_or(0);
                        self.memory_panel.selected_saved.extend(0..=end);
                        self.memory_panel.saved_selection_anchor = Some(0);
                    }
                    if shift_s && !self.memory_panel.selected_saved.is_empty() {
                        let start = self
                            .memory_panel
                            .selected_saved
                            .iter()
                            .copied()
                            .min()
                            .unwrap_or(0);
                        self.memory_panel
                            .selected_saved
                            .extend(start..self.memory_panel.saved.len());
                        self.memory_panel.saved_selection_anchor =
                            self.memory_panel.saved.len().checked_sub(1);
                    }
                    if shift_delete {
                        self.delete_unselected_saved_memory();
                    } else if delete {
                        self.delete_selected_saved_memory();
                    } else if edit
                        && let Some(index) = self.memory_panel.selected_saved.iter().copied().min()
                    {
                        let position = ui
                            .ctx()
                            .pointer_latest_pos()
                            .unwrap_or(ui.next_widget_position())
                            + vec2(12.0, 12.0);
                        self.begin_saved_memory_value_edit(index, position);
                    }
                }
                let stt_width = 36.0;
                let header_column_width = ((ui.available_width() - stt_width - 21.0) / 4.0).max(70.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    Self::memory_table_cell(ui, stt_width, RichText::new("#").strong());
                    Self::memory_table_cell(
                        ui,
                        header_column_width,
                        RichText::new(self.tr("Address", "Address")).strong(),
                    );
                    Self::memory_table_cell(
                        ui,
                        header_column_width,
                        RichText::new(self.tr("Type", "Type")).strong(),
                    );
                    Self::memory_table_cell(
                        ui,
                        header_column_width,
                        RichText::new(self.tr("Value", "Value")).strong(),
                    );
                    Self::memory_table_cell(
                        ui,
                        header_column_width,
                        RichText::new(self.tr("Description", "Description")).strong(),
                    );
                });
                ui.separator();
                let row_height = 26.0;
                let count = self.memory_panel.saved.len();
                egui::ScrollArea::vertical()
                    .id_salt("saved-memory-addresses")
                    .auto_shrink([false, false])
                    .max_height(ui.available_height())
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for index in 0..count {
                            if index >= self.memory_panel.saved.len() {
                                continue;
                            }
                            let saved = self.memory_panel.saved[index].clone();
                            let selected = self.memory_panel.selected_saved.contains(&index);
                            let mut open_address = false;
                            let mut edit_value = false;
                            let mut delete = false;
                            let mut freeze_selection = None;
                            let mut instruction_watch = None;
                            let mut find_stable_pointer = false;
                            let mut deep_pointer_scan = false;
                            let mut save_to_library = false;
                            let mut persist_pointer_changes = false;
                            let mut row_hits = Vec::new();
                            let row_width = ui.available_width();
                            let column_width = ((row_width - stt_width - 21.0) / 4.0).max(70.0);
                            let full_row_rect = egui::Rect::from_min_size(
                                ui.next_widget_position(),
                                vec2(row_width, row_height),
                            );
                            if selected {
                                ui.painter().rect_stroke(
                                    full_row_rect.shrink(1.0),
                                    3.0,
                                    egui::Stroke::new(1.0, Color32::from_rgb(84, 178, 222)),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let row_response = ui
                                .interact(
                                    full_row_rect,
                                    ui.id().with(("saved-memory-row", index)),
                                    Sense::click(),
                                )
                                .on_hover_cursor(egui::CursorIcon::Default);
                            let mut response = row_response.clone();
                            ui.allocate_ui_with_layout(
                                vec2(row_width, row_height),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    ui.set_width(row_width);
                                    ui.add_space(3.0);
                                    let stt_response = Self::memory_label_cell(
                                        ui,
                                        stt_width,
                                        row_height,
                                        egui::Label::new((index + 1).to_string())
                                            .selectable(false)
                                            .sense(Sense::hover()),
                                    );
                                    row_hits.push(stt_response);
                                    let address_response = Self::memory_label_cell(
                                        ui,
                                        column_width,
                                        row_height,
                                        egui::Label::new(format_memory_address(saved.address))
                                            .selectable(false)
                                            .sense(Sense::hover()),
                                    );
                                    address_response
                                        .clone()
                                        .on_hover_cursor(egui::CursorIcon::Default);
                                    row_hits.push(address_response.clone());
                                    let type_response = Self::memory_label_cell(
                                        ui,
                                        column_width,
                                        row_height,
                                        egui::Label::new(match saved.text_encoding {
                                            Some(TextEncoding::Utf8) => "Text (UTF-8)",
                                            Some(TextEncoding::Utf16) => "Text (UTF-16)",
                                            None => memory_type_label(saved.value_type),
                                        })
                                        .selectable(false)
                                        .truncate(),
                                    );
                                    type_response
                                        .clone()
                                        .on_hover_cursor(egui::CursorIcon::Default);
                                    row_hits.push(type_response);
                                    let value_response = Self::memory_label_cell(
                                        ui,
                                        column_width,
                                        row_height,
                                        egui::Label::new(
                                            saved
                                                .current_text
                                                .clone()
                                                .or_else(|| {
                                                    saved.current.map(|value| {
                                                        format_scan_value(
                                                            value,
                                                            self.memory_panel.hex,
                                                        )
                                                    })
                                                })
                                                .unwrap_or_else(|| "?".to_owned()),
                                        )
                                        .selectable(false)
                                        .sense(Sense::hover()),
                                    );
                                    value_response
                                        .clone()
                                        .on_hover_cursor(egui::CursorIcon::Default);
                                    row_hits.push(value_response.clone());
                                    if self.memory_panel.edit_description_index == Some(index) {
                                        let description_response = ui.add_sized(
                                            [column_width, row_height],
                                            egui::TextEdit::singleline(
                                                &mut self.memory_panel.saved[index].description,
                                            ),
                                        );
                                        description_response.request_focus();
                                        row_hits.push(description_response.clone());
                                        if description_response.clicked_elsewhere()
                                            || (description_response.has_focus()
                                                && ui.input(|input| {
                                                    input.key_pressed(egui::Key::Enter)
                                                        || input.key_pressed(egui::Key::Escape)
                                                }))
                                        {
                                            self.memory_panel.edit_description_index = None;
                                            persist_pointer_changes = true;
                                        }
                                    } else {
                                        let description = if saved.description.is_empty() {
                                            RichText::new("description").weak()
                                        } else {
                                            RichText::new(&saved.description)
                                        };
                                        let description_response = Self::memory_label_cell(
                                            ui,
                                            column_width,
                                            row_height,
                                            egui::Label::new(description)
                                                .selectable(false)
                                                .truncate()
                                                .sense(Sense::hover()),
                                        );
                                        description_response
                                            .clone()
                                            .on_hover_cursor(egui::CursorIcon::Default);
                                        row_hits.push(description_response.clone());
                                    }
                                    let mut frozen = saved.frozen.is_some();
                                    let frozen_response = ui
                                        .add_enabled_ui(saved.text_encoding.is_none(), |ui| {
                                            ui.add_sized(
                                                [18.0, 18.0],
                                                egui::Checkbox::without_text(&mut frozen),
                                            )
                                        })
                                        .inner
                                        .on_hover_text("Freeze");
                                    row_hits.push(frozen_response.clone());
                                    if saved.text_encoding.is_none() && frozen_response.changed() {
                                        self.memory_panel.saved[index].frozen =
                                            if frozen { saved.current } else { None };
                                    }
                                },
                            );
                            if persist_pointer_changes {
                                self.persist_memory_pointers();
                            }
                            for hit in row_hits {
                                response = response.union(hit);
                            }
                            if ui.input(|input| {
                                input.pointer.button_pressed(egui::PointerButton::Primary)
                            }) && let Some(pointer) = ui.ctx().pointer_latest_pos()
                                && full_row_rect.contains(pointer)
                            {
                                let column = ((pointer.x - full_row_rect.left() - 3.0)
                                    / column_width)
                                    .floor() as isize;
                                let now = Instant::now();
                                let double_clicked = self
                                    .memory_panel
                                    .last_saved_cell_click
                                    .is_some_and(|(last_index, last_column, last_click)| {
                                        last_index == index
                                            && last_column == column
                                            && now.duration_since(last_click)
                                                <= Duration::from_millis(500)
                                    });
                                self.memory_panel.last_saved_cell_click =
                                    (!double_clicked).then_some((index, column, now));
                                if double_clicked {
                                    match column {
                                        0 => open_address = true,
                                        2 => edit_value = true,
                                        3 => self.memory_panel.edit_description_index = Some(index),
                                        _ => {}
                                    }
                                }
                            }
                            // Use the union response so clicks on the full-width row remain
                            // selectable even when a cell renderer owns the pointer event.
                            if response.clicked() {
                                self.memory_panel.saved_list_active = true;
                            }
                            if response.clicked() && !response.double_clicked() {
                                self.select_saved_memory_row(index, selected, ui);
                            }
                            if response.secondary_clicked() {
                                self.memory_panel.saved_list_active = true;
                                if !self.memory_panel.selected_saved.contains(&index) {
                                    self.memory_panel.selected_saved.clear();
                                    self.memory_panel.selected_saved.insert(index);
                                    self.memory_panel.saved_selection_anchor = Some(index);
                                }
                            }
                            response.context_menu(|ui| {
                                let selected_count = self.memory_panel.selected_saved.len();
                                let single_target = selected_count == 1;
                                let debugger_arch = self
                                    .memory_panel
                                    .process_pid
                                    .and_then(|pid| process_pointer_width(pid).ok())
                                    .map_or("auto", |width| if width == 4 { "x86" } else { "x64" });
                                let editable_selection = selected_count > 0
                                    && self.memory_panel.selected_saved.iter().all(
                                        |selected_index| {
                                            self.memory_panel
                                                .saved
                                                .get(*selected_index)
                                                .is_some_and(|entry| {
                                                    entry.value_type == saved.value_type
                                                        && entry.text_encoding
                                                            == saved.text_encoding
                                                })
                                        },
                                    );
                                let can_save =
                                    self.memory_panel
                                        .selected_saved
                                        .iter()
                                        .any(|selected_index| {
                                            self.memory_panel
                                                .saved
                                                .get(*selected_index)
                                                .is_some_and(|entry| entry.text_encoding.is_none())
                                        });
                                if ui
                                    .add_enabled(
                                        can_save,
                                        Button::new(self.tr(
                                            "Save address to library",
                                            "Lưu địa chỉ vào thư viện",
                                        )),
                                    )
                                    .clicked()
                                {
                                    save_to_library = true;
                                    ui.close();
                                }
                                if !single_target {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} {}",
                                            selected_count,
                                            self.tr("addresses selected", "địa chỉ được chọn")
                                        ))
                                        .weak()
                                        .small(),
                                    );
                                }
                                if ui
                                    .add_enabled(
                                        editable_selection,
                                        Button::new(if single_target {
                                            self.tr("Edit value", "Sửa giá trị")
                                        } else {
                                            self.tr(
                                                "Edit selected values",
                                                "Sửa các giá trị được chọn",
                                            )
                                        }),
                                    )
                                    .clicked()
                                {
                                    edit_value = true;
                                    ui.close();
                                }
                                let all_frozen =
                                    self.memory_panel
                                        .selected_saved
                                        .iter()
                                        .all(|selected_index| {
                                            self.memory_panel
                                                .saved
                                                .get(*selected_index)
                                                .is_some_and(|entry| entry.frozen.is_some())
                                        });
                                if ui
                                    .add_enabled(
                                        can_save,
                                        Button::new(if all_frozen {
                                            self.tr("Unfreeze selected", "Bỏ đóng băng được chọn")
                                        } else {
                                            self.tr("Freeze selected", "Đóng băng được chọn")
                                        }),
                                    )
                                    .clicked()
                                {
                                    freeze_selection = Some(!all_frozen);
                                    ui.close();
                                }
                                ui.separator();
                                if ui
                                    .add_enabled(
                                        single_target,
                                        Button::new(format!(
                                            "{} ({debugger_arch})",
                                            self.tr(
                                                "Find instructions accessing this address",
                                                "Tìm lệnh truy cập địa chỉ này"
                                            )
                                        )),
                                    )
                                    .clicked()
                                {
                                    instruction_watch = Some(true);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        single_target,
                                        Button::new(format!(
                                            "{} ({debugger_arch})",
                                            self.tr(
                                                "Find instructions writing this address",
                                                "Tìm lệnh ghi vào địa chỉ này"
                                            )
                                        )),
                                    )
                                    .clicked()
                                {
                                    instruction_watch = Some(false);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        single_target,
                                        Button::new(self.tr(
                                            "Find stable pointer automatically",
                                            "Tự động tìm pointer ổn định",
                                        )),
                                    )
                                    .clicked()
                                {
                                    find_stable_pointer = true;
                                    ui.close();
                                }
                                let deep_label = if self
                                    .memory_panel
                                    .deep_pointer_dialog
                                    .as_ref()
                                    .is_some_and(|dialog| dialog.map_a.is_some())
                                {
                                    self.tr(
                                        "Deep pointer scan: compare with map A",
                                        "Quét pointer sâu: so sánh với map A",
                                    )
                                } else {
                                    self.tr(
                                        "Deep pointer scan: create map A",
                                        "Quét pointer sâu: tạo map A",
                                    )
                                };
                                if ui
                                    .add_enabled(single_target, Button::new(deep_label))
                                    .clicked()
                                {
                                    deep_pointer_scan = true;
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        single_target,
                                        Button::new(self.tr(
                                            "Browse this memory region",
                                            "Duyệt vùng bộ nhớ này",
                                        )),
                                    )
                                    .clicked()
                                {
                                    self.memory_panel.memory_view_dialog = Some(MemoryViewDialog {
                                        address: saved.address,
                                        tracked_base: None,
                                        kind: MemoryViewKind::Bytes,
                                        display_type: MemoryDisplayType::ByteHex,
                                        relative_addresses: false,
                                        pinned: true,
                                        elements: default_structure_elements(),
                                        pending_add: None,
                                        pending_track: None,
                                        pointer_width: self
                                            .memory_panel
                                            .process_pid
                                            .and_then(|pid| process_pointer_width(pid).ok())
                                            .unwrap_or(8),
                                        previous_bytes: Vec::new(),
                                        byte_change_times: HashMap::new(),
                                        classes: vec![StructureClass {
                                            name: "Class_0".to_owned(),
                                            address: saved.address,
                                            elements: default_structure_elements(),
                                        }],
                                        selected_class: 0,
                                        class_detection_status: String::new(),
                                        class_detection_attempted: false,
                                        auto_dissected: false,
                                        history: Vec::new(),
                                    });
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        single_target,
                                        Button::new(self.tr(
                                            "Dissect data/structure",
                                            "Phân tích dữ liệu/cấu trúc",
                                        )),
                                    )
                                    .clicked()
                                {
                                    self.memory_panel.memory_view_dialog = Some(MemoryViewDialog {
                                        address: saved.address,
                                        tracked_base: None,
                                        kind: MemoryViewKind::Structure,
                                        display_type: MemoryDisplayType::ByteHex,
                                        relative_addresses: false,
                                        pinned: true,
                                        elements: default_structure_elements(),
                                        pending_add: None,
                                        pending_track: None,
                                        pointer_width: self
                                            .memory_panel
                                            .process_pid
                                            .and_then(|pid| process_pointer_width(pid).ok())
                                            .unwrap_or(8),
                                        previous_bytes: Vec::new(),
                                        byte_change_times: HashMap::new(),
                                        classes: vec![StructureClass {
                                            name: "Class_0".to_owned(),
                                            address: saved.address,
                                            elements: default_structure_elements(),
                                        }],
                                        selected_class: 0,
                                        class_detection_status: String::new(),
                                        class_detection_attempted: false,
                                        auto_dissected: false,
                                         history: Vec::new(),
                                    });
                                    ui.close();
                                }
                                if let Some(pointer) = saved.pointer.as_ref()
                                    && pointer.module.is_some()
                                    && ui
                                        .add_enabled(
                                            single_target,
                                            Button::new(self.tr(
                                                "Copy pointer for Macro",
                                                "Sao chép pointer cho Macro",
                                            )),
                                        )
                                        .clicked()
                                {
                                    ui.ctx().copy_text(format_pointer_expression(pointer));
                                    ui.close();
                                }
                                ui.separator();
                                if ui
                                    .add_enabled(
                                        single_target,
                                        Button::new(self.tr(
                                            "Change address / Pointer",
                                            "Thay đổi địa chỉ / Pointer",
                                        )),
                                    )
                                    .clicked()
                                {
                                    open_address = true;
                                    ui.close();
                                }
                                if ui
                                    .button(if single_target {
                                        self.tr("Delete", "Xóa")
                                    } else {
                                        self.tr("Delete selected", "Xóa các mục đã chọn")
                                    })
                                    .clicked()
                                {
                                    delete = true;
                                    ui.close();
                                }
                            });
                            #[cfg(windows)]
                            if let Some(reads_and_writes) = instruction_watch {
                                self.open_instruction_watch(saved.address, reads_and_writes);
                            }
                            if find_stable_pointer {
                                self.start_stable_pointer_scan(&saved);
                                ui.ctx().request_repaint();
                            }
                            if deep_pointer_scan {
                                self.start_or_compare_deep_pointer_scan(&saved);
                                ui.ctx().request_repaint();
                            }
                            if save_to_library {
                                let indices = self
                                    .memory_panel
                                    .selected_saved
                                    .iter()
                                    .copied()
                                    .collect::<Vec<_>>();
                                let mut saved_count = 0;
                                for selected_index in indices {
                                    let Some(entry) =
                                        self.memory_panel.saved.get_mut(selected_index)
                                    else {
                                        continue;
                                    };
                                    if entry.text_encoding.is_some() {
                                        continue;
                                    }
                                    entry.saved_to_library = true;
                                    if entry.description.is_empty() {
                                        entry.description =
                                            format_prefixed_memory_address(entry.address);
                                    }
                                    saved_count += 1;
                                }
                                self.persist_memory_pointers();
                                self.memory_panel.status =
                                    format!("{saved_count} address(es) saved to library");
                            }
                            if open_address {
                                let (address, offsets, pointer) =
                                    saved.pointer.as_ref().map_or_else(
                                        || {
                                            (
                                                format_prefixed_memory_address(saved.address),
                                                String::new(),
                                                false,
                                            )
                                        },
                                        |spec| {
                                            // If module-based, show "module+offset" instead of raw base (0)
                                            let base_str = if let Some((module, offset)) = &spec.module {
                                                format!("{}+{:X}", module, offset)
                                            } else {
                                                format_prefixed_memory_address(spec.base)
                                            };
                                            (
                                                base_str,
                                                spec.offsets
                                                    .iter()
                                                    .map(|offset| format!("{:X}", offset))
                                                    .collect::<Vec<_>>()
                                                    .join(", "),
                                                true,
                                            )
                                        },
                                    );

                                self.memory_panel.address_dialog = Some(AddressDialog {
                                    index,
                                    address,
                                    offsets,
                                    pointer,
                                    position: ui
                                        .ctx()
                                        .pointer_latest_pos()
                                        .unwrap_or(full_row_rect.left_bottom())
                                        + vec2(12.0, 20.0),
                                    rect: None,
                                });
                            }
                            if edit_value {
                                let position = ui
                                    .ctx()
                                    .pointer_latest_pos()
                                    .unwrap_or(full_row_rect.right_center())
                                    + vec2(12.0, 8.0);
                                self.begin_saved_memory_value_edit(index, position);
                            }
                            if let Some(freeze) = freeze_selection {
                                let selected = self.memory_panel.selected_saved.clone();
                                for selected_index in selected {
                                    if let Some(entry) =
                                        self.memory_panel.saved.get_mut(selected_index)
                                        && entry.text_encoding.is_none()
                                    {
                                        entry.frozen = freeze.then_some(entry.current).flatten();
                                    }
                                }
                                self.sync_memory_freeze_targets();
                            }
                            if delete {
                                self.delete_selected_saved_memory();
                            }
                        }
                    });
                if self.memory_panel.saved.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new(self.tr("No saved addresses", "No saved addresses"))
                                .weak(),
                        );
                    });
                }
            });
        self.render_saved_memory_value_editor(ui.ctx());
    }

    fn render_memory_settings(&mut self, ctx: &egui::Context) {
        if !self.memory_panel.memory_settings_open {
            return;
        }
        let mut changed = false;
        egui::CentralPanel::default()
            .frame(Self::memory_popup_frame(ctx))
            .show(ctx, |ui| {
                ui.label("Debugger method");
                changed |= ui
                    .radio_value(
                        &mut self.state.memory_debugger_method,
                        MemoryDebuggerMethod::Windows,
                        "Windows debugger",
                    )
                    .changed();
                changed |= ui
                    .radio_value(
                        &mut self.state.memory_debugger_method,
                        MemoryDebuggerMethod::Veh,
                        "VEH debugger",
                    )
                    .changed();
                if self.state.memory_debugger_method == MemoryDebuggerMethod::Veh {
                    ui.label(
                        RichText::new("Requires an injected VEH helper; not available yet")
                            .small()
                            .weak(),
                    );
                }
                ui.separator();
                ui.label("Target architecture");
                for (value, label) in [
                    (MemoryDebuggerArchitecture::Auto, "Auto detect"),
                    (MemoryDebuggerArchitecture::X86, "32-bit (x86)"),
                    (MemoryDebuggerArchitecture::X64, "64-bit (x64)"),
                ] {
                    changed |= ui
                        .radio_value(&mut self.state.memory_debugger_architecture, value, label)
                        .changed();
                }
                ui.separator();
                ui.label("Pointer scan safety");
                ui.label(
                    RichText::new(
                        "Recommended: Safe for normal use. Deep reads more memory and may take longer.",
                    )
                    .small()
                    .weak(),
                );
                ui.horizontal(|ui| {
                    if ui.button("Safe (5 levels / 0x1000)").clicked() {
                        self.state.memory_pointer_scan_depth = PointerScanLimits::SAFE.max_depth;
                        self.state.memory_pointer_scan_offset =
                            format!("{:X}", PointerScanLimits::SAFE.max_offset);
                        self.state.memory_pointer_scan_memory_mb =
                            PointerScanLimits::SAFE.max_bytes / (1024 * 1024);
                        self.state.memory_pointer_scan_result_limit =
                            PointerScanLimits::SAFE.result_limit;
                        changed = true;
                    }
                    if ui.button("Deep (7 levels / 0x4000)").clicked() {
                        self.state.memory_pointer_scan_depth = PointerScanLimits::DEEP.max_depth;
                        self.state.memory_pointer_scan_offset =
                            format!("{:X}", PointerScanLimits::DEEP.max_offset);
                        self.state.memory_pointer_scan_memory_mb =
                            PointerScanLimits::DEEP.max_bytes / (1024 * 1024);
                        self.state.memory_pointer_scan_result_limit =
                            PointerScanLimits::DEEP.result_limit;
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Levels");
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.state.memory_pointer_scan_depth).range(3..=8))
                        .changed();
                    ui.label("Max offset (hex)");
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut self.state.memory_pointer_scan_offset).desired_width(80.0))
                        .changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Map memory (MB)");
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.state.memory_pointer_scan_memory_mb).range(256..=4096))
                        .changed();
                    ui.label("Path limit");
                    changed |= ui
                        .add(
                            egui::DragValue::new(
                                &mut self.state.memory_pointer_scan_result_limit,
                            )
                            .range(64..=PointerScanLimits::MAX_RESULT_LIMIT),
                        )
                        .changed();
                });
            });
        if changed {
            self.persist();
        }
    }

    #[cfg(windows)]
    fn pointer_scan_limits(&self) -> PointerScanLimits {
        let offset = usize::from_str_radix(
            self.state
                .memory_pointer_scan_offset
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X"),
            16,
        )
        .unwrap_or(PointerScanLimits::SAFE.max_offset)
        .clamp(0x100, 0x10000);
        PointerScanLimits {
            max_offset: offset,
            max_depth: self.state.memory_pointer_scan_depth.clamp(3, 8),
            result_limit: self
                .state
                .memory_pointer_scan_result_limit
                .clamp(64, PointerScanLimits::MAX_RESULT_LIMIT),
            max_bytes: self
                .state
                .memory_pointer_scan_memory_mb
                .clamp(256, 4096)
                .saturating_mul(1024 * 1024),
        }
    }

    fn render_dll_studio_window(&mut self, ctx: &egui::Context) {
        if !self.memory_panel.show_dll_studio {
            return;
        }

        let mut open = self.memory_panel.show_dll_studio;
        let title = self.tr("Auto DLL Studio - Generator & Injector", "Auto DLL Studio - Trình Tạo & Tiêm DLL");
        egui::Window::new(title)
            .open(&mut open)
            .default_size([740.0, 540.0])
            .min_size([580.0, 420.0])
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Auto DLL Generator & Injector")
                            .strong()
                            .size(16.0)
                            .color(egui::Color32::from_rgb(100, 200, 255)),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new(self.tr(
                            "Generate C++ DLL project or inject memory scripts without coding",
                            "Tạo file DLL hoặc Inject script memory không cần C++",
                        ))
                        .small()
                        .weak(),
                    );
                });
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Section 1: Project Settings
                    ui.group(|ui| {
                        ui.label(
                            egui::RichText::new(self.tr(
                                "1. Project Configuration",
                                "1. Cấu hình Dự án (Project Config)",
                            ))
                            .strong(),
                        );
                        egui::Grid::new("dll-proj-grid")
                            .num_columns(2)
                            .spacing([10.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(self.tr("Project Name:", "Tên DLL (Project Name):"));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.memory_panel.dll_config.project_name)
                                        .desired_width(200.0),
                                );
                                ui.end_row();

                                ui.label(self.tr("Target Process:", "Tiến trình Mục tiêu (Target Process):"));
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.memory_panel.dll_config.target_process)
                                            .desired_width(180.0),
                                    );
                                    if let Some(proc_name) = &self.memory_panel.process_selector.split('(').next() {
                                        let clean_name = proc_name.trim().to_string();
                                        if !clean_name.is_empty()
                                            && ui.button(self.tr("Get Current Process", "Lấy Process Hiện tại")).clicked()
                                        {
                                            self.memory_panel.dll_config.target_process = clean_name;
                                        }
                                    }
                                });
                                ui.end_row();

                                ui.label(self.tr("Debug Console Window:", "Cửa sổ Console Debug:"));
                                let console_label = self.tr(
                                    "AllocConsole (Show debug console log window)",
                                    "AllocConsole (Hiện cửa sổ đen Debug log)",
                                );
                                ui.checkbox(
                                    &mut self.memory_panel.dll_config.alloc_console_for_debug,
                                    console_label,
                                );
                                ui.end_row();
                            });
                    });

                    ui.add_space(8.0);

                    // Section 2: Memory Entries Table
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(self.tr(
                                    "2. RAM Address & Patch Config",
                                    "2. Danh sách Địa chỉ RAM / Patch Config",
                                ))
                                .strong(),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .button(self.tr("+ Add New Address", "+ Thêm địa chỉ mới"))
                                    .clicked()
                                {
                                    self.memory_panel
                                        .dll_config
                                        .entries
                                        .push(crate::dll_generator::DllMemoryEntry::default());
                                }
                                if ui
                                    .button(self.tr("Import Saved Addresses", "Import từ Địa chỉ Đã lưu"))
                                    .clicked()
                                {
                                    let saved_list = self.memory_panel.saved.clone();
                                    let count = saved_list.len();
                                    for saved in &saved_list {
                                        let addr_str = format!("0x{:X}", saved.address);
                                        let val_str = saved
                                            .current
                                            .as_ref()
                                            .map(|v| format_scan_value(*v, false))
                                            .or_else(|| saved.current_text.clone())
                                            .unwrap_or_else(|| "0".to_string());
                                        let offsets_vec = saved
                                            .pointer
                                            .as_ref()
                                            .map(|p| p.offsets.clone())
                                            .unwrap_or_default();
                                        let entry = crate::dll_generator::DllMemoryEntry {
                                            enabled: true,
                                            name: if saved.description.is_empty() {
                                                format!("RAM_0x{:X}", saved.address)
                                            } else {
                                                saved.description.clone()
                                            },
                                            address: addr_str,
                                            offsets: offsets_vec,
                                            value_type: crate::dll_generator::DllValueType::Float,
                                            value_to_write: val_str,
                                            mode: crate::dll_generator::DllPatchMode::HotkeyToggle,
                                            hotkey_vk: Some(0x70),
                                        };
                                        self.memory_panel.dll_config.entries.push(entry);
                                    }
                                    let msg_template = self.tr(
                                        "Imported {} addresses from saved list!",
                                        "Đã import {} địa chỉ từ danh sách đã lưu!",
                                    );
                                    self.memory_panel.dll_status_msg =
                                        msg_template.replace("{}", &count.to_string());
                                }
                            });
                        });
                        ui.separator();

                        let mut to_delete = None;
                        let entries_len = self.memory_panel.dll_config.entries.len();

                        if entries_len == 0 {
                            ui.label(
                                egui::RichText::new(self.tr(
                                    "No RAM addresses added. Click '+ Add New Address' or 'Import Saved Addresses'.",
                                    "Chưa có địa chỉ RAM nào. Hãy bấm '+ Thêm địa chỉ mới' hoặc 'Import từ Địa chỉ Đã lưu'.",
                                ))
                                .weak(),
                            );
                        }

                        let is_vietnamese = self.state.ui_language == crate::model::UiLanguage::Vietnamese;
                        let tr_func = |en: &'static str, vi: &'static str| -> &'static str {
                            if is_vietnamese { vi } else { en }
                        };

                        for (idx, entry) in self.memory_panel.dll_config.entries.iter_mut().enumerate() {
                            ui.push_id(idx, |ui| {
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut entry.enabled, "");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut entry.name)
                                            .desired_width(120.0)
                                            .hint_text(tr_func("Name", "Tên")),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut entry.address)
                                            .desired_width(110.0)
                                            .hint_text(tr_func("Address 0x...", "Địa chỉ 0x...")),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut entry.value_to_write)
                                            .desired_width(70.0)
                                            .hint_text(tr_func("Value", "Giá trị")),
                                    );

                                    egui::ComboBox::from_id_salt(format!("valtype-{}", idx))
                                        .width(100.0)
                                        .selected_text(entry.value_type.label())
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut entry.value_type,
                                                crate::dll_generator::DllValueType::Float,
                                                "Float (f32)",
                                            );
                                            ui.selectable_value(
                                                &mut entry.value_type,
                                                crate::dll_generator::DllValueType::Double,
                                                "Double (f64)",
                                            );
                                            ui.selectable_value(
                                                &mut entry.value_type,
                                                crate::dll_generator::DllValueType::Int32,
                                                "Int32 (i32)",
                                            );
                                            ui.selectable_value(
                                                &mut entry.value_type,
                                                crate::dll_generator::DllValueType::Int64,
                                                "Int64 (i64)",
                                            );
                                            ui.selectable_value(
                                                &mut entry.value_type,
                                                crate::dll_generator::DllValueType::Bytes,
                                                "Bytes (NOP)",
                                            );
                                        });

                                    egui::ComboBox::from_id_salt(format!("mode-{}", idx))
                                        .width(140.0)
                                        .selected_text(entry.mode.label())
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut entry.mode,
                                                crate::dll_generator::DllPatchMode::HotkeyToggle,
                                                tr_func("Hotkey Toggle", "Phím tắt Toggle"),
                                            );
                                            ui.selectable_value(
                                                &mut entry.mode,
                                                crate::dll_generator::DllPatchMode::Freeze,
                                                tr_func("Continuous Freeze", "Khóa liên tục"),
                                            );
                                            ui.selectable_value(
                                                &mut entry.mode,
                                                crate::dll_generator::DllPatchMode::WriteOnce,
                                                tr_func("Write Once", "Ghi 1 lần"),
                                            );
                                            ui.selectable_value(
                                                &mut entry.mode,
                                                crate::dll_generator::DllPatchMode::NopInstruction,
                                                tr_func("NOP Instruction", "NOP Instruction"),
                                            );
                                        });

                                    if ui.button(tr_func("Delete", "Xóa")).clicked() {
                                        to_delete = Some(idx);
                                    }
                                });
                            });
                        }

                        if let Some(idx) = to_delete {
                            self.memory_panel.dll_config.entries.remove(idx);
                        }
                    });

                    ui.add_space(8.0);

                    // Section 3: Action Buttons & Output
                    ui.group(|ui| {
                        ui.label(
                            egui::RichText::new(self.tr("3. Export & Inject", "3. Xuất & Tiêm (Export & Inject)"))
                                .strong(),
                        );
                        if !self.memory_panel.dll_status_msg.is_empty() {
                            ui.label(
                                egui::RichText::new(&self.memory_panel.dll_status_msg)
                                    .color(egui::Color32::from_rgb(255, 215, 0)),
                            );
                        }

                        ui.horizontal(|ui| {
                            if ui
                                .button(self.tr(
                                    "Export C++ Project & Frida Script",
                                    "Xuất Dự án C++ & Frida Script",
                                ))
                                .clicked()
                            {
                                let export_dir =
                                    std::path::PathBuf::from("exports").join(&self.memory_panel.dll_config.project_name);
                                match crate::dll_generator::export_dll_project(
                                    &self.memory_panel.dll_config,
                                    &export_dir,
                                ) {
                                    Ok(path) => {
                                        let prefix = self.tr(
                                            "[OK] Project exported successfully to: ",
                                            "[OK] Đã xuất thành công dự án vào: ",
                                        );
                                        self.memory_panel.dll_status_msg = format!("{}{}", prefix, path.display());
                                    }
                                    Err(err) => {
                                        let prefix = self.tr("[Error] Export failed: ", "[Lỗi] Lỗi xuất dự án: ");
                                        self.memory_panel.dll_status_msg = format!("{}{}", prefix, err);
                                    }
                                }
                            }

                            if ui
                                .button(self.tr("Inject Frida Script Directly", "Inject Frida Script trực tiếp"))
                                .clicked()
                            {
                                if let Some(pid) = self.memory_panel.process_pid {
                                    let script =
                                        crate::dll_generator::generate_frida_js_script(&self.memory_panel.dll_config);
                                    let frida_helper = self.paths.frida_helper_exe.clone();
                                    self.network_panel.frida_log.clear();
                                    self.network_panel.frida_session = Some(crate::frida_injector::Session::attach(
                                        frida_helper,
                                        pid,
                                        script,
                                    ));
                                    let prefix = self.tr(
                                        "[OK] Frida Agent started for PID ",
                                        "[OK] Đã khởi chạy Frida Agent thành công cho PID ",
                                    );
                                    self.memory_panel.dll_status_msg = format!("{}{}", prefix, pid);
                                } else {
                                    self.memory_panel.dll_status_msg = self
                                        .tr(
                                            "[!] Please select a Target Process in Memory Scanner first!",
                                            "[!] Hãy chọn một Tiến trình (Process) trong Memory Scanner trước!",
                                        )
                                        .to_string();
                                }
                            }
                        });

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(self.tr("DLL File Path:", "Đường dẫn file DLL:"));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.memory_panel.inject_dll_file_path)
                                    .desired_width(260.0)
                                    .hint_text("C:\\path\\to\\file.dll"),
                            );
                            if ui
                                .button(self.tr("Inject DLL File into Process", "Inject DLL File vào Process"))
                                .clicked()
                            {
                                if let Some(pid) = self.memory_panel.process_pid {
                                    let path = std::path::PathBuf::from(&self.memory_panel.inject_dll_file_path);
                                    if !path.exists() {
                                        self.memory_panel.dll_status_msg = self
                                            .tr("[Error] DLL file does not exist!", "[Lỗi] Tệp DLL không tồn tại!")
                                            .to_string();
                                    } else {
                                        match crate::dll_generator::inject_dll_into_process(pid, &path) {
                                            Ok(_) => {
                                                let prefix = self.tr(
                                                    "[OK] Injected DLL successfully into PID ",
                                                    "[OK] Đã Inject DLL thành công vào PID ",
                                                );
                                                self.memory_panel.dll_status_msg = format!("{}{}", prefix, pid);
                                            }
                                            Err(err) => {
                                                let prefix = self.tr(
                                                    "[Error] Injection failed: ",
                                                    "[Lỗi] Lỗi Inject DLL: ",
                                                );
                                                self.memory_panel.dll_status_msg = format!("{}{}", prefix, err);
                                            }
                                        }
                                    }
                                } else {
                                    self.memory_panel.dll_status_msg = self
                                        .tr(
                                            "[!] Please select a Target Process in Memory Scanner first!",
                                            "[!] Hãy chọn một Tiến trình (Process) trong Memory Scanner trước!",
                                        )
                                        .to_string();
                                }
                            }
                        });
                    });
                });
            });

        self.memory_panel.show_dll_studio = open;
    }

    fn render_saved_address_library(&mut self, ctx: &egui::Context) {
        if !self.memory_panel.saved_library_open {
            return;
        }
        let mut load = None;
        let mut delete = None;
        egui::CentralPanel::default()
            .frame(Self::memory_popup_frame(ctx))
            .show(ctx, |ui| {
                let mut apps = self
                    .state
                    .memory_pointer_list
                    .iter()
                    .map(|entry| {
                        if entry.app_name.is_empty() {
                            entry.module.clone()
                        } else {
                            entry.app_name.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                apps.sort_by_key(|name| name.to_ascii_lowercase());
                apps.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for app in apps {
                        egui::CollapsingHeader::new(&app)
                            .default_open(true)
                            .show(ui, |ui| {
                                for (index, entry) in
                                    self.state.memory_pointer_list.iter().enumerate()
                                {
                                    let entry_app = if entry.app_name.is_empty() {
                                        &entry.module
                                    } else {
                                        &entry.app_name
                                    };
                                    if !entry_app.eq_ignore_ascii_case(&app) {
                                        continue;
                                    }
                                    ui.horizontal(|ui| {
                                        let address = if entry.module.is_empty() {
                                            if entry.code_module.is_empty() {
                                                entry.absolute_address.map_or_else(
                                                    || "Invalid address".to_owned(),
                                                    format_prefixed_memory_address,
                                                )
                                            } else {
                                                format!(
                                                    "{}+{:X} @ {:+X}",
                                                    entry.code_module,
                                                    entry.code_offset,
                                                    entry.code_address_offset
                                                )
                                            }
                                        } else {
                                            format!(
                                                "{}+{:X} [{}]",
                                                entry.module,
                                                entry.module_offset,
                                                entry
                                                    .offsets
                                                    .iter()
                                                    .map(|offset| format!("{offset:X}"))
                                                    .collect::<Vec<_>>()
                                                    .join(" → ")
                                            )
                                        };
                                        ui.label(if entry.name.is_empty() {
                                            &address
                                        } else {
                                            &entry.name
                                        })
                                        .on_hover_text(&address);
                                        if ui.small_button("Load").clicked() {
                                            load = Some(index);
                                        }
                                        if ui.small_button("Delete").clicked() {
                                            delete = Some(index);
                                        }
                                    });
                                }
                            });
                    }
                });
                if self.state.memory_pointer_list.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("No saved addresses").weak())
                    });
                }
            });
        if let Some(index) = load
            && let Some(entry) = self.state.memory_pointer_list.get(index).cloned()
            && let Some(value_type) = memory_type_from_config(&entry.value_type)
        {
            if !entry.code_module.is_empty() && entry.runtime_address.is_none() {
                self.memory_panel.status = format!(
                    "{} is unresolved â€” run Find written for {}+{:X}",
                    entry.name, entry.code_module, entry.code_offset
                );
                return;
            }
            let mut pointer = (!entry.module.is_empty()).then(|| PointerSpec {
                base: 0,
                module: Some((entry.module.clone(), entry.module_offset)),
                offsets: entry.offsets.clone(),
            });
            let mut address = entry
                .runtime_address
                .filter(|_| entry.runtime_process_id == self.memory_panel.process_pid)
                .or(entry.absolute_address)
                .unwrap_or_default();
            if let (Some(pid), Some(pointer)) = (self.memory_panel.process_pid, pointer.as_mut())
                && let Ok(base) = resolve_module_offset(pid, &entry.module, entry.module_offset)
            {
                pointer.base = base;
                address = resolve_memory_address(pid, base, Some(pointer)).unwrap_or_default();
            }
            let current = self
                .memory_panel
                .process_pid
                .and_then(|pid| read_scan_value(pid, address, value_type).ok());
            self.memory_panel.saved.push(SavedMemoryAddress {
                address,
                value_type,
                current,
                text_encoding: None,
                text_byte_len: 0,
                current_text: None,
                description: entry.name,
                pointer,
                frozen: None,
                saved_to_library: false,
            });
            self.memory_panel.status = "Saved address loaded".to_owned();
        }
        if let Some(index) = delete {
            self.state.memory_pointer_list.remove(index);
            crate::overlay::set_memory_pointer_entries(&self.state.memory_pointer_list);
            self.persist();
        }
    }

    fn render_memory_code_list(&mut self, ctx: &egui::Context) {
        if !self.memory_panel.code_list_open {
            return;
        }
        let mut corrected_actions = false;
        if let Some(pid) = self.memory_panel.process_pid {
            for entry in &mut self.state.memory_code_list {
                if let Ok(address) = resolve_module_offset(pid, &entry.module, entry.offset)
                    && let Ok(writes) = instruction_writes_memory(pid, address)
                    && entry.writes != writes
                {
                    entry.writes = writes;
                    corrected_actions = true;
                }
            }
        }
        if corrected_actions {
            self.persist();
        }

        enum CodeAction {
            OpenDisassembler(usize),
            ReplaceNop(usize),
            RestoreOriginal(usize),
            StartAccessWatch(usize),
            Rename(usize),
            Delete(usize),
            ReplaceAll,
        }

        let mut pending_action = None;

        egui::CentralPanel::default()
            .frame(Self::memory_popup_frame(ctx))
            .show(ctx, |ui| {
                if let Some(edit_idx) = self.memory_panel.edit_code_name_index {
                    if edit_idx < self.state.memory_code_list.len() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Rename code:").strong());
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.memory_panel.edit_code_name_input)
                                    .desired_width(260.0)
                                    .hint_text("Enter name (e.g. camera_write / speed_hack)"),
                            );
                            let save_clicked = ui.button("Save").clicked();
                            let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if save_clicked || enter_pressed {
                                if let Some(entry) = self.state.memory_code_list.get_mut(edit_idx) {
                                    entry.name = self.memory_panel.edit_code_name_input.trim().to_owned();
                                    self.persist();
                                    crate::overlay::set_memory_code_entries(&self.state.memory_code_list);
                                }
                                self.memory_panel.edit_code_name_index = None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.memory_panel.edit_code_name_index = None;
                            }
                        });
                        ui.separator();
                    } else {
                        self.memory_panel.edit_code_name_index = None;
                    }
                }

                ui.horizontal(|ui| {
                    Self::memory_view_cell(ui, 190.0, "Address / Module");
                    Self::memory_view_cell(ui, 310.0, "Name / Instruction");
                    Self::memory_view_cell(ui, 180.0, "Action / Status");
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, entry) in self.state.memory_code_list.iter().enumerate() {
                        let address_str = format!("{}+{:X}", entry.module, entry.offset);
                        let instruction_text = if entry.replaced {
                            format!("{} (Replaced: NOP)", entry.name)
                        } else if !entry.name.is_empty() && entry.name != entry.instruction {
                            format!("{}: {}", entry.name, entry.instruction)
                        } else {
                            entry.instruction.clone()
                        };

                        let row_res = ui
                            .horizontal(|ui| {
                                let address_response =
                                    Self::memory_view_cell(ui, 190.0, &address_str);
                                let instruction_response =
                                    Self::memory_view_cell(ui, 310.0, &instruction_text);
                                let action_label = if entry.replaced {
                                    "NOP Active"
                                } else if entry.writes {
                                    "Find written"
                                } else {
                                    "Find accessed"
                                };
                                let action_response = ui.button(action_label);
                                if action_response.clicked() {
                                    pending_action = Some(CodeAction::StartAccessWatch(index));
                                }
                                let rename_response = ui.small_button("Rename");
                                if rename_response.clicked() {
                                    pending_action = Some(CodeAction::Rename(index));
                                }
                                let delete_response = ui.small_button("Del");
                                if delete_response.clicked() {
                                    pending_action = Some(CodeAction::Delete(index));
                                }
                                address_response
                                    .union(instruction_response)
                                    .union(action_response)
                                    .union(rename_response)
                                    .union(delete_response)
                            })
                            .inner;

                        row_res.context_menu(|ui| {
                            if ui.button("Copy code entry").clicked() {
                                ui.ctx().copy_text(format!(
                                    "{address_str}\t{}",
                                    entry.instruction
                                ));
                                ui.close();
                            }
                            ui.separator();
                            let is_vietnamese = self.state.ui_language == crate::model::UiLanguage::Vietnamese;

                            let rename_label = if is_vietnamese { "Đổi tên mã này (Rename)" } else { "Rename code entry" };
                            if ui.button(rename_label).clicked() {
                                pending_action = Some(CodeAction::Rename(index));
                                ui.close_menu();
                            }

                            let disasm_label = if is_vietnamese {
                                "Mở bộ gỡ mã (Disassembler) tại đây"
                            } else {
                                "Open the disassembler at this location"
                            };
                            if ui.button(disasm_label).clicked() {
                                pending_action = Some(CodeAction::OpenDisassembler(index));
                                ui.close_menu();
                            }

                            ui.separator();

                            let nop_label = if is_vietnamese {
                                "Thay thế bằng mã không làm gì (Replace with code that does nothing)"
                            } else {
                                "Replace with code that does nothing"
                            };
                            if ui
                                .add_enabled(!entry.replaced, egui::Button::new(nop_label))
                                .clicked()
                            {
                                pending_action = Some(CodeAction::ReplaceNop(index));
                                ui.close_menu();
                            }

                            let restore_label = if is_vietnamese {
                                "Khôi phục mã gốc (Restore with original code)"
                            } else {
                                "Restore with original code"
                            };
                            if ui
                                .add_enabled(entry.replaced, egui::Button::new(restore_label))
                                .clicked()
                            {
                                pending_action = Some(CodeAction::RestoreOriginal(index));
                                ui.close_menu();
                            }

                            ui.separator();

                            let watch_label = if entry.writes {
                                if is_vietnamese {
                                    "Tìm các địa chỉ được ghi bởi mã này"
                                } else {
                                    "Find out what addresses this code writes to"
                                }
                            } else if is_vietnamese {
                                "Tìm các địa chỉ được truy cập bởi mã này"
                            } else {
                                "Find out what addresses this code accesses"
                            };
                            if ui.button(watch_label).clicked() {
                                pending_action = Some(CodeAction::StartAccessWatch(index));
                                ui.close_menu();
                            }

                            ui.separator();

                            let del_label = if is_vietnamese { "Xóa khỏi danh sách" } else { "Remove from list" };
                            if ui.button(del_label).clicked() {
                                pending_action = Some(CodeAction::Delete(index));
                                ui.close_menu();
                            }

                            let replace_all_label = if is_vietnamese { "Thay thế tất cả (Replace all)" } else { "Replace all" };
                            if ui.button(replace_all_label).clicked() {
                                pending_action = Some(CodeAction::ReplaceAll);
                                ui.close_menu();
                            }
                        });
                    }
                });
                if self.state.memory_code_list.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("No saved instructions").weak());
                    });
                }
            });

        match pending_action {
            Some(CodeAction::OpenDisassembler(index)) => {
                #[cfg(windows)]
                if let Some(pid) = self.memory_panel.process_pid
                    && let Some(entry) = self.state.memory_code_list.get(index)
                    && let Ok(address) = resolve_module_offset(pid, &entry.module, entry.offset)
                {
                    self.open_disassembler_at_address(address);
                }
            }
            Some(CodeAction::ReplaceNop(index)) => {
                #[cfg(windows)]
                self.replace_code_entry_with_nop(index);
            }
            Some(CodeAction::RestoreOriginal(index)) => {
                #[cfg(windows)]
                self.restore_code_entry_with_original_bytes(index);
            }
            Some(CodeAction::StartAccessWatch(index)) => {
                #[cfg(windows)]
                self.open_code_access_watch(index);
            }
            Some(CodeAction::Rename(index)) => {
                if let Some(entry) = self.state.memory_code_list.get(index) {
                    self.memory_panel.edit_code_name_index = Some(index);
                    self.memory_panel.edit_code_name_input = entry.name.clone();
                }
            }
            Some(CodeAction::Delete(index)) => {
                #[cfg(windows)]
                if self.state.memory_code_list[index].replaced {
                    self.restore_code_entry_with_original_bytes(index);
                }
                self.state.memory_code_list.remove(index);
                crate::overlay::set_memory_code_entries(&self.state.memory_code_list);
                self.persist();
            }
            Some(CodeAction::ReplaceAll) =>
            {
                #[cfg(windows)]
                for i in 0..self.state.memory_code_list.len() {
                    if !self.state.memory_code_list[i].replaced {
                        self.replace_code_entry_with_nop(i);
                    }
                }
            }
            None => {}
        }
    }

    #[cfg(windows)]
    fn open_disassembler_at_address(&mut self, address: usize) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process first".to_owned();
            return;
        };
        let (lines, status) =
            match disassemble_from(pid, address, self.state.memory_debugger_architecture, 30) {
                Ok(lines) => (lines, "Disassembler active".to_owned()),
                Err(error) => (Vec::new(), format!("Disassembly failed: {error}")),
            };
        self.memory_panel.disassembler_dialog = Some(DisassemblerDialog {
            address,
            lines,
            status,
        });
    }

    #[cfg(windows)]
    fn replace_code_entry_with_nop(&mut self, code_index: usize) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process first".to_owned();
            return;
        };
        let Some(entry) = self.state.memory_code_list.get(code_index) else {
            return;
        };
        let Ok(address) = resolve_module_offset(pid, &entry.module, entry.offset) else {
            self.memory_panel.status = format!("Unable to resolve address for {}", entry.name);
            return;
        };

        let original_bytes = match &entry.original_bytes {
            Some(bytes) if !bytes.is_empty() => bytes.clone(),
            _ => match get_instruction_bytes(pid, address) {
                Ok(bytes) if !bytes.is_empty() => bytes,
                _ => {
                    self.memory_panel.status =
                        format!("Could not read instruction bytes at 0x{address:X}");
                    return;
                }
            },
        };

        let len = original_bytes.len();
        let nop_bytes = vec![0x90u8; len];

        match write_code_bytes(pid, address, &nop_bytes) {
            Ok(()) => {
                let entry = &mut self.state.memory_code_list[code_index];
                entry.original_bytes = Some(original_bytes);
                entry.replaced = true;
                self.memory_panel.status = format!(
                    "Replaced '{}' with code that does nothing (NOP)",
                    entry.name
                );
                self.persist();
            }
            Err(error) => {
                self.memory_panel.status =
                    format!("Failed to replace instruction with NOP: {error}");
            }
        }
    }

    #[cfg(windows)]
    fn restore_code_entry_with_original_bytes(&mut self, code_index: usize) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process first".to_owned();
            return;
        };
        let Some(entry) = self.state.memory_code_list.get(code_index) else {
            return;
        };
        let Some(original_bytes) = &entry.original_bytes else {
            return;
        };
        if !entry.replaced || original_bytes.is_empty() {
            return;
        }
        let Ok(address) = resolve_module_offset(pid, &entry.module, entry.offset) else {
            self.memory_panel.status = format!("Unable to resolve address for {}", entry.name);
            return;
        };

        let bytes_to_restore = original_bytes.clone();
        match write_code_bytes(pid, address, &bytes_to_restore) {
            Ok(()) => {
                let entry = &mut self.state.memory_code_list[code_index];
                entry.replaced = false;
                self.memory_panel.status = format!("Restored original code for '{}'", entry.name);
                self.persist();
            }
            Err(error) => {
                self.memory_panel.status =
                    format!("Failed to restore original instruction: {error}");
            }
        }
    }

    #[cfg(windows)]
    fn add_instruction_to_code_list(&mut self, address: usize, instruction: &str, writes: bool) {
        let Some(pid) = self.memory_panel.process_pid else {
            return;
        };
        let Ok((module, offset)) = module_offset_for_address(pid, address) else {
            self.memory_panel.status = "Instruction is not inside a loaded module".to_owned();
            return;
        };
        if self
            .state
            .memory_code_list
            .iter()
            .any(|entry| entry.module.eq_ignore_ascii_case(&module) && entry.offset == offset)
        {
            self.memory_panel.status = "Instruction is already in the code list".to_owned();
            return;
        }
        let writes = instruction_writes_memory(pid, address).unwrap_or(writes);
        self.state.memory_code_list.push(MemoryCodeEntry {
            name: instruction.to_owned(),
            module,
            offset,
            instruction: instruction.to_owned(),
            writes,
            original_bytes: None,
            replaced: false,
        });
        crate::overlay::set_memory_code_entries(&self.state.memory_code_list);
        self.memory_panel.code_list_open = true;
        self.memory_panel.status = "Instruction added to code list".to_owned();
        self.persist();
    }

    #[cfg(windows)]
    fn open_code_access_watch(&mut self, code_index: usize) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        if self.state.memory_debugger_method == MemoryDebuggerMethod::Veh {
            self.memory_panel.status =
                "VEH debugger requires the injected helper and is not available yet".to_owned();
            return;
        }
        let Some(entry) = self.state.memory_code_list.get(code_index).cloned() else {
            return;
        };
        let instruction_address = match resolve_module_offset(pid, &entry.module, entry.offset) {
            Ok(address) => address,
            Err(error) => {
                self.memory_panel.status = format!("Unable to resolve saved code: {error}");
                return;
            }
        };
        let current_instruction = disassemble_from(
            pid,
            instruction_address,
            self.state.memory_debugger_architecture,
            1,
        )
        .ok()
        .and_then(|mut lines| lines.pop())
        .map(|(_, _, instruction)| instruction);
        if current_instruction
            .as_ref()
            .is_none_or(|current| !is_instruction_compatible(&entry.instruction, current))
        {
            let (_tx, rx) = mpsc::channel();
            self.memory_panel.code_access_dialog = Some(CodeAccessDialog {
                code_index,
                instruction_address,
                status: format!(
                    "Stale code anchor - expected '{}', found '{}'",
                    entry.instruction,
                    current_instruction.as_deref().unwrap_or("unreadable code")
                ),
                addresses: Vec::new(),
                rx,
                active: None,
                pinned: true,
                selected: None,
                value_type: self.memory_panel.value_type,
                values: HashMap::new(),
                tracked_name: String::new(),
                tracked_offset: "0".to_owned(),
                save_tracked: false,
                auto_stop_on_hit: false,
                hits_sort: 0,
            });
            return;
        }
        self.close_memory_debuggers();
        let (tx, rx) = mpsc::channel();
        let started = AccessWatch::start(
            pid,
            instruction_address,
            self.state.memory_debugger_architecture,
            move |event| {
                let _ = tx.send(event);
            },
        );
        let (active, status) = match started {
            Ok(active) => (Some(active), "Attaching debugger…".to_owned()),
            Err(error) => (None, format!("Unable to start debugger: {error}")),
        };
        self.memory_panel.code_access_dialog = Some(CodeAccessDialog {
            code_index,
            instruction_address,
            status,
            addresses: Vec::new(),
            rx,
            active,
            pinned: true,
            selected: None,
            value_type: self.memory_panel.value_type,
            values: HashMap::new(),
            tracked_name: String::new(),
            tracked_offset: "0".to_owned(),
            save_tracked: false,
            auto_stop_on_hit: false,
            hits_sort: 0,
        });
    }

    #[cfg(windows)]
    fn start_stable_pointer_scan(&mut self, saved: &SavedMemoryAddress) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        let Some(expected_value) = saved.current else {
            self.memory_panel.status = self
                .tr("Unable to read this address", "Không thể đọc địa chỉ này")
                .to_owned();
            return;
        };
        let target = saved.address;
        let progress = Arc::new(AtomicUsize::new(0));
        let limits = self.pointer_scan_limits();
        let modules = match process_modules(pid) {
            Ok(modules) => modules,
            Err(error) => {
                let status = format!("Unable to list process modules: {error}");
                self.memory_panel.status.clone_from(&status);
                self.memory_panel.stable_pointer_dialog = Some(StablePointerDialog {
                    source_address: target,
                    source_pid: pid,
                    value_type: saved.value_type,
                    expected_value,
                    status,
                    candidates: Vec::new(),
                    selected: None,
                    rx: None,
                    progress,
                    limits,
                    filter: String::new(),
                    exe_only: false,
                    last_live_refresh: Instant::now(),
                    validation_pid: None,
                    validation_cursor: 0,
                    filter_rx: None,
                });
                return;
            }
        };
        let worker_progress = Arc::clone(&progress);
        let pointer_width = process_pointer_width(pid).unwrap_or(std::mem::size_of::<usize>());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = scan_pointer_paths_with_budget(
                pid,
                target,
                &modules,
                pointer_width,
                limits.max_offset,
                limits.max_depth,
                limits.result_limit,
                limits.max_bytes,
                worker_progress,
            )
            .map_err(|error| error.to_string());
            let _ = tx.send(StablePointerJobResult { pid, result });
        });
        self.memory_panel.stable_pointer_dialog = Some(StablePointerDialog {
            source_address: target,
            source_pid: pid,
            value_type: saved.value_type,
            expected_value,
            status: "Scanning pointer paths…".to_owned(),
            candidates: Vec::new(),
            selected: None,
            rx: Some(rx),
            progress,
            limits,
            filter: String::new(),
            exe_only: false,
            last_live_refresh: Instant::now(),
            validation_pid: None,
            validation_cursor: 0,
            filter_rx: None,
        });
    }

    #[cfg(not(windows))]
    fn start_stable_pointer_scan(&mut self, _saved: &SavedMemoryAddress) {
        self.memory_panel.status = "Stable pointer scan is available on Windows only".to_owned();
    }

    #[cfg(windows)]
    fn start_or_compare_deep_pointer_scan(&mut self, saved: &SavedMemoryAddress) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        let modules = match process_modules(pid) {
            Ok(modules) => modules,
            Err(error) => {
                let status = format!("Unable to list process modules: {error}");
                self.memory_panel.status.clone_from(&status);
                if let Some(dialog) = self.memory_panel.deep_pointer_dialog.as_mut() {
                    dialog.status = status;
                    dialog.rx = None;
                } else {
                    self.memory_panel.deep_pointer_dialog = Some(DeepPointerDialog {
                        map_a: None,
                        source_pid: pid,
                        source_address: saved.address,
                        value_type: saved.value_type,
                        status,
                        rx: None,
                        progress: Arc::new(AtomicUsize::new(0)),
                        candidates: Vec::new(),
                        selected: HashSet::new(),
                        selection_anchor: None,
                        filter: String::new(),
                        exe_only: false,
                        display_type: saved.value_type,
                        resolved_rows: HashMap::new(),
                    });
                }
                return;
            }
        };
        let progress = Arc::new(AtomicUsize::new(0));
        let pointer_width = process_pointer_width(pid).unwrap_or(std::mem::size_of::<usize>());
        let limits = self.pointer_scan_limits();
        let worker_progress = Arc::clone(&progress);
        let (tx, rx) = mpsc::channel();
        if let Some((map_a, target_a)) = self
            .memory_panel
            .deep_pointer_dialog
            .as_ref()
            .and_then(|dialog| dialog.map_a.clone().map(|map| (map, dialog.source_address)))
        {
            let target = saved.address;
            thread::spawn(move || {
                let result = capture_pointer_map_with_budget(
                    pid,
                    &modules,
                    pointer_width,
                    limits.max_bytes,
                    worker_progress,
                )
                .map(|map_b| {
                    // ponytail: search past the display limit before intersecting maps;
                    // otherwise different traversal order can hide every common path.
                    let comparison_limit = limits.result_limit.saturating_mul(16).clamp(
                        PointerScanLimits::MAX_RESULT_LIMIT,
                        PointerScanLimits::MAX_RESULT_LIMIT.saturating_mul(4),
                    );
                    let paths_a = map_a.paths_to(
                        target_a,
                        limits.max_offset,
                        limits.max_depth,
                        comparison_limit,
                    );
                    let paths_b = map_b.paths_to(
                        target,
                        limits.max_offset,
                        limits.max_depth,
                        comparison_limit,
                    );
                    let paths_b = paths_b.into_iter().collect::<HashSet<_>>();
                    paths_a
                        .into_iter()
                        .filter(|path| paths_b.contains(path))
                        .take(limits.result_limit)
                        .collect()
                })
                .map_err(|error| error.to_string());
                let _ = tx.send(DeepPointerJobResult::Compared(result));
            });
            let dialog = self.memory_panel.deep_pointer_dialog.as_mut().unwrap();
            dialog.value_type = saved.value_type;
            dialog.status = "Capturing map B and comparing with map A…".to_owned();
            dialog.rx = Some(rx);
            dialog.progress = progress;
            dialog.candidates.clear();
            dialog.resolved_rows.clear();
            dialog.selected.clear();
            dialog.selection_anchor = None;
        } else {
            thread::spawn(move || {
                let result = capture_pointer_map_with_budget(
                    pid,
                    &modules,
                    pointer_width,
                    limits.max_bytes,
                    worker_progress,
                )
                .map_err(|error| error.to_string());
                let _ = tx.send(DeepPointerJobResult::MapA(result));
            });
            self.memory_panel.deep_pointer_dialog = Some(DeepPointerDialog {
                map_a: None,
                source_pid: pid,
                source_address: saved.address,
                value_type: saved.value_type,
                status: "Capturing pointer map A…".to_owned(),
                rx: Some(rx),
                progress,
                candidates: Vec::new(),
                selected: HashSet::new(),
                selection_anchor: None,
                filter: String::new(),
                exe_only: false,
                display_type: saved.value_type,
                resolved_rows: HashMap::new(),
            });
        }
    }

    fn open_camera_matrix_dialog(&mut self) {
        let mut expressions = self
            .memory_panel
            .selected_saved
            .iter()
            .copied()
            .filter_map(|index| self.memory_panel.saved.get(index))
            .map(|saved| {
                saved.pointer.as_ref().map_or_else(
                    || format_prefixed_memory_address(saved.address),
                    format_pointer_expression,
                )
            })
            .take(3)
            .collect::<Vec<_>>();
        expressions.resize(3, String::new());
        if expressions.iter().all(String::is_empty) {
            expressions = vec![
                self.state.memory_camera_x.clone(),
                self.state.memory_camera_y.clone(),
                self.state.memory_camera_z.clone(),
            ];
        }
        self.memory_panel.camera_matrix_dialog = Some(CameraMatrixDialog {
            x: expressions[0].clone(),
            y: expressions[1].clone(),
            z: expressions[2].clone(),
            viewport_width: self.state.memory_camera_viewport_width.clone(),
            viewport_height: self.state.memory_camera_viewport_height.clone(),
            status: "Enter X, Y and Z pointer expressions, then start the scan.".to_owned(),
            candidates: Vec::new(),
            selected: None,
            rx: None,
            progress: Arc::new(AtomicUsize::new(0)),
            baseline: HashMap::new(),
            world: None,
            projection_variant: 0,
            last_preview_refresh: Instant::now() - Duration::from_secs(1),
            stability_sample: None,
            auto_pick_started: None,
        });
    }

    #[cfg(windows)]
    fn read_camera_world_component(pid: u32, expression: &str) -> Result<f32, String> {
        let expression = expression.trim();
        if let Some(literal) = expression.strip_prefix('=') {
            return literal
                .trim()
                .parse::<f32>()
                .map_err(|_| "Invalid literal value".to_owned());
        }
        let address = if let Some((module, module_offset, offsets)) =
            parse_pointer_expression(expression)
        {
            let base = resolve_module_offset(pid, &module, module_offset)
                .map_err(|error| error.to_string())?;
            let pointer = PointerSpec {
                base,
                module: Some((module, module_offset)),
                offsets,
            };
            resolve_memory_address(pid, base, Some(&pointer)).map_err(|error| error.to_string())?
        } else {
            parse_memory_address(expression)
                .ok_or_else(|| "Use an address, pointer expression, or =number".to_owned())?
        };
        match read_scan_value(pid, address, ScanValueType::F32)
            .map_err(|error| error.to_string())?
        {
            ScanValue::F32(value) => Ok(value),
            _ => unreachable!(),
        }
    }

    #[cfg(windows)]
    fn start_camera_matrix_scan(&mut self, dialog: &mut CameraMatrixDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.status = "Select a process first.".to_owned();
            return;
        };
        let world = match [
            Self::read_camera_world_component(pid, &dialog.x),
            Self::read_camera_world_component(pid, &dialog.y),
            Self::read_camera_world_component(pid, &dialog.z),
        ] {
            [Ok(x), Ok(y), Ok(z)] => [x, y, z],
            values => {
                dialog.status = values
                    .into_iter()
                    .enumerate()
                    .find_map(|(index, value)| {
                        value
                            .err()
                            .map(|error| format!("{}: {error}", ['X', 'Y', 'Z'][index]))
                    })
                    .unwrap_or_else(|| "Unable to read target coordinates".to_owned());
                return;
            }
        };
        let progress = Arc::new(AtomicUsize::new(0));
        let worker_progress = Arc::clone(&progress);
        let max_bytes = self
            .state
            .memory_pointer_scan_memory_mb
            .clamp(256, 4096)
            .saturating_mul(1024 * 1024);
        let result_limit = self.state.memory_pointer_scan_result_limit.clamp(256, 4096);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result =
                scan_view_projection_candidates(pid, max_bytes, result_limit, worker_progress)
                    .map_err(|error| error.to_string());
            let _ = tx.send(CameraMatrixJobResult { pid, result });
        });
        dialog.world = Some(world);
        dialog.candidates.clear();
        dialog.selected = None;
        dialog.baseline.clear();
        dialog.stability_sample = None;
        dialog.auto_pick_started = None;
        dialog.progress = progress;
        dialog.rx = Some(rx);
        dialog.status = format!(
            "Scanning matrix-shaped data for target ({:.3}, {:.3}, {:.3})…",
            world[0], world[1], world[2]
        );
    }

    #[cfg(not(windows))]
    fn start_camera_matrix_scan(&mut self, dialog: &mut CameraMatrixDialog) {
        dialog.status = "Camera matrix scanning is available on Windows.".to_owned();
    }

    fn filter_rotated_camera_matrices(&mut self, dialog: &mut CameraMatrixDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.status = "Select the restarted/current process first.".to_owned();
            return;
        };
        let before = dialog.candidates.len();
        let selected_address = dialog
            .selected
            .and_then(|index| dialog.candidates.get(index))
            .map(|candidate| candidate.address);
        let mut filtered = Vec::with_capacity(before);
        for candidate in &dialog.candidates {
            let Ok(bytes) = read_memory_bytes(pid, candidate.address, 64) else {
                continue;
            };
            let Some(matrix) = decode_f32_matrix(&bytes) else {
                continue;
            };
            let baseline = dialog
                .baseline
                .get(&candidate.address)
                .unwrap_or(&candidate.matrix);
            let changed = baseline
                .iter()
                .zip(matrix)
                .any(|(old, new)| (old - new).abs() > 1.0e-4);
            if changed {
                let mut candidate = candidate.clone();
                candidate.matrix = matrix;
                filtered.push(candidate);
            }
        }
        if filtered.is_empty() {
            dialog.status = format!(
                "No matrices changed since the previous snapshot; kept all {before} candidate(s). Rotate farther, then filter again."
            );
            return;
        }
        dialog.candidates = filtered;
        dialog.baseline = dialog
            .candidates
            .iter()
            .map(|candidate| (candidate.address, candidate.matrix))
            .collect();
        dialog.selected = selected_address.and_then(|address| {
            dialog
                .candidates
                .iter()
                .position(|candidate| candidate.address == address)
        });
        dialog.status = format!(
            "Kept {} of {before} matrices whose 16 float values changed with the camera.",
            dialog.candidates.len()
        );
    }

    fn start_camera_stability_filter(&mut self, dialog: &mut CameraMatrixDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.status = "Select a process first.".to_owned();
            return;
        };
        let baseline = dialog
            .candidates
            .iter()
            .filter_map(|candidate| {
                let bytes = read_memory_bytes(pid, candidate.address, 64).ok()?;
                decode_f32_matrix(&bytes).map(|matrix| (candidate.address, matrix))
            })
            .collect::<HashMap<_, _>>();
        if baseline.is_empty() {
            dialog.status = "None of the candidate matrices can currently be read.".to_owned();
            return;
        }
        dialog.stability_sample = Some((Instant::now(), baseline));
        dialog.status = "Keep the target and camera completely still for 1 second…".to_owned();
    }

    fn finish_camera_stability_filter(&mut self, dialog: &mut CameraMatrixDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.stability_sample = None;
            dialog.status = "Process changed during the stability check.".to_owned();
            return;
        };
        let Some((_, baseline)) = dialog.stability_sample.take() else {
            return;
        };
        let before = dialog.candidates.len();
        let selected_address = dialog
            .selected
            .and_then(|index| dialog.candidates.get(index))
            .map(|candidate| candidate.address);
        let mut stable = Vec::with_capacity(before);
        for candidate in &dialog.candidates {
            let Some(old) = baseline.get(&candidate.address) else {
                continue;
            };
            let Ok(bytes) = read_memory_bytes(pid, candidate.address, 64) else {
                continue;
            };
            let Some(matrix) = decode_f32_matrix(&bytes) else {
                continue;
            };
            let max_delta = old
                .iter()
                .zip(matrix)
                .map(|(old, new)| (old - new).abs())
                .fold(0.0f32, f32::max);
            if max_delta <= 5.0e-4 {
                let mut candidate = candidate.clone();
                candidate.matrix = matrix;
                stable.push(candidate);
            }
        }
        if stable.is_empty() {
            dialog.status = format!(
                "All {before} candidates changed while the camera was still; kept the old list. Keep both the camera and target still, then try again."
            );
            return;
        }
        dialog.candidates = stable;
        dialog.baseline = dialog
            .candidates
            .iter()
            .map(|candidate| (candidate.address, candidate.matrix))
            .collect();
        dialog.selected = selected_address.and_then(|address| {
            dialog
                .candidates
                .iter()
                .position(|candidate| candidate.address == address)
        });
        dialog.status = format!(
            "Removed continuously changing animation data; kept {} of {before} stable candidate(s). Now rotate the camera, then filter after rotation.",
            dialog.candidates.len()
        );
    }

    #[cfg(windows)]
    fn finish_camera_auto_pick(&mut self, dialog: &mut CameraMatrixDialog) {
        dialog.auto_pick_started = None;
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.status = "Select a process first.".to_owned();
            return;
        };
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            dialog.status = "Unable to read the cursor position.".to_owned();
            return;
        }
        let mut width = dialog
            .viewport_width
            .parse::<f32>()
            .unwrap_or(1920.0)
            .max(1.0);
        let mut height = dialog
            .viewport_height
            .parse::<f32>()
            .unwrap_or(1080.0)
            .max(1.0);
        let mut target = [cursor.x as f32, cursor.y as f32];
        if let Some(window) = window_list::list_open_windows()
            .into_iter()
            .find(|window| window.process_id == pid)
            && let Some(frame) = window_list::capture_window_client_preview_with_candidates(
                Some(&window.selector),
                &[],
                false,
                64,
            )
        {
            target[0] -= frame.screen_x as f32;
            target[1] -= frame.screen_y as f32;
            width = frame.logical_width.max(1) as f32;
            height = frame.logical_height.max(1) as f32;
            dialog.viewport_width = frame.logical_width.max(1).to_string();
            dialog.viewport_height = frame.logical_height.max(1).to_string();
            self.state
                .memory_camera_viewport_width
                .clone_from(&dialog.viewport_width);
            self.state
                .memory_camera_viewport_height
                .clone_from(&dialog.viewport_height);
            self.persist();
        }
        for candidate in &mut dialog.candidates {
            if let Ok(bytes) = read_memory_bytes(pid, candidate.address, 64)
                && let Some(matrix) = decode_f32_matrix(&bytes)
            {
                candidate.matrix = matrix;
            }
        }
        let world = dialog.world.unwrap_or([0.0; 3]);
        let Some((candidate, variant, _, error)) =
            best_camera_projection(&dialog.candidates, world, width, height, target)
        else {
            dialog.status =
                "No candidate can project the target onto the game viewport.".to_owned();
            return;
        };
        dialog.selected = Some(candidate);
        dialog.projection_variant = variant;
        let tolerance = width.hypot(height) * 0.08;
        dialog.status = if error <= tolerance {
            format!(
                "Auto-matched {} with {} ({error:.0}px from the target).",
                format_prefixed_memory_address(dialog.candidates[candidate].address),
                PROJECTION_CONVENTIONS[variant].0,
            )
        } else {
            format!(
                "No reliable match; the nearest projection is {error:.0}px away. Filter again or verify X/Y/Z."
            )
        };
    }

    fn render_camera_matrix_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.camera_matrix_dialog.take() else {
            return;
        };
        if let Some(rx) = dialog.rx.as_ref() {
            match rx.try_recv() {
                Ok(job) => {
                    dialog.rx = None;
                    if self.memory_panel.process_pid != Some(job.pid) {
                        dialog.status = "Process changed while scanning; start again.".to_owned();
                    } else {
                        match job.result {
                            Ok(candidates) => {
                                dialog.baseline = candidates
                                    .iter()
                                    .map(|candidate| (candidate.address, candidate.matrix))
                                    .collect();
                                dialog.status = format!(
                                    "Found {} matrix-shaped candidate(s). Keep the camera and target still, then click Remove motion while still.",
                                    candidates.len()
                                );
                                dialog.candidates = candidates;
                            }
                            Err(error) => dialog.status = format!("Camera scan failed: {error}"),
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    let mb = dialog.progress.load(Ordering::Relaxed) / (1024 * 1024);
                    dialog.status = format!("Scanning memory… {mb} MB read");
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    dialog.rx = None;
                    dialog.status = "Camera scan worker stopped unexpectedly.".to_owned();
                }
            }
        }
        if dialog
            .stability_sample
            .as_ref()
            .is_some_and(|(started, _)| started.elapsed() >= Duration::from_secs(1))
        {
            self.finish_camera_stability_filter(&mut dialog);
        } else if dialog.stability_sample.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        #[cfg(windows)]
        if dialog
            .auto_pick_started
            .is_some_and(|started| started.elapsed() >= Duration::from_secs(3))
        {
            self.finish_camera_auto_pick(&mut dialog);
        } else if let Some(started) = dialog.auto_pick_started {
            let remaining = 3.0f32 - started.elapsed().as_secs_f32();
            dialog.status = format!(
                "Move the cursor onto the target in the game… capturing in {:.1}s",
                remaining.max(0.0)
            );
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if dialog.selected.is_some()
            && dialog.last_preview_refresh.elapsed() >= Duration::from_millis(33)
        {
            if let (Some(pid), Some(candidate)) = (
                self.memory_panel.process_pid,
                dialog
                    .selected
                    .and_then(|index| dialog.candidates.get_mut(index)),
            ) && let Ok(bytes) = read_memory_bytes(pid, candidate.address, 64)
                && let Some(matrix) = decode_f32_matrix(&bytes)
            {
                candidate.matrix = matrix;
            }
            dialog.last_preview_refresh = Instant::now();
        }
        if dialog.selected.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        let mut persist_camera_inputs = false;
        egui::CentralPanel::default()
            .frame(Self::memory_popup_frame(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("X");
                    persist_camera_inputs |= ui.add(
                        egui::TextEdit::singleline(&mut dialog.x)
                            .desired_width(190.0)
                            .hint_text("module+offset [offsets]"),
                    ).changed();
                    ui.label("Y");
                    persist_camera_inputs |= ui.add(
                        egui::TextEdit::singleline(&mut dialog.y)
                            .desired_width(190.0)
                            .hint_text("module+offset [offsets]"),
                    ).changed();
                    ui.label("Z");
                    persist_camera_inputs |= ui.add(
                        egui::TextEdit::singleline(&mut dialog.z)
                            .desired_width(190.0)
                            .hint_text("module+offset [offsets]"),
                    ).changed();
                });
                let readings = self.memory_panel.process_pid.map(|pid| [
                    Self::read_camera_world_component(pid, &dialog.x),
                    Self::read_camera_world_component(pid, &dialog.y),
                    Self::read_camera_world_component(pid, &dialog.z),
                ]);
                if let Some([Ok(x), Ok(y), Ok(z)]) = readings.as_ref() {
                    dialog.world = Some([*x, *y, *z]);
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Live target values").strong());
                    for (index, axis) in ['X', 'Y', 'Z'].into_iter().enumerate() {
                        let (text, color) = match readings.as_ref().map(|values| &values[index]) {
                            Some(Ok(value)) => (format!("{axis}: {value:.6}"), Color32::LIGHT_GREEN),
                            Some(Err(error)) => (format!("{axis}: {error}"), Color32::LIGHT_RED),
                            None => (format!("{axis}: select a process"), ui.visuals().weak_text_color()),
                        };
                        ui.label(RichText::new(text).color(color).monospace());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Game viewport");
                    persist_camera_inputs |= ui.add(
                        egui::TextEdit::singleline(&mut dialog.viewport_width).desired_width(70.0),
                    ).changed();
                    ui.label("×");
                    persist_camera_inputs |= ui.add(
                        egui::TextEdit::singleline(&mut dialog.viewport_height).desired_width(70.0),
                    ).changed();
                    if ui
                        .add_enabled(dialog.rx.is_none(), Button::new("Start scan"))
                        .clicked()
                    {
                        self.start_camera_matrix_scan(&mut dialog);
                    }
                    if ui
                        .add_enabled(
                            !dialog.candidates.is_empty()
                                && dialog.rx.is_none()
                                && dialog.stability_sample.is_none(),
                            Button::new("Remove motion while still"),
                        )
                        .clicked()
                    {
                        self.start_camera_stability_filter(&mut dialog);
                    }
                    if ui
                        .add_enabled(
                            !dialog.candidates.is_empty()
                                && dialog.rx.is_none()
                                && dialog.stability_sample.is_none(),
                            Button::new("Filter after rotation"),
                        )
                        .clicked()
                    {
                        self.filter_rotated_camera_matrices(&mut dialog);
                    }
                    if ui
                        .add_enabled(
                            !dialog.candidates.is_empty()
                                && dialog.rx.is_none()
                                && dialog.stability_sample.is_none()
                                && dialog.auto_pick_started.is_none(),
                            Button::new("Auto-match target (3s)"),
                        )
                        .on_hover_text(
                            "Click, then move the cursor onto the target in the game. MacroNest selects the matrix and projection convention automatically.",
                        )
                        .clicked()
                    {
                        dialog.auto_pick_started = Some(Instant::now());
                    }
                    if ui
                        .add_enabled(dialog.selected.is_some(), Button::new("Add matrix address"))
                        .clicked()
                        && let (Some(pid), Some(index)) =
                            (self.memory_panel.process_pid, dialog.selected)
                        && let Some(candidate) = dialog.candidates.get(index)
                    {
                        let current =
                            read_scan_value(pid, candidate.address, ScanValueType::F32).ok();
                        self.memory_panel.saved.push(SavedMemoryAddress {
                            address: candidate.address,
                            value_type: ScanValueType::F32,
                            current,
                            text_encoding: None,
                            text_byte_len: 0,
                            current_text: None,
                            description: "View-projection matrix (16 floats)".to_owned(),
                            pointer: None,
                            frozen: None,
                            saved_to_library: false,
                        });
                    }
                });
                ui.label(RichText::new(&dialog.status).small().weak());
                ui.separator();
                let width = dialog
                    .viewport_width
                    .parse::<f32>()
                    .unwrap_or(1920.0)
                    .max(1.0);
                let height = dialog
                    .viewport_height
                    .parse::<f32>()
                    .unwrap_or(1080.0)
                    .max(1.0);
                let world = dialog.world.unwrap_or([0.0; 3]);
                ui.horizontal(|ui| {
                    ui.label("Detected convention");
                    egui::ComboBox::from_id_salt("camera-matrix-projection-convention")
                        .selected_text(PROJECTION_CONVENTIONS[dialog.projection_variant].0)
                        .show_ui(ui, |ui| {
                            for (index, (label, ..)) in PROJECTION_CONVENTIONS.iter().enumerate() {
                                ui.selectable_value(&mut dialog.projection_variant, index, *label);
                            }
                        });
                    ui.label(RichText::new("Use Auto-match target; this menu is only a manual fallback.").small().weak());
                });
                ui.columns(2, |columns| {
                    egui::ScrollArea::vertical().show(&mut columns[0], |ui| {
                        egui::Grid::new("camera-matrix-candidates")
                            .striped(true)
                            .num_columns(4)
                            .show(ui, |ui| {
                                ui.label("Address");
                                ui.label("Layout");
                                ui.label("Screen X");
                                ui.label("Screen Y");
                                ui.end_row();
                                for (index, candidate) in dialog.candidates.iter().enumerate() {
                                    let projections = project_world_variants(
                                        &candidate.matrix,
                                        world,
                                        width,
                                        height,
                                    );
                                    let (layout, point) = projections
                                        .iter()
                                        .enumerate()
                                        .filter_map(|(index, point)| point.map(|point| (PROJECTION_CONVENTIONS[index].0, point)))
                                        .find(|(_, point)| point[0] >= 0.0 && point[0] <= width && point[1] >= 0.0 && point[1] <= height)
                                        .or_else(|| projections.iter().enumerate().find_map(|(index, point)| point.map(|point| (PROJECTION_CONVENTIONS[index].0, point))))
                                        .unwrap_or(("—", [f32::NAN; 2]));
                                    let selected = dialog.selected == Some(index);
                                    if ui
                                        .selectable_label(
                                            selected,
                                            format_prefixed_memory_address(candidate.address),
                                        )
                                        .clicked()
                                    {
                                        dialog.selected = Some(index);
                                    }
                                    ui.label(layout);
                                    ui.label(if point[0].is_finite() {
                                        format!("{:.0}", point[0])
                                    } else {
                                        "—".to_owned()
                                    });
                                    ui.label(if point[1].is_finite() {
                                        format!("{:.0}", point[1])
                                    } else {
                                        "—".to_owned()
                                    });
                                    ui.end_row();
                                }
                            });
                    });
                    columns[1].label("Projection preview");
                    let (rect, _) = columns[1].allocate_exact_size(
                        vec2(columns[1].available_width(), 260.0),
                        Sense::hover(),
                    );
                    columns[1]
                        .painter()
                        .rect_filled(rect, 2.0, Color32::from_rgb(12, 14, 17));
                    if let Some(candidate) = dialog
                        .selected
                        .and_then(|index| dialog.candidates.get(index))
                    {
                        if let Some(point) = project_world_variants(
                            &candidate.matrix,
                            world,
                            width,
                            height,
                        )[dialog.projection_variant]
                        {
                            let position = egui::pos2(
                                rect.left() + rect.width() * point[0] / width,
                                rect.top() + rect.height() * point[1] / height,
                            );
                            if rect.contains(position) {
                                columns[1].painter().circle_filled(
                                    position,
                                    5.0,
                                    Color32::LIGHT_GREEN,
                                );
                                columns[1].painter().circle_stroke(
                                    position,
                                    9.0,
                                    egui::Stroke::new(1.5, Color32::WHITE),
                                );
                            }
                        }
                    }
                });
            });
        if persist_camera_inputs {
            self.state.memory_camera_x.clone_from(&dialog.x);
            self.state.memory_camera_y.clone_from(&dialog.y);
            self.state.memory_camera_z.clone_from(&dialog.z);
            self.state
                .memory_camera_viewport_width
                .clone_from(&dialog.viewport_width);
            self.state
                .memory_camera_viewport_height
                .clone_from(&dialog.viewport_height);
            self.persist();
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        self.memory_panel.camera_matrix_dialog = Some(dialog);
    }

    fn render_stable_pointer_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.stable_pointer_dialog.take() else {
            return;
        };
        if let Some(rx) = dialog.rx.as_ref()
            && let Ok(outcome) = rx.try_recv()
        {
            dialog.rx = None;
            if outcome.pid != dialog.source_pid {
                dialog.status = "The source process changed during pointer scan".to_owned();
            } else {
                match outcome.result {
                    Ok(paths) => {
                        dialog.candidates = paths
                            .into_iter()
                            .map(|path| StablePointerCandidate {
                                path,
                                valid: None,
                                resolved_base: None,
                                resolved_address: None,
                                observed_value: None,
                                live_value: None,
                                filter_value: None,
                            })
                            .collect();
                        dialog.candidates.sort_by_key(|candidate| {
                            !candidate.path.module.to_ascii_lowercase().ends_with(".exe")
                        });
                        dialog.selected = (!dialog.candidates.is_empty()).then_some(0);
                        dialog.status = if dialog.candidates.is_empty() {
                            format!(
                                "No module-based pointer paths found after reading {:.1} MB ({} levels, max offset 0x{:X})",
                                dialog.progress.load(Ordering::Relaxed) as f64 / 1_048_576.0,
                                dialog.limits.max_depth,
                                dialog.limits.max_offset,
                            )
                        } else {
                            let limit_note = (dialog.candidates.len()
                                >= dialog.limits.result_limit)
                                .then_some(" (path limit reached)")
                                .unwrap_or_default();
                            format!(
                                "{} candidate(s){limit_note}. Restart the game, restore the target value, select the new process, then Validate.",
                                dialog.candidates.len(),
                            )
                        };
                    }
                    Err(error) => dialog.status = format!("Pointer scan failed: {error}"),
                }
            }
        }
        if let Some(rx) = dialog.filter_rx.as_ref()
            && let Ok(outcome) = rx.try_recv()
        {
            dialog.filter_rx = None;
            if self.memory_panel.process_pid != Some(outcome.pid) {
                dialog.status = "The selected process changed during pointer filtering".to_owned();
            } else {
                match outcome.result {
                    Ok(filtered) => {
                        let values = filtered
                            .into_iter()
                            .map(|candidate| {
                                (candidate.address, candidate.current(dialog.value_type))
                            })
                            .collect::<HashMap<_, _>>();
                        dialog.candidates.retain_mut(|candidate| {
                            let Some(value) = candidate
                                .resolved_address
                                .and_then(|address| values.get(&address).copied())
                            else {
                                return false;
                            };
                            candidate.filter_value = Some(value);
                            candidate.live_value = Some(value);
                            true
                        });
                        dialog.selected = (!dialog.candidates.is_empty()).then_some(0);
                        dialog.status = format!(
                            "{}: {} → {} candidate(s)",
                            outcome.action.label(),
                            outcome.input_count,
                            dialog.candidates.len(),
                        );
                    }
                    Err(error) => {
                        dialog.status = format!("Pointer candidate filter failed: {error}")
                    }
                }
            }
        }

        let mut validate = false;
        let mut add = None;
        egui::CentralPanel::default()
            .frame(Self::memory_popup_frame(ctx))
            .show(ctx, |ui| {
                ui.label(&dialog.status);
                if dialog.rx.is_some() {
                    let scanned = dialog.progress.load(Ordering::Relaxed);
                    ui.label(format!("Read {:.1} MB", scanned as f64 / 1_048_576.0));
                    ui.spinner();
                    return;
                }
                ui.horizontal(|ui| {
                    let new_process = self
                        .memory_panel
                        .process_pid
                        .is_some_and(|pid| pid != dialog.source_pid);
                    if ui
                        .add_enabled(
                            new_process
                                && !dialog.candidates.is_empty()
                                && dialog.validation_pid.is_none()
                                && dialog.filter_rx.is_none(),
                            Button::new("Validate after restart"),
                        )
                        .clicked()
                    {
                        validate = true;
                    }
                    if dialog.validation_pid.is_some() || dialog.filter_rx.is_some() {
                        ui.spinner();
                    }
                    if ui
                        .add_enabled(
                            dialog.selected.is_some() && dialog.filter_rx.is_none(),
                            Button::new("Save selected pointer"),
                        )
                        .clicked()
                    {
                        add = Some(true);
                    }
                    let filter_resp = ui.add(
                        egui::TextEdit::singleline(&mut dialog.filter)
                            .desired_width(150.0)
                            .hint_text(RichText::new("Search module...").weak()),
                    );
                    Self::apply_vietnamese_input_if_changed(
                        &filter_resp,
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
                        &mut dialog.filter,
                    );
                    ui.checkbox(&mut dialog.exe_only, "EXE only");
                });
                ui.separator();
                const STATUS_WIDTH: f32 = 108.0;
                const ROOT_WIDTH: f32 = 195.0;
                const OFFSETS_WIDTH: f32 = 170.0;
                const ADDRESS_WIDTH: f32 = 145.0;
                const VALUE_WIDTH: f32 = 92.0;
                const CURRENT_WIDTH: f32 = 92.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (width, title) in [
                        (STATUS_WIDTH, "Status"),
                        (ROOT_WIDTH, "Root"),
                        (OFFSETS_WIDTH, "Offsets"),
                        (ADDRESS_WIDTH, "Resolved"),
                        (VALUE_WIDTH, "Value"),
                        (CURRENT_WIDTH, "Current"),
                    ] {
                        Self::memory_label_cell(
                            ui,
                            width,
                            20.0,
                            egui::Label::new(RichText::new(title).strong()).truncate(),
                        );
                    }
                });
                ui.separator();
                let filter = dialog.filter.trim().to_ascii_lowercase();
                let visible_indices = dialog
                    .candidates
                    .iter()
                    .enumerate()
                    .filter_map(|(index, candidate)| {
                        let module = candidate.path.module.to_ascii_lowercase();
                        (!(dialog.exe_only && !module.ends_with(".exe"))
                            && (filter.is_empty() || module.contains(&filter)))
                        .then_some(index)
                    })
                    .collect::<Vec<_>>();
                let refresh_visible_values = dialog.validation_pid.is_none()
                    && dialog.filter_rx.is_none()
                    && dialog.last_live_refresh.elapsed() >= Duration::from_millis(100);
                let refresh_pid = self.memory_panel.process_pid;
                egui::ScrollArea::both().show_rows(ui, 24.0, visible_indices.len(), |ui, rows| {
                    if refresh_visible_values {
                        if let Some(pid) = refresh_pid {
                            // ponytail: refresh only rendered rows; reading every pointer candidate
                            // here makes large validation results stall the UI for seconds.
                            for visible_row in rows.clone() {
                                let candidate =
                                    &mut dialog.candidates[visible_indices[visible_row]];
                                candidate.live_value =
                                    candidate.resolved_address.and_then(|address| {
                                        read_scan_value(pid, address, dialog.value_type).ok()
                                    });
                            }
                        }
                        dialog.last_live_refresh = Instant::now();
                    }
                    ui.set_min_width(
                        STATUS_WIDTH
                            + ROOT_WIDTH
                            + OFFSETS_WIDTH
                            + ADDRESS_WIDTH
                            + VALUE_WIDTH
                            + CURRENT_WIDTH,
                    );
                    for visible_row in rows {
                        let index = visible_indices[visible_row];
                        let candidate = &dialog.candidates[index];
                        let state = match candidate.valid {
                            Some(true) => "VERIFIED",
                            Some(false) => "BROKEN",
                            None if candidate.observed_value.is_some() => "VALUE CHANGED",
                            None => "NOT CHECKED",
                        };
                        let offsets = candidate
                            .path
                            .offsets
                            .iter()
                            .map(|offset| format!("{offset:X}"))
                            .collect::<Vec<_>>()
                            .join(" → ");
                        let root = format!(
                            "{}+{:X}",
                            candidate.path.module, candidate.path.module_offset
                        );
                        let address = candidate
                            .resolved_address
                            .map_or_else(|| "—".to_owned(), format_prefixed_memory_address);
                        let value = candidate.observed_value.map_or_else(
                            || "—".to_owned(),
                            |value| editable_scan_value(value, false),
                        );
                        let current = candidate.live_value.map_or_else(
                            || "—".to_owned(),
                            |value| editable_scan_value(value, false),
                        );
                        let row_rect = egui::Rect::from_min_size(
                            ui.next_widget_position(),
                            vec2(ui.available_width(), 24.0),
                        );
                        let response = ui.interact(
                            row_rect,
                            ui.id().with(("stable-pointer-row", index)),
                            Sense::click(),
                        );
                        if dialog.selected == Some(index) {
                            ui.painter().rect_filled(
                                row_rect,
                                2.0,
                                ui.visuals().selection.bg_fill.gamma_multiply(0.55),
                            );
                        }
                        ui.allocate_ui_with_layout(
                            row_rect.size(),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for (width, text) in [
                                    (STATUS_WIDTH, state.to_owned()),
                                    (ROOT_WIDTH, root),
                                    (OFFSETS_WIDTH, offsets),
                                    (ADDRESS_WIDTH, address),
                                    (VALUE_WIDTH, value),
                                    (CURRENT_WIDTH, current),
                                ] {
                                    Self::memory_label_cell(
                                        ui,
                                        width,
                                        24.0,
                                        egui::Label::new(text).truncate().selectable(false),
                                    );
                                }
                            },
                        );
                        if response.double_clicked() {
                            dialog.selected = Some(index);
                            add = Some(true);
                        } else if response.clicked() {
                            dialog.selected = Some(index);
                        }
                        response.context_menu(|ui| {
                            if ui.button("Add resolved address to Address list").clicked() {
                                dialog.selected = Some(index);
                                add = Some(false);
                                ui.close();
                            }
                        });
                    }
                });
            });

        if validate {
            self.validate_stable_pointer_candidates(&mut dialog);
        }
        if dialog.validation_pid.is_some() {
            self.advance_stable_pointer_validation(&mut dialog);
        }
        if let Some(save_to_library) = add {
            self.add_stable_pointer_candidate(&mut dialog, save_to_library);
        }
        if dialog.validation_pid.is_some() || dialog.filter_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if dialog.rx.is_some() || !dialog.candidates.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.memory_panel.stable_pointer_dialog = Some(dialog);
    }

    fn render_deep_pointer_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.deep_pointer_dialog.take() else {
            return;
        };
        if let Some(rx) = dialog.rx.as_ref()
            && let Ok(result) = rx.try_recv()
        {
            dialog.rx = None;
            match result {
                DeepPointerJobResult::MapA(Ok(map)) => {
                    dialog.map_a = Some(Arc::new(map));
                    dialog.status = "Map A ready. Restart the game, find the new target address, then right-click it and choose Compare with map A.".to_owned();
                }
                DeepPointerJobResult::Compared(Ok(paths)) => {
                    dialog.candidates = paths;
                    dialog.resolved_rows.clear();
                    dialog.selected.clear();
                    dialog.selection_anchor = None;
                    dialog.status = format!(
                        "Compared map A and map B: {} common pointer path(s).",
                        dialog.candidates.len()
                    );
                }
                DeepPointerJobResult::MapA(Err(error))
                | DeepPointerJobResult::Compared(Err(error)) => {
                    dialog.status = format!("Deep pointer scan failed: {error}");
                }
            }
        }
        let mut open = true;
        let mut clear = false;
        let mut add = false;
        let mut add_one = None;
        let title = "Deep pointer scan — map comparison";
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_position(egui::pos2(0.0, 0.0))
            .with_inner_size(vec2(760.0, 520.0))
            .with_min_inner_size(vec2(520.0, 300.0))
            .with_clamp_size_to_monitor_size(true)
            .with_decorations(false)
            .with_resizable(true)
            .with_always_on_top();
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("memory-deep-pointer-scan"),
            builder,
            |ctx, _| {
                Self::constrain_memory_popup_to_monitor(ctx);
                if ctx.input(|input| input.viewport().close_requested()) {
                    open = false;
                }
                let mut unpin = false;
                Self::render_memory_popup_titlebar(
                    ctx,
                    self.state.ui_language,
                    title,
                    &mut unpin,
                    &mut open,
                );
                egui::CentralPanel::default()
                    .frame(Self::memory_popup_frame(ctx))
                    .show(ctx, |ui| {
                        ui.label(&dialog.status);
                        if dialog.rx.is_some() {
                            let read = dialog.progress.load(Ordering::Relaxed);
                            ui.label(format!("Read {:.1} MB", read as f64 / 1_048_576.0));
                            ui.spinner();
                            return;
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    !dialog.selected.is_empty(),
                                    Button::new(format!(
                                        "Add selected ({})",
                                        dialog.selected.len()
                                    )),
                                )
                                .clicked()
                            {
                                add = true;
                            }
                            if ui.button("Clear map A").clicked() {
                                clear = true;
                            }
                            let filter_resp = ui.add(
                                egui::TextEdit::singleline(&mut dialog.filter)
                                    .desired_width(150.0)
                                    .hint_text(RichText::new("Search module...").weak()),
                            );
                            Self::apply_vietnamese_input_if_changed(
                                &filter_resp,
                                self.state.vietnamese_input_enabled,
                                self.state.vietnamese_input_mode,
                                &mut dialog.filter,
                            );
                            ui.checkbox(&mut dialog.exe_only, "EXE only");
                        });
                        ui.separator();
                        const ROOT_WIDTH: f32 = 250.0;
                        const OFFSETS_WIDTH: f32 = 180.0;
                        const ADDRESS_WIDTH: f32 = 150.0;
                        const VALUE_WIDTH: f32 = 130.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            Self::memory_label_cell(
                                ui,
                                ROOT_WIDTH,
                                22.0,
                                egui::Label::new(RichText::new("Root").strong()),
                            );
                            Self::memory_label_cell(
                                ui,
                                OFFSETS_WIDTH,
                                22.0,
                                egui::Label::new(RichText::new("Offsets").strong()),
                            );
                            Self::memory_label_cell(
                                ui,
                                ADDRESS_WIDTH,
                                22.0,
                                egui::Label::new(RichText::new("Address").strong()),
                            );
                            Self::memory_label_cell(
                                ui,
                                VALUE_WIDTH,
                                22.0,
                                egui::Label::new(RichText::new("Value").strong()),
                            );
                        });
                        ui.separator();
                        let filter = dialog.filter.trim().to_ascii_lowercase();
                        if ctx.input(|input| {
                            input.modifiers.command && input.key_pressed(egui::Key::A)
                        }) {
                            dialog.selected = dialog
                                .candidates
                                .iter()
                                .enumerate()
                                .filter(|(_, path)| {
                                    let module = path.module.to_ascii_lowercase();
                                    (!dialog.exe_only || module.ends_with(".exe"))
                                        && (filter.is_empty() || module.contains(&filter))
                                })
                                .map(|(index, _)| index)
                                .collect();
                        }
                        egui::ScrollArea::both().show(ui, |ui| {
                            for (index, path) in dialog.candidates.iter().enumerate() {
                                let module_lower = path.module.to_ascii_lowercase();
                                if (dialog.exe_only && !module_lower.ends_with(".exe"))
                                    || (!filter.is_empty() && !module_lower.contains(&filter))
                                {
                                    continue;
                                }
                                let root = format!("{}+{:X}", path.module, path.module_offset);
                                let offsets = path
                                    .offsets
                                    .iter()
                                    .map(|offset| format!("{offset:X}"))
                                    .collect::<Vec<_>>()
                                    .join(" → ");
                                let row_rect = egui::Rect::from_min_size(
                                    ui.next_widget_position(),
                                    vec2(
                                        (ROOT_WIDTH + OFFSETS_WIDTH + ADDRESS_WIDTH + VALUE_WIDTH)
                                            .max(ui.available_width()),
                                        24.0,
                                    ),
                                );
                                let stale = dialog.resolved_rows.get(&index).is_none_or(|row| {
                                    row.updated_at.elapsed() >= Duration::from_millis(750)
                                });
                                if ui.is_rect_visible(row_rect) && stale {
                                    let resolved = self.memory_panel.process_pid.and_then(|pid| {
                                        let base = resolve_module_offset(
                                            pid,
                                            &path.module,
                                            path.module_offset,
                                        )
                                        .ok()?;
                                        let pointer = PointerSpec {
                                            base,
                                            module: Some((path.module.clone(), path.module_offset)),
                                            offsets: path.offsets.clone(),
                                        };
                                        resolve_memory_address(pid, base, Some(&pointer)).ok()
                                    });
                                    let value = self
                                        .memory_panel
                                        .process_pid
                                        .zip(resolved)
                                        .and_then(|(pid, address)| {
                                            read_scan_value(pid, address, dialog.display_type).ok()
                                        });
                                    dialog.resolved_rows.insert(
                                        index,
                                        DeepPointerResolvedRow {
                                            address: resolved,
                                            value,
                                            updated_at: Instant::now(),
                                        },
                                    );
                                }
                                let resolved =
                                    dialog.resolved_rows.get(&index).and_then(|row| row.address);
                                let address_text = resolved
                                    .map_or_else(|| "—".to_owned(), format_prefixed_memory_address);
                                let value_text = dialog
                                    .resolved_rows
                                    .get(&index)
                                    .and_then(|row| row.value)
                                    .map_or_else(
                                        || "—".to_owned(),
                                        |value| editable_scan_value(value, false),
                                    );
                                let response = ui
                                    .interact(
                                        row_rect,
                                        ui.id().with(("deep-pointer-row", index)),
                                        Sense::click(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::Default);
                                if dialog.selected.contains(&index) {
                                    ui.painter().rect_filled(
                                        row_rect,
                                        2.0,
                                        ui.visuals().selection.bg_fill.gamma_multiply(0.55),
                                    );
                                }
                                ui.allocate_ui_at_rect(row_rect, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        Self::memory_label_cell(
                                            ui,
                                            ROOT_WIDTH,
                                            24.0,
                                            egui::Label::new(root).truncate(),
                                        );
                                        Self::memory_label_cell(
                                            ui,
                                            OFFSETS_WIDTH,
                                            24.0,
                                            egui::Label::new(offsets).truncate(),
                                        );
                                        Self::memory_label_cell(
                                            ui,
                                            ADDRESS_WIDTH,
                                            24.0,
                                            egui::Label::new(address_text).truncate(),
                                        );
                                        Self::memory_label_cell(
                                            ui,
                                            VALUE_WIDTH,
                                            24.0,
                                            egui::Label::new(value_text).truncate(),
                                        );
                                    });
                                });
                                if response.clicked() {
                                    let (shift, additive) = ui.input(|input| {
                                        (input.modifiers.shift, input.modifiers.command)
                                    });
                                    if shift && let Some(anchor) = dialog.selection_anchor {
                                        if !additive {
                                            dialog.selected.clear();
                                        }
                                        let (start, end) = if anchor <= index {
                                            (anchor, index)
                                        } else {
                                            (index, anchor)
                                        };
                                        dialog.selected.extend(start..=end);
                                    } else if additive {
                                        if !dialog.selected.insert(index) {
                                            dialog.selected.remove(&index);
                                        }
                                        dialog.selection_anchor = Some(index);
                                    } else {
                                        dialog.selected.clear();
                                        dialog.selected.insert(index);
                                        dialog.selection_anchor = Some(index);
                                    }
                                }
                                response.context_menu(|ui| {
                                    if ui.button("Copy pointer for Macro").clicked() {
                                        ui.ctx().copy_text(format_pointer_expression(
                                            &PointerSpec {
                                                base: 0,
                                                module: Some((
                                                    path.module.clone(),
                                                    path.module_offset,
                                                )),
                                                offsets: path.offsets.clone(),
                                            },
                                        ));
                                        ui.close();
                                    }
                                    if ui
                                        .add_enabled(
                                            resolved.is_some(),
                                            Button::new("Add to Address list"),
                                        )
                                        .clicked()
                                    {
                                        add_one = Some(index);
                                        ui.close();
                                    }
                                    ui.separator();
                                    ui.label(RichText::new("Display type").strong());
                                    for value_type in [
                                        ScanValueType::I8,
                                        ScanValueType::I16,
                                        ScanValueType::I32,
                                        ScanValueType::F32,
                                        ScanValueType::I64,
                                        ScanValueType::F64,
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut dialog.display_type,
                                                value_type,
                                                memory_type_label(value_type),
                                            )
                                            .clicked()
                                        {
                                            dialog.resolved_rows.clear();
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        });
                    });
                Self::render_memory_popup_resize_handles(ctx);
            },
        );
        if (add || add_one.is_some())
            && let Some(pid) = self.memory_panel.process_pid
        {
            let mut added = 0usize;
            let indices = add_one.map_or_else(
                || dialog.selected.iter().copied().collect::<Vec<_>>(),
                |index| vec![index],
            );
            for index in indices {
                let Some(path) = dialog.candidates.get(index).cloned() else {
                    continue;
                };
                let Ok(base) = resolve_module_offset(pid, &path.module, path.module_offset) else {
                    continue;
                };
                let pointer = PointerSpec {
                    base,
                    module: Some((path.module.clone(), path.module_offset)),
                    offsets: path.offsets,
                };
                if let Ok(address) = resolve_memory_address(pid, base, Some(&pointer)) {
                    self.memory_panel.saved.push(SavedMemoryAddress {
                        address,
                        value_type: dialog.display_type,
                        current: read_scan_value(pid, address, dialog.display_type).ok(),
                        text_encoding: None,
                        text_byte_len: 0,
                        current_text: None,
                        description: format!("{}+{:X}", path.module, path.module_offset),
                        pointer: Some(pointer),
                        frozen: None,
                        saved_to_library: false,
                    });
                    added += 1;
                }
            }
            self.memory_panel.status = format!("Added {added} deep pointer(s) to Address list");
        }
        if clear {
            self.memory_panel.status = "Pointer map A cleared".to_owned();
        } else if open {
            ctx.request_repaint_after(Duration::from_millis(if dialog.rx.is_some() {
                100
            } else {
                250
            }));
            self.memory_panel.deep_pointer_dialog = Some(dialog);
        }
    }

    #[cfg(windows)]
    fn validate_stable_pointer_candidates(&mut self, dialog: &mut StablePointerDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            return;
        };
        for candidate in &mut dialog.candidates {
            candidate.valid = None;
            candidate.resolved_base = None;
            candidate.resolved_address = None;
            candidate.observed_value = None;
            candidate.live_value = None;
            candidate.filter_value = None;
        }
        dialog.validation_pid = Some(pid);
        dialog.validation_cursor = 0;
        dialog.status = format!(
            "Validating 0/{} pointer path(s)...",
            dialog.candidates.len()
        );
    }

    #[cfg(windows)]
    fn advance_stable_pointer_validation(&mut self, dialog: &mut StablePointerDialog) {
        let Some(pid) = dialog.validation_pid else {
            return;
        };
        if self.memory_panel.process_pid != Some(pid) {
            dialog.validation_pid = None;
            dialog.status = "The selected process changed during validation".to_owned();
            return;
        }
        const BATCH_SIZE: usize = 16;
        let end = dialog
            .validation_cursor
            .saturating_add(BATCH_SIZE)
            .min(dialog.candidates.len());
        let modules_map: std::collections::HashMap<String, usize> = process_modules(pid)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, base, _)| (name.to_ascii_lowercase(), base))
            .collect();

        for candidate in &mut dialog.candidates[dialog.validation_cursor..end] {
            let module_lower = candidate.path.module.to_ascii_lowercase();
            let Some(&mod_base) = modules_map.get(&module_lower) else {
                candidate.valid = Some(false);
                continue;
            };
            let base = mod_base.wrapping_add(candidate.path.module_offset);
            let spec = PointerSpec {
                base,
                module: Some((candidate.path.module.clone(), candidate.path.module_offset)),
                offsets: candidate.path.offsets.clone(),
            };
            candidate.resolved_base = Some(base);
            let Ok(address) = resolve_memory_address(pid, base, Some(&spec)) else {
                candidate.valid = Some(false);
                continue;
            };
            candidate.resolved_address = Some(address);
            let Ok(observed) = read_scan_value(pid, address, dialog.value_type) else {
                candidate.valid = Some(false);
                continue;
            };
            candidate.observed_value = Some(observed);
            candidate.live_value = Some(observed);
            candidate.filter_value = Some(observed);
            if observed == dialog.expected_value {
                candidate.valid = Some(true);
            } else {
                candidate.valid = None;
            }
        }
        dialog.validation_cursor = end;
        if end < dialog.candidates.len() {
            dialog.status = format!(
                "Validating {end}/{} pointer path(s)...",
                dialog.candidates.len()
            );
            return;
        }
        let valid = dialog
            .candidates
            .iter()
            .filter(|candidate| candidate.valid == Some(true))
            .count();
        let changed = dialog
            .candidates
            .iter()
            .filter(|candidate| candidate.valid.is_none() && candidate.observed_value.is_some())
            .count();
        let broken = dialog.candidates.len().saturating_sub(valid + changed);
        dialog.validation_pid = None;
        dialog
            .candidates
            .sort_by_key(|candidate| match candidate.valid {
                Some(true) => 0,
                None if candidate.observed_value.is_some() => 1,
                _ => 2,
            });
        dialog.selected = (!dialog.candidates.is_empty()).then_some(0);
        dialog.status = format!(
            "PID {pid}: {valid} verified, {changed} resolved with a different value, {broken} broken. Expected {}.",
            editable_scan_value(dialog.expected_value, false)
        );
    }

    #[cfg(not(windows))]
    fn validate_stable_pointer_candidates(&mut self, _dialog: &mut StablePointerDialog) {}

    #[cfg(not(windows))]
    fn advance_stable_pointer_validation(&mut self, _dialog: &mut StablePointerDialog) {}

    fn add_stable_pointer_candidate(
        &mut self,
        dialog: &mut StablePointerDialog,
        save_to_library: bool,
    ) {
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.status = "Select a process first".to_owned();
            return;
        };
        let Some(candidate) = dialog
            .selected
            .and_then(|index| dialog.candidates.get(index))
        else {
            dialog.status = "Please select a pointer candidate first!".to_owned();
            return;
        };
        #[cfg(windows)]
        let Ok(base) =
            resolve_module_offset(pid, &candidate.path.module, candidate.path.module_offset)
        else {
            let msg = "Pointer module is not loaded".to_owned();
            self.memory_panel.status = msg.clone();
            dialog.status = msg;
            return;
        };
        #[cfg(not(windows))]
        let base = candidate.resolved_base.unwrap_or_default();
        let pointer = PointerSpec {
            base,
            module: Some((candidate.path.module.clone(), candidate.path.module_offset)),
            offsets: candidate.path.offsets.clone(),
        };
        let Ok(address) = resolve_memory_address(pid, base, Some(&pointer)) else {
            let msg = "Unable to resolve pointer".to_owned();
            self.memory_panel.status = msg.clone();
            dialog.status = msg;
            return;
        };
        let current = read_scan_value(pid, address, dialog.value_type).ok();
        let desc = format!(
            "{}+{:X}",
            candidate.path.module, candidate.path.module_offset
        );
        self.memory_panel.saved.push(SavedMemoryAddress {
            address,
            value_type: dialog.value_type,
            current,
            text_encoding: None,
            text_byte_len: 0,
            current_text: None,
            description: desc.clone(),
            pointer: Some(pointer),
            frozen: None,
            saved_to_library: save_to_library,
        });
        let msg = format!("✔ Pointer {desc} added to Address list!");
        self.memory_panel.status = msg.clone();
        dialog.status = msg;
        if save_to_library {
            self.persist_memory_pointers();
        }
    }

    #[cfg(windows)]
    fn open_instruction_watch(&mut self, address: usize, reads_and_writes: bool) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        if self.state.memory_debugger_method == MemoryDebuggerMethod::Veh {
            self.memory_panel.status =
                "VEH debugger requires the injected helper and is not available yet".to_owned();
            return;
        }
        self.close_memory_debuggers();
        let (tx, rx) = mpsc::channel();
        let notify = move |event| {
            let _ = tx.send(event);
        };
        let started = if reads_and_writes {
            AddressAccessWatch::start(
                pid,
                address,
                self.state.memory_debugger_architecture,
                notify,
            )
            .map(ActiveInstructionWatch::Accesses)
        } else {
            WriteWatch::start(
                pid,
                address,
                self.state.memory_debugger_architecture,
                notify,
            )
            .map(ActiveInstructionWatch::Writes)
        };
        let (active, status) = match started {
            Ok(active) => (Some(active), "Attaching debugger…".to_owned()),
            Err(error) => (None, format!("Unable to start debugger: {error}")),
        };
        self.memory_panel.instruction_watch_dialog = Some(InstructionWatchDialog {
            address,
            writes_only: !reads_and_writes,
            status,
            hits: Vec::new(),
            selected: None,
            rx,
            active,
            pinned: true,
            pending_code_add: None,
            pending_disassembler: None,
            auto_stop_on_hit: false,
            hits_sort: 0,
        });
    }

    #[cfg(windows)]
    fn render_instruction_watch_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.instruction_watch_dialog.take() else {
            return;
        };
        while let Ok(event) = dialog.rx.try_recv() {
            match event {
                WatchEvent::Started {
                    armed_threads,
                    total_threads,
                } => {
                    dialog.status = format!(
                        "Debugger running — {armed_threads}/{total_threads} thread(s) armed"
                    )
                }
                WatchEvent::AddressHit {
                    instruction_address,
                    instruction,
                    details,
                    ..
                } => {
                    let selected_address = dialog
                        .selected
                        .and_then(|index| dialog.hits.get(index))
                        .map(|hit| hit.address);
                    if let Some(hit) = dialog
                        .hits
                        .iter_mut()
                        .find(|hit| hit.address == instruction_address)
                    {
                        hit.count += 1;
                        hit.instruction = instruction;
                        hit.details = details;
                    } else if dialog.hits.len() < MAX_VISIBLE_INSTRUCTIONS {
                        dialog.hits.push(InstructionHit {
                            address: instruction_address,
                            instruction,
                            details,
                            count: 1,
                        });
                        dialog.hits.sort_unstable_by_key(|hit| hit.address);
                    }
                    dialog.selected = selected_address.and_then(|address| {
                        dialog.hits.iter().position(|hit| hit.address == address)
                    });
                    let total: usize = dialog.hits.iter().map(|hit| hit.count).sum();
                    dialog.status = format!("{total} hit(s), {} instruction(s)", dialog.hits.len());
                    if dialog.auto_stop_on_hit && let Some(mut active) = dialog.active.take() {
                        active.stop();
                        dialog.status = "First hit captured — debugger detached safely".to_owned();
                    }
                }
                WatchEvent::AccessHit { .. } => {}
                WatchEvent::CaptureLimitReached(limit) => {
                    dialog.status = if limit == 1 {
                        "First hit captured — debugger detached safely".to_owned()
                    } else {
                        format!("Debugger safely stopped after {limit} accesses")
                    };
                }
                WatchEvent::Error(error) => {
                    dialog.status = format!("Debugger stopped: {error}");
                    dialog.active = None;
                }
                WatchEvent::Stopped => {
                    if !dialog.status.starts_with("Debugger stopped:")
                        && !dialog.status.starts_with("Debugger safely stopped")
                    {
                        dialog.status = "Debugger stopped".to_owned();
                    }
                    dialog.active = None;
                }
            }
        }
        let mut open = true;
        let title = format!(
            "Find instructions {} — {}",
            if dialog.writes_only {
                "writing"
            } else {
                "accessing"
            },
            format_prefixed_memory_address(dialog.address)
        );
        let mut start_requested = false;
        if dialog.pinned {
            let builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_position(egui::pos2(0.0, 0.0))
                .with_inner_size(vec2(760.0, 560.0))
                .with_min_inner_size(vec2(520.0, 320.0))
                .with_clamp_size_to_monitor_size(true)
                .with_decorations(false)
                .with_resizable(true)
                .with_always_on_top();
            let mut unpin = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of((
                    "memory-instruction-watch",
                    dialog.address,
                )),
                builder,
                |ctx, _| {
                    Self::constrain_memory_popup_to_monitor(ctx);
                    if ctx.input(|input| input.viewport().close_requested()) {
                        open = false;
                    }
                    Self::render_memory_popup_titlebar(
                        ctx,
                        self.state.ui_language,
                        &title,
                        &mut unpin,
                        &mut open,
                    );
                    egui::CentralPanel::default()
                        .frame(Self::memory_popup_frame(ctx))
                        .show(ctx, |ui| {
                            start_requested |= Self::render_instruction_watch_body(ui, &mut dialog);
                        });
                    Self::render_memory_popup_resize_handles(ctx);
                },
            );
            if unpin {
                dialog.pinned = false;
            }
        } else {
            egui::Window::new(title)
                .default_size(vec2(760.0, 500.0))
                .collapsible(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    if ui.button("Pin").clicked() {
                        dialog.pinned = true;
                    }
                    start_requested |= Self::render_instruction_watch_body(ui, &mut dialog);
                });
        }
        if start_requested {
            self.restart_instruction_watch(&mut dialog);
        }
        if let Some(index) = dialog.pending_code_add.take()
            && let Some(hit) = dialog.hits.get(index)
        {
            self.add_instruction_to_code_list(hit.address, &hit.instruction, dialog.writes_only);
        }
        if let Some(index) = dialog.pending_disassembler.take()
            && let Some(hit) = dialog.hits.get(index)
        {
            let result = self
                .memory_panel
                .process_pid
                .ok_or_else(|| "Select a process".to_owned())
                .and_then(|pid| {
                    disassemble_from(
                        pid,
                        hit.address,
                        self.state.memory_debugger_architecture,
                        128,
                    )
                    .map_err(|error| error.to_string())
                });
            self.memory_panel.disassembler_dialog = Some(match result {
                Ok(lines) => DisassemblerDialog {
                    address: hit.address,
                    lines,
                    status: "Ready".to_owned(),
                },
                Err(status) => DisassemblerDialog {
                    address: hit.address,
                    lines: Vec::new(),
                    status,
                },
            });
        }
        if open {
            self.memory_panel.instruction_watch_dialog = Some(dialog);
            ctx.request_repaint_after(Duration::from_millis(35));
        } else if let Some(mut active) = dialog.active {
            active.stop();
        }
    }

    #[cfg(windows)]
    fn restart_instruction_watch(&mut self, dialog: &mut InstructionWatchDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.status = "Select a process".to_owned();
            return;
        };
        let (tx, rx) = mpsc::channel();
        let notify = move |event| {
            let _ = tx.send(event);
        };
        let address = dialog.address;
        let reads_and_writes = !dialog.writes_only;
        let started = if reads_and_writes {
            AddressAccessWatch::start(
                pid,
                address,
                self.state.memory_debugger_architecture,
                notify,
            )
            .map(ActiveInstructionWatch::Accesses)
        } else {
            WriteWatch::start(
                pid,
                address,
                self.state.memory_debugger_architecture,
                notify,
            )
            .map(ActiveInstructionWatch::Writes)
        };
        match started {
            Ok(active) => {
                dialog.rx = rx;
                dialog.active = Some(active);
                dialog.status = "Attaching debugger…".to_owned();
            }
            Err(error) => {
                dialog.status = format!("Unable to start debugger: {error}");
            }
        }
    }

    #[cfg(windows)]
    fn extract_aob_bytes_from_details(details: &str) -> String {
        for line in details.lines() {
            if line.contains("<<") || line.contains("0x") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[0].starts_with("0x") {
                    let mut bytes = Vec::new();
                    for part in &parts[1..] {
                        if part.len() == 2 && u8::from_str_radix(part, 16).is_ok() {
                            bytes.push(*part);
                        } else if !bytes.is_empty() {
                            break;
                        }
                    }
                    if !bytes.is_empty() {
                        return bytes.join(" ");
                    }
                }
            }
        }
        String::new()
    }

    #[cfg(windows)]
    fn render_instruction_watch_body(ui: &mut egui::Ui, dialog: &mut InstructionWatchDialog) -> bool {
        let mut clear_captured = false;
        let mut start_requested = false;
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(&dialog.status).selectable(true));
            ui.checkbox(&mut dialog.auto_stop_on_hit, "Auto-stop on 1st hit");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(dialog.selected.is_some(), Button::new("Add to code list"))
                    .clicked()
                {
                    dialog.pending_code_add = dialog.selected;
                }
                if ui
                    .add_enabled(dialog.selected.is_some(), Button::new("Show disassembler"))
                    .clicked()
                {
                    dialog.pending_disassembler = dialog.selected;
                }
                if ui
                    .add_enabled(dialog.selected.is_some(), Button::new("Copy Bytes (AOB)"))
                    .clicked()
                {
                    if let Some(index) = dialog.selected {
                        if let Some(hit) = dialog.hits.get(index) {
                            let aob = Self::extract_aob_bytes_from_details(&hit.details);
                            if !aob.is_empty() {
                                ui.ctx().copy_text(aob.clone());
                                dialog.status = format!("Copied AOB bytes: {aob}");
                            }
                        }
                    }
                }
                if dialog.active.is_some() {
                    if ui.button("Stop").clicked() {
                        if let Some(mut active) = dialog.active.take() {
                            active.stop();
                        }
                        if !dialog.status.starts_with("Debugger safely stopped") {
                            dialog.status = "Debugger stopped".to_owned();
                        }
                    }
                } else if ui.button("Start / Re-attach").clicked() {
                    start_requested = true;
                }
                if ui.button("Clear captured").clicked() {
                    clear_captured = true;
                }
            });
        });
        if clear_captured {
            dialog.hits.clear();
            dialog.selected = None;
            dialog.status = if dialog.active.is_some() {
                "Capture cleared; perform only the target action now".to_owned()
            } else {
                "Captured instructions cleared".to_owned()
            };
        }
        ui.separator();
        let stt_width = 40.0;
        let address_width = 170.0;
        let hits_width = 95.0;
        let instruction_width = (ui.available_width() - stt_width - address_width - hits_width - 24.0).max(140.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            Self::memory_table_cell(ui, stt_width, RichText::new("#").strong());
            Self::memory_table_cell(ui, address_width, RichText::new("Address").strong());
            Self::memory_table_cell(ui, instruction_width, RichText::new("Instruction").strong());
            let hits_label = match dialog.hits_sort {
                1 => "Hits (^)",
                2 => "Hits (v)",
                _ => "Hits",
            };
            if ui
                .add_sized(
                    [hits_width, 24.0],
                    egui::Button::new(RichText::new(hits_label).strong()).frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Click to cycle sort: Low to High (^) -> High to Low (v) -> Default")
                .clicked()
            {
                dialog.hits_sort = (dialog.hits_sort + 1) % 3;
            }
        });
        let list_height = (ui.available_height() * 0.45).clamp(140.0, 300.0);
        let mut context_code_add = None;
        let mut context_disassembler = None;
        let mut display_hits: Vec<(usize, &InstructionHit)> = dialog.hits.iter().enumerate().collect();
        match dialog.hits_sort {
            1 => display_hits.sort_by_key(|(_, hit)| hit.count),
            2 => display_hits.sort_by_key(|(_, hit)| std::cmp::Reverse(hit.count)),
            _ => {}
        }
        egui::ScrollArea::vertical()
            .id_salt("instruction-watch-hits")
            .max_height(list_height)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                for (row_idx, (original_index, hit)) in display_hits.into_iter().enumerate() {
                    let response = ui
                        .horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let stt = Self::memory_view_cell(ui, stt_width, &(row_idx + 1).to_string());
                            let address = Self::memory_view_cell(
                                ui,
                                address_width,
                                &format_prefixed_memory_address(hit.address),
                            );
                            let instruction =
                                Self::memory_view_cell(ui, instruction_width, &hit.instruction);
                            let count =
                                Self::memory_view_cell(ui, hits_width, &format!("{} hit(s)", hit.count));
                            stt.union(address).union(instruction).union(count)
                        })
                        .inner
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Click this instruction to select it for Add to code list");
                    if dialog.selected == Some(original_index) {
                        ui.painter().rect_filled(
                            response.rect,
                            2.0,
                            Color32::from_rgba_premultiplied(84, 178, 222, 64),
                        );
                        ui.painter().rect_stroke(
                            response.rect,
                            2.0,
                            egui::Stroke::new(1.0, Color32::from_rgb(84, 178, 222)),
                            egui::StrokeKind::Inside,
                        );
                    } else if response.hovered() {
                        ui.painter().rect_filled(
                            response.rect,
                            2.0,
                            Color32::from_rgba_premultiplied(84, 178, 222, 36),
                        );
                    }
                    if response.clicked() {
                        dialog.selected = Some(original_index);
                    }
                    if response.double_clicked() {
                        context_code_add = Some(original_index);
                    }
                    response.context_menu(|ui| {
                        let aob = Self::extract_aob_bytes_from_details(&hit.details);
                        if !aob.is_empty() {
                            if ui.button(format!("Copy Bytes (AOB): {aob}")).clicked() {
                                ui.ctx().copy_text(aob);
                                ui.close();
                            }
                        }
                        if ui.button("Copy Address").clicked() {
                            ui.ctx().copy_text(format_prefixed_memory_address(hit.address));
                            ui.close();
                        }
                        if ui.button("Copy instruction and details").clicked() {
                            ui.ctx().copy_text(format!(
                                "{}  {}  {} hit(s)\n\n{}",
                                format_prefixed_memory_address(hit.address),
                                hit.instruction,
                                hit.count,
                                hit.details,
                            ));
                            ui.close();
                        }
                        if ui.button("Add to code list").clicked() {
                            context_code_add = Some(original_index);
                            ui.close();
                        }
                        if ui.button("Show disassembler").clicked() {
                            context_disassembler = Some(original_index);
                            ui.close();
                        }
                    });
                }
            });
        if context_code_add.is_some() {
            dialog.pending_code_add = context_code_add;
        }
        if context_disassembler.is_some() {
            dialog.pending_disassembler = context_disassembler;
        }
        ui.separator();
        if let Some(index) = dialog.selected {
            if let Some(hit) = dialog.hits.get(index) {
                let mut details_str = hit.details.clone();
                egui::ScrollArea::both()
                    .id_salt("instruction-watch-details")
                    .max_height(200.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut details_str)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .interactive(true),
                        );
                    });
            }
        }
        start_requested
    }

    #[cfg(windows)]
    fn render_disassembler_dialog(&mut self, ctx: &egui::Context) {
        let active = self.memory_panel.disassembler_dialog.is_some();
        if !self.render_detached_memory_popup(
            ctx,
            "memory-disassembler-host",
            "Memory viewer — Disassembler",
            active,
            Self::render_disassembler_body,
        ) {
            self.memory_panel.disassembler_dialog = None;
        }
    }

    #[cfg(windows)]
    fn render_disassembler_body(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.memory_panel.disassembler_dialog.as_ref() else {
            return;
        };
        egui::CentralPanel::default()
            .frame(Self::memory_popup_frame(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format_prefixed_memory_address(dialog.address))
                            .monospace()
                            .strong(),
                    );
                    ui.label(RichText::new(&dialog.status).weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Copy Bytes (AOB)").clicked() {
                            if let Some((_, bytes, _)) = dialog.lines.iter().find(|(addr, _, _)| *addr == dialog.address) {
                                ui.ctx().copy_text(bytes.clone());
                            } else if let Some((_, bytes, _)) = dialog.lines.first() {
                                ui.ctx().copy_text(bytes.clone());
                            }
                        }
                        if ui.button("Copy Address").clicked() {
                            ui.ctx().copy_text(format_prefixed_memory_address(dialog.address));
                        }
                    });
                });
                ui.separator();
                let addr_w = 170.0;
                let bytes_w = 190.0;
                let opcode_w = (ui.available_width() - addr_w - bytes_w - 24.0).max(180.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    Self::memory_table_cell(ui, addr_w, RichText::new("Address").strong());
                    Self::memory_table_cell(ui, bytes_w, RichText::new("Bytes (AOB)").strong());
                    Self::memory_table_cell(ui, opcode_w, RichText::new("Instruction").strong());
                });
                egui::ScrollArea::both()
                    .id_salt("memory-disassembly-lines")
                    .max_height((ui.available_height() * 0.62).max(160.0))
                    .show(ui, |ui| {
                        for (address, bytes, opcode) in &dialog.lines {
                            let selected = *address == dialog.address;
                            let response = ui
                                .horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    let address_response = Self::memory_view_cell(
                                        ui,
                                        addr_w,
                                        &format_prefixed_memory_address(*address),
                                    );
                                    let bytes_response = Self::memory_view_cell(ui, bytes_w, bytes);
                                    let opcode_response = Self::memory_view_cell(ui, opcode_w, opcode);
                                    address_response
                                        .union(bytes_response)
                                        .union(opcode_response)
                                })
                                .inner;
                            response.context_menu(|ui| {
                                if ui.button(format!("Copy Bytes (AOB): {bytes}")).clicked() {
                                    ui.ctx().copy_text(bytes.clone());
                                    ui.close();
                                }
                                if ui.button("Copy Address").clicked() {
                                    ui.ctx().copy_text(format_prefixed_memory_address(*address));
                                    ui.close();
                                }
                                if ui.button("Copy disassembly line").clicked() {
                                    ui.ctx().copy_text(format!(
                                        "{}\t{bytes}\t{opcode}",
                                        format_prefixed_memory_address(*address)
                                    ));
                                    ui.close();
                                }
                            });
                            if selected {
                                ui.painter().rect_stroke(
                                    response.rect,
                                    2.0,
                                    egui::Stroke::new(1.0, Color32::from_rgb(84, 178, 222)),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                    });
                ui.separator();
                ui.label(RichText::new("Memory around instruction (Selectable Hex Dump)").strong());
                if let Some(pid) = self.memory_panel.process_pid {
                    let base = dialog.address.saturating_sub(64);
                    match read_memory_bytes(pid, base, 256) {
                        Ok(bytes) => {
                            let mut hex_lines = String::new();
                            for (row, chunk) in bytes.chunks(16).enumerate() {
                                let hex = chunk
                                    .iter()
                                    .map(|byte| format!("{byte:02X}"))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                let ascii = chunk
                                    .iter()
                                    .map(|byte| {
                                        if byte.is_ascii_graphic() {
                                            *byte as char
                                        } else {
                                            '.'
                                        }
                                    })
                                    .collect::<String>();
                                hex_lines.push_str(&format!(
                                    "{}  {:<48}  {}\n",
                                    format_prefixed_memory_address(base + row * 16),
                                    hex,
                                    ascii
                                ));
                            }
                            egui::ScrollArea::both()
                                .id_salt("memory-disassembly-bytes")
                                .max_height(160.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut hex_lines)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY)
                                            .interactive(true),
                                    );
                                });
                        }
                        Err(error) => {
                            ui.label(format!(
                                "{}: {error}",
                                self.tr("Unable to read memory", "Không thể đọc bộ nhớ")
                            ));
                        }
                    }
                }
            });
    }

    #[cfg(windows)]
    fn render_code_access_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.code_access_dialog.take() else {
            return;
        };
        while let Ok(event) = dialog.rx.try_recv() {
            match event {
                WatchEvent::Started {
                    armed_threads,
                    total_threads,
                } => {
                    dialog.status = format!(
                        "Debugger running — {armed_threads}/{total_threads} thread(s) armed"
                    )
                }
                WatchEvent::AccessHit { data_address } => {
                    let selected_address = dialog
                        .selected
                        .and_then(|index| dialog.addresses.get(index))
                        .map(|(address, _)| *address);
                    if let Some((_, count)) = dialog
                        .addresses
                        .iter_mut()
                        .find(|(address, _)| *address == data_address)
                    {
                        *count += 1;
                    } else {
                        dialog.addresses.push((data_address, 1));
                        dialog
                            .addresses
                            .sort_unstable_by_key(|(address, _)| *address);
                    }
                    if let Some(pid) = self.memory_panel.process_pid
                        && let Ok(value) = read_scan_value(pid, data_address, dialog.value_type)
                    {
                        dialog.values.insert(
                            data_address,
                            format_scan_value(value, self.memory_panel.hex),
                        );
                    }
                    dialog.selected = selected_address.and_then(|address| {
                        dialog
                            .addresses
                            .iter()
                            .position(|(candidate, _)| *candidate == address)
                    });
                    let total: usize = dialog.addresses.iter().map(|(_, count)| count).sum();
                    dialog.status =
                        format!("{total} hit(s), {} address(es)", dialog.addresses.len());
                    if dialog.auto_stop_on_hit && let Some(mut active) = dialog.active.take() {
                        active.stop();
                        dialog.status = "First address captured — debugger detached safely".to_owned();
                    }
                }
                WatchEvent::Error(error) => {
                    dialog.status = format!("Debugger stopped: {error}");
                    dialog.active = None;
                }
                WatchEvent::CaptureLimitReached(limit) => {
                    dialog.status = if limit == 1 {
                        "First address captured — debugger detached safely".to_owned()
                    } else {
                        format!("Debugger safely stopped after {limit} accesses")
                    };
                }
                WatchEvent::Stopped => {
                    dialog.status = "Debugger stopped".to_owned();
                    dialog.active = None;
                }
                WatchEvent::AddressHit { .. } => {}
            }
        }
        let entry = self.state.memory_code_list.get(dialog.code_index);
        let title = entry.map_or_else(
            || {
                format!(
                    "Find addresses — {}",
                    format_prefixed_memory_address(dialog.instruction_address)
                )
            },
            |entry| format!("Find addresses — {}+{:X}", entry.module, entry.offset),
        );
        let mut open = true;
        let mut unpin = false;
        let mut add = None;
        let mut browse = None;
        let mut refresh_values = false;
        let mut start_requested = false;
        let mut apply_esp = None;
        if dialog.pinned {
            let builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_position(egui::pos2(0.0, 0.0))
                .with_inner_size(vec2(620.0, 520.0))
                .with_min_inner_size(vec2(420.0, 280.0))
                .with_clamp_size_to_monitor_size(true)
                .with_decorations(false)
                .with_resizable(true)
                .with_always_on_top();
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of(("memory-code-access", dialog.code_index)),
                builder,
                |ctx, _| {
                    Self::constrain_memory_popup_to_monitor(ctx);
                    if ctx.input(|input| input.viewport().close_requested()) {
                        open = false;
                    }
                    Self::render_memory_popup_titlebar(
                        ctx,
                        self.state.ui_language,
                        &title,
                        &mut unpin,
                        &mut open,
                    );
                    egui::CentralPanel::default()
                        .frame(Self::memory_popup_frame(ctx))
                        .show(ctx, |ui| {
                            let result = Self::render_code_access_body(ui, &mut dialog, &self.state.esp_presets);
                            add = result.0;
                            browse = result.1;
                            refresh_values |= result.2;
                            start_requested |= result.3;
                            if result.4.is_some() {
                                apply_esp = result.4;
                            }
                        });
                    Self::render_memory_popup_resize_handles(ctx);
                },
            );
            if unpin {
                dialog.pinned = false;
            }
        } else {
            egui::Window::new(&title)
                .default_size(vec2(620.0, 440.0))
                .collapsible(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    if ui.button("Pin").clicked() {
                        dialog.pinned = true;
                    }
                    let result = Self::render_code_access_body(ui, &mut dialog, &self.state.esp_presets);
                    add = result.0;
                    browse = result.1;
                    refresh_values |= result.2;
                    start_requested |= result.3;
                    if result.4.is_some() {
                        apply_esp = result.4;
                    }
                });
        }
        if let Some((address, preset_id)) = apply_esp {
            let addr_hex = format_prefixed_memory_address(address);
            let addr_z = format_prefixed_memory_address(address.wrapping_add(4));
            let addr_y = format_prefixed_memory_address(address.wrapping_add(8));
            if let Some(target) = self
                .state
                .esp_presets
                .iter_mut()
                .find(|p| p.id == preset_id)
            {
                target.target_x = addr_hex;
                target.target_z = addr_z;
                target.target_y = addr_y;
            }
            self.persist_esp_presets();
        }
        if start_requested {
            self.restart_code_access_watch(&mut dialog);
        }
        if refresh_values {
            self.refresh_code_access_values(&mut dialog);
        }
        if dialog.save_tracked {
            dialog.save_tracked = false;
            self.save_tracked_code_address(&mut dialog);
        }
        if let Some(address) = add {
            self.add_code_access_address(address, dialog.value_type);
        }
        if let Some(address) = browse {
            self.memory_panel.memory_view_dialog = Some(MemoryViewDialog {
                address,
                tracked_base: Some(address),
                kind: MemoryViewKind::Bytes,
                display_type: MemoryDisplayType::ByteHex,
                relative_addresses: false,
                pinned: true,
                elements: default_structure_elements(),
                pending_add: None,
                pending_track: None,
                pointer_width: self
                    .memory_panel
                    .process_pid
                    .and_then(|pid| process_pointer_width(pid).ok())
                    .unwrap_or(8),
                previous_bytes: Vec::new(),
                byte_change_times: HashMap::new(),
                classes: vec![StructureClass {
                    name: "Class_0".to_owned(),
                    address,
                    elements: default_structure_elements(),
                }],
                selected_class: 0,
                class_detection_status: String::new(),
                class_detection_attempted: false,
                auto_dissected: false,
                history: Vec::new(),
            });
        }
        if open {
            self.memory_panel.code_access_dialog = Some(dialog);
            ctx.request_repaint_after(Duration::from_millis(35));
        } else if let Some(mut active) = dialog.active {
            active.stop();
        }
    }

    #[cfg(windows)]
    fn restart_code_access_watch(&mut self, dialog: &mut CodeAccessDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.status = "Select a process".to_owned();
            return;
        };
        let (tx, rx) = mpsc::channel();
        let notify = move |event| {
            let _ = tx.send(event);
        };
        let started = if dialog.auto_stop_on_hit {
            AccessWatch::start_once(
                pid,
                dialog.instruction_address,
                self.state.memory_debugger_architecture,
                notify,
            )
        } else {
            AccessWatch::start(
                pid,
                dialog.instruction_address,
                self.state.memory_debugger_architecture,
                notify,
            )
        };
        match started {
            Ok(active) => {
                dialog.rx = rx;
                dialog.active = Some(active);
                dialog.status = "Attaching debugger…".to_owned();
            }
            Err(error) => {
                dialog.status = format!("Unable to start debugger: {error}");
            }
        }
    }

    #[cfg(windows)]
    fn render_code_access_body(
        ui: &mut egui::Ui,
        dialog: &mut CodeAccessDialog,
        esp_presets: &[EspPreset],
    ) -> (Option<usize>, Option<usize>, bool, bool, Option<(usize, u32)>) {
        let mut add = None;
        let mut browse = None;
        let mut refresh_values = false;
        let mut start_requested = false;
        let mut apply_esp = None;
        ui.add(egui::Label::new(&dialog.status).selectable(true));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Refresh values").clicked() {
                refresh_values = true;
            }
            if dialog.active.is_some() {
                if ui.button("Stop").clicked() {
                    if let Some(mut active) = dialog.active.take() {
                        active.stop();
                    }
                    dialog.status = "Debugger stopped".to_owned();
                }
            } else if ui.button("Start / Re-attach").clicked() {
                start_requested = true;
            }
            ui.checkbox(&mut dialog.auto_stop_on_hit, "Auto-stop on 1st hit");
            if ui
                .add_enabled(dialog.selected.is_some(), Button::new("Add selected"))
                .clicked()
            {
                add = dialog
                    .selected
                    .and_then(|index| dialog.addresses.get(index))
                    .map(|(address, _)| *address);
            }
            if ui
                .add_enabled(dialog.selected.is_some(), Button::new("Browse selected"))
                .clicked()
            {
                browse = dialog
                    .selected
                    .and_then(|index| dialog.addresses.get(index))
                    .map(|(address, _)| *address);
            }
        });
        let previous_type = dialog.value_type;
        ui.horizontal(|ui| {
            ui.label("Display type");
            egui::ComboBox::from_id_salt("code-access-value-type")
                .selected_text(memory_type_label(dialog.value_type))
                .show_ui(ui, |ui| {
                    for value_type in [
                        ScanValueType::I8,
                        ScanValueType::I16,
                        ScanValueType::I32,
                        ScanValueType::I64,
                        ScanValueType::F32,
                        ScanValueType::F64,
                    ] {
                        ui.selectable_value(
                            &mut dialog.value_type,
                            value_type,
                            memory_type_label(value_type),
                        );
                    }
                });
        });
        if dialog.value_type != previous_type {
            refresh_values = true;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Tracked address");
            ui.add(
                egui::TextEdit::singleline(&mut dialog.tracked_name)
                    .desired_width(170.0)
                    .hint_text("name, e.g. camera_pitch"),
            );
            ui.label("Captured +");
            ui.add(
                egui::TextEdit::singleline(&mut dialog.tracked_offset)
                    .desired_width(70.0)
                    .hint_text("hex"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    dialog.selected.is_some() && !dialog.tracked_name.trim().is_empty(),
                    Button::new("Save tracked"),
                )
                .on_hover_text(
                    "Save this code-derived address; memory actions using @name will rebind it automatically after the game restarts",
                )
                .clicked()
            {
                dialog.save_tracked = true;
            }
        });
        ui.separator();
        let stt_width = 40.0;
        let address_width = 170.0;
        let hits_width = 95.0;
        let value_width = (ui.available_width() - stt_width - address_width - hits_width - 24.0).max(140.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            Self::memory_table_cell(ui, stt_width, RichText::new("#").strong());
            Self::memory_table_cell(ui, address_width, RichText::new("Address").strong());
            let hits_label = match dialog.hits_sort {
                1 => "Hits (^)",
                2 => "Hits (v)",
                _ => "Hits",
            };
            if ui
                .add_sized(
                    [hits_width, 24.0],
                    egui::Button::new(RichText::new(hits_label).strong()).frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Click to cycle sort: Low to High (^) -> High to Low (v) -> Default")
                .clicked()
            {
                dialog.hits_sort = (dialog.hits_sort + 1) % 3;
            }
            Self::memory_table_cell(ui, value_width, RichText::new("Current").strong());
        });
        let mut context_add = None;
        let mut display_addresses: Vec<(usize, &(usize, usize))> =
            dialog.addresses.iter().enumerate().collect();
        match dialog.hits_sort {
            1 => display_addresses.sort_by_key(|(_, (_, count))| *count),
            2 => display_addresses.sort_by_key(|(_, (_, count))| std::cmp::Reverse(*count)),
            _ => {}
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (row_idx, (original_index, (address, count))) in display_addresses.into_iter().enumerate() {
                let response = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let stt_response = Self::memory_view_cell(ui, stt_width, &(row_idx + 1).to_string());
                        let address_response = Self::memory_view_cell(
                            ui,
                            address_width,
                            &format_prefixed_memory_address(*address),
                        );
                        let count_response = Self::memory_view_cell(ui, hits_width, &count.to_string());
                        let value_response = Self::memory_view_cell(
                            ui,
                            value_width,
                            dialog.values.get(address).map_or("-", String::as_str),
                        );
                        stt_response.union(address_response).union(count_response).union(value_response)
                    })
                    .inner
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Click to select this address; double-click to add it");
                if dialog.selected == Some(original_index) {
                    ui.painter().rect_filled(
                        response.rect,
                        2.0,
                        Color32::from_rgba_premultiplied(84, 178, 222, 64),
                    );
                    ui.painter().rect_stroke(
                        response.rect,
                        2.0,
                        egui::Stroke::new(1.0, Color32::from_rgb(84, 178, 222)),
                        egui::StrokeKind::Inside,
                    );
                } else if response.hovered() {
                    ui.painter().rect_filled(
                        response.rect,
                        2.0,
                        Color32::from_rgba_premultiplied(84, 178, 222, 36),
                    );
                }
                if response.clicked() {
                    dialog.selected = Some(original_index);
                }
                if response.double_clicked() {
                    add = Some(*address);
                }
                response.context_menu(|ui| {
                    if ui.button("Copy address and value").clicked() {
                        ui.ctx().copy_text(format!(
                            "{}\t{}\t{}",
                            format_prefixed_memory_address(*address),
                            count,
                            dialog.values.get(address).map_or("-", String::as_str),
                        ));
                        ui.close();
                    }
                    if ui.button("Add to Address list").clicked() {
                        context_add = Some(*address);
                        ui.close();
                    }
                    if ui.button("Browse this memory region").clicked() {
                        browse = Some(*address);
                        ui.close();
                    }
                    ui.menu_button("Apply to ESP Target (X, Z, Y)", |ui| {
                        if esp_presets.is_empty() {
                            ui.label("No ESP presets found");
                        } else {
                            for preset in esp_presets {
                                let label = if preset.name.trim().is_empty() {
                                    format!("ESP Preset #{}", preset.id)
                                } else {
                                    preset.name.clone()
                                };
                                if ui.button(&label).clicked() {
                                    apply_esp = Some((*address, preset.id));
                                    ui.close();
                                }
                            }
                        }
                    });
                });
            }
        });
        if context_add.is_some() {
            add = context_add;
        }
        if dialog.addresses.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("Interact with the game to capture addresses");
            });
        }
        (add, browse, refresh_values, start_requested, apply_esp)
    }

    #[cfg(windows)]
    fn refresh_code_access_values(&self, dialog: &mut CodeAccessDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.status = "Select the restarted process before refreshing values".to_owned();
            return;
        };
        dialog.values.clear();
        for &(address, _) in &dialog.addresses {
            if let Ok(value) = read_scan_value(pid, address, dialog.value_type) {
                dialog
                    .values
                    .insert(address, format_scan_value(value, self.memory_panel.hex));
            }
        }
    }

    #[cfg(windows)]
    fn add_code_access_address(&mut self, address: usize, value_type: ScanValueType) {
        if self
            .memory_panel
            .saved
            .iter()
            .any(|saved| saved.address == address && saved.value_type == value_type)
        {
            return;
        }
        let current = self
            .memory_panel
            .process_pid
            .and_then(|pid| read_scan_value(pid, address, value_type).ok());
        self.memory_panel.saved.push(SavedMemoryAddress {
            address,
            value_type,
            current,
            text_encoding: None,
            text_byte_len: 0,
            current_text: None,
            description: String::new(),
            pointer: None,
            frozen: None,
            saved_to_library: false,
        });
        self.memory_panel.status =
            format!("Address {} added", format_prefixed_memory_address(address));
    }

    #[cfg(windows)]
    fn save_tracked_code_address(&mut self, dialog: &mut CodeAccessDialog) {
        let Some(captured) = dialog
            .selected
            .and_then(|index| dialog.addresses.get(index))
            .map(|(address, _)| *address)
        else {
            return;
        };
        let Some(offset) = parse_signed_hex_offset(&dialog.tracked_offset) else {
            dialog.status = "Invalid tracked offset; use hex such as 4, C, or -4".to_owned();
            return;
        };
        let Some(address) = captured.checked_add_signed(offset) else {
            dialog.status = "Tracked address overflow".to_owned();
            return;
        };
        let Some(code) = self.state.memory_code_list.get(dialog.code_index) else {
            return;
        };
        let code_module = code.module.clone();
        let code_offset = code.offset;
        let name = dialog.tracked_name.trim().to_owned();
        let app_name = self
            .memory_panel
            .process_pid
            .and_then(|pid| {
                process_modules(pid)
                    .ok()?
                    .first()
                    .map(|module| module.0.clone())
            })
            .unwrap_or_else(|| code_module.clone());
        let tracked_value = self
            .memory_panel
            .process_pid
            .and_then(|pid| read_scan_value(pid, address, dialog.value_type).ok())
            .map(|value| format_scan_value(value, false))
            .unwrap_or_default();
        let tracked_signature = self
            .memory_panel
            .process_pid
            .and_then(|pid| tracked_object_signature(pid, captured, &code.instruction));
        let signature_saved = tracked_signature.is_some();
        let entry = MemoryPointerEntry {
            name: name.clone(),
            app_name,
            module: String::new(),
            module_offset: 0,
            offsets: Vec::new(),
            value_type: memory_type_config(dialog.value_type).to_owned(),
            absolute_address: None,
            code_module: code_module.clone(),
            code_offset,
            code_address_offset: offset,
            runtime_address: Some(address),
            runtime_process_id: self.memory_panel.process_pid,
            tracked_value,
            tracked_signature: tracked_signature.unwrap_or_default(),
        };
        if let Some(existing) = self
            .state
            .memory_pointer_list
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case(&name))
        {
            *existing = entry;
        } else {
            self.state.memory_pointer_list.push(entry);
        }
        crate::overlay::set_memory_pointer_entries(&self.state.memory_pointer_list);
        self.persist();
        self.add_code_access_address(address, dialog.value_type);
        dialog.status = format!(
            "Tracked address @{name} saved - {}+{:X} @ {offset:+X}{}",
            code_module,
            code_offset,
            if signature_saved {
                " (object signature captured)"
            } else {
                " (object signature unavailable)"
            }
        );
    }

    #[cfg(windows)]
    fn open_memory_module_list(&mut self) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        match process_modules(pid) {
            Ok(mut modules) => {
                modules.sort_by_key(|(_, base, _)| *base);
                self.memory_panel.module_list_dialog = Some(ModuleListDialog {
                    modules,
                    filter: String::new(),
                    status: String::new(),
                });
            }
            Err(error) => {
                let status = format!("Unable to enumerate modules: {error}");
                self.memory_panel.status.clone_from(&status);
                self.memory_panel.module_list_dialog = Some(ModuleListDialog {
                    modules: Vec::new(),
                    filter: String::new(),
                    status,
                });
            }
        }
    }

    #[cfg(windows)]
    fn render_memory_module_list(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.memory_panel.module_list_dialog.as_mut() else {
            return;
        };
        egui::CentralPanel::default()
            .frame(Self::memory_popup_frame(ctx))
            .show(ctx, |ui| {
                if !dialog.status.is_empty() {
                    ui.label(RichText::new(&dialog.status).color(Color32::LIGHT_RED));
                }
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.filter)
                        .desired_width(f32::INFINITY)
                        .hint_text(RichText::new("Search module...").weak()),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    Self::memory_view_cell(ui, 170.0, "Base address");
                    Self::memory_view_cell(ui, 120.0, "Size");
                    Self::memory_view_cell(ui, 340.0, "Module");
                });
                ui.separator();
                let filter = dialog.filter.trim().to_ascii_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (name, base, size) in &dialog.modules {
                        if !filter.is_empty() && !name.to_ascii_lowercase().contains(&filter) {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [170.0, 22.0],
                                egui::Label::new(format_prefixed_memory_address(*base))
                                    .selectable(true),
                            );
                            ui.add_sized(
                                [120.0, 22.0],
                                egui::Label::new(format!("0x{size:X}")).selectable(true),
                            );
                            ui.add_sized(
                                [340.0, 22.0],
                                egui::Label::new(name).selectable(true).truncate(),
                            );
                        });
                    }
                });
            });
    }

    fn render_memory_view_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.memory_view_dialog.take() else {
            return;
        };
        let address = dialog.address;
        let kind = dialog.kind;
        let title = match kind {
            MemoryViewKind::Bytes => format!(
                "{} — {}",
                self.tr("Memory region", "Vùng bộ nhớ"),
                format_prefixed_memory_address(address)
            ),
            MemoryViewKind::Structure => format!(
                "{} — {}",
                self.tr("Dissect data/structure", "Phân tích dữ liệu/cấu trúc"),
                format_prefixed_memory_address(address)
            ),
        };
        let bytes = self
            .memory_panel
            .process_pid
            .and_then(|pid| read_memory_bytes(pid, address, 512).ok());
        let region = self
            .memory_panel
            .process_pid
            .and_then(|pid| query_memory_region(pid, address).ok());
        let mut open = true;
        if dialog.pinned {
            let builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_position(egui::pos2(0.0, 0.0))
                .with_inner_size(vec2(760.0, 430.0))
                .with_min_inner_size(vec2(520.0, 280.0))
                .with_clamp_size_to_monitor_size(true)
                .with_decorations(false)
                .with_resizable(true)
                .with_always_on_top();
            let mut unpin = false;
            ctx.show_viewport_immediate(
                // ID must be stable — do NOT include `address` here or navigation
                // will destroy and recreate the viewport on every jump.
                egui::ViewportId::from_hash_of("memory-view-pinned-struct"),

                builder,
                |ctx, _| {
                    Self::constrain_memory_popup_to_monitor(ctx);
                    if ctx.input(|input| input.viewport().close_requested()) {
                        open = false;
                    }
                    Self::render_memory_popup_titlebar(
                        ctx,
                        self.state.ui_language,
                        &title,
                        &mut unpin,
                        &mut open,
                    );
                    egui::CentralPanel::default()
                        .frame(Self::memory_popup_frame(ctx))
                        .show(ctx, |ui| {
                            Self::render_memory_view_body(
                                ui,
                                self.state.ui_language,
                                self.memory_panel.process_pid,
                                &mut dialog,
                                bytes.as_deref(),
                                region,
                            );
                        });
                    Self::render_memory_popup_resize_handles(ctx);
                },
            );
            if unpin {
                dialog.pinned = false;
            }
            self.add_pending_structure_address(&mut dialog);
            self.apply_pending_tracked_field(&mut dialog);
            if open {
                self.memory_panel.memory_view_dialog = Some(dialog);
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            return;
        }
        egui::Window::new(&title)
            .id(egui::Id::new("memory-view-main"))
            .default_size(vec2(720.0, 430.0))
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                let pin_label = self.tr("Pin", "Ghim");
                if ui.button(pin_label).clicked() {
                    dialog.pinned = true;
                }
                Self::render_memory_view_body(
                    ui,
                    self.state.ui_language,
                    self.memory_panel.process_pid,
                    &mut dialog,
                    bytes.as_deref(),
                    region,
                );
            });
        if !open {
            self.memory_panel.memory_view_dialog = None;
        } else {
            self.add_pending_structure_address(&mut dialog);
            self.apply_pending_tracked_field(&mut dialog);
            self.memory_panel.memory_view_dialog = Some(dialog);
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn render_memory_view_body(
        ui: &mut egui::Ui,
        language: crate::model::UiLanguage,
        process_pid: Option<u32>,
        dialog: &mut MemoryViewDialog,
        bytes: Option<&[u8]>,
        region: Option<MemoryRegionInfo>,
    ) {
        let Some(bytes) = bytes else {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if !dialog.history.is_empty() {
                        if ui.button(Self::tr_lang(language, "◀ Back", "◀ Quay lại")).clicked() {
                            if let Some(prev_addr) = dialog.history.pop() {
                                dialog.address = prev_addr;
                                if let Some(idx) = dialog.classes.iter().position(|c| c.address == prev_addr) {
                                    dialog.selected_class = idx;
                                    dialog.elements = dialog.classes[idx].elements.clone();
                                }
                                dialog.previous_bytes.clear();
                                dialog.auto_dissected = false;
                            }
                        }
                    }
                    ui.label(
                        RichText::new(format!(
                            "{}: {}",
                            Self::tr_lang(
                                language,
                                "Unable to read memory region",
                                "Không thể đọc vùng bộ nhớ"
                            ),
                            format_prefixed_memory_address(dialog.address)
                        ))
                        .color(Color32::from_rgb(255, 100, 100)),
                    );
                });
                if !dialog.class_detection_status.is_empty() {
                    ui.label(
                        RichText::new(&dialog.class_detection_status)
                            .small()
                            .color(Color32::from_rgb(255, 170, 70)),
                    );
                }
            });
            return;
        };
        // Update byte change highlight map: compare new bytes with previous_bytes.
        let current_time = ui.input(|i| i.time);
        if !dialog.previous_bytes.is_empty() && dialog.previous_bytes.len() == bytes.len() {
            for (i, (&new_byte, &old_byte)) in bytes.iter().zip(dialog.previous_bytes.iter()).enumerate() {
                if new_byte != old_byte {
                    dialog.byte_change_times.insert(i, current_time);
                }
            }
        }
        // Always update previous_bytes for next frame comparison.
        dialog.previous_bytes = bytes.to_vec();
        // Request repaint while there are still fading highlights active.
        const FADE_DURATION: f64 = 1.5;
        let has_active_fade = dialog
            .byte_change_times
            .values()
            .any(|&t| current_time - t < FADE_DURATION);
        if has_active_fade {
            ui.ctx().request_repaint();
        }
        if matches!(dialog.kind, MemoryViewKind::Bytes) {
            if let Some(region) = region {
                ui.label(
                    RichText::new(format!(
                        "Protect:{}   AllocationBase={:X}   Base={:X}   Size={:X}",
                        format_memory_protection(region.protect),
                        region.allocation_base,
                        region.base,
                        region.size,
                    ))
                    .monospace(),
                );
                ui.separator();
            }
        }
        match dialog.kind {
            MemoryViewKind::Bytes => {
                egui::ScrollArea::both().show(ui, |ui| {
                    Self::render_memory_region_grid(ui, language, dialog, bytes, current_time)
                });
            }
            MemoryViewKind::Structure => {
                // Auto-identify class on first open
                if !dialog.class_detection_attempted {
                    dialog.class_detection_attempted = true;
                    let previous_address = dialog.address;
                    identify_structure_class(process_pid, dialog);
                    if dialog.address != previous_address {
                        ui.label(&dialog.class_detection_status);
                        return;
                    }
                }
                // Auto-dissect on first render (like CE — no button needed)
                if !dialog.auto_dissected && !bytes.is_empty() {
                    dialog.auto_dissected = true;
                    dialog.elements = auto_structure_elements(bytes, dialog.pointer_width);
                    if let Some(active) = dialog.classes.get_mut(dialog.selected_class) {
                        active.elements = dialog.elements.clone();
                    }
                }
                // Top Toolbar: single line layout without left sidebar
                ui.horizontal(|ui| {
                    if !dialog.history.is_empty() {
                        if ui.small_button(Self::tr_lang(language, "◀ Back", "◀ Quay lại")).clicked() {
                            if let Some(prev_addr) = dialog.history.pop() {
                                dialog.address = prev_addr;
                                if let Some(idx) = dialog.classes.iter().position(|c| c.address == prev_addr) {
                                    dialog.selected_class = idx;
                                    dialog.elements = dialog.classes[idx].elements.clone();
                                }
                                dialog.previous_bytes.clear();
                                dialog.auto_dissected = false;
                            }
                        }
                    }
                    if dialog.classes.len() > 1 {
                        let selected_name = dialog.classes.get(dialog.selected_class).map(|c| c.name.as_str()).unwrap_or("Class").to_owned();
                        let class_list: Vec<(usize, String)> = dialog.classes.iter().enumerate().map(|(i, c)| (i, c.name.clone())).collect();
                        let mut switch_to = None;
                        egui::ComboBox::from_id_salt("struct-class-select")
                            .selected_text(&selected_name)
                            .show_ui(ui, |ui| {
                                for (index, name) in &class_list {
                                    if ui.selectable_label(dialog.selected_class == *index, name).clicked() {
                                        switch_to = Some(*index);
                                    }
                                }
                            });
                        if let Some(index) = switch_to {
                            if let Some(active) = dialog.classes.get_mut(dialog.selected_class) {
                                active.address = dialog.address;
                                active.elements = dialog.elements.clone();
                            }
                            dialog.selected_class = index;
                            if let Some(class) = dialog.classes.get(index).cloned() {
                                dialog.address = class.address;
                                dialog.elements = class.elements;
                                dialog.previous_bytes.clear();
                                dialog.auto_dissected = false;
                            }
                        }
                    } else {
                        let class_name = dialog
                            .classes
                            .get(dialog.selected_class)
                            .map(|c| c.name.as_str())
                            .unwrap_or("unnamed structure");
                        ui.label(
                            RichText::new(format!(
                                "{class_name}   [{}-bit]",
                                dialog.pointer_width * 8
                            ))
                            .strong(),
                        );
                    }

                    ui.separator();
                    if ui.small_button(Self::tr_lang(language, "Re-identify", "Nhận diện lại")).clicked() {
                        identify_structure_class(process_pid, dialog);
                    }
                    if ui.small_button(Self::tr_lang(language, "Re-dissect", "Dissect lại")).clicked() {
                        dialog.elements = auto_structure_elements(bytes, dialog.pointer_width);
                        for el in &mut dialog.elements { el.detected_class = None; }
                    }
                    if !dialog.class_detection_status.is_empty() {
                        ui.label(
                            RichText::new(&dialog.class_detection_status)
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
                });
                ui.separator();

                // Main structure elements view full width
                egui::ScrollArea::both()
                    .id_salt("structure-elements")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .max_height(ui.available_height())
                    .show(ui, |ui| Self::render_structure_elements(ui, dialog, bytes, process_pid));
                if let Some(active) = dialog.classes.get_mut(dialog.selected_class) {
                    active.address = dialog.address;
                    active.elements = dialog.elements.clone();
                }
                dialog.previous_bytes = bytes.to_vec();
            }
        }
    }

    fn render_memory_region_grid(
        ui: &mut egui::Ui,
        language: crate::model::UiLanguage,
        dialog: &mut MemoryViewDialog,
        bytes: &[u8],
        current_time: f64,
    ) {
        let unit = memory_display_width(dialog.display_type);
        let value_width = memory_display_cell_width(dialog.display_type);
        let address_width = 145.0;
        let ascii_width = 210.0;
        let columns = (((ui.available_width() - address_width - ascii_width) / value_width).floor()
            as usize)
            .clamp(1, 32 / unit);
        let row_bytes = columns * unit;

        ui.horizontal(|ui| {
            Self::memory_view_cell(
                ui,
                address_width,
                &Self::tr_lang(language, "Address", "Địa chỉ"),
            );
            for column in 0..columns {
                let low_byte = dialog.address.wrapping_add(column * unit) & 0xFF;
                Self::memory_view_cell(ui, value_width, &format!("{low_byte:02X}"));
            }
            Self::memory_view_cell(ui, ascii_width, "0123456789ABCDEF0123456789ABCDEF");
        });
        ui.separator();

        for (row, chunk) in bytes.chunks(row_bytes).enumerate() {
            let row_address = dialog.address.saturating_add(row * row_bytes);
            let row_res = ui
                .horizontal(|ui| {
                    let shown_address = if dialog.relative_addresses {
                        format!("+{:04X}", row * row_bytes)
                    } else {
                        format_memory_address(row_address)
                    };
                    Self::memory_view_cell(ui, address_width, &shown_address);
                    for (column, value) in chunk.chunks(unit).take(columns).enumerate() {
                        let text = (value.len() == unit)
                            .then(|| format_memory_display(value, dialog.display_type))
                            .unwrap_or_default();
                        let cell = Self::memory_view_cell(ui, value_width, &text);
                        // Red-fade highlight for changed bytes (Cheat Engine style)
                        let byte_offset = row * row_bytes + column * unit;
                        if let Some(&change_time) = dialog.byte_change_times.get(&byte_offset) {
                            const FADE_DURATION: f64 = 1.5;
                            let age = current_time - change_time;
                            if age < FADE_DURATION {
                                let alpha = ((1.0 - age / FADE_DURATION) * 210.0) as u8;
                                ui.painter().rect_filled(
                                    cell.rect,
                                    2.0,
                                    Color32::from_rgba_unmultiplied(220, 40, 40, alpha),
                                );
                            }
                        }
                        let cell_address = row_address.saturating_add(column * unit);
                        cell.context_menu(|ui| {
                            let add_label = Self::tr_lang(
                                language,
                                "Add this address to the list",
                                "Thêm địa chỉ này vào danh sách",
                            );
                            if ui.button(add_label).clicked() {
                                dialog.pending_add = Some((
                                    cell_address,
                                    memory_display_scan_type(dialog.display_type),
                                ));
                                ui.close();
                            }
                            if ui.button("Use as tracked field").clicked() {
                                dialog.pending_track = Some(cell_address);
                                ui.close();
                            }
                            ui.separator();
                            let display_label =
                                Self::tr_lang(language, "Display Type", "Kiểu hiển thị");
                            ui.menu_button(display_label, |ui| {
                                for (display_type, label) in memory_display_types() {
                                    if ui
                                        .selectable_value(
                                            &mut dialog.display_type,
                                            display_type,
                                            label,
                                        )
                                        .clicked()
                                    {
                                        ui.close();
                                    }
                                }
                            });
                            let rel_label = Self::tr_lang(
                                language,
                                "Show relative addresses",
                                "Hiện địa chỉ tương đối",
                            );
                            ui.checkbox(&mut dialog.relative_addresses, rel_label);
                            let dissect_label = Self::tr_lang(
                                language,
                                "Open in dissect data/structure",
                                "Mở trong phân tích dữ liệu/cấu trúc",
                            );
                            if ui.button(dissect_label).clicked() {
                                dialog.kind = MemoryViewKind::Structure;
                                ui.close();
                            }
                        });
                    }
                    let ascii = chunk
                        .iter()
                        .map(|byte| {
                            if byte.is_ascii_graphic() {
                                *byte as char
                            } else {
                                '.'
                            }
                        })
                        .collect::<String>();
                    Self::memory_view_cell(ui, ascii_width, &ascii);
                })
                .response;

            row_res.context_menu(|ui| {
                let add_label = Self::tr_lang(
                    language,
                    "Add this address to the list",
                    "Thêm địa chỉ này vào danh sách",
                );
                if ui.button(add_label).clicked() {
                    dialog.pending_add =
                        Some((row_address, memory_display_scan_type(dialog.display_type)));
                    ui.close();
                }
                ui.separator();
                let display_label = Self::tr_lang(language, "Display Type", "Kiểu hiển thị");
                ui.menu_button(display_label, |ui| {
                    for (display_type, label) in memory_display_types() {
                        if ui
                            .selectable_value(&mut dialog.display_type, display_type, label)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
                let rel_label = Self::tr_lang(
                    language,
                    "Show relative addresses",
                    "Hiện địa chỉ tương đối",
                );
                ui.checkbox(&mut dialog.relative_addresses, rel_label);
                let dissect_label = Self::tr_lang(
                    language,
                    "Open in dissect data/structure",
                    "Mở trong phân tích dữ liệu/cấu trúc",
                );
                if ui.button(dissect_label).clicked() {
                    dialog.kind = MemoryViewKind::Structure;
                    ui.close();
                }
            });
        }
    }

    fn render_structure_elements(ui: &mut egui::Ui, dialog: &mut MemoryViewDialog, bytes: &[u8], process_pid: Option<u32>) {
        // CE-style column widths
        const W_BTN:   f32 = 20.0;
        const W_DESC:  f32 = 340.0;
        const W_ADDRV: f32 = 280.0;

        // Header \u2014 3 separate Grid cells (one per column)
        egui::Grid::new("struct-header")
            .min_col_width(0.0)
            .spacing([0.0, 0.0])
            .show(ui, |ui| {
                // Cell 1: empty arrow column
                ui.add_sized([W_BTN, 18.0], egui::Label::new(""));
                // Cell 2: description header, left-aligned
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    ui.set_min_width(W_DESC);
                    ui.label(RichText::new("Offset - description").strong());
                });
                // Cell 3: address:value header, left-aligned
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    ui.set_min_width(W_ADDRV);
                    ui.label(RichText::new("Address : Value").strong());
                });
                ui.end_row();
            });
        ui.separator();

        let mut open_pointer_class: Option<usize> = None;
        egui::Grid::new("struct-elements")
            .min_col_width(0.0)
            .spacing([0.0, 1.0])
            .striped(true)
            .show(ui, |ui| {
        for element in &mut dialog.elements {
            let width = element.value_type.width(dialog.pointer_width);
            let Some(raw) = bytes.get(element.offset..element.offset.saturating_add(width)) else {
                // Still need to advance all 3 columns so the grid stays consistent
                ui.add_sized([W_BTN, 18.0], egui::Label::new(""));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    ui.add_sized([W_DESC, 18.0], egui::Label::new(""));
                });
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    ui.add_sized([W_ADDRV, 18.0], egui::Label::new(""));
                });
                ui.end_row();
                continue;
            };
            let changed = dialog
                .previous_bytes
                .get(element.offset..element.offset.saturating_add(width))
                .is_some_and(|previous| previous != raw);
            let element_address = dialog.address.saturating_add(element.offset);
            let mut add_request = None;
            let mut navigate_to: Option<usize> = None;

            // Lazy RTTI detection for Pointer fields
            if element.value_type == StructureElementType::Pointer {
                let pointed_addr = decode_pointer(raw).unwrap_or(0);
                if pointed_addr != 0 && element.detected_class.is_none() {
                    #[cfg(windows)]
                    if let Some(pid) = process_pid {
                        element.detected_class = Some(
                            detect_structure_identity(pid, pointed_addr, dialog.pointer_width)
                                .map(|id| id.name)
                                .unwrap_or_default(),
                        );
                    }
                    #[cfg(not(windows))]
                    { element.detected_class = Some(String::new()); }
                }
            }

            let description = ce_element_description(element);
            let value_str = if element.value_type == StructureElementType::Pointer {
                let addr = decode_pointer(raw).unwrap_or(0);
                if addr == 0 { "NULL".to_owned() } else { format!("P->{addr:X}") }
            } else {
                format_structure_value(raw, element.value_type)
            };
            let is_ptr = element.value_type == StructureElementType::Pointer;
            let pointed = if is_ptr { decode_pointer(raw).unwrap_or(0) } else { 0 };

            // Col 1: arrow button (inline expansion toggle)
            {
                let (btn_text, btn_color) = if is_ptr && pointed != 0 {
                    if element.expanded {
                        ("v", Color32::from_rgb(255, 200, 100))
                    } else {
                        (">", Color32::from_rgb(100, 210, 140))
                    }
                } else {
                    (" ", ui.visuals().weak_text_color())
                };
                let btn = ui.add_sized(
                    [W_BTN, 18.0],
                    egui::Button::new(RichText::new(btn_text).monospace().color(btn_color))
                        .frame(false),
                );
                if btn.clicked() && is_ptr && pointed != 0 {
                    element.expanded = !element.expanded;
                    if element.expanded && element.child_elements.is_empty() {
                        if let Some(pid) = process_pid {
                            if let Ok(child_bytes) = read_memory_bytes(pid, pointed, 128) {
                                element.child_elements = auto_structure_elements(&child_bytes, dialog.pointer_width);
                            }
                        }
                    }
                }
                if is_ptr {
                    btn.on_hover_text(if pointed != 0 {
                        if element.expanded { "Collapse child structure" } else { "Expand child structure inline" }
                    } else {
                        "NULL pointer"
                    });
                }
            }

            // Col 2: offset-description — genuinely left-aligned
            let desc_color = if is_ptr { Color32::from_rgb(100, 210, 140) } else { ui.visuals().text_color() };
            let desc_resp = ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.set_min_width(W_DESC);
                ui.set_max_width(W_DESC);
                ui.add(
                    egui::Label::new(
                        RichText::new(&description)
                            .monospace()
                            .size(12.5)
                            .color(desc_color),
                    )
                    .selectable(true)
                    .truncate(),
                )
            }).inner;
            let desc_resp = desc_resp.on_hover_text(&description);

            // Col 3: address : value — genuinely left-aligned
            let av_text = format!("{} : {}", format_memory_address(element_address), value_str);
            let av_color = if changed { Color32::from_rgb(255, 170, 70) } else { ui.visuals().text_color() };
            let av = ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.set_min_width(W_ADDRV);
                ui.set_max_width(W_ADDRV);
                ui.add(
                    egui::Label::new(
                        RichText::new(&av_text)
                            .monospace()
                            .size(12.5)
                            .color(av_color),
                    )
                    .selectable(true)
                    .truncate(),
                )
            }).inner;
            if av.double_clicked() {
                if is_ptr && pointed != 0 {
                    element.expanded = !element.expanded;
                    if element.expanded && element.child_elements.is_empty() {
                        if let Some(pid) = process_pid {
                            if let Ok(child_bytes) = read_memory_bytes(pid, pointed, 128) {
                                element.child_elements = auto_structure_elements(&child_bytes, dialog.pointer_width);
                            }
                        }
                    }
                } else if !is_ptr {
                    add_request = Some((
                        element_address,
                        element.value_type.scan_type(dialog.pointer_width),
                    ));
                }
            }

            // Context menu on the whole row
            desc_resp.context_menu(|ui| Self::structure_element_context_menu(ui, element, element_address, &mut add_request));
            av.context_menu(|ui| Self::structure_element_context_menu(ui, element, element_address, &mut add_request));

            if add_request.is_some() { dialog.pending_add = add_request; }

            ui.end_row();

            // Render inline expanded child elements if expanded
            if is_ptr && element.expanded && pointed != 0 {
                let mut read_success = false;
                if let Some(pid) = process_pid {
                    if let Ok(child_bytes) = read_memory_bytes(pid, pointed, 128) {
                        read_success = true;
                        if element.child_elements.is_empty() {
                            element.child_elements = auto_structure_elements(&child_bytes, dialog.pointer_width);
                        }
                        for child in &mut element.child_elements {
                            let child_w = child.value_type.width(dialog.pointer_width);
                            let Some(c_raw) = child_bytes.get(child.offset..child.offset.saturating_add(child_w)) else {
                                continue;
                            };
                            let child_addr = pointed.saturating_add(child.offset);
                            let c_val_str = if child.value_type == StructureElementType::Pointer {
                                let c_ptr = decode_pointer(c_raw).unwrap_or(0);
                                if c_ptr == 0 { "NULL".to_owned() } else { format!("P->{c_ptr:X}") }
                            } else {
                                format_structure_value(c_raw, child.value_type)
                            };

                            // Indented child row
                            ui.add_sized([W_BTN, 18.0], egui::Label::new(""));
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                                ui.set_min_width(W_DESC);
                                ui.set_max_width(W_DESC);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("   ↳ +{:04X} - {}", child.offset, child.value_type.label()))
                                            .monospace()
                                            .size(11.5)
                                            .color(Color32::from_rgb(180, 220, 255)),
                                    )
                                    .selectable(true)
                                    .truncate(),
                                );
                            });
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                                ui.set_min_width(W_ADDRV);
                                ui.set_max_width(W_ADDRV);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("{} : {}", format_memory_address(child_addr), c_val_str))
                                            .monospace()
                                            .size(11.5)
                                            .color(ui.visuals().text_color()),
                                    )
                                    .selectable(true)
                                    .truncate(),
                                );
                            });
                            ui.end_row();
                        }
                    }
                }
                if !read_success {
                    ui.add_sized([W_BTN, 18.0], egui::Label::new(""));
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        ui.set_min_width(W_DESC);
                        ui.set_max_width(W_DESC);
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("   ↳ Unable to read memory at {}", format_memory_address(pointed)))
                                    .monospace()
                                    .size(11.5)
                                    .color(Color32::from_rgb(255, 140, 70)),
                            )
                            .selectable(true)
                            .truncate(),
                        );
                    });
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        ui.set_min_width(W_ADDRV);
                        ui.set_max_width(W_ADDRV);
                        ui.add(egui::Label::new(""));
                    });
                    ui.end_row();
                }
            }
        } // for element
        }); // Grid
    }

    /// Context menu shared by description and value columns.
    fn structure_element_context_menu(
        ui: &mut egui::Ui,
        element: &mut StructureElement,
        element_address: usize,
        add_request: &mut Option<(usize, ScanValueType)>,
    ) {
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(
                egui::TextEdit::singleline(&mut element.name)
                    .desired_width(140.0)
                    .hint_text("field name"),
            );
        });
        ui.menu_button("Change type", |ui| {
            for (value_type, label) in StructureElementType::ALL {
                if ui
                    .selectable_value(&mut element.value_type, value_type, label)
                    .clicked()
                {
                    element.detected_class = None;
                    ui.close();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Offset:");
            ui.add(
                egui::DragValue::new(&mut element.offset)
                    .hexadecimal(4, false, true)
                    .speed(1),
            );
        });
        if ui.button("Add to address list").clicked() {
            *add_request = Some((
                element_address,
                element.value_type.scan_type(std::mem::size_of::<usize>()),
            ));
            ui.close();
        }
    }

    fn add_pending_structure_address(&mut self, dialog: &mut MemoryViewDialog) {
        let Some((address, value_type)) = dialog.pending_add.take() else {
            return;
        };
        if self
            .memory_panel
            .saved
            .iter()
            .any(|saved| saved.address == address && saved.value_type == value_type)
        {
            return;
        }
        let current = self
            .memory_panel
            .process_pid
            .and_then(|pid| read_scan_value(pid, address, value_type).ok());
        self.memory_panel.saved.push(SavedMemoryAddress {
            address,
            value_type,
            current,
            text_encoding: None,
            text_byte_len: 0,
            current_text: None,
            description: String::new(),
            pointer: None,
            frozen: None,
            saved_to_library: false,
        });
        self.memory_panel.status =
            format!("Address {} added", format_prefixed_memory_address(address));
    }

    #[cfg(windows)]
    fn apply_pending_tracked_field(&mut self, dialog: &mut MemoryViewDialog) {
        let Some(field_address) = dialog.pending_track.take() else {
            return;
        };
        let Some(code_dialog) = self.memory_panel.code_access_dialog.as_mut() else {
            self.memory_panel.status = "Open Find written before tracking a field".to_owned();
            return;
        };
        let Some(captured) = dialog.tracked_base.or_else(|| {
            code_dialog
                .selected
                .and_then(|index| code_dialog.addresses.get(index))
                .map(|(address, _)| *address)
        }) else {
            code_dialog.status = "Select a captured address before tracking a field".to_owned();
            return;
        };
        let offset = if field_address >= captured {
            format!("{:X}", field_address - captured)
        } else {
            format!("-{:X}", captured - field_address)
        };
        code_dialog.tracked_offset = offset.clone();
        code_dialog.status = format!(
            "Tracked field {} = captured {} {:+#X}",
            format_prefixed_memory_address(field_address),
            format_prefixed_memory_address(captured),
            field_address as i128 - captured as i128,
        );
        if code_dialog.tracked_name.trim().is_empty() {
            code_dialog
                .status
                .push_str("; enter a name, then Save tracked");
        } else {
            code_dialog.save_tracked = true;
        }
    }

    fn memory_view_cell(ui: &mut egui::Ui, width: f32, text: &str) -> egui::Response {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
        let mut cell = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        cell.add_sized(
            rect.size(),
            egui::Label::new(RichText::new(text).monospace())
                .selectable(true)
                .sense(Sense::click_and_drag()),
        )
        .on_hover_cursor(egui::CursorIcon::Default)
    }

    fn render_memory_address_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.address_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let window = egui::CentralPanel::default()
            .frame(Self::memory_popup_frame(ctx))
            .show(ctx, |ui| {
                ui.checkbox(&mut dialog.pointer, "Pointer (x64)");
                ui.horizontal(|ui| {
                    ui.label(if dialog.pointer { "Base" } else { "Address" });
                    ui.text_edit_singleline(&mut dialog.address);
                });
                if dialog.pointer {
                    ui.horizontal(|ui| {
                        ui.label("Offsets");
                        ui.text_edit_singleline(&mut dialog.offsets);
                    });

                    // Show resolution chain like Cheat Engine
                    if let Some(pid) = self.memory_panel.process_pid {
                        // Get the actual pointer spec from the saved entry (has module info)
                        let saved_spec = self
                            .memory_panel
                            .saved
                            .get(dialog.index)
                            .and_then(|e| e.pointer.clone());

                        // Parse offsets from the text field (user may have edited them)
                        let offsets = dialog
                            .offsets
                            .split([',', ';', ' '])
                            .filter(|part| !part.trim().is_empty())
                            .map(parse_hex_offset)
                            .collect::<Option<Vec<_>>>();

                        if let Some(offsets) = offsets {
                            ui.add_space(6.0);
                            ui.separator();
                            #[cfg(windows)]
                            {
                                use crate::process_memory::read_scan_value;
                                // Resolve the actual base address:
                                // If saved spec has a module, resolve that; otherwise parse text field
                                let resolved_base = saved_spec
                                    .as_ref()
                                    .and_then(|spec| spec.module.as_ref())
                                    .and_then(|(module, offset)| {
                                        resolve_module_offset(pid, module, *offset).ok()
                                    })
                                    .or_else(|| parse_memory_address(&dialog.address));

                                if let Some(base) = resolved_base {
                                    // Show the base label with module name if applicable
                                    let base_label = if let Some(spec) = &saved_spec {
                                        if let Some((module, offset)) = &spec.module {
                                            format!("{} + {:X} → {:X}", module, offset, base)
                                        } else {
                                            format!("Base → {:X}", base)
                                        }
                                    } else {
                                        format!("Base → {:X}", base)
                                    };
                                    ui.label(RichText::new(base_label).monospace().weak());

                                    let mut current = base;
                                    let mut valid = true;
                                    for (i, &offset) in offsets.iter().enumerate() {
                                        let is_last = i == offsets.len() - 1;
                                        match read_scan_value(pid, current, crate::process_memory::ScanValueType::I64) {
                                            Ok(crate::process_memory::ScanValue::I64(next)) => {
                                                let deref = next as usize;
                                                let result = deref.wrapping_add(offset);
                                                ui.label(
                                                    RichText::new(format!(
                                                        "[{:X}] + {:X} = {:X}{}",
                                                        deref,
                                                        offset,
                                                        result,
                                                        if is_last { "  ← final" } else { "" }
                                                    ))
                                                    .monospace()
                                                    .color(if is_last {
                                                        Color32::from_rgb(100, 220, 130)
                                                    } else {
                                                        Color32::GRAY
                                                    }),
                                                );
                                                current = result;
                                            }
                                            Err(_) => {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "[{:X}]  cannot read memory",
                                                        current
                                                    ))
                                                    .monospace()
                                                    .color(Color32::from_rgb(220, 80, 80)),
                                                );
                                                valid = false;
                                                break;
                                            }
                                            _ => unreachable!(),
                                        }
                                    }
                                    if valid && offsets.is_empty() {
                                        ui.label(
                                            RichText::new(format!("→ {:X}", current))
                                                .monospace()
                                                .color(Color32::from_rgb(100, 220, 130)),
                                        );
                                    }
                                } else {
                                    ui.label(
                                        RichText::new("Cannot resolve base address")
                                            .monospace()
                                            .color(Color32::from_rgb(220, 80, 80)),
                                    );
                                }
                            }
                            ui.separator();
                        }
                    }

                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        dialog.rect = Some(window.response.rect);
        if save {
            self.apply_memory_address_dialog(&dialog);
            open = false;
        }
        if cancel {
            open = false;
        }
        if open {
            self.memory_panel.address_dialog = Some(dialog);
            // Refresh the chain display every ~100ms
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }


    fn apply_memory_address_dialog(&mut self, dialog: &AddressDialog) {
        let pointer = if dialog.pointer {
            let offsets = dialog
                .offsets
                .split([',', ';', ' '])
                .filter(|part| !part.trim().is_empty())
                .map(parse_hex_offset)
                .collect::<Option<Vec<_>>>();
            let Some(offsets) = offsets else {
                self.memory_panel.status = "Invalid pointer offsets".to_owned();
                return;
            };
            // Try to parse as "module+offset" (e.g. "neox_engine.dll+8AEEFA0")
            let addr_str = dialog.address.trim();
            let module_spec = addr_str
                .rsplit_once('+')
                .filter(|(module, _)| {
                    let m = module.trim();
                    m.contains('.') && !m.starts_with("0x") && !m.starts_with("0X")
                })
                .and_then(|(module, offset)| {
                    parse_hex_offset(offset.trim()).map(|off| (module.trim().to_owned(), off))
                });

            if let Some((module_name, module_offset)) = module_spec {
                Some(PointerSpec {
                    base: 0,
                    module: Some((module_name, module_offset)),
                    offsets,
                })
            } else {
                let Some(base) = parse_memory_address(addr_str) else {
                    self.memory_panel.status = "Invalid address".to_owned();
                    return;
                };
                Some(PointerSpec {
                    base,
                    module: None,
                    offsets,
                })
            }
        } else {
            let Some(base) = parse_memory_address(dialog.address.trim()) else {
                self.memory_panel.status = "Invalid address".to_owned();
                return;
            };
            // Non-pointer: resolve to the raw address
            if let Some(saved) = self.memory_panel.saved.get_mut(dialog.index) {
                saved.address = base;
                saved.pointer = None;
                saved.frozen = None;
            }
            self.persist_memory_pointers();
            return;
        };
        let pid = self.memory_panel.process_pid;
        let resolved = pid
            .and_then(|pid| {
                resolve_memory_address(
                    pid,
                    pointer.as_ref().map_or(0, |p| p.base),
                    pointer.as_ref(),
                )
                .ok()
            })
            .unwrap_or_default();
        if let Some(saved) = self.memory_panel.saved.get_mut(dialog.index) {
            saved.address = resolved;
            saved.pointer = pointer;
            saved.frozen = None;
        }
        self.persist_memory_pointers();
    }


    fn start_stable_pointer_filter(&mut self, action: MemoryScanAction) -> bool {
        let Some(mut dialog) = self.memory_panel.stable_pointer_dialog.take() else {
            return false;
        };
        let Some(pid) = self.memory_panel.process_pid else {
            dialog.status = "Select the restarted process before filtering candidates".to_owned();
            self.memory_panel.stable_pointer_dialog = Some(dialog);
            return true;
        };
        if dialog.validation_pid.is_some() || dialog.filter_rx.is_some() || dialog.rx.is_some() {
            self.memory_panel.stable_pointer_dialog = Some(dialog);
            return true;
        }
        let range = if action == MemoryScanAction::Between {
            let Some(min) = parse_scan_value(
                &self.memory_panel.between_min_input,
                dialog.value_type,
                self.memory_panel.hex,
            ) else {
                dialog.status = "Invalid minimum value".to_owned();
                self.memory_panel.stable_pointer_dialog = Some(dialog);
                return true;
            };
            let Some(max) = parse_scan_value(
                &self.memory_panel.between_max_input,
                dialog.value_type,
                self.memory_panel.hex,
            ) else {
                dialog.status = "Invalid maximum value".to_owned();
                self.memory_panel.stable_pointer_dialog = Some(dialog);
                return true;
            };
            if !scan_bounds_are_ordered(min, max) {
                dialog.status = "Minimum must not exceed maximum".to_owned();
                self.memory_panel.stable_pointer_dialog = Some(dialog);
                return true;
            }
            Some((min, max))
        } else {
            None
        };
        let exact = if matches!(
            action,
            MemoryScanAction::FirstScan
                | MemoryScanAction::Exact
                | MemoryScanAction::Less
                | MemoryScanAction::Greater
        ) {
            let Some(value) = parse_scan_value(
                &self.memory_panel.value_input,
                dialog.value_type,
                self.memory_panel.hex,
            ) else {
                dialog.status = "Invalid value".to_owned();
                self.memory_panel.stable_pointer_dialog = Some(dialog);
                return true;
            };
            Some(value)
        } else {
            None
        };
        let mut inputs = dialog
            .candidates
            .iter()
            .filter_map(|candidate| {
                Some(ScanCandidate::new(
                    candidate.resolved_address?,
                    candidate
                        .filter_value
                        .or(candidate.observed_value)
                        .or(candidate.live_value)?,
                ))
            })
            .collect::<Vec<_>>();
        if inputs.is_empty() {
            dialog.status = "Validate candidates before applying value filters".to_owned();
            self.memory_panel.stable_pointer_dialog = Some(dialog);
            return true;
        }
        inputs.sort_unstable_by_key(|candidate| candidate.address);
        let input_count = dialog.candidates.len();
        let comparison = if action == MemoryScanAction::FirstScan {
            Some(ScanComparison::Exact)
        } else {
            action.comparison()
        };
        let value_type = dialog.value_type;
        let (tx, rx) = mpsc::channel();
        dialog.filter_rx = Some(rx);
        dialog.status = format!(
            "{} — filtering {} candidate(s)…",
            action.label(),
            input_count
        );
        thread::spawn(move || {
            let result = if let Some(comparison) = comparison {
                filter_scan_candidates(pid, inputs, value_type, comparison, exact, range)
            } else {
                refresh_scan_candidates(pid, &mut inputs, value_type).map(|()| inputs)
            }
            .map_err(|error| error.to_string());
            let _ = tx.send(StablePointerFilterResult {
                pid,
                action,
                input_count,
                result,
            });
        });
        self.memory_panel.last_action = action.label().to_owned();
        self.memory_panel.stable_pointer_dialog = Some(dialog);
        true
    }

    fn start_memory_action(&mut self, action: MemoryScanAction) {
        if self.start_stable_pointer_filter(action) {
            return;
        }
        #[cfg(windows)]
        self.close_memory_debuggers();
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        if self.memory_panel.scanning {
            return;
        }
        let value_type = self.memory_panel.value_type;
        let text_encoding = self.memory_panel.text_encoding;
        if text_encoding.is_some()
            && !matches!(
                action,
                MemoryScanAction::FirstScan | MemoryScanAction::Exact
            )
        {
            self.memory_panel.status = "Text scan supports First scan and New value".to_owned();
            return;
        }
        let range = if action == MemoryScanAction::Between {
            let Some(min) = parse_scan_value(
                &self.memory_panel.between_min_input,
                value_type,
                self.memory_panel.hex,
            ) else {
                self.memory_panel.status = "Invalid minimum value".to_owned();
                return;
            };
            let Some(max) = parse_scan_value(
                &self.memory_panel.between_max_input,
                value_type,
                self.memory_panel.hex,
            ) else {
                self.memory_panel.status = "Invalid maximum value".to_owned();
                return;
            };
            if !scan_bounds_are_ordered(min, max) {
                self.memory_panel.status = "Minimum must not exceed maximum".to_owned();
                return;
            }
            Some((min, max))
        } else {
            None
        };
        let exact = if matches!(
            action,
            MemoryScanAction::Unknown | MemoryScanAction::Between
        ) || matches!(
            action,
            MemoryScanAction::Increased
                | MemoryScanAction::Decreased
                | MemoryScanAction::Changed
                | MemoryScanAction::Unchanged
        ) {
            None
        } else {
            if self.memory_panel.is_aob_scan {
                if self.memory_panel.value_input.trim().is_empty() {
                    self.memory_panel.status = "AOB pattern cannot be empty".to_owned();
                    return;
                }
                None
            } else if text_encoding.is_some() {
                if self.memory_panel.value_input.is_empty() {
                    self.memory_panel.status = "Text cannot be empty".to_owned();
                    return;
                }
                None
            } else {
                match parse_scan_value(
                    &self.memory_panel.value_input,
                    value_type,
                    self.memory_panel.hex,
                ) {
                    Some(value) => Some(value),
                    None => {
                        self.memory_panel.status = "Invalid value".to_owned();
                        return;
                    }
                }
            }
        };
        let limit_input = self.memory_panel.result_limit_input.trim();
        let result_limit =
            if limit_input.is_empty() || limit_input.eq_ignore_ascii_case("unlimited") {
                DEFAULT_SCAN_LIMIT
            } else {
                limit_input
                    .replace(['.', ',', '_'], "")
                    .parse::<usize>()
                    .unwrap_or(DEFAULT_SCAN_LIMIT)
                    .max(1_000)
            };
        self.memory_panel.result_limit_input = if result_limit == DEFAULT_SCAN_LIMIT {
            "Unlimited".to_owned()
        } else {
            result_limit.to_string()
        };
        let alignment = self
            .memory_panel
            .fast_scan_alignment
            .trim()
            .parse::<usize>()
            .unwrap_or(value_type.width())
            .clamp(1, 4096);
        self.memory_panel.fast_scan_alignment = alignment.to_string();
        let scan_options = if self.memory_panel.is_aob_scan {
            MemoryScanOptions {
                writable: false,
                executable: true,
                copy_on_write: self.memory_panel.scan_copy_on_write,
                active_memory_only: false,
                mem_private: true,
                mem_image: true,
                mem_mapped: true,
                alignment: None,
            }
        } else {
            MemoryScanOptions {
                writable: self.memory_panel.scan_writable,
                executable: self.memory_panel.scan_executable,
                copy_on_write: self.memory_panel.scan_copy_on_write,
                active_memory_only: self.memory_panel.scan_active_memory_only,
                mem_private: self.memory_panel.scan_mem_private,
                mem_image: self.memory_panel.scan_mem_image,
                mem_mapped: self.memory_panel.scan_mem_mapped,
                alignment: self.memory_panel.fast_scan.then_some(alignment),
            }
        };
        let candidates = if action.comparison().is_some() && text_encoding.is_none() {
            std::mem::take(&mut self.memory_panel.candidates)
        } else {
            self.memory_panel.candidates.clear();
            Vec::new()
        };
        let text_candidates = if action == MemoryScanAction::Exact && text_encoding.is_some() {
            std::mem::take(&mut self.memory_panel.text_candidates)
        } else {
            self.memory_panel.text_candidates.clear();
            Vec::new()
        };
        self.memory_panel.scan_progress.store(0, Ordering::Relaxed);
        self.memory_panel.live_candidate_values.clear();
        self.memory_panel.scan_input_count = if action.comparison().is_some() {
            candidates.len().max(text_candidates.len())
        } else {
            0
        };
        let progress = Arc::clone(&self.memory_panel.scan_progress);
        let (tx, rx) = mpsc::channel();
        self.memory_panel.scanning = true;
        if action.comparison().is_none() {
            self.memory_panel.has_scan_session = true;
        }
        self.memory_panel.status = format!("{} — loading…", action.label());
        self.memory_panel.last_action = action.label().to_owned();
        self.memory_panel.selected_results.clear();
        self.memory_panel.job_rx = Some(rx);
        let text = self.memory_panel.value_input.clone();
        let case_sensitive = self.memory_panel.text_case_sensitive;
        let null_terminated = self.memory_panel.text_null_terminated;
        let pause_while_scanning = self.memory_panel.pause_while_scanning;
        let is_aob = self.memory_panel.is_aob_scan;
        thread::spawn(move || {
            let _pause = if pause_while_scanning {
                match PausedProcess::new(pid) {
                    Ok(paused) => Some(paused),
                    Err(error) => {
                        let _ = tx.send(ScanJobResult {
                            pid,
                            action,
                            result: Err(format!("Unable to pause target: {error}")),
                        });
                        return;
                    }
                }
            } else {
                None
            };
            let result = if is_aob {
                if action == MemoryScanAction::Exact && !text_candidates.is_empty() {
                    filter_aob_scan_candidates(pid, text_candidates, &text)
                } else {
                    scan_aob_memory_with_progress(
                        pid,
                        &text,
                        result_limit,
                        scan_options,
                        progress,
                    )
                }
                .map(ScanJobCandidates::Text)
            } else if let Some(encoding) = text_encoding {
                if action == MemoryScanAction::Exact && !text_candidates.is_empty() {
                    filter_text_scan_candidates(
                        pid,
                        text_candidates,
                        &text,
                        encoding,
                        case_sensitive,
                        null_terminated,
                    )
                } else {
                    scan_text_memory_with_progress(
                        pid,
                        &text,
                        encoding,
                        case_sensitive,
                        null_terminated,
                        result_limit,
                        scan_options,
                        progress,
                    )
                }
                .map(ScanJobCandidates::Text)
            } else if let Some(comparison) = action.comparison() {
                filter_scan_candidates(pid, candidates, value_type, comparison, exact, range)
                    .map(ScanJobCandidates::Numeric)
            } else {
                scan_memory_range_with_progress(
                    pid,
                    exact,
                    range,
                    value_type,
                    result_limit,
                    scan_options,
                    progress,
                )
                .map(ScanJobCandidates::Numeric)
            }
            .map_err(|error| error.to_string());
            let _ = tx.send(ScanJobResult {
                pid,
                action,
                result,
            });
        });
    }

    #[cfg(windows)]
    fn close_memory_debuggers(&mut self) {
        if let Some(mut dialog) = self.memory_panel.instruction_watch_dialog.take()
            && let Some(mut active) = dialog.active.take()
        {
            active.stop();
        }
        if let Some(mut dialog) = self.memory_panel.code_access_dialog.take()
            && let Some(mut active) = dialog.active.take()
        {
            active.stop();
        }
    }

    fn poll_memory_job(&mut self) {
        let Some(rx) = self.memory_panel.job_rx.as_ref() else {
            return;
        };
        let Ok(outcome) = rx.try_recv() else {
            return;
        };
        self.memory_panel.job_rx = None;
        self.memory_panel.scanning = false;
        if self.memory_panel.process_pid != Some(outcome.pid) {
            return;
        }
        match outcome.result {
            Ok(ScanJobCandidates::Numeric(candidates)) => {
                let count = candidates.len();
                self.memory_panel.candidates = candidates;
                self.memory_panel.live_candidate_values.clear();
                self.memory_panel.text_candidates.clear();
                self.memory_panel.status =
                    format!("{} — {count} result(s)", outcome.action.label());
            }
            Ok(ScanJobCandidates::Text(candidates)) => {
                let count = candidates.len();
                self.memory_panel.text_candidates = candidates;
                self.memory_panel.candidates.clear();
                self.memory_panel.live_candidate_values.clear();
                self.memory_panel.status =
                    format!("{} — {count} text result(s)", outcome.action.label());
            }
            Err(error) => {
                self.memory_panel.status = format!("{} failed: {error}", outcome.action.label());
            }
        }
    }

    fn reset_memory_scan(&mut self, status: &str) {
        self.memory_panel.job_rx = None;
        self.memory_panel.scanning = false;
        self.memory_panel.candidates.clear();
        self.memory_panel.live_candidate_values.clear();
        self.memory_panel.text_candidates.clear();
        self.memory_panel.selected_results.clear();
        self.memory_panel.marked_result_addresses.clear();
        self.memory_panel.selection_anchor = None;
        self.memory_panel.visible_scan_ranges = [None, None];
        self.memory_panel.pending_write_checks.clear();
        self.memory_panel.has_scan_session = false;
        self.memory_panel.status = status.to_owned();
        self.memory_panel.last_action = status.to_owned();
    }

    fn add_selected_memory_results(&mut self) {
        let mut indices = self
            .memory_panel
            .selected_results
            .iter()
            .copied()
            .collect::<Vec<_>>();
        indices.sort_unstable();
        for index in indices {
            if let Some(candidate) = self.memory_panel.text_candidates.get(index) {
                let address = candidate.address;
                if self
                    .memory_panel
                    .saved
                    .iter()
                    .any(|saved| saved.address == address)
                {
                    continue;
                }
                self.memory_panel.saved.push(SavedMemoryAddress {
                    address,
                    value_type: ScanValueType::I8,
                    current: None,
                    text_encoding: self.memory_panel.text_encoding,
                    text_byte_len: match self.memory_panel.text_encoding {
                        Some(TextEncoding::Utf16) => candidate.current.encode_utf16().count() * 2,
                        _ => candidate.current.len(),
                    },
                    current_text: Some(candidate.current.clone()),
                    description: match self.memory_panel.text_encoding {
                        Some(TextEncoding::Utf16) => "Text (UTF-16)".to_owned(),
                        _ => "Text (UTF-8)".to_owned(),
                    },
                    pointer: None,
                    frozen: None,
                    saved_to_library: false,
                });
                continue;
            }
            let Some(candidate) = self.memory_panel.candidates.get(index).copied() else {
                continue;
            };
            let current = candidate.current(self.memory_panel.value_type);
            if self.memory_panel.saved.iter().any(|saved| {
                saved.address == candidate.address
                    && saved.value_type == self.memory_panel.value_type
            }) {
                continue;
            }
            self.memory_panel.saved.push(SavedMemoryAddress {
                address: candidate.address,
                value_type: self.memory_panel.value_type,
                current: Some(current),
                text_encoding: None,
                text_byte_len: 0,
                current_text: None,
                description: String::new(),
                pointer: None,
                frozen: None,
                saved_to_library: false,
            });
        }
        self.memory_panel.status = format!("{} address(es) saved", self.memory_panel.saved.len());
    }

    fn add_manual_memory_address(&mut self) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        let pointer = parse_pointer_expression(&self.memory_panel.manual_address).map(
            |(module, module_offset, offsets)| PointerSpec {
                base: 0,
                module: Some((module, module_offset)),
                offsets,
            },
        );
        let address = if let Some(pointer) = pointer.as_ref() {
            match resolve_memory_address(pid, pointer.base, Some(pointer)) {
                Ok(address) => address,
                Err(error) => {
                    self.memory_panel.status = format!("Unable to resolve pointer: {error}");
                    return;
                }
            }
        } else if let Some(address) = parse_memory_address(&self.memory_panel.manual_address) {
            address
        } else {
            self.memory_panel.status = "Invalid address or pointer expression".to_owned();
            return;
        };
        let value_type = self.memory_panel.value_type;
        let current = read_scan_value(pid, address, value_type).ok();
        let description = pointer
            .as_ref()
            .map(format_pointer_expression)
            .unwrap_or_default();
        self.memory_panel.saved.push(SavedMemoryAddress {
            address,
            value_type,
            current,
            text_encoding: None,
            text_byte_len: 0,
            current_text: None,
            description,
            pointer,
            frozen: None,
            saved_to_library: false,
        });
        self.memory_panel.manual_address.clear();
    }

    fn open_manual_structure_view(&mut self) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        let Some(address) = parse_memory_address(&self.memory_panel.manual_address) else {
            self.memory_panel.status = "Invalid address".to_owned();
            return;
        };
        let elements = default_structure_elements();
        self.memory_panel.memory_view_dialog = Some(MemoryViewDialog {
            address,
            tracked_base: None,
            kind: MemoryViewKind::Structure,
            display_type: MemoryDisplayType::ByteHex,
            relative_addresses: true,
            pinned: true,
            elements: elements.clone(),
            pending_add: None,
            pending_track: None,
            pointer_width: process_pointer_width(pid).unwrap_or(8),
            previous_bytes: Vec::new(),
            byte_change_times: HashMap::new(),
            classes: vec![StructureClass {
                name: "Class_0".to_owned(),
                address,
                elements,
            }],
            selected_class: 0,
            class_detection_status: String::new(),
            class_detection_attempted: false,
            auto_dissected: false,
            history: Vec::new(),
        });
    }

    fn refresh_memory_values(&mut self) {
        if self.memory_panel.last_refresh.elapsed() >= Duration::from_millis(50) {
            self.memory_panel.last_refresh = Instant::now();
            if let Some(pid) = self.memory_panel.process_pid {
                for (start, end, rendered_at) in
                    self.memory_panel.visible_scan_ranges.into_iter().flatten()
                {
                    if rendered_at.elapsed() > Duration::from_millis(100) {
                        continue;
                    }
                    let end = end.min(self.memory_panel.candidates.len());
                    if start < end {
                        let mut visible = self.memory_panel.candidates[start..end].to_vec();
                        if refresh_scan_candidates(pid, &mut visible, self.memory_panel.value_type)
                            .is_ok()
                        {
                            for (offset, candidate) in visible.into_iter().enumerate() {
                                self.memory_panel.live_candidate_values.insert(
                                    start + offset,
                                    candidate.current(self.memory_panel.value_type),
                                );
                            }
                        }
                    }
                }
            }
        }
        if let Some(pid) = self.memory_panel.process_pid {
            let now = Instant::now();
            let mut pending = Vec::new();
            let mut verified = 0usize;
            let mut overwritten = 0usize;
            let mut unreadable = 0usize;
            for check in self.memory_panel.pending_write_checks.drain(..) {
                if now < check.due {
                    pending.push(check);
                    continue;
                }
                match read_scan_value(pid, check.address, check.value_type) {
                    Ok(observed) if observed == check.expected => verified += 1,
                    Ok(_) => overwritten += 1,
                    Err(_) => unreadable += 1,
                }
            }
            self.memory_panel.pending_write_checks = pending;
            if overwritten > 0 {
                self.memory_panel.status = format!(
                    "Game overwrote {overwritten} value(s) shortly after the write; freeze or find the authoritative address"
                );
            } else if verified > 0 && self.memory_panel.pending_write_checks.is_empty() {
                self.memory_panel.status = if unreadable == 0 {
                    format!(
                        "Write persisted at {verified} address(es); if the game did not react, these are mirrored/render values"
                    )
                } else {
                    format!("Write verified at {verified} address(es), {unreadable} unreadable")
                };
            }
        }
        if self.memory_panel.last_saved_refresh.elapsed() < Duration::from_millis(50) {
            return;
        }
        self.memory_panel.last_saved_refresh = Instant::now();
        let Some(pid) = self.memory_panel.process_pid else {
            return;
        };
        for saved in &mut self.memory_panel.saved {
            if let Some(pointer) = saved.pointer.as_ref()
                && let Ok(address) = resolve_memory_address(pid, pointer.base, Some(pointer))
            {
                saved.address = address;
            }
            if let Some(encoding) = saved.text_encoding {
                saved.current_text =
                    read_text_memory(pid, saved.address, saved.text_byte_len, encoding).ok();
                saved.current = None;
            } else {
                saved.current = read_scan_value(pid, saved.address, saved.value_type).ok();
                saved.current_text = None;
            }
        }
    }

    fn sync_memory_freeze_targets(&mut self) {
        let targets = self
            .memory_panel
            .saved
            .iter()
            .filter_map(|saved| {
                saved.frozen.map(|value| FreezeTarget {
                    address: saved.address,
                    value,
                    pointer: saved.pointer.clone(),
                })
            })
            .collect::<Vec<_>>();
        if !targets.is_empty() {
            self.memory_panel.freeze_worker.ensure_started();
        }
        if let Ok(mut config) = self.memory_panel.freeze_worker.config.lock() {
            config.0 = (!targets.is_empty())
                .then_some(self.memory_panel.process_pid)
                .flatten();
            config.1 = targets;
        }
    }

    fn begin_saved_memory_value_edit(&mut self, index: usize, position: egui::Pos2) {
        let Some(saved) = self.memory_panel.saved.get(index) else {
            return;
        };
        self.memory_panel.edit_value_index = Some(index);
        self.memory_panel.edit_value_input = saved
            .current_text
            .clone()
            .or_else(|| {
                saved
                    .current
                    .map(|value| editable_scan_value(value, self.memory_panel.hex))
            })
            .unwrap_or_default();
        self.memory_panel.edit_value_position = Some(position);
    }

    fn render_saved_memory_value_editor(&mut self, ctx: &egui::Context) {
        let (Some(index), Some(position)) = (
            self.memory_panel.edit_value_index,
            self.memory_panel.edit_value_position,
        ) else {
            return;
        };
        let mut commit = false;
        let mut cancel = false;
        egui::Area::new(egui::Id::new("saved-memory-value-editor"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ctx, |ui| {
                Frame::popup(ui.style()).inner_margin(8).show(ui, |ui| {
                    ui.label(RichText::new("Edit selected value(s)").strong());
                    let response = ui.add_sized(
                        [190.0, 24.0],
                        egui::TextEdit::singleline(&mut self.memory_panel.edit_value_input),
                    );
                    Self::apply_vietnamese_input_if_changed(
                        &response,
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
                        &mut self.memory_panel.edit_value_input,
                    );
                    response.request_focus();
                    ui.horizontal(|ui| {
                        commit = ui.button("Apply").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                    commit |= response.has_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    cancel |= ui.input(|input| input.key_pressed(egui::Key::Escape));
                });
            });
        if commit {
            self.commit_saved_memory_value(index);
        } else if cancel {
            self.memory_panel.edit_value_index = None;
            self.memory_panel.edit_value_position = None;
        }
    }

    fn commit_saved_memory_value(&mut self, index: usize) {
        let Some(pid) = self.memory_panel.process_pid else {
            return;
        };
        let Some(saved) = self.memory_panel.saved.get(index).cloned() else {
            return;
        };
        let targets = if self.memory_panel.selected_saved.contains(&index) {
            self.memory_panel
                .selected_saved
                .iter()
                .copied()
                .collect::<Vec<_>>()
        } else {
            vec![index]
        };
        let mut written = 0;
        if let Some(encoding) = saved.text_encoding {
            for target in targets {
                let Some(entry) = self.memory_panel.saved.get(target).cloned() else {
                    continue;
                };
                if entry.text_encoding != Some(encoding)
                    || write_text_memory(
                        pid,
                        entry.address,
                        &self.memory_panel.edit_value_input,
                        encoding,
                        entry.text_byte_len,
                    )
                    .is_err()
                {
                    continue;
                }
                self.memory_panel.saved[target].current_text =
                    Some(self.memory_panel.edit_value_input.clone());
                written += 1;
            }
        } else {
            let Some(value) = parse_scan_value(
                &self.memory_panel.edit_value_input,
                saved.value_type,
                self.memory_panel.hex,
            ) else {
                self.memory_panel.status = "Invalid value".to_owned();
                return;
            };
            for target in targets {
                let Some(entry) = self.memory_panel.saved.get(target).cloned() else {
                    continue;
                };
                if entry.text_encoding.is_some()
                    || entry.value_type != saved.value_type
                    || write_scan_value(pid, entry.address, value).is_err()
                {
                    continue;
                }
                let observed = read_scan_value(pid, entry.address, entry.value_type).ok();
                self.memory_panel.saved[target].current = observed.or(Some(value));
                self.memory_panel
                    .pending_write_checks
                    .push(PendingWriteCheck {
                        due: Instant::now() + Duration::from_millis(250),
                        address: entry.address,
                        value_type: entry.value_type,
                        expected: value,
                    });
                if self.memory_panel.saved[target].frozen.is_some() {
                    self.memory_panel.saved[target].frozen = Some(value);
                }
                written += 1;
            }
        }
        self.memory_panel.edit_value_index = None;
        self.memory_panel.edit_value_position = None;
        self.memory_panel.status = format!("Value written to {written} address(es)");
    }

    fn delete_selected_saved_memory(&mut self) {
        let selected = &self.memory_panel.selected_saved;
        self.memory_panel.saved = self
            .memory_panel
            .saved
            .drain(..)
            .enumerate()
            .filter_map(|(index, saved)| (!selected.contains(&index)).then_some(saved))
            .collect();
        self.memory_panel.selected_saved.clear();
        self.memory_panel.edit_value_index = None;
        self.memory_panel.edit_value_position = None;
        self.sync_memory_freeze_targets();
        self.persist_memory_pointers();
    }

    fn delete_unselected_saved_memory(&mut self) {
        let selected = &self.memory_panel.selected_saved;
        self.memory_panel.saved = self
            .memory_panel
            .saved
            .drain(..)
            .enumerate()
            .filter_map(|(index, saved)| selected.contains(&index).then_some(saved))
            .collect();
        self.memory_panel.selected_saved = (0..self.memory_panel.saved.len()).collect();
        self.memory_panel.saved_selection_anchor =
            (!self.memory_panel.saved.is_empty()).then_some(0);
        self.memory_panel.edit_value_index = None;
        self.memory_panel.edit_value_position = None;
        self.sync_memory_freeze_targets();
        self.persist_memory_pointers();
    }

    fn reindex_saved_selection_after_delete(&mut self, deleted: usize) {
        self.memory_panel.selected_saved = self
            .memory_panel
            .selected_saved
            .drain()
            .filter_map(|index| {
                if index == deleted {
                    None
                } else {
                    Some(if index > deleted { index - 1 } else { index })
                }
            })
            .collect();
    }

    fn capture_memory_hotkey(&mut self, ctx: &egui::Context) {
        let Some(action) = self.memory_panel.capturing_hotkey else {
            return;
        };
        if let Some(binding) = self.capture_next_input(ctx) {
            self.finish_memory_hotkey_capture(action, binding);
        }
    }

    fn finish_memory_hotkey_capture(&mut self, action: MemoryScanAction, binding: HotkeyBinding) {
        self.memory_panel.hotkeys.insert(action, binding);
        self.memory_panel.capturing_hotkey = None;
        self.capture_hotkey_combo_keys = None;
        self.capture_hotkey_combo_vks.clear();
        self.persist_memory_hotkeys();
    }

    fn poll_memory_hotkeys(&mut self, ctx: &egui::Context) {
        let events = crate::overlay::take_memory_trigger_events();
        if !events.is_empty() {
            let bindings = self
                .memory_panel
                .hotkeys
                .iter()
                .map(|(action, binding)| (*action, binding.clone()))
                .collect::<Vec<_>>();
            for event in events {
                for (action, expected) in &bindings {
                    if hotkey::binding_matches(expected, &event) {
                        self.start_memory_action(*action);
                    }
                }
            }
        }
        if !self.memory_panel.hotkeys.is_empty() || self.memory_panel.scanning {
            ctx.request_repaint_after(Duration::from_millis(35));
        }
    }

    fn persist_memory_hotkeys(&mut self) {
        let mut hotkeys = self
            .memory_panel
            .hotkeys
            .iter()
            .map(|(action, binding)| (action.config_key().to_owned(), binding.clone()))
            .collect::<Vec<_>>();
        hotkeys.sort_by(|left, right| left.0.cmp(&right.0));
        self.state.memory_scan_hotkeys = hotkeys;
        self.persist();
    }

    fn persist_memory_pointers(&mut self) {
        for saved in &self.memory_panel.saved {
            if !saved.saved_to_library {
                continue;
            }
            let module_pointer = saved
                .pointer
                .as_ref()
                .and_then(|pointer| pointer.module.as_ref().map(|root| (pointer, root)));
            let app_name = module_pointer
                .map(|(_, (module, _))| module.clone())
                .or_else(|| {
                    self.memory_panel.process_pid.and_then(|pid| {
                        #[cfg(windows)]
                        return process_modules(pid)
                            .ok()?
                            .first()
                            .map(|entry| entry.0.clone());
                        #[cfg(not(windows))]
                        None
                    })
                })
                .unwrap_or_else(|| "Unknown application".to_owned());
            let entry = MemoryPointerEntry {
                name: saved.description.clone(),
                app_name,
                module: module_pointer.map_or_else(String::new, |(_, root)| root.0.clone()),
                module_offset: module_pointer.map_or(0, |(_, root)| root.1),
                offsets: module_pointer
                    .map_or_else(Vec::new, |(pointer, _)| pointer.offsets.clone()),
                value_type: memory_type_config(saved.value_type).to_owned(),
                absolute_address: module_pointer.is_none().then_some(saved.address),
                code_module: String::new(),
                code_offset: 0,
                code_address_offset: 0,
                runtime_address: None,
                runtime_process_id: None,
                tracked_value: String::new(),
                tracked_signature: String::new(),
            };
            if let Some(existing) = self.state.memory_pointer_list.iter_mut().find(|existing| {
                existing.module.eq_ignore_ascii_case(&entry.module)
                    && existing.module_offset == entry.module_offset
                    && existing.offsets == entry.offsets
                    && existing.absolute_address == entry.absolute_address
            }) {
                *existing = entry;
            } else {
                self.state.memory_pointer_list.push(entry);
            }
        }
        crate::overlay::set_memory_pointer_entries(&self.state.memory_pointer_list);
        self.persist();
    }
}

fn memory_type_label(value_type: ScanValueType) -> &'static str {
    match value_type {
        ScanValueType::I8 => "Byte",
        ScanValueType::I16 => "2 Bytes",
        ScanValueType::I32 => "4 Bytes",
        ScanValueType::F32 => "Float",
        ScanValueType::I64 => "8 Bytes",
        ScanValueType::F64 => "Double",
    }
}

fn memory_type_config(value_type: ScanValueType) -> &'static str {
    match value_type {
        ScanValueType::I8 => "i8",
        ScanValueType::I16 => "i16",
        ScanValueType::I32 => "i32",
        ScanValueType::F32 => "f32",
        ScanValueType::I64 => "i64",
        ScanValueType::F64 => "f64",
    }
}

fn memory_type_from_config(value_type: &str) -> Option<ScanValueType> {
    Some(match value_type {
        "i8" => ScanValueType::I8,
        "i16" => ScanValueType::I16,
        "i32" => ScanValueType::I32,
        "f32" => ScanValueType::F32,
        "i64" => ScanValueType::I64,
        "f64" => ScanValueType::F64,
        _ => return None,
    })
}

fn format_pointer_expression(pointer: &PointerSpec) -> String {
    let root = pointer.module.as_ref().map_or_else(
        || format_prefixed_memory_address(pointer.base),
        |(module, offset)| format!("{module}+{offset:X}"),
    );
    let offsets = pointer
        .offsets
        .iter()
        .map(|offset| format!("{offset:X}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{root} [{offsets}]")
}

fn compact_hotkey_label(label: &str) -> String {
    let keys = hotkey::split_key_list(label);
    if let Some(key) = keys
        .iter()
        .rev()
        .find(|key| !hotkey::is_modifier_key_name(key))
    {
        return key.chars().take(3).collect();
    }
    keys.iter()
        .filter_map(|key| key.chars().next())
        .take(3)
        .collect()
}

fn parse_scan_value(text: &str, value_type: ScanValueType, hex: bool) -> Option<ScanValue> {
    let text = text.trim().replace('_', "");
    let direct = match value_type {
        ScanValueType::F32 => text
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
            .map(ScanValue::F32),
        ScanValueType::F64 => text
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(ScanValue::F64),
        ScanValueType::I8 if hex => {
            parse_hex_signed(&text, 8).map(|value| ScanValue::I8(value as i8))
        }
        ScanValueType::I16 if hex => {
            parse_hex_signed(&text, 16).map(|value| ScanValue::I16(value as i16))
        }
        ScanValueType::I32 if hex => {
            parse_hex_signed(&text, 32).map(|value| ScanValue::I32(value as i32))
        }
        ScanValueType::I64 if hex => parse_hex_signed(&text, 64).map(ScanValue::I64),
        ScanValueType::I8 => text.parse().ok().map(ScanValue::I8),
        ScanValueType::I16 => text.parse().ok().map(ScanValue::I16),
        ScanValueType::I32 => text.parse().ok().map(ScanValue::I32),
        ScanValueType::I64 => text.parse().ok().map(ScanValue::I64),
    };
    if direct.is_some() || hex {
        return direct;
    }
    if !text.chars().any(|character| "+-*/^()".contains(character))
        || !text.chars().all(|character| {
            character.is_ascii_digit()
                || character.is_ascii_whitespace()
                || ".+-*/^()".contains(character)
        })
    {
        return None;
    }
    let value = crate::overlay::evaluate_math_expression_f64(&text);
    if !value.is_finite() {
        return None;
    }
    match value_type {
        ScanValueType::F32 if value >= f32::MIN as f64 && value <= f32::MAX as f64 => {
            Some(ScanValue::F32(value as f32))
        }
        ScanValueType::F64 => Some(ScanValue::F64(value)),
        ScanValueType::I8 if value >= i8::MIN as f64 && value <= i8::MAX as f64 => {
            Some(ScanValue::I8(value.trunc() as i8))
        }
        ScanValueType::I16 if value >= i16::MIN as f64 && value <= i16::MAX as f64 => {
            Some(ScanValue::I16(value.trunc() as i16))
        }
        ScanValueType::I32 if value >= i32::MIN as f64 && value <= i32::MAX as f64 => {
            Some(ScanValue::I32(value.trunc() as i32))
        }
        ScanValueType::I64 if value >= i64::MIN as f64 && value <= i64::MAX as f64 => {
            Some(ScanValue::I64(value.trunc() as i64))
        }
        _ => None,
    }
}

fn scan_bounds_are_ordered(min: ScanValue, max: ScanValue) -> bool {
    macro_rules! ordered {
        ($variant:path) => {
            if let ($variant(min), $variant(max)) = (min, max) {
                return min <= max;
            }
        };
    }
    ordered!(ScanValue::I8);
    ordered!(ScanValue::I16);
    ordered!(ScanValue::I32);
    ordered!(ScanValue::F32);
    ordered!(ScanValue::I64);
    ordered!(ScanValue::F64);
    false
}

fn parse_hex_signed(text: &str, bits: u32) -> Option<i64> {
    let text = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    if let Some(digits) = text.strip_prefix('-') {
        return i64::from_str_radix(digits, 16).ok()?.checked_neg();
    }
    let unsigned = u64::from_str_radix(text, 16).ok()?;
    let shift = 64u32.checked_sub(bits)?;
    Some(((unsigned << shift) as i64) >> shift)
}

fn format_scan_value(value: ScanValue, hex: bool) -> String {
    match value {
        ScanValue::I8(value) if hex => format!("0x{:02X}", value as u8),
        ScanValue::I16(value) if hex => format!("0x{:04X}", value as u16),
        ScanValue::I32(value) if hex => format!("0x{:08X}", value as u32),
        ScanValue::I64(value) if hex => format!("0x{:016X}", value as u64),
        ScanValue::I8(value) => value.to_string(),
        ScanValue::I16(value) => value.to_string(),
        ScanValue::I32(value) => value.to_string(),
        ScanValue::F32(value) => format_compact_float(value as f64, 6),
        ScanValue::I64(value) => value.to_string(),
        ScanValue::F64(value) => format_compact_float(value, 10),
    }
}

fn format_compact_float(value: f64, precision: usize) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    if !value.is_finite() {
        return value.to_string();
    }
    let absolute = value.abs();
    if !(0.001..1_000_000_000.0).contains(&absolute) {
        return format!("{value:.precision$e}");
    }
    format!("{value:.precision$}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn default_structure_elements() -> Vec<StructureElement> {
    (0..512)
        .step_by(4)
        .map(|offset| StructureElement {
            offset,
            value_type: StructureElementType::I32,
            name: format!("field_{offset:04X}"),
            detected_class: None,
            expanded: false,
            child_elements: Vec::new(),
        })
        .collect()
}

/// CE-style description for a structure element row.
/// e.g. "+0000 - Pointer to instance of CPlayer" or "+0008 - 4 Bytes" or "+000C - Float"
fn ce_element_description(element: &StructureElement) -> String {
    let type_desc = match element.value_type {
        StructureElementType::Pointer => {
            match element.detected_class.as_deref() {
                Some(name) if !name.is_empty() => format!("Pointer to instance of {name}"),
                _ => "Pointer".to_owned(),
            }
        }
        StructureElementType::Byte => "Byte".to_owned(),
        StructureElementType::I16 => "2 Bytes".to_owned(),
        StructureElementType::I32 => "4 Bytes".to_owned(),
        StructureElementType::I64 => "8 Bytes".to_owned(),
        StructureElementType::Float => "Float".to_owned(),
        StructureElementType::Double => "Double".to_owned(),
    };
    // Include user-set name if it's not the default generated name
    let name_part = if !element.name.is_empty()
        && element.name != format!("field_{:04X}", element.offset)
    {
        format!(" ({})", element.name)
    } else {
        String::new()
    };
    format!("+{:04X} - {type_desc}{name_part}", element.offset)
}

struct StructureIdentity {
    name: String,
    address: usize,
    evidence: String,
}

fn identify_structure_class(process_pid: Option<u32>, dialog: &mut MemoryViewDialog) {
    let Some(pid) = process_pid else {
        dialog.class_detection_status = "Select a process to identify this class".to_owned();
        return;
    };
    let original_address = dialog.address;
    let Some(identity) = detect_structure_identity(pid, original_address, dialog.pointer_width)
    else {
        if let Some(class) = dialog.classes.get_mut(dialog.selected_class)
            && (class.name == "Class_0" || class.name.starts_with("Unknown @"))
        {
            class.name = format!("Unknown @ {}", format_memory_address(original_address));
        }
        dialog.class_detection_status =
            "No usable RTTI or vtable was found; field names must be mapped manually".to_owned();
        return;
    };

    let moved_to_object_base = identity.address != original_address;
    if let Some(class) = dialog.classes.get_mut(dialog.selected_class) {
        class.name = identity.name;
        class.address = identity.address;
        if moved_to_object_base {
            class.elements = default_structure_elements();
        }
    }
    dialog.address = identity.address;
    if moved_to_object_base {
        dialog.elements = default_structure_elements();
        dialog.previous_bytes.clear();
    }
    dialog.class_detection_status = identity.evidence;
}

#[cfg(windows)]
fn detect_structure_identity(
    pid: u32,
    field_address: usize,
    pointer_width: usize,
) -> Option<StructureIdentity> {
    let modules = process_modules(pid).ok()?;
    let mut vtable_fallback = None;
    // ponytail: nearby-field recovery is capped at 0x100. A wider object search belongs in a
    // dedicated scanner; keeping this bounded makes opening Dissect data effectively instant.
    for field_offset in (0..=0x100usize).step_by(4) {
        let Some(object_address) = field_address.checked_sub(field_offset) else {
            continue;
        };
        let Some(vtable) = read_remote_pointer(pid, object_address, pointer_width) else {
            continue;
        };
        let Some((module_name, module_base, module_size)) = module_containing(&modules, vtable)
        else {
            continue;
        };
        let Some(first_method) = read_remote_pointer(pid, vtable, pointer_width) else {
            continue;
        };
        if module_containing(&modules, first_method).is_none() {
            continue;
        }

        if let Some(class_name) =
            detect_msvc_rtti_name(pid, vtable, pointer_width, module_base, module_size)
        {
            return Some(StructureIdentity {
                name: class_name.clone(),
                address: object_address,
                evidence: format!(
                    "MSVC RTTI: {class_name}; object base {}{}",
                    format_memory_address(object_address),
                    (field_offset != 0)
                        .then(|| format!(" (-0x{field_offset:X} from selected field)"))
                        .unwrap_or_default(),
                ),
            });
        }

        vtable_fallback.get_or_insert_with(|| StructureIdentity {
            name: format!("Unknown ({module_name} vtable)"),
            address: object_address,
            evidence: format!(
                "Vtable: {module_name}+{:X}; RTTI name is absent or stripped{}",
                vtable - module_base,
                (field_offset != 0)
                    .then(|| format!("; object base is -0x{field_offset:X} from selected field"))
                    .unwrap_or_default(),
            ),
        });
    }
    vtable_fallback
}

#[cfg(not(windows))]
fn detect_structure_identity(
    _pid: u32,
    _field_address: usize,
    _pointer_width: usize,
) -> Option<StructureIdentity> {
    None
}

#[cfg(windows)]
fn module_containing(
    modules: &[(String, usize, usize)],
    address: usize,
) -> Option<(&str, usize, usize)> {
    modules.iter().find_map(|(name, base, size)| {
        (*base..base.saturating_add(*size))
            .contains(&address)
            .then_some((name.as_str(), *base, *size))
    })
}

#[cfg(windows)]
fn read_remote_pointer(pid: u32, address: usize, pointer_width: usize) -> Option<usize> {
    let bytes = read_memory_bytes(pid, address, pointer_width).ok()?;
    (bytes.len() == pointer_width)
        .then(|| decode_pointer(&bytes))
        .flatten()
}

#[cfg(windows)]
fn detect_msvc_rtti_name(
    pid: u32,
    vtable: usize,
    pointer_width: usize,
    module_base: usize,
    module_size: usize,
) -> Option<String> {
    let locator = read_remote_pointer(pid, vtable.checked_sub(pointer_width)?, pointer_width)?;
    if !(module_base..module_base.saturating_add(module_size)).contains(&locator) {
        return None;
    }

    let (type_descriptor, name_offset) = if pointer_width == 8 {
        let locator_bytes = read_memory_bytes(pid, locator, 24).ok()?;
        if locator_bytes.len() != 24 {
            return None;
        }
        let type_rva = u32::from_le_bytes(locator_bytes[12..16].try_into().ok()?) as usize;
        let self_rva = u32::from_le_bytes(locator_bytes[20..24].try_into().ok()?) as usize;
        let image_base = locator.checked_sub(self_rva)?;
        if image_base != module_base {
            return None;
        }
        (image_base.checked_add(type_rva)?, 16)
    } else if pointer_width == 4 {
        let locator_bytes = read_memory_bytes(pid, locator, 20).ok()?;
        if locator_bytes.len() != 20 {
            return None;
        }
        (
            u32::from_le_bytes(locator_bytes[12..16].try_into().ok()?) as usize,
            8,
        )
    } else {
        return None;
    };

    let raw = read_memory_bytes(pid, type_descriptor.checked_add(name_offset)?, 256).ok()?;
    let end = raw.iter().position(|byte| *byte == 0)?;
    let decorated = std::str::from_utf8(raw.get(..end)?).ok()?;
    demangle_msvc_type_name(decorated)
}

fn demangle_msvc_type_name(decorated: &str) -> Option<String> {
    let body = decorated
        .strip_prefix(".?AV")
        .or_else(|| decorated.strip_prefix(".?AU"))
        .or_else(|| decorated.strip_prefix(".?AW4"))?
        .strip_suffix("@@")?;
    if body.is_empty() {
        return None;
    }
    if body.contains("?$") {
        return Some(body.to_owned());
    }
    let mut parts = body
        .split('@')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    parts.reverse();
    Some(parts.join("::"))
}

fn auto_structure_elements(bytes: &[u8], pointer_width: usize) -> Vec<StructureElement> {
    let max_user_address = if pointer_width == 8 { 0x7FFF_FFFF_FFFF } else { 0xFFFF_FFFF };
    let step = 4;
    (0..bytes.len())
        .step_by(step)
        .map(|offset| {
            let pointer = bytes
                .get(offset..offset.saturating_add(pointer_width))
                .and_then(decode_pointer);
            let value_type =
                if pointer.is_some_and(|value| value >= 0x1_0000 && value <= max_user_address && value % pointer_width == 0) {
                    StructureElementType::Pointer
                } else if bytes.get(offset..offset + 4).is_some_and(|raw| {
                    let value = f32::from_le_bytes(raw.try_into().unwrap());
                    value.is_normal() && value.abs() <= 1_000_000.0
                }) {
                    StructureElementType::Float
                } else {
                    StructureElementType::I32
                };
            StructureElement {
                offset,
                value_type,
                name: format!("field_{offset:04X}"),
                detected_class: None,
                expanded: false,
                child_elements: Vec::new(),
            }
        })
        .collect()
}

fn decode_pointer(bytes: &[u8]) -> Option<usize> {
    match bytes.len() {
        4 => Some(u32::from_le_bytes(bytes.try_into().ok()?) as usize),
        8 => Some(u64::from_le_bytes(bytes.try_into().ok()?) as usize),
        _ => None,
    }
}

fn format_structure_value(bytes: &[u8], value_type: StructureElementType) -> String {
    match value_type {
        StructureElementType::Byte => format!("{:02X}", bytes[0]),
        StructureElementType::I16 => i16::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        StructureElementType::I32 => i32::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        StructureElementType::I64 => i64::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        StructureElementType::Float => {
            format_compact_float(f32::from_le_bytes(bytes.try_into().unwrap()) as f64, 6)
        }
        StructureElementType::Double => {
            format_compact_float(f64::from_le_bytes(bytes.try_into().unwrap()), 10)
        }
        StructureElementType::Pointer => {
            decode_pointer(bytes).map_or_else(|| "P->?".to_owned(), |value| format!("P->{value:X}"))
        }
    }
}

fn memory_display_width(display_type: MemoryDisplayType) -> usize {
    match display_type {
        MemoryDisplayType::ByteHex | MemoryDisplayType::ByteDecimal => 1,
        MemoryDisplayType::I16Hex | MemoryDisplayType::I16Decimal => 2,
        MemoryDisplayType::I32Hex | MemoryDisplayType::I32Decimal | MemoryDisplayType::Float => 4,
        MemoryDisplayType::I64Hex | MemoryDisplayType::I64Decimal | MemoryDisplayType::Double => 8,
    }
}

fn memory_display_scan_type(display_type: MemoryDisplayType) -> ScanValueType {
    match display_type {
        MemoryDisplayType::ByteHex | MemoryDisplayType::ByteDecimal => ScanValueType::I8,
        MemoryDisplayType::I16Hex | MemoryDisplayType::I16Decimal => ScanValueType::I16,
        MemoryDisplayType::I32Hex | MemoryDisplayType::I32Decimal => ScanValueType::I32,
        MemoryDisplayType::I64Hex | MemoryDisplayType::I64Decimal => ScanValueType::I64,
        MemoryDisplayType::Float => ScanValueType::F32,
        MemoryDisplayType::Double => ScanValueType::F64,
    }
}

fn memory_display_cell_width(display_type: MemoryDisplayType) -> f32 {
    match display_type {
        MemoryDisplayType::ByteHex => 32.0,
        MemoryDisplayType::ByteDecimal => 42.0,
        MemoryDisplayType::I16Hex => 54.0,
        MemoryDisplayType::I16Decimal => 66.0,
        MemoryDisplayType::I32Hex => 82.0,
        MemoryDisplayType::I32Decimal | MemoryDisplayType::Float => 100.0,
        MemoryDisplayType::I64Hex => 142.0,
        MemoryDisplayType::I64Decimal | MemoryDisplayType::Double => 154.0,
    }
}

fn format_memory_protection(protect: u32) -> &'static str {
    match protect & 0xFF {
        0x01 => "NoAccess",
        0x02 => "ReadOnly",
        0x04 => "Read/Write",
        0x08 => "WriteCopy",
        0x10 => "Execute",
        0x20 => "Execute/Read",
        0x40 => "Execute/Read/Write",
        0x80 => "Execute/WriteCopy",
        _ => "Unknown",
    }
}

fn memory_display_types() -> [(MemoryDisplayType, &'static str); 10] {
    [
        (MemoryDisplayType::ByteHex, "Byte hex"),
        (MemoryDisplayType::ByteDecimal, "Byte decimal"),
        (MemoryDisplayType::I16Hex, "2 Byte hex"),
        (MemoryDisplayType::I16Decimal, "2 Byte decimal"),
        (MemoryDisplayType::I32Hex, "4 Byte hex"),
        (MemoryDisplayType::I32Decimal, "4 Byte decimal"),
        (MemoryDisplayType::I64Hex, "8 Byte hex"),
        (MemoryDisplayType::I64Decimal, "8 Byte decimal"),
        (MemoryDisplayType::Float, "Float"),
        (MemoryDisplayType::Double, "Double"),
    ]
}

fn format_memory_display(bytes: &[u8], display_type: MemoryDisplayType) -> String {
    match display_type {
        MemoryDisplayType::ByteHex => bytes
            .iter()
            .map(|value| format!("{value:02X}"))
            .collect::<Vec<_>>()
            .join(" "),
        MemoryDisplayType::ByteDecimal => bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        MemoryDisplayType::I16Hex => bytes
            .chunks_exact(2)
            .map(|chunk| format!("{:04X}", u16::from_le_bytes([chunk[0], chunk[1]])))
            .collect::<Vec<_>>()
            .join(" "),
        MemoryDisplayType::I16Decimal => bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]).to_string())
            .collect::<Vec<_>>()
            .join(" "),
        MemoryDisplayType::I32Hex => bytes
            .chunks_exact(4)
            .map(|chunk| {
                format!(
                    "{:08X}",
                    u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                )
            })
            .collect::<Vec<_>>()
            .join(" "),
        MemoryDisplayType::I32Decimal => bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]).to_string())
            .collect::<Vec<_>>()
            .join(" "),
        MemoryDisplayType::I64Hex => bytes
            .chunks_exact(8)
            .map(|chunk| format!("{:016X}", u64::from_le_bytes(chunk.try_into().unwrap())))
            .collect::<Vec<_>>()
            .join(" "),
        MemoryDisplayType::I64Decimal => bytes
            .chunks_exact(8)
            .map(|chunk| u64::from_le_bytes(chunk.try_into().unwrap()).to_string())
            .collect::<Vec<_>>()
            .join(" "),
        MemoryDisplayType::Float => bytes
            .chunks_exact(4)
            .map(|chunk| {
                format_compact_float(
                    f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64,
                    6,
                )
            })
            .collect::<Vec<_>>()
            .join(" "),
        MemoryDisplayType::Double => bytes
            .chunks_exact(8)
            .map(|chunk| format_compact_float(f64::from_le_bytes(chunk.try_into().unwrap()), 10))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn editable_scan_value(value: ScanValue, hex: bool) -> String {
    let formatted = format_scan_value(value, hex);
    formatted
        .strip_prefix("0x")
        .unwrap_or(&formatted)
        .to_owned()
}

fn decode_f32_matrix(bytes: &[u8]) -> Option<[f32; 16]> {
    if bytes.len() < 64 {
        return None;
    }
    let mut matrix = [0.0f32; 16];
    for (index, value) in matrix.iter_mut().enumerate() {
        let start = index * 4;
        *value = f32::from_le_bytes(bytes[start..start + 4].try_into().ok()?);
        if !value.is_finite() {
            return None;
        }
    }
    Some(matrix)
}

const PROJECTION_CONVENTIONS: [(&str, [usize; 3], bool, bool); 24] = [
    ("Row XYZ", [0, 1, 2], false, false),
    ("Row XYZ / flip Y", [0, 1, 2], false, true),
    ("Column XYZ", [0, 1, 2], true, false),
    ("Column XYZ / flip Y", [0, 1, 2], true, true),
    ("Row XZY", [0, 2, 1], false, false),
    ("Row XZY / flip Y", [0, 2, 1], false, true),
    ("Column XZY", [0, 2, 1], true, false),
    ("Column XZY / flip Y", [0, 2, 1], true, true),
    ("Row YXZ", [1, 0, 2], false, false),
    ("Row YXZ / flip Y", [1, 0, 2], false, true),
    ("Column YXZ", [1, 0, 2], true, false),
    ("Column YXZ / flip Y", [1, 0, 2], true, true),
    ("Row YZX", [1, 2, 0], false, false),
    ("Row YZX / flip Y", [1, 2, 0], false, true),
    ("Column YZX", [1, 2, 0], true, false),
    ("Column YZX / flip Y", [1, 2, 0], true, true),
    ("Row ZXY", [2, 0, 1], false, false),
    ("Row ZXY / flip Y", [2, 0, 1], false, true),
    ("Column ZXY", [2, 0, 1], true, false),
    ("Column ZXY / flip Y", [2, 0, 1], true, true),
    ("Row ZYX", [2, 1, 0], false, false),
    ("Row ZYX / flip Y", [2, 1, 0], false, true),
    ("Column ZYX", [2, 1, 0], true, false),
    ("Column ZYX / flip Y", [2, 1, 0], true, true),
];

fn project_world_variants(
    matrix: &[f32; 16],
    world: [f32; 3],
    width: f32,
    height: f32,
) -> [Option<[f32; 2]>; 24] {
    let mut results = [None; 24];
    for (index, (_, order, column, flip_y)) in PROJECTION_CONVENTIONS.iter().enumerate() {
        let [x, y, z] = [world[order[0]], world[order[1]], world[order[2]]];
        let (clip_x, clip_y, clip_w) = if *column {
            (
                x * matrix[0] + y * matrix[1] + z * matrix[2] + matrix[3],
                x * matrix[4] + y * matrix[5] + z * matrix[6] + matrix[7],
                x * matrix[12] + y * matrix[13] + z * matrix[14] + matrix[15],
            )
        } else {
            (
                x * matrix[0] + y * matrix[4] + z * matrix[8] + matrix[12],
                x * matrix[1] + y * matrix[5] + z * matrix[9] + matrix[13],
                x * matrix[3] + y * matrix[7] + z * matrix[11] + matrix[15],
            )
        };
        if !clip_w.is_finite() || clip_w.abs() <= 1.0e-4 {
            continue;
        }
        let ndc_x = clip_x / clip_w;
        let ndc_y = clip_y / clip_w;
        let screen_x = (ndc_x + 1.0) * 0.5 * width;
        let screen_y = if *flip_y {
            (ndc_y + 1.0) * 0.5 * height
        } else {
            (1.0 - ndc_y) * 0.5 * height
        };
        if screen_x.is_finite() && screen_y.is_finite() {
            results[index] = Some([screen_x, screen_y]);
        }
    }
    results
}

fn best_camera_projection(
    candidates: &[ViewProjectionCandidate],
    world: [f32; 3],
    width: f32,
    height: f32,
    target: [f32; 2],
) -> Option<(usize, usize, [f32; 2], f32)> {
    candidates
        .iter()
        .enumerate()
        .flat_map(|(candidate_index, candidate)| {
            project_world_variants(&candidate.matrix, world, width, height)
                .into_iter()
                .enumerate()
                .filter_map(move |(variant, point)| {
                    let point = point?;
                    let margin_x = width * 0.25;
                    let margin_y = height * 0.25;
                    ((-margin_x..=width + margin_x).contains(&point[0])
                        && (-margin_y..=height + margin_y).contains(&point[1]))
                    .then(|| {
                        let error = (point[0] - target[0]).hypot(point[1] - target[1]);
                        (candidate_index, variant, point, error)
                    })
                })
        })
        .min_by(|left, right| left.3.total_cmp(&right.3))
}

fn parse_memory_address(text: &str) -> Option<usize> {
    let compact = text.trim().replace([' ', '_'], "");
    let operator = compact
        .char_indices()
        .skip(1)
        .find(|(_, character)| matches!(character, '+' | '-'));
    let Some((mut position, _)) = operator else {
        return parse_memory_address_term(&compact);
    };
    let mut address = parse_memory_address_term(&compact[..position])?;
    while position < compact.len() {
        let operation = compact.as_bytes()[position];
        let start = position + 1;
        position = compact[start..]
            .char_indices()
            .find(|(_, character)| matches!(character, '+' | '-'))
            .map_or(compact.len(), |(next, _)| start + next);
        let offset = parse_hex_offset(&compact[start..position])?;
        address = if operation == b'+' {
            address.checked_add(offset)?
        } else {
            address.checked_sub(offset)?
        };
    }
    Some(address)
}

fn parse_pointer_expression(text: &str) -> Option<(String, usize, Vec<usize>)> {
    let text = text.trim();
    let offsets_start = text.rfind('[')?;
    let offsets_text = text.get(offsets_start + 1..)?.strip_suffix(']')?;
    let (module, module_offset) = text[..offsets_start].trim().rsplit_once('+')?;
    let module = module.trim();
    if module.is_empty() {
        return None;
    }
    let offsets = offsets_text
        .split([',', ';'])
        .map(parse_hex_offset)
        .collect::<Option<Vec<_>>>()?;
    if offsets.is_empty() {
        return None;
    }
    Some((module.to_owned(), parse_hex_offset(module_offset)?, offsets))
}

fn parse_memory_address_term(text: &str) -> Option<usize> {
    let (digits, radix) = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .map_or_else(
            || {
                if text
                    .chars()
                    .any(|character| character.is_ascii_alphabetic())
                {
                    (text, 16)
                } else {
                    (text, 10)
                }
            },
            |digits| (digits, 16),
        );
    usize::from_str_radix(digits, radix).ok()
}

fn format_memory_address(address: usize) -> String {
    // Eight digits align common low addresses without filling x64 values with leading zeroes.
    format!("{address:08X}")
}

fn format_prefixed_memory_address(address: usize) -> String {
    format!("0x{}", format_memory_address(address))
}

fn parse_hex_offset(text: &str) -> Option<usize> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    usize::from_str_radix(digits, 16).ok()
}

fn parse_signed_hex_offset(text: &str) -> Option<isize> {
    let text = text.trim();
    let (negative, digits) = text
        .strip_prefix('-')
        .map_or((false, text), |digits| (true, digits));
    let digits = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
        .unwrap_or(digits);
    let value = isize::from_str_radix(digits, 16).ok()?;
    Some(if negative { -value } else { value })
}

#[cfg(windows)]
fn instruction_memory_displacement(instruction: &str) -> isize {
    let Some(open) = instruction.find('[') else {
        return 0;
    };
    let Some(close) = instruction[open + 1..].find(']') else {
        return 0;
    };
    let expression = &instruction[open + 1..open + 1 + close];
    let Some((index, negative)) = expression
        .char_indices()
        .rev()
        .find_map(|(index, character)| match character {
            '+' => Some((index, false)),
            '-' => Some((index, true)),
            _ => None,
        })
    else {
        return 0;
    };
    let mut digits = expression[index + 1..].trim();
    if let Some(stripped) = digits
        .strip_suffix('h')
        .or_else(|| digits.strip_suffix('H'))
    {
        digits = stripped;
    }
    let Ok(value) = isize::from_str_radix(
        digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0X"))
            .unwrap_or(digits),
        16,
    ) else {
        return 0;
    };
    if negative { -value } else { value }
}

#[cfg(windows)]
fn tracked_object_signature(
    pid: u32,
    captured_address: usize,
    instruction: &str,
) -> Option<String> {
    let displacement = instruction_memory_displacement(instruction);
    let object_base = captured_address.checked_add_signed(-displacement)?;
    let pointer_width = process_pointer_width(pid).ok()?;
    let modules = process_modules(pid).ok()?;
    let read_pointer = |address: usize| -> Option<usize> {
        match pointer_width {
            4 => match read_scan_value(pid, address, ScanValueType::I32).ok()? {
                ScanValue::I32(value) => Some(value as u32 as usize),
                _ => None,
            },
            8 => match read_scan_value(pid, address, ScanValueType::I64).ok()? {
                ScanValue::I64(value) => Some(value as usize),
                _ => None,
            },
            _ => None,
        }
    };
    let module_signature = |pointer: usize| {
        modules.iter().find_map(|(module, base, size)| {
            (*base..base.saturating_add(*size))
                .contains(&pointer)
                .then(|| format!("{module}+{:X}", pointer - *base))
        })
    };
    let mut direct = Vec::new();
    for slot in (0..0x400usize).step_by(pointer_width) {
        let Some(address) = object_base.checked_add(slot) else {
            continue;
        };
        let Some(pointer) = read_pointer(address) else {
            continue;
        };
        if let Some(signature) = module_signature(pointer) {
            direct.push(format!("{slot:X}={signature}"));
            if direct.len() == 3 {
                break;
            }
        }
    }
    if !direct.is_empty() {
        return Some(direct.join(";"));
    }
    // Some Unity/IL2CPP objects keep their module pointer one object deeper.
    // Store the short slot path so the same nested object can be found after ASLR.
    let mut nested = Vec::new();
    for root_slot in (0..0x400usize).step_by(pointer_width) {
        let Some(root_address) = object_base.checked_add(root_slot) else {
            continue;
        };
        let Some(nested_base) = read_pointer(root_address) else {
            continue;
        };
        if nested_base == 0 {
            continue;
        }
        for nested_slot in (0..0x100usize).step_by(pointer_width) {
            let Some(nested_address) = nested_base.checked_add(nested_slot) else {
                continue;
            };
            let Some(pointer) = read_pointer(nested_address) else {
                continue;
            };
            if let Some(signature) = module_signature(pointer) {
                nested.push(format!("{root_slot:X}>{nested_slot:X}={signature}"));
                if nested.len() == 3 {
                    return Some(nested.join(";"));
                }
            }
        }
    }
    if !nested.is_empty() {
        return Some(nested.join(";"));
    }
    None
}

fn resolve_memory_address(
    pid: u32,
    base: usize,
    pointer: Option<&PointerSpec>,
) -> std::io::Result<usize> {
    let Some(pointer) = pointer else {
        return Ok(base);
    };
    #[cfg(windows)]
    let mut address = pointer
        .module
        .as_ref()
        .map_or(Ok(pointer.base), |(module, offset)| {
            resolve_module_offset(pid, module, *offset)
        })?;
    #[cfg(not(windows))]
    let mut address = pointer.base;
    for offset in &pointer.offsets {
        let value = read_scan_value(pid, address, ScanValueType::I64)?;
        let ScanValue::I64(next) = value else {
            unreachable!();
        };
        address = (next as usize).checked_add(*offset).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "pointer overflow")
        })?;
    }
    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_supported_scan_types() {
        assert_eq!(
            parse_scan_value("52", ScanValueType::I32, false),
            Some(ScanValue::I32(52))
        );
        assert_eq!(
            parse_scan_value("3.5", ScanValueType::F64, false),
            Some(ScanValue::F64(3.5))
        );
        assert_eq!(
            parse_scan_value("FFFFFFFF", ScanValueType::I32, true),
            Some(ScanValue::I32(-1))
        );
        assert_eq!(
            parse_scan_value("4532324+2", ScanValueType::I32, false),
            Some(ScanValue::I32(4532326))
        );
        assert_eq!(
            parse_scan_value("3232/3", ScanValueType::I32, false),
            Some(ScanValue::I32(1077))
        );
        assert_eq!(
            parse_scan_value("443*8", ScanValueType::F64, false),
            Some(ScanValue::F64(3544.0))
        );
    }

    #[test]
    fn parses_decimal_and_hex_addresses() {
        assert_eq!(parse_memory_address("4096"), Some(4096));
        assert_eq!(parse_memory_address("0x1000"), Some(4096));
        assert_eq!(parse_memory_address("7FF6_ABCD"), Some(0x7FF6_ABCD));
        assert_eq!(parse_memory_address("0x1000+10-8"), Some(0x1008));
        assert_eq!(parse_signed_hex_offset("C"), Some(12));
        assert_eq!(parse_signed_hex_offset("-0x10"), Some(-16));
        assert_eq!(
            normalize_instruction("MOVUPS [RDI+288h], XMM0"),
            normalize_instruction("movups [rdi+288h],xmm0")
        );
    }

    #[test]
    fn parses_pasted_pointer_expression() {
        assert_eq!(
            parse_pointer_expression("the-hust-banhmi.exe+6389C60 [258, 180, 354]"),
            Some((
                "the-hust-banhmi.exe".to_owned(),
                0x6389C60,
                vec![0x258, 0x180, 0x354],
            ))
        );
    }

    #[test]
    fn decodes_x86_and_x64_class_pointers() {
        assert_eq!(
            decode_pointer(&0x1234_5678u32.to_le_bytes()),
            Some(0x1234_5678)
        );
        assert_eq!(
            decode_pointer(&0x1234_5678_9ABC_DEF0u64.to_le_bytes()),
            Some(0x1234_5678_9ABC_DEF0)
        );
    }

    #[test]
    fn auto_dissect_recognizes_aligned_x86_pointer() {
        let mut bytes = vec![0; 8];
        bytes[..4].copy_from_slice(&0x0040_1000u32.to_le_bytes());
        let elements = auto_structure_elements(&bytes, 4);
        assert_eq!(elements[0].value_type, StructureElementType::Pointer);
        assert_eq!(elements[0].value_type.width(4), 4);
    }

    #[test]
    fn demangles_basic_msvc_rtti_class_names() {
        assert_eq!(
            demangle_msvc_type_name(".?AVCamera@Game@@").as_deref(),
            Some("Game::Camera")
        );
        assert_eq!(demangle_msvc_type_name("not-rtti"), None);
    }

    #[test]
    fn modifier_capture_keeps_full_combo_while_keys_are_released() {
        let mut pending = hotkey::parse_binding("Ctrl+Shift");
        update_pending_modifier_capture(&mut pending, hotkey::parse_binding("Ctrl").unwrap());
        assert_eq!(hotkey::format_binding(pending.as_ref()), "Ctrl+Shift");
    }
}
