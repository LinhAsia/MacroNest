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
    model::{HotkeyBinding, MemoryCodeEntry, MemoryDebuggerArchitecture, MemoryDebuggerMethod, MemoryPointerEntry},
    process_memory::{
        MemoryRegionInfo, PointerPath, ScanCandidate, ScanComparison, ScanValue, ScanValueType,
        filter_scan_candidates, query_memory_region, read_memory_bytes, read_scan_value,
        refresh_scan_candidates, scan_memory_with_progress, scan_pointer_paths, write_scan_value,
    },
    window_list,
};

#[cfg(windows)]
use crate::memory_debugger::debugger::{
    AccessWatch, AddressAccessWatch, WatchEvent, WriteWatch, module_offset_for_address,
    instruction_writes_memory, list_processes, process_modules, resolve_module_offset,
};

use super::CrosshairApp;

const DEFAULT_SCAN_LIMIT: usize = 10_000_000;
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
            _ => return None,
        })
    }
}

#[derive(Clone)]
struct SavedMemoryAddress {
    address: usize,
    value_type: ScanValueType,
    current: Option<ScanValue>,
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
}

struct StablePointerJobResult {
    pid: u32,
    result: Result<Vec<PointerPath>, String>,
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
    filter: String,
    exe_only: bool,
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

#[derive(Clone, Copy, PartialEq, Eq)]
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

    fn width(self) -> usize {
        match self {
            Self::Byte => 1,
            Self::I16 => 2,
            Self::I32 | Self::Float => 4,
            Self::I64 | Self::Double | Self::Pointer => 8,
        }
    }

    fn scan_type(self) -> ScanValueType {
        match self {
            Self::Float => ScanValueType::F32,
            Self::Double => ScanValueType::F64,
            Self::I64 | Self::Pointer => ScanValueType::I64,
            Self::Byte => ScanValueType::I8,
            Self::I16 => ScanValueType::I16,
            Self::I32 => ScanValueType::I32,
        }
    }
}

struct StructureElement {
    offset: usize,
    value_type: StructureElementType,
}

struct MemoryViewDialog {
    address: usize,
    kind: MemoryViewKind,
    display_type: MemoryDisplayType,
    relative_addresses: bool,
    pinned: bool,
    elements: Vec<StructureElement>,
    pending_add: Option<(usize, ScanValueType)>,
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
}

struct ScanJobResult {
    pid: u32,
    action: MemoryScanAction,
    result: Result<Vec<ScanCandidate>, String>,
}

#[derive(Clone)]
struct FreezeTarget {
    address: usize,
    value: ScanValue,
    pointer: Option<PointerSpec>,
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
        let worker_config = Arc::clone(&config);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
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
        });
        Self {
            config,
            stop,
            worker: Some(worker),
        }
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
    value_type: ScanValueType,
    value_input: String,
    hex: bool,
    result_limit_input: String,
    candidates: Vec<ScanCandidate>,
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
    hotkeys: HashMap<MemoryScanAction, HotkeyBinding>,
    hotkey_was_down: HashMap<MemoryScanAction, bool>,
    capturing_hotkey: Option<MemoryScanAction>,
    edit_value_index: Option<usize>,
    edit_value_input: String,
    edit_description_index: Option<usize>,
    address_dialog: Option<AddressDialog>,
    memory_view_dialog: Option<MemoryViewDialog>,
    memory_settings_open: bool,
    code_list_open: bool,
    stable_pointer_dialog: Option<StablePointerDialog>,
    saved_library_open: bool,
    #[cfg(windows)]
    instruction_watch_dialog: Option<InstructionWatchDialog>,
    #[cfg(windows)]
    code_access_dialog: Option<CodeAccessDialog>,
    last_refresh: Instant,
    freeze_worker: MemoryFreezeWorker,
}

impl Default for MemoryPanelState {
    fn default() -> Self {
        Self {
            process_selector: String::new(),
            process_pid: None,
            value_type: ScanValueType::I32,
            value_input: "0".to_owned(),
            hex: false,
            result_limit_input: DEFAULT_SCAN_LIMIT.to_string(),
            candidates: Vec::new(),
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
            hotkeys: HashMap::new(),
            hotkey_was_down: HashMap::new(),
            capturing_hotkey: None,
            edit_value_index: None,
            edit_value_input: String::new(),
            edit_description_index: None,
            address_dialog: None,
            memory_view_dialog: None,
            memory_settings_open: false,
            code_list_open: false,
            stable_pointer_dialog: None,
            saved_library_open: false,
            #[cfg(windows)]
            instruction_watch_dialog: None,
            #[cfg(windows)]
            code_access_dialog: None,
            last_refresh: Instant::now(),
            freeze_worker: MemoryFreezeWorker::default(),
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
            self.memory_panel.saved.retain_mut(|saved| {
                if saved.saved_to_library {
                    saved.current = None;
                    saved.frozen = None;
                    true
                } else {
                    false
                }
            });
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
            ui.label(RichText::new("Memory Scanner").strong().size(17.0));
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
                    "Unpin results"
                } else {
                    "Pin results"
                };
                if ui.button(pin_label).clicked() {
                    self.memory_panel.pinned = !self.memory_panel.pinned;
                }
                if ui.button("Memory settings").clicked() {
                    self.memory_panel.memory_settings_open = true;
                }
                if ui.button("Advanced options").clicked() {
                    self.memory_panel.code_list_open = true;
                }
                if ui.button("Saved addresses").clicked() {
                    self.memory_panel.saved_library_open = true;
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

        self.render_memory_address_dialog(ui.ctx());
        self.render_memory_view_dialog(ui.ctx());
        self.render_memory_settings(ui.ctx());
        self.render_memory_code_list(ui.ctx());
        self.render_saved_address_library(ui.ctx());
        self.render_stable_pointer_dialog(ui.ctx());
        #[cfg(windows)]
        self.render_instruction_watch_dialog(ui.ctx());
        #[cfg(windows)]
        self.render_code_access_dialog(ui.ctx());
        self.sync_memory_freeze_targets();
    }

    pub(crate) fn render_memory_pinned_viewport(&mut self, ctx: &egui::Context) {
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
                            self.memory_panel.candidates.len()
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

    fn render_memory_popup_resize_handles(ctx: &egui::Context) {
        let rect = ctx.content_rect();
        let edge = 8.0;
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
                    if ui.add_sized([58.0, 28.0], Button::new("Unpin")).clicked() {
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
        let size = ui.available_size();
        Frame::group(ui.style())
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_min_size(size - vec2(18.0, 18.0));
                ui.label(RichText::new("Scan").strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let process_label = if let Some(label) = self
                        .memory_panel
                        .process_selector
                        .strip_prefix("pid:")
                        .and_then(|value| value.split_once(':').map(|(_, name)| name))
                    {
                        format!("{label} — PID {}", self.memory_panel.process_pid.unwrap_or_default())
                    } else {
                        self.open_window_infos
                            .iter()
                            .find(|window| window.selector == self.memory_panel.process_selector)
                            .map(|window| Self::simplify_window_title(&window.title))
                            .unwrap_or_else(|| "Select process".to_owned())
                    };
                    let process_combo = egui::ComboBox::from_id_salt("memory-process")
                        .width(ui.available_width())
                        .selected_text(Self::truncate_window_title(&process_label, 52))
                        .show_ui(ui, |ui| {
                            ui.label(RichText::new("Window processes (grouped)").strong());
                            for window in self.open_window_infos.clone() {
                                let selected =
                                    window.selector == self.memory_panel.process_selector;
                                if ui
                                    .selectable_label(
                                        selected,
                                        Self::truncate_window_title(
                                            &Self::simplify_window_title(&window.title),
                                            70,
                                        ),
                                    )
                                    .clicked()
                                {
                                    let selector = window.selector;
                                    self.memory_panel.process_selector = selector.clone();
                                    let pid = window_list::process_id_for_window(Some(&selector));
                                    if self.memory_panel.process_pid != pid {
                                        self.reset_memory_scan("Process changed");
                                        self.memory_panel.saved.retain_mut(|saved| {
                                            let portable = saved
                                                .pointer
                                                .as_ref()
                                                .is_some_and(|pointer| pointer.module.is_some());
                                            if portable {
                                                saved.current = None;
                                                saved.frozen = None;
                                            }
                                            portable
                                        });
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
                            }
                            #[cfg(windows)]
                            if let Ok(processes) = list_processes() {
                                ui.separator();
                                ui.label(RichText::new("All processes (individual PID)").strong());
                                for (pid, name) in processes {
                                    if ui
                                        .selectable_label(
                                            self.memory_panel.process_pid == Some(pid),
                                            format!("{name} — PID {pid}"),
                                        )
                                        .clicked()
                                    {
                                        self.select_memory_process(
                                            format!("pid:{pid}:{name}"),
                                            Some(pid),
                                            ui.ctx(),
                                        );
                                    }
                                }
                            }
                        });
                    if process_combo.response.clicked() {
                        self.ensure_open_windows_ready(true);
                    }
                });
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("memory-value-type")
                        .width(90.0)
                        .selected_text(memory_type_label(self.memory_panel.value_type))
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
                                    .selectable_value(
                                        &mut self.memory_panel.value_type,
                                        value_type,
                                        memory_type_label(value_type),
                                    )
                                    .changed()
                                {
                                    self.reset_memory_scan("Value type changed");
                                }
                            }
                        });
                    let value_response = ui.add(
                        egui::TextEdit::singleline(&mut self.memory_panel.value_input)
                            .desired_width(120.0)
                            .hint_text("value"),
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
                    ui.checkbox(&mut self.memory_panel.hex, "Hex");
                });
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
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Limit");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.memory_panel.result_limit_input)
                            .desired_width(110.0),
                    );
                });
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.memory_panel.manual_address)
                            .desired_width(180.0)
                            .hint_text("address / 0x..."),
                    );
                    if ui.button("Add address").clicked() {
                        self.add_manual_memory_address();
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
                                        Button::new("Reset")
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
        ui.horizontal(|ui| {
            let enabled = !self.memory_panel.scanning
                && self.memory_panel.process_pid.is_some()
                && (matches!(
                    action,
                    MemoryScanAction::FirstScan | MemoryScanAction::Unknown
                ) || !self.memory_panel.candidates.is_empty());
            if ui
                .add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [(width - if hotkey { 34.0 } else { 0.0 }).max(52.0), 26.0],
                        Button::new(action.label()),
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
                        self.memory_panel.hotkey_was_down.remove(&action);
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

    fn render_memory_scan_results(&mut self, ui: &mut egui::Ui, pinned: bool) {
        let size = ui.available_size();
        let frame = Frame::group(ui.style()).inner_margin(egui::Margin::same(5));
        frame.show(ui, |ui| {
            ui.set_min_size(size - vec2(12.0, 12.0));
            if !pinned {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Scan results").strong());
                    ui.label(format!("{}", self.memory_panel.candidates.len()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Add ↓").clicked() {
                            self.add_selected_memory_results();
                        }
                    });
                });
            }
            let result_column_width =
                ((ui.available_width() - if pinned { 0.0 } else { 25.0 }) / 3.0).max(80.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if !pinned {
                    ui.add_space(22.0);
                }
                Self::memory_table_cell(ui, result_column_width, RichText::new("Address").strong());
                Self::memory_table_cell(ui, result_column_width, RichText::new("Current").strong());
                Self::memory_table_cell(
                    ui,
                    result_column_width,
                    RichText::new("Previous").strong(),
                );
            });
            ui.separator();
            let visible_count = self.memory_panel.candidates.len().min(MAX_VISIBLE_RESULTS);
            if !pinned
                && !self.memory_panel.saved_list_active
                && ui.ctx().memory(|memory| memory.focused().is_none())
                && ui.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::A))
            {
                self.memory_panel.selected_results = (0..visible_count).collect();
            }
            if self.memory_panel.candidates.is_empty() && !self.memory_panel.scanning {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("No scan results").weak());
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
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for index in rows {
                        let candidate = self.memory_panel.candidates[index];
                        let selected = self.memory_panel.selected_results.contains(&index);
                        let marked = self
                            .memory_panel
                            .marked_result_addresses
                            .contains(&candidate.address);
                        let row_width = ui.available_width();
                        let full_row_rect = egui::Rect::from_min_size(
                            ui.next_widget_position(),
                            vec2(row_width, 22.0),
                        );
                        let response = ui
                            .interact(
                                full_row_rect,
                                ui.id()
                                    .with(("memory-result-row", pinned, candidate.address)),
                                Sense::click(),
                            )
                            .on_hover_cursor(egui::CursorIcon::Default);
                        ui.allocate_ui_with_layout(
                            vec2(row_width, 22.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if !pinned {
                                    ui.add_space(3.0);
                                    let mut checked = selected;
                                    if ui.checkbox(&mut checked, "").changed() {
                                        self.select_memory_result(index, checked, ui);
                                    }
                                }
                                Self::memory_table_cell(
                                    ui,
                                    result_column_width,
                                    RichText::new(format!("0x{:016X}", candidate.address))
                                        .monospace(),
                                );
                                Self::memory_table_cell(
                                    ui,
                                    result_column_width,
                                    RichText::new(format_scan_value(
                                        candidate.current,
                                        self.memory_panel.hex,
                                    ))
                                    .monospace(),
                                );
                                Self::memory_table_cell(
                                    ui,
                                    result_column_width,
                                    RichText::new(format_scan_value(
                                        candidate.previous,
                                        self.memory_panel.hex,
                                    ))
                                    .monospace(),
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
                                        .remove(&candidate.address);
                                } else {
                                    self.memory_panel
                                        .marked_result_addresses
                                        .insert(candidate.address);
                                }
                                ui.close();
                            }
                        });
                        if !pinned && response.clicked() && !response.double_clicked() {
                            let toggle =
                                ui.input(|input| input.modifiers.ctrl || input.modifiers.command);
                            self.select_memory_result(
                                index,
                                if toggle { !selected } else { true },
                                ui,
                            );
                        }
                        if ui.input(|input| {
                            input
                                .pointer
                                .button_double_clicked(egui::PointerButton::Primary)
                        }) && ui
                            .ctx()
                            .pointer_latest_pos()
                            .is_some_and(|pointer| full_row_rect.contains(pointer))
                        {
                            self.memory_panel.selected_results.clear();
                            self.memory_panel.selected_results.insert(index);
                            self.add_selected_memory_results();
                        }
                    }
                });
            if self.memory_panel.candidates.len() > MAX_VISIBLE_RESULTS {
                ui.label(
                    RichText::new(format!(
                        "Showing first {MAX_VISIBLE_RESULTS} of {}",
                        self.memory_panel.candidates.len()
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
        cell_response.union(cell.add(label))
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
                    ui.label(RichText::new("Address list").strong());
                    let selected = self.memory_panel.selected_saved.len();
                    if selected > 0 {
                        if ui.button(format!("Write selected ({selected})")).clicked() {
                            self.write_selected_saved_memory();
                        }
                        if ui.button(format!("Freeze selected ({selected})")).clicked() {
                            self.freeze_selected_saved_memory();
                        }
                        if ui.button("Delete").clicked() {
                            self.delete_selected_saved_memory();
                        }
                    }
                });
                ui.separator();
                let editing = self.memory_panel.edit_value_index.is_some()
                    || self.memory_panel.edit_description_index.is_some();
                if self.memory_panel.saved_list_active
                    && !editing
                    && ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::A))
                {
                    self.memory_panel.selected_saved = (0..self.memory_panel.saved.len()).collect();
                }
                if self.memory_panel.saved_list_active
                    && !editing
                    && ui.input(|input| input.key_pressed(egui::Key::Delete))
                {
                    self.delete_selected_saved_memory();
                }
                let header_column_width = ((ui.available_width() - 42.0) / 4.0).max(80.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(21.0);
                    Self::memory_table_cell(
                        ui,
                        header_column_width,
                        RichText::new("Address").strong(),
                    );
                    Self::memory_table_cell(
                        ui,
                        header_column_width,
                        RichText::new("Type").strong(),
                    );
                    Self::memory_table_cell(
                        ui,
                        header_column_width,
                        RichText::new("Value").strong(),
                    );
                    Self::memory_table_cell(
                        ui,
                        header_column_width,
                        RichText::new("Description").strong(),
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
                        // ponytail: saved addresses are user-managed; direct rows avoid stale
                        // virtualized hitboxes when a cell switches between label and TextEdit.
                        for index in 0..count {
                            if index >= self.memory_panel.saved.len() {
                                continue;
                            }
                            let saved = self.memory_panel.saved[index].clone();
                            let selected = self.memory_panel.selected_saved.contains(&index);
                            let mut open_address = false;
                            let mut edit_value = false;
                            let mut delete = false;
                            let mut instruction_watch = None;
                            let mut find_stable_pointer = false;
                            let mut save_to_library = false;
                            let mut persist_pointer_changes = false;
                            let mut row_hits = Vec::new();
                            let mut checkbox_changed = false;
                            let row_width = ui.available_width();
                            let column_width = ((row_width - 42.0) / 4.0).max(80.0);
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
                                    let mut checked = selected;
                                    let checked_response = ui.add_sized(
                                        [18.0, 18.0],
                                        egui::Checkbox::without_text(&mut checked),
                                    );
                                    row_hits.push(checked_response.clone());
                                    if checked_response.changed() {
                                        checkbox_changed = true;
                                        if checked {
                                            self.memory_panel.selected_saved.insert(index);
                                        } else {
                                            self.memory_panel.selected_saved.remove(&index);
                                        }
                                    }
                                    let address_response = Self::memory_label_cell(
                                        ui,
                                        column_width,
                                        row_height,
                                        egui::Label::new(format!("0x{:016X}", saved.address))
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
                                        egui::Label::new(memory_type_label(saved.value_type))
                                            .selectable(false)
                                            .truncate(),
                                    );
                                    type_response
                                        .clone()
                                        .on_hover_cursor(egui::CursorIcon::Default);
                                    row_hits.push(type_response);
                                    if self.memory_panel.edit_value_index == Some(index) {
                                        let response = ui.add_sized(
                                            [column_width, 20.0],
                                            egui::TextEdit::singleline(
                                                &mut self.memory_panel.edit_value_input,
                                            ),
                                        );
                                        response.request_focus();
                                        row_hits.push(response.clone());
                                        if response.clicked_elsewhere()
                                            || (response.has_focus()
                                                && ui.input(|input| {
                                                    input.key_pressed(egui::Key::Enter)
                                                }))
                                        {
                                            self.commit_saved_memory_value(index);
                                        }
                                        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                                            self.memory_panel.edit_value_index = None;
                                        }
                                    } else {
                                        let value_response = Self::memory_label_cell(
                                            ui,
                                            column_width,
                                            row_height,
                                            egui::Label::new(
                                                saved
                                                    .current
                                                    .map(|value| {
                                                        format_scan_value(
                                                            value,
                                                            self.memory_panel.hex,
                                                        )
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
                                    }
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
                                        .add_sized(
                                            [18.0, 18.0],
                                            egui::Checkbox::without_text(&mut frozen),
                                        )
                                        .on_hover_text("Freeze");
                                    row_hits.push(frozen_response.clone());
                                    if frozen_response.changed() {
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
                                let column = ((pointer.x - full_row_rect.left() - 21.0)
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
                            if row_response.clicked() || checkbox_changed {
                                self.memory_panel.saved_list_active = true;
                            }
                            if row_response.clicked()
                                && !row_response.double_clicked()
                                && !checkbox_changed
                            {
                                self.select_saved_memory_row(index, selected, ui);
                            }
                            response.context_menu(|ui| {
                                if ui.button("Save address to library").clicked() {
                                    save_to_library = true;
                                    ui.close();
                                }
                                if ui
                                    .button("Find instructions accessing this address (x64)")
                                    .clicked()
                                {
                                    instruction_watch = Some(true);
                                    ui.close();
                                }
                                if ui
                                    .button("Find instructions writing this address (x64)")
                                    .clicked()
                                {
                                    instruction_watch = Some(false);
                                    ui.close();
                                }
                                if ui.button("Find stable pointer automatically").clicked() {
                                    find_stable_pointer = true;
                                    ui.close();
                                }
                                if ui.button("Browse this memory region").clicked() {
                                    self.memory_panel.memory_view_dialog = Some(MemoryViewDialog {
                                        address: saved.address,
                                        kind: MemoryViewKind::Bytes,
                                        display_type: MemoryDisplayType::ByteHex,
                                        relative_addresses: false,
                                        pinned: true,
                                        elements: default_structure_elements(),
                                        pending_add: None,
                                    });
                                    ui.close();
                                }
                                if ui.button("Dissect data/structure").clicked() {
                                    self.memory_panel.memory_view_dialog = Some(MemoryViewDialog {
                                        address: saved.address,
                                        kind: MemoryViewKind::Structure,
                                        display_type: MemoryDisplayType::ByteHex,
                                        relative_addresses: false,
                                        pinned: true,
                                        elements: default_structure_elements(),
                                        pending_add: None,
                                    });
                                    ui.close();
                                }
                                if let Some(pointer) = saved.pointer.as_ref()
                                    && pointer.module.is_some()
                                    && ui.button("Copy pointer for Macro").clicked()
                                {
                                    ui.ctx().copy_text(format_pointer_expression(pointer));
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button("Change address / Pointer").clicked() {
                                    open_address = true;
                                    ui.close();
                                }
                                if ui.button("Edit value").clicked() {
                                    edit_value = true;
                                    ui.close();
                                }
                                if ui.button("Delete").clicked() {
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
                            }
                            if save_to_library {
                                self.memory_panel.saved[index].saved_to_library = true;
                                if self.memory_panel.saved[index].description.is_empty() {
                                    self.memory_panel.saved[index].description = format!("0x{:X}", saved.address);
                                }
                                self.persist_memory_pointers();
                                self.memory_panel.status = "Address saved to library".to_owned();
                            }
                            if open_address {
                                let (address, offsets, pointer) =
                                    saved.pointer.as_ref().map_or_else(
                                        || (format!("0x{:X}", saved.address), String::new(), false),
                                        |spec| {
                                            (
                                                format!("0x{:X}", spec.base),
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
                                self.memory_panel.edit_value_index = Some(index);
                                self.memory_panel.edit_value_input = saved
                                    .current
                                    .map(|value| editable_scan_value(value, self.memory_panel.hex))
                                    .unwrap_or_default();
                            }
                            if delete {
                                self.memory_panel.saved.remove(index);
                                self.reindex_saved_selection_after_delete(index);
                                self.persist_memory_pointers();
                            }
                        }
                    });
                if self.memory_panel.saved.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("No saved addresses").weak());
                    });
                }
            });
    }

    fn render_memory_settings(&mut self, ctx: &egui::Context) {
        if !self.memory_panel.memory_settings_open {
            return;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Memory settings")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
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
            });
        self.memory_panel.memory_settings_open = open;
        if changed {
            self.persist();
        }
    }

    fn render_saved_address_library(&mut self, ctx: &egui::Context) {
        if !self.memory_panel.saved_library_open {
            return;
        }
        let mut open = true;
        let mut load = None;
        let mut delete = None;
        egui::Window::new("Saved addresses")
            .default_size(vec2(620.0, 420.0))
            .collapsible(false)
            .open(&mut open)
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
                                for (index, entry) in self.state.memory_pointer_list.iter().enumerate() {
                                    let entry_app = if entry.app_name.is_empty() { &entry.module } else { &entry.app_name };
                                    if !entry_app.eq_ignore_ascii_case(&app) {
                                        continue;
                                    }
                                    ui.horizontal(|ui| {
                                        let address = if entry.module.is_empty() {
                                            entry.absolute_address.map_or_else(|| "Invalid address".to_owned(), |address| format!("0x{address:X}"))
                                        } else {
                                            format!("{}+{:X} [{}]", entry.module, entry.module_offset, entry.offsets.iter().map(|offset| format!("{offset:X}")).collect::<Vec<_>>().join(" → "))
                                        };
                                        ui.label(if entry.name.is_empty() { &address } else { &entry.name }).on_hover_text(&address);
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
                    ui.centered_and_justified(|ui| ui.label(RichText::new("No saved addresses").weak()));
                }
            });
        self.memory_panel.saved_library_open = open;
        if let Some(index) = load
            && let Some(entry) = self.state.memory_pointer_list.get(index).cloned()
            && let Some(value_type) = memory_type_from_config(&entry.value_type)
        {
            let mut pointer = (!entry.module.is_empty()).then(|| PointerSpec {
                base: 0,
                module: Some((entry.module.clone(), entry.module_offset)),
                offsets: entry.offsets.clone(),
            });
            let mut address = entry.absolute_address.unwrap_or_default();
            if let (Some(pid), Some(pointer)) = (self.memory_panel.process_pid, pointer.as_mut())
                && let Ok(base) = resolve_module_offset(pid, &entry.module, entry.module_offset)
            {
                pointer.base = base;
                address = resolve_memory_address(pid, base, Some(pointer)).unwrap_or_default();
            }
            let current = self.memory_panel.process_pid.and_then(|pid| read_scan_value(pid, address, value_type).ok());
            self.memory_panel.saved.push(SavedMemoryAddress {
                address,
                value_type,
                current,
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
        let mut open = true;
        let mut start = None;
        let mut delete = None;
        egui::Window::new("Advanced options — Code list")
            .default_size(vec2(720.0, 360.0))
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    Self::memory_view_cell(ui, 190.0, "Module + offset");
                    Self::memory_view_cell(ui, 310.0, "Instruction");
                    Self::memory_view_cell(ui, 150.0, "Action");
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, entry) in self.state.memory_code_list.iter().enumerate() {
                        ui.horizontal(|ui| {
                            Self::memory_view_cell(
                                ui,
                                190.0,
                                &format!("{}+{:X}", entry.module, entry.offset),
                            );
                            Self::memory_view_cell(ui, 310.0, &entry.instruction);
                            let action = if entry.writes {
                                "Find written addresses"
                            } else {
                                "Find accessed addresses"
                            };
                            if ui.button(action).clicked() {
                                start = Some(index);
                            }
                            if ui.small_button("Delete").clicked() {
                                delete = Some(index);
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
        self.memory_panel.code_list_open = open;
        if let Some(index) = delete {
            self.state.memory_code_list.remove(index);
            self.persist();
        }
        #[cfg(windows)]
        if let Some(index) = start {
            self.open_code_access_watch(index);
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
        });
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
        let Some(entry) = self.state.memory_code_list.get(code_index) else {
            return;
        };
        let instruction_address = match resolve_module_offset(pid, &entry.module, entry.offset) {
            Ok(address) => address,
            Err(error) => {
                self.memory_panel.status = format!("Unable to resolve saved code: {error}");
                return;
            }
        };
        if let Some(dialog) = self.memory_panel.code_access_dialog.as_mut()
            && let Some(active) = dialog.active.as_mut()
        {
            active.stop();
        }
        let (tx, rx) = mpsc::channel();
        let started = AccessWatch::start(pid, instruction_address, self.state.memory_debugger_architecture, move |event| {
            let _ = tx.send(event);
        });
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
        });
    }

    #[cfg(windows)]
    fn start_stable_pointer_scan(&mut self, saved: &SavedMemoryAddress) {
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        let Some(expected_value) = saved.current else {
            self.memory_panel.status = "Unable to read this address".to_owned();
            return;
        };
        let modules = match process_modules(pid) {
            Ok(modules) => modules,
            Err(error) => {
                self.memory_panel.status = format!("Unable to list process modules: {error}");
                return;
            }
        };
        let target = saved.address;
        let progress = Arc::new(AtomicUsize::new(0));
        let worker_progress = Arc::clone(&progress);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = scan_pointer_paths(pid, target, &modules, 0x1000, 5, 128, worker_progress)
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
            filter: String::new(),
            exe_only: false,
        });
    }

    #[cfg(not(windows))]
    fn start_stable_pointer_scan(&mut self, _saved: &SavedMemoryAddress) {
        self.memory_panel.status = "Stable pointer scan is available on Windows only".to_owned();
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
                            })
                            .collect();
                        dialog.candidates.sort_by_key(|candidate| {
                            !candidate.path.module.to_ascii_lowercase().ends_with(".exe")
                        });
                        dialog.selected = (!dialog.candidates.is_empty()).then_some(0);
                        dialog.status = if dialog.candidates.is_empty() {
                            format!(
                                "No module-based pointer paths found after reading {:.1} MB (5 levels, max offset 0x1000)",
                                dialog.progress.load(Ordering::Relaxed) as f64 / 1_048_576.0
                            )
                        } else {
                            format!(
                                "{} candidate(s). Restart the game, restore the target value, select the new process, then Validate.",
                                dialog.candidates.len()
                            )
                        };
                    }
                    Err(error) => dialog.status = format!("Pointer scan failed: {error}"),
                }
            }
        }

        let mut open = true;
        let mut validate = false;
        let mut add = false;
        egui::Window::new(format!(
            "Find stable pointer — 0x{:X}",
            dialog.source_address
        ))
        .default_size(vec2(720.0, 430.0))
        .min_size(vec2(520.0, 300.0))
        .collapsible(false)
        .open(&mut open)
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
                        new_process && !dialog.candidates.is_empty(),
                        Button::new("Validate after restart"),
                    )
                    .clicked()
                {
                    validate = true;
                }
                if ui
                    .add_enabled(
                        dialog.selected.is_some(),
                        Button::new("Save selected pointer"),
                    )
                    .clicked()
                {
                    add = true;
                }
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.filter)
                        .desired_width(150.0)
                        .hint_text(RichText::new("Search module...").weak()),
                );
                ui.checkbox(&mut dialog.exe_only, "EXE only");
            });
            ui.separator();
            const STATUS_WIDTH: f32 = 108.0;
            const ROOT_WIDTH: f32 = 195.0;
            const OFFSETS_WIDTH: f32 = 170.0;
            const ADDRESS_WIDTH: f32 = 145.0;
            const VALUE_WIDTH: f32 = 92.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (width, title) in [
                    (STATUS_WIDTH, "Status"),
                    (ROOT_WIDTH, "Root"),
                    (OFFSETS_WIDTH, "Offsets"),
                    (ADDRESS_WIDTH, "Resolved"),
                    (VALUE_WIDTH, "Value"),
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
            egui::ScrollArea::both().show(ui, |ui| {
                ui.set_min_width(
                    STATUS_WIDTH + ROOT_WIDTH + OFFSETS_WIDTH + ADDRESS_WIDTH + VALUE_WIDTH,
                );
                let filter = dialog.filter.trim().to_ascii_lowercase();
                for (index, candidate) in dialog.candidates.iter().enumerate() {
                    let module_lower = candidate.path.module.to_ascii_lowercase();
                    if (dialog.exe_only && !module_lower.ends_with(".exe"))
                        || (!filter.is_empty() && !module_lower.contains(&filter))
                    {
                        continue;
                    }
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
                        .map_or_else(|| "—".to_owned(), |address| format!("0x{address:X}"));
                    let value = candidate
                        .observed_value
                        .map_or_else(|| "—".to_owned(), |value| editable_scan_value(value, false));
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
                    if response.clicked() {
                        dialog.selected = Some(index);
                    }
                }
            });
        });

        if validate {
            self.validate_stable_pointer_candidates(&mut dialog);
        }
        if add {
            self.add_stable_pointer_candidate(&dialog);
        }
        if open {
            if dialog.rx.is_some() {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            self.memory_panel.stable_pointer_dialog = Some(dialog);
        }
    }

    #[cfg(windows)]
    fn validate_stable_pointer_candidates(&mut self, dialog: &mut StablePointerDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            return;
        };
        let mut valid = 0usize;
        let mut changed = 0usize;
        let mut broken = 0usize;
        for candidate in &mut dialog.candidates {
            candidate.resolved_base = None;
            candidate.resolved_address = None;
            candidate.observed_value = None;
            let Ok(base) =
                resolve_module_offset(pid, &candidate.path.module, candidate.path.module_offset)
            else {
                candidate.valid = Some(false);
                broken += 1;
                continue;
            };
            let spec = PointerSpec {
                base,
                module: Some((candidate.path.module.clone(), candidate.path.module_offset)),
                offsets: candidate.path.offsets.clone(),
            };
            candidate.resolved_base = Some(base);
            let Ok(address) = resolve_memory_address(pid, base, Some(&spec)) else {
                candidate.valid = Some(false);
                broken += 1;
                continue;
            };
            candidate.resolved_address = Some(address);
            let Ok(observed) = read_scan_value(pid, address, dialog.value_type) else {
                candidate.valid = Some(false);
                broken += 1;
                continue;
            };
            candidate.observed_value = Some(observed);
            if observed == dialog.expected_value {
                candidate.valid = Some(true);
                valid += 1;
            } else {
                candidate.valid = None;
                changed += 1;
            }
        }
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

    fn add_stable_pointer_candidate(&mut self, dialog: &StablePointerDialog) {
        let Some(pid) = self.memory_panel.process_pid else {
            return;
        };
        let Some(candidate) = dialog
            .selected
            .and_then(|index| dialog.candidates.get(index))
        else {
            return;
        };
        #[cfg(windows)]
        let Ok(base) =
            resolve_module_offset(pid, &candidate.path.module, candidate.path.module_offset)
        else {
            self.memory_panel.status = "Pointer module is not loaded".to_owned();
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
            self.memory_panel.status = "Unable to resolve pointer".to_owned();
            return;
        };
        let current = read_scan_value(pid, address, dialog.value_type).ok();
        self.memory_panel.saved.push(SavedMemoryAddress {
            address,
            value_type: dialog.value_type,
            current,
            description: format!(
                "{}+{:X}",
                candidate.path.module, candidate.path.module_offset
            ),
            pointer: Some(pointer),
            frozen: None,
            saved_to_library: true,
        });
        self.memory_panel.status = "Stable pointer added to Address list".to_owned();
        self.persist_memory_pointers();
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
        if let Some(dialog) = self.memory_panel.instruction_watch_dialog.as_mut()
            && let Some(active) = dialog.active.as_mut()
        {
            active.stop();
        }
        let (tx, rx) = mpsc::channel();
        let notify = move |event| {
            let _ = tx.send(event);
        };
        let started = if reads_and_writes {
            AddressAccessWatch::start(pid, address, self.state.memory_debugger_architecture, notify).map(ActiveInstructionWatch::Accesses)
        } else {
            WriteWatch::start(pid, address, self.state.memory_debugger_architecture, notify).map(ActiveInstructionWatch::Writes)
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
        });
    }

    #[cfg(windows)]
    fn render_instruction_watch_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.instruction_watch_dialog.take() else {
            return;
        };
        while let Ok(event) = dialog.rx.try_recv() {
            match event {
                WatchEvent::Started => dialog.status = "Debugger running".to_owned(),
                WatchEvent::AddressHit {
                    instruction_address,
                    instruction,
                    details,
                    ..
                } => {
                    if let Some(hit) = dialog
                        .hits
                        .iter_mut()
                        .find(|hit| hit.address == instruction_address)
                    {
                        hit.count += 1;
                    } else if dialog.hits.len() < MAX_VISIBLE_INSTRUCTIONS {
                        dialog.hits.push(InstructionHit {
                            address: instruction_address,
                            instruction,
                            details,
                            count: 1,
                        });
                        dialog.hits.sort_unstable_by_key(|hit| hit.address);
                    }
                    let total: usize = dialog.hits.iter().map(|hit| hit.count).sum();
                    dialog.status = format!("{total} hit(s), {} instruction(s)", dialog.hits.len());
                }
                WatchEvent::AccessHit { .. } => {}
                WatchEvent::Error(error) => {
                    dialog.status = format!("Debugger stopped: {error}");
                    dialog.active = None;
                }
                WatchEvent::Stopped => {
                    if !dialog.status.starts_with("Debugger stopped:") {
                        dialog.status = "Debugger stopped".to_owned();
                    }
                    dialog.active = None;
                }
            }
        }
        let mut open = true;
        let title = format!(
            "Find instructions {} — 0x{:016X}",
            if dialog.writes_only {
                "writing"
            } else {
                "accessing"
            },
            dialog.address
        );
        if dialog.pinned {
            let monitor_height = ctx
                .input(|input| input.viewport().monitor_size)
                .map_or(900.0, |size| size.y);
            let builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_position(egui::pos2(0.0, 0.0))
                .with_inner_size(vec2(760.0, monitor_height))
                .with_min_inner_size(vec2(520.0, 300.0))
                .with_clamp_size_to_monitor_size(true)
                .with_decorations(false)
                .with_resizable(true)
                .with_always_on_top();
            let mut unpin = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of((
                    "memory-instruction-watch",
                    dialog.address,
                    dialog.writes_only,
                )),
                builder,
                |ctx, _| {
                    Self::constrain_memory_popup_to_monitor(ctx);
                    if ctx.input(|input| input.viewport().close_requested()) {
                        open = false;
                    }
                    Self::render_memory_popup_titlebar(ctx, &title, &mut unpin, &mut open);
                    egui::CentralPanel::default()
                        .frame(Self::memory_popup_frame(ctx))
                        .show(ctx, |ui| {
                            Self::render_instruction_watch_body(ui, &mut dialog);
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
                    Self::render_instruction_watch_body(ui, &mut dialog);
                });
        }
        if let Some(index) = dialog.pending_code_add.take()
            && let Some(hit) = dialog.hits.get(index)
        {
            self.add_instruction_to_code_list(hit.address, &hit.instruction, dialog.writes_only);
        }
        if open {
            self.memory_panel.instruction_watch_dialog = Some(dialog);
            ctx.request_repaint_after(Duration::from_millis(35));
        } else if let Some(mut active) = dialog.active {
            active.stop();
        }
    }

    #[cfg(windows)]
    fn render_instruction_watch_body(ui: &mut egui::Ui, dialog: &mut InstructionWatchDialog) {
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(&dialog.status).selectable(true));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(dialog.selected.is_some(), Button::new("Add to code list"))
                    .clicked()
                {
                    dialog.pending_code_add = dialog.selected;
                }
                if dialog.active.is_some() && ui.button("Stop").clicked() {
                    if let Some(active) = dialog.active.as_mut() {
                        active.stop();
                    }
                    dialog.active = None;
                    dialog.status = "Debugger stopped".to_owned();
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(210.0)
            .show(ui, |ui| {
                for (index, hit) in dialog.hits.iter().enumerate() {
                    let instruction_width = (ui.available_width() - 290.0).max(180.0);
                    let response = ui
                        .horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let address = Self::memory_label_cell(
                                ui,
                                190.0,
                                22.0,
                                egui::Label::new(
                                    RichText::new(format!("0x{:016X}", hit.address)).monospace(),
                                )
                                .selectable(true),
                            );
                            let instruction = Self::memory_label_cell(
                                ui,
                                instruction_width,
                                22.0,
                                egui::Label::new(RichText::new(&hit.instruction).monospace())
                                    .selectable(true),
                            );
                            let count = Self::memory_label_cell(
                                ui,
                                100.0,
                                22.0,
                                egui::Label::new(
                                    RichText::new(format!("{} hit(s)", hit.count)).monospace(),
                                )
                                .selectable(true),
                            );
                            address.union(instruction).union(count)
                        })
                        .inner;
                    if response.clicked() {
                        dialog.selected = Some(index);
                    }
                }
            });
        ui.separator();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        dialog
                            .selected
                            .and_then(|index| dialog.hits.get(index))
                            .map(|hit| hit.details.as_str())
                            .unwrap_or("Interact with the target process to capture instructions."),
                    )
                    .monospace(),
                )
                .selectable(true),
            );
        });
    }

    #[cfg(windows)]
    fn render_code_access_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.code_access_dialog.take() else {
            return;
        };
        while let Ok(event) = dialog.rx.try_recv() {
            match event {
                WatchEvent::Started => dialog.status = "Debugger running".to_owned(),
                WatchEvent::AccessHit { data_address } => {
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
                    let total: usize = dialog.addresses.iter().map(|(_, count)| count).sum();
                    dialog.status =
                        format!("{total} hit(s), {} address(es)", dialog.addresses.len());
                }
                WatchEvent::Error(error) => {
                    dialog.status = format!("Debugger stopped: {error}");
                    dialog.active = None;
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
            || format!("Find addresses — 0x{:X}", dialog.instruction_address),
            |entry| format!("Find addresses — {}+{:X}", entry.module, entry.offset),
        );
        let mut open = true;
        let mut unpin = false;
        let mut add = None;
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
                    Self::render_memory_popup_titlebar(ctx, &title, &mut unpin, &mut open);
                    egui::CentralPanel::default()
                        .frame(Self::memory_popup_frame(ctx))
                        .show(ctx, |ui| {
                            add = Self::render_code_access_body(ui, &mut dialog);
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
                    add = Self::render_code_access_body(ui, &mut dialog);
                });
        }
        if let Some(address) = add {
            self.add_code_access_address(address, dialog.value_type);
        }
        if open {
            self.memory_panel.code_access_dialog = Some(dialog);
            ctx.request_repaint_after(Duration::from_millis(35));
        } else if let Some(mut active) = dialog.active {
            active.stop();
        }
    }

    #[cfg(windows)]
    fn render_code_access_body(ui: &mut egui::Ui, dialog: &mut CodeAccessDialog) -> Option<usize> {
        let mut add = None;
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(&dialog.status).selectable(true));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(dialog.selected.is_some(), Button::new("Add selected"))
                    .clicked()
                {
                    add = dialog
                        .selected
                        .and_then(|index| dialog.addresses.get(index))
                        .map(|(address, _)| *address);
                }
                if dialog.active.is_some() && ui.button("Stop").clicked() {
                    if let Some(active) = dialog.active.as_mut() {
                        active.stop();
                    }
                    dialog.active = None;
                    dialog.status = "Debugger stopped".to_owned();
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            Self::memory_view_cell(ui, 240.0, "Address");
            Self::memory_view_cell(ui, 120.0, "Hits");
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, (address, count)) in dialog.addresses.iter().enumerate() {
                let response = ui
                    .horizontal(|ui| {
                        Self::memory_view_cell(ui, 240.0, &format!("0x{address:X}"));
                        Self::memory_view_cell(ui, 120.0, &count.to_string());
                    })
                    .response;
                if response.clicked() {
                    dialog.selected = Some(index);
                }
                if response.double_clicked() {
                    add = Some(*address);
                }
            }
        });
        if dialog.addresses.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("Interact with the game to capture addresses");
            });
        }
        add
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
            description: String::new(),
            pointer: None,
            frozen: None,
            saved_to_library: false,
        });
        self.memory_panel.status = format!("Address 0x{address:X} added");
    }

    fn render_memory_view_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.memory_view_dialog.take() else {
            return;
        };
        let address = dialog.address;
        let kind = dialog.kind;
        let title = match kind {
            MemoryViewKind::Bytes => format!("Memory region — 0x{address:X}"),
            MemoryViewKind::Structure => format!("Dissect data/structure — 0x{address:X}"),
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
            let monitor_height = ctx
                .input(|input| input.viewport().monitor_size)
                .map_or(900.0, |size| size.y);
            let builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_position(egui::pos2(0.0, 0.0))
                .with_inner_size(vec2(720.0, monitor_height))
                .with_min_inner_size(vec2(520.0, 280.0))
                .with_clamp_size_to_monitor_size(true)
                .with_decorations(false)
                .with_resizable(true)
                .with_always_on_top();
            let mut unpin = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of((
                    "memory-tool-view",
                    address,
                    matches!(kind, MemoryViewKind::Structure),
                )),
                builder,
                |ctx, _| {
                    Self::constrain_memory_popup_to_monitor(ctx);
                    if ctx.input(|input| input.viewport().close_requested()) {
                        open = false;
                    }
                    Self::render_memory_popup_titlebar(ctx, &title, &mut unpin, &mut open);
                    egui::CentralPanel::default()
                        .frame(Self::memory_popup_frame(ctx))
                        .show(ctx, |ui| {
                            Self::render_memory_view_body(
                                ui,
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
            if open {
                self.memory_panel.memory_view_dialog = Some(dialog);
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            return;
        }
        egui::Window::new(title)
            .default_size(vec2(720.0, 430.0))
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                if ui.button("Pin").clicked() {
                    dialog.pinned = true;
                }
                Self::render_memory_view_body(ui, &mut dialog, bytes.as_deref(), region);
            });
        if !open {
            self.memory_panel.memory_view_dialog = None;
        } else {
            self.add_pending_structure_address(&mut dialog);
            self.memory_panel.memory_view_dialog = Some(dialog);
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn render_memory_view_body(
        ui: &mut egui::Ui,
        dialog: &mut MemoryViewDialog,
        bytes: Option<&[u8]>,
        region: Option<MemoryRegionInfo>,
    ) {
        let Some(bytes) = bytes else {
            ui.label("Unable to read this memory region");
            return;
        };
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
        egui::ScrollArea::both().show(ui, |ui| match dialog.kind {
            MemoryViewKind::Bytes => {
                Self::render_memory_region_grid(ui, dialog, bytes);
            }
            MemoryViewKind::Structure => {
                Self::render_structure_elements(ui, dialog, bytes);
            }
        });
    }

    fn render_memory_region_grid(ui: &mut egui::Ui, dialog: &mut MemoryViewDialog, bytes: &[u8]) {
        let unit = memory_display_width(dialog.display_type);
        let value_width = memory_display_cell_width(dialog.display_type);
        let address_width = 145.0;
        let ascii_width = 210.0;
        let columns = (((ui.available_width() - address_width - ascii_width) / value_width).floor()
            as usize)
            .clamp(1, 32 / unit);
        let row_bytes = columns * unit;

        ui.horizontal(|ui| {
            Self::memory_view_cell(ui, address_width, "Address");
            for column in 0..columns {
                let low_byte = dialog.address.wrapping_add(column * unit) & 0xFF;
                Self::memory_view_cell(ui, value_width, &format!("{low_byte:02X}"));
            }
            Self::memory_view_cell(ui, ascii_width, "0123456789ABCDEF0123456789ABCDEF");
        });
        ui.separator();

        for (row, chunk) in bytes.chunks(row_bytes).enumerate() {
            let row_address = dialog.address.saturating_add(row * row_bytes);
            ui.horizontal(|ui| {
                let shown_address = if dialog.relative_addresses {
                    format!("+{:04X}", row * row_bytes)
                } else {
                    format!("{row_address:X}")
                };
                Self::memory_view_cell(ui, address_width, &shown_address);
                for (column, value) in chunk.chunks(unit).take(columns).enumerate() {
                    let text = (value.len() == unit)
                        .then(|| format_memory_display(value, dialog.display_type))
                        .unwrap_or_default();
                    let cell = Self::memory_view_cell(ui, value_width, &text);
                    let cell_address = row_address.saturating_add(column * unit);
                    cell.context_menu(|ui| {
                        if ui.button("Add this address to the list").clicked() {
                            dialog.pending_add =
                                Some((cell_address, memory_display_scan_type(dialog.display_type)));
                            ui.close();
                        }
                        ui.separator();
                        ui.menu_button("Display Type", |ui| {
                            for (display_type, label) in memory_display_types() {
                                if ui
                                    .selectable_value(&mut dialog.display_type, display_type, label)
                                    .clicked()
                                {
                                    ui.close();
                                }
                            }
                        });
                        ui.checkbox(&mut dialog.relative_addresses, "Show relative addresses");
                        if ui.button("Open in dissect data/structure").clicked() {
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
            });
        }
    }

    fn render_structure_elements(ui: &mut egui::Ui, dialog: &mut MemoryViewDialog, bytes: &[u8]) {
        ui.horizontal(|ui| {
            Self::memory_view_cell(ui, 72.0, "Offset");
            Self::memory_view_cell(ui, 110.0, "Type");
            Self::memory_view_cell(ui, 152.0, "Address");
            Self::memory_view_cell(ui, 260.0, "Value");
        });
        ui.separator();
        for element in &mut dialog.elements {
            let width = element.value_type.width();
            let Some(raw) = bytes.get(element.offset..element.offset.saturating_add(width)) else {
                continue;
            };
            let element_address = dialog.address.saturating_add(element.offset);
            let mut add_request = None;
            let row = ui
                .horizontal(|ui| {
                    Self::memory_view_cell(ui, 72.0, &format!("+{:04X}", element.offset));
                    Self::memory_view_cell(ui, 110.0, element.value_type.label());
                    let address_response =
                        Self::memory_view_cell(ui, 152.0, &format!("{element_address:X}"));
                    Self::memory_view_cell(
                        ui,
                        260.0,
                        &format_structure_value(raw, element.value_type),
                    );
                    if address_response.double_clicked() {
                        add_request = Some((element_address, element.value_type.scan_type()));
                    }
                })
                .response;
            row.context_menu(|ui| {
                ui.menu_button("Change element", |ui| {
                    for (value_type, label) in StructureElementType::ALL {
                        if ui
                            .selectable_value(&mut element.value_type, value_type, label)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Offset");
                    ui.add(
                        egui::DragValue::new(&mut element.offset)
                            .hexadecimal(4, false, true)
                            .speed(1),
                    );
                });
            });
            if add_request.is_some() {
                dialog.pending_add = add_request;
            }
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
            description: String::new(),
            pointer: None,
            frozen: None,
            saved_to_library: false,
        });
        self.memory_panel.status = format!("Address 0x{address:X} added");
    }

    fn memory_view_cell(ui: &mut egui::Ui, width: f32, text: &str) -> egui::Response {
        Self::memory_label_cell(
            ui,
            width,
            18.0,
            egui::Label::new(RichText::new(text).monospace()).selectable(true),
        )
    }

    fn render_memory_address_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.address_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let window = egui::Window::new("Change address")
            .collapsible(false)
            .resizable(false)
            .default_pos(dialog.position)
            .open(&mut open)
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
        if let Some(window) = window {
            dialog.rect = Some(window.response.rect);
        }
        if save {
            self.apply_memory_address_dialog(&dialog);
            open = false;
        }
        if cancel {
            open = false;
        }
        if open {
            self.memory_panel.address_dialog = Some(dialog);
        }
    }

    fn apply_memory_address_dialog(&mut self, dialog: &AddressDialog) {
        let Some(base) = parse_memory_address(&dialog.address) else {
            self.memory_panel.status = "Invalid address".to_owned();
            return;
        };
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
            Some(PointerSpec {
                base,
                module: None,
                offsets,
            })
        } else {
            None
        };
        let resolved = self
            .memory_panel
            .process_pid
            .and_then(|pid| resolve_memory_address(pid, base, pointer.as_ref()).ok())
            .unwrap_or(base);
        if let Some(saved) = self.memory_panel.saved.get_mut(dialog.index) {
            saved.address = resolved;
            saved.pointer = pointer;
            saved.frozen = None;
        }
        self.persist_memory_pointers();
    }

    fn start_memory_action(&mut self, action: MemoryScanAction) {
        #[cfg(windows)]
        self.stop_memory_debuggers_for_scan();
        let Some(pid) = self.memory_panel.process_pid else {
            self.memory_panel.status = "Select a process".to_owned();
            return;
        };
        if self.memory_panel.scanning {
            return;
        }
        let value_type = self.memory_panel.value_type;
        let exact = if matches!(action, MemoryScanAction::Unknown)
            || matches!(
                action,
                MemoryScanAction::Increased
                    | MemoryScanAction::Decreased
                    | MemoryScanAction::Changed
                    | MemoryScanAction::Unchanged
            ) {
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
        };
        let result_limit = self
            .memory_panel
            .result_limit_input
            .replace(['.', ',', '_'], "")
            .parse::<usize>()
            .unwrap_or(DEFAULT_SCAN_LIMIT)
            .clamp(1_000, DEFAULT_SCAN_LIMIT);
        self.memory_panel.result_limit_input = result_limit.to_string();
        let candidates = if action.comparison().is_some() {
            std::mem::take(&mut self.memory_panel.candidates)
        } else {
            self.memory_panel.candidates.clear();
            Vec::new()
        };
        self.memory_panel.scan_progress.store(0, Ordering::Relaxed);
        self.memory_panel.scan_input_count = if action.comparison().is_some() {
            candidates.len()
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
        thread::spawn(move || {
            let result = if let Some(comparison) = action.comparison() {
                filter_scan_candidates(pid, candidates, comparison, exact)
            } else {
                scan_memory_with_progress(pid, exact, value_type, result_limit, progress)
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
    fn stop_memory_debuggers_for_scan(&mut self) {
        if let Some(dialog) = self.memory_panel.instruction_watch_dialog.as_mut()
            && let Some(mut active) = dialog.active.take()
        {
            active.stop();
            dialog.status = "Debugger stopped before memory scan".to_owned();
        }
        if let Some(dialog) = self.memory_panel.code_access_dialog.as_mut()
            && let Some(mut active) = dialog.active.take()
        {
            active.stop();
            dialog.status = "Debugger stopped before memory scan".to_owned();
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
            Ok(candidates) => {
                let count = candidates.len();
                self.memory_panel.candidates = candidates;
                self.memory_panel.status =
                    format!("{} — {count} result(s)", outcome.action.label());
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
        self.memory_panel.selected_results.clear();
        self.memory_panel.marked_result_addresses.clear();
        self.memory_panel.selection_anchor = None;
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
            let Some(candidate) = self.memory_panel.candidates.get(index).copied() else {
                continue;
            };
            if self.memory_panel.saved.iter().any(|saved| {
                saved.address == candidate.address
                    && saved.value_type == candidate.current.value_type()
            }) {
                continue;
            }
            self.memory_panel.saved.push(SavedMemoryAddress {
                address: candidate.address,
                value_type: candidate.current.value_type(),
                current: Some(candidate.current),
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
        let Some(address) = parse_memory_address(&self.memory_panel.manual_address) else {
            self.memory_panel.status = "Invalid address".to_owned();
            return;
        };
        let value_type = self.memory_panel.value_type;
        let current = read_scan_value(pid, address, value_type).ok();
        self.memory_panel.saved.push(SavedMemoryAddress {
            address,
            value_type,
            current,
            description: String::new(),
            pointer: None,
            frozen: None,
            saved_to_library: false,
        });
        self.memory_panel.manual_address.clear();
    }

    fn refresh_memory_values(&mut self) {
        if self.memory_panel.last_refresh.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.memory_panel.last_refresh = Instant::now();
        let Some(pid) = self.memory_panel.process_pid else {
            return;
        };
        let visible_count = self.memory_panel.candidates.len().min(MAX_VISIBLE_RESULTS);
        let _ = refresh_scan_candidates(pid, &mut self.memory_panel.candidates[..visible_count]);
        for saved in &mut self.memory_panel.saved {
            if let Some(pointer) = saved.pointer.as_ref()
                && let Ok(address) = resolve_memory_address(pid, pointer.base, Some(pointer))
            {
                saved.address = address;
            }
            saved.current = read_scan_value(pid, saved.address, saved.value_type).ok();
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
        if let Ok(mut config) = self.memory_panel.freeze_worker.config.lock() {
            config.0 = (!targets.is_empty())
                .then_some(self.memory_panel.process_pid)
                .flatten();
            config.1 = targets;
        }
    }

    fn commit_saved_memory_value(&mut self, index: usize) {
        let Some(pid) = self.memory_panel.process_pid else {
            return;
        };
        let Some(saved) = self.memory_panel.saved.get(index).cloned() else {
            return;
        };
        let Some(value) = parse_scan_value(
            &self.memory_panel.edit_value_input,
            saved.value_type,
            self.memory_panel.hex,
        ) else {
            self.memory_panel.status = "Invalid value".to_owned();
            return;
        };
        match write_scan_value(pid, saved.address, value) {
            Ok(()) => {
                self.memory_panel.saved[index].current = Some(value);
                if self.memory_panel.saved[index].frozen.is_some() {
                    self.memory_panel.saved[index].frozen = Some(value);
                }
                self.memory_panel.edit_value_index = None;
                self.memory_panel.status = "Value written".to_owned();
            }
            Err(error) => self.memory_panel.status = format!("Write failed: {error}"),
        }
    }

    fn write_selected_saved_memory(&mut self) {
        let Some(pid) = self.memory_panel.process_pid else {
            return;
        };
        let selected = self.memory_panel.selected_saved.clone();
        let mut written = 0;
        for index in selected {
            let Some(saved) = self.memory_panel.saved.get(index) else {
                continue;
            };
            let Some(value) = parse_scan_value(
                &self.memory_panel.value_input,
                saved.value_type,
                self.memory_panel.hex,
            ) else {
                continue;
            };
            if write_scan_value(pid, saved.address, value).is_ok() {
                written += 1;
            }
        }
        self.memory_panel.status = format!("Wrote {written} address(es)");
    }

    fn freeze_selected_saved_memory(&mut self) {
        let selected = self.memory_panel.selected_saved.clone();
        for index in selected {
            if let Some(saved) = self.memory_panel.saved.get_mut(index) {
                saved.frozen = saved.current;
            }
        }
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
        self.memory_panel.hotkey_was_down.insert(action, true);
        self.memory_panel.capturing_hotkey = None;
        self.capture_hotkey_combo_keys = None;
        self.capture_hotkey_combo_vks.clear();
        self.persist_memory_hotkeys();
    }

    fn poll_memory_hotkeys(&mut self, ctx: &egui::Context) {
        let bindings = self
            .memory_panel
            .hotkeys
            .iter()
            .map(|(action, binding)| (*action, binding.clone()))
            .collect::<Vec<_>>();
        for (action, binding) in bindings {
            let down = memory_binding_is_down(&binding);
            let was_down = self
                .memory_panel
                .hotkey_was_down
                .insert(action, down)
                .unwrap_or(false);
            if down && !was_down {
                self.start_memory_action(action);
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
                            return process_modules(pid).ok()?.first().map(|entry| entry.0.clone());
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
                    offsets: module_pointer.map_or_else(Vec::new, |(pointer, _)| pointer.offsets.clone()),
                    value_type: memory_type_config(saved.value_type).to_owned(),
                    absolute_address: module_pointer.is_none().then_some(saved.address),
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
        || format!("0x{:X}", pointer.base),
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
        })
        .collect()
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
            format!("P->0x{:X}", u64::from_le_bytes(bytes.try_into().unwrap()))
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

fn parse_hex_offset(text: &str) -> Option<usize> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    usize::from_str_radix(digits, 16).ok()
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

#[cfg(windows)]
fn memory_binding_is_down(binding: &HotkeyBinding) -> bool {
    let keys = hotkey::binding_key_names(binding);
    !keys.is_empty()
        && keys.iter().all(|key| {
            hotkey::key_name_to_vk(key).is_some_and(|vk| unsafe {
                (windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk as i32) as u16
                    & 0x8000)
                    != 0
            })
        })
}

#[cfg(not(windows))]
fn memory_binding_is_down(_binding: &HotkeyBinding) -> bool {
    false
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
    }

    #[test]
    fn modifier_capture_keeps_full_combo_while_keys_are_released() {
        let mut pending = hotkey::parse_binding("Ctrl+Shift");
        update_pending_modifier_capture(&mut pending, hotkey::parse_binding("Ctrl").unwrap());
        assert_eq!(hotkey::format_binding(pending.as_ref()), "Ctrl+Shift");
    }
}
