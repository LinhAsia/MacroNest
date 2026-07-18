use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{self, Button, Color32, Frame, RichText, Sense, vec2};

use crate::{
    hotkey,
    model::HotkeyBinding,
    process_memory::{
        ScanCandidate, ScanComparison, ScanValue, ScanValueType, filter_scan_candidates,
        read_scan_value, refresh_scan_candidates, scan_memory_with_progress, write_scan_value,
    },
    window_list,
};

use super::CrosshairApp;

const DEFAULT_SCAN_LIMIT: usize = 10_000_000;
// ponytail: keep live polling bounded; add paged candidate refresh before raising this ceiling.
const MAX_VISIBLE_RESULTS: usize = 1_000;

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
}

#[derive(Clone)]
struct PointerSpec {
    base: usize,
    offsets: Vec<usize>,
}

struct AddressDialog {
    index: usize,
    address: String,
    offsets: String,
    pointer: bool,
}

struct ScanJobResult {
    pid: u32,
    action: MemoryScanAction,
    result: Result<Vec<ScanCandidate>, String>,
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
    selection_anchor: Option<usize>,
    saved: Vec<SavedMemoryAddress>,
    selected_saved: HashSet<usize>,
    manual_address: String,
    status: String,
    last_action: String,
    job_rx: Option<Receiver<ScanJobResult>>,
    scanning: bool,
    scan_progress: Arc<AtomicUsize>,
    scan_input_count: usize,
    pinned: bool,
    hotkeys: HashMap<MemoryScanAction, HotkeyBinding>,
    hotkey_was_down: HashMap<MemoryScanAction, bool>,
    capturing_hotkey: Option<MemoryScanAction>,
    edit_value_index: Option<usize>,
    edit_value_input: String,
    address_dialog: Option<AddressDialog>,
    last_refresh: Instant,
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
            selection_anchor: None,
            saved: Vec::new(),
            selected_saved: HashSet::new(),
            manual_address: String::new(),
            status: "Ready".to_owned(),
            last_action: "Ready".to_owned(),
            job_rx: None,
            scanning: false,
            scan_progress: Arc::new(AtomicUsize::new(0)),
            scan_input_count: 0,
            pinned: false,
            hotkeys: HashMap::new(),
            hotkey_was_down: HashMap::new(),
            capturing_hotkey: None,
            edit_value_index: None,
            edit_value_input: String::new(),
            address_dialog: None,
            last_refresh: Instant::now(),
        }
    }
}

impl MemoryPanelState {
    pub(crate) fn with_hotkeys(stored: &[(String, HotkeyBinding)]) -> Self {
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
    pub(crate) fn render_memory_panel(&mut self, ui: &mut egui::Ui) {
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
    }

    pub(crate) fn render_memory_pinned_viewport(&mut self, ctx: &egui::Context) {
        if !self.memory_panel.pinned {
            return;
        }
        self.poll_memory_job();
        self.poll_memory_hotkeys(ctx);
        self.refresh_memory_values();
        let builder = egui::ViewportBuilder::default()
            .with_title("MacroNest — Scan results")
            .with_inner_size(vec2(560.0, 430.0))
            .with_min_inner_size(vec2(400.0, 260.0))
            .with_always_on_top();
        let mut unpin = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("memory-scan-results"),
            builder,
            |ctx, _| {
                if ctx.input(|input| input.viewport().close_requested()) {
                    unpin = true;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Scan results").strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Unpin").clicked() {
                                unpin = true;
                            }
                        });
                    });
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
                ctx.request_repaint_after(Duration::from_millis(50));
            },
        );
        if unpin {
            self.memory_panel.pinned = false;
        }
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
                    let process_label = self
                        .open_window_infos
                        .iter()
                        .find(|window| window.selector == self.memory_panel.process_selector)
                        .map(|window| Self::simplify_window_title(&window.title))
                        .unwrap_or_else(|| "Select process".to_owned());
                    egui::ComboBox::from_id_salt("memory-process")
                        .width(ui.available_width())
                        .selected_text(Self::truncate_window_title(&process_label, 52))
                        .show_ui(ui, |ui| {
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
                                    let pid = window_list::process_id_for_window(Some(&selector));
                                    if self.memory_panel.process_pid != pid {
                                        self.reset_memory_scan("Process changed");
                                        self.memory_panel.saved.clear();
                                    }
                                    self.memory_panel.process_selector = selector;
                                    self.memory_panel.process_pid = pid;
                                }
                            }
                        });
                });
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("memory-value-type")
                        .width(90.0)
                        .selected_text(memory_type_label(self.memory_panel.value_type))
                        .show_ui(ui, |ui| {
                            for value_type in [
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
                    let selected = self.memory_panel.selected_results.len();
                    if ui
                        .add_enabled(
                            selected > 0,
                            Button::new(format!("Add selected ({selected})")),
                        )
                        .clicked()
                    {
                        self.add_selected_memory_results();
                    }
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
                        if let Some(action) = action {
                            self.memory_action_button(ui, action, true);
                        } else if reset_last && index == 2 {
                            if ui
                                .add_enabled_ui(!self.memory_panel.scanning, |ui| {
                                    ui.add_sized(
                                        [(ui.available_width() - 34.0).max(0.0), 26.0],
                                        Button::new("Reset"),
                                    )
                                })
                                .inner
                                .clicked()
                            {
                                self.reset_memory_scan("New scan");
                            }
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
                    .map(|label| RichText::new(label).size(10.0))
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
                    Self::paint_expanded_hotkey(ui, &response, label);
                }
                if response.clicked() {
                    if assigned_label.is_some() {
                        self.memory_panel.hotkeys.remove(&action);
                        self.memory_panel.hotkey_was_down.remove(&action);
                        self.memory_panel.capturing_hotkey = None;
                        self.persist_memory_hotkeys();
                    } else {
                        self.memory_panel.capturing_hotkey = Some(action);
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
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                ui.add_sized(
                    [190.0, 18.0],
                    egui::Label::new(RichText::new("Address").strong()),
                );
                ui.add_sized(
                    [112.0, 18.0],
                    egui::Label::new(RichText::new("Current").strong()),
                );
                ui.label(RichText::new("Previous").strong());
            });
            ui.separator();
            let visible_count = self.memory_panel.candidates.len().min(MAX_VISIBLE_RESULTS);
            if !pinned
                && ui.ctx().memory(|memory| memory.focused().is_none())
                && ui.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::A))
            {
                self.memory_panel.selected_results = (0..visible_count).collect();
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
                    for index in rows {
                        let candidate = self.memory_panel.candidates[index];
                        let selected = self.memory_panel.selected_results.contains(&index);
                        ui.horizontal(|ui| {
                            if !pinned {
                                let mut checked = selected;
                                if ui.checkbox(&mut checked, "").changed() {
                                    self.select_memory_result(index, checked, ui);
                                }
                            }
                            let address_response = ui.add_sized(
                                [190.0, 18.0],
                                egui::Label::new(format!("0x{:016X}", candidate.address))
                                    .sense(Sense::click()),
                            );
                            ui.add_sized(
                                [112.0, 18.0],
                                egui::Label::new(format_scan_value(
                                    candidate.current,
                                    self.memory_panel.hex,
                                )),
                            );
                            ui.monospace(format_scan_value(
                                candidate.previous,
                                self.memory_panel.hex,
                            ));
                            if !pinned && address_response.double_clicked() {
                                self.memory_panel.selected_results.clear();
                                self.memory_panel.selected_results.insert(index);
                                self.add_selected_memory_results();
                            }
                        });
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
            if self.memory_panel.candidates.is_empty() && !self.memory_panel.scanning {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("No scan results").weak());
                });
            }
        });
    }

    fn select_memory_result(&mut self, index: usize, selected: bool, ui: &egui::Ui) {
        if ui.input(|input| input.modifiers.shift)
            && let Some(anchor) = self.memory_panel.selection_anchor
        {
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
        } else if selected {
            self.memory_panel.selected_results.insert(index);
        } else {
            self.memory_panel.selected_results.remove(&index);
        }
        self.memory_panel.selection_anchor = Some(index);
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
                let row_height = 26.0;
                let count = self.memory_panel.saved.len();
                egui::ScrollArea::vertical()
                    .id_salt("saved-memory-addresses")
                    .auto_shrink([false, false])
                    .max_height(ui.available_height())
                    .show_rows(ui, row_height, count, |ui, rows| {
                        for index in rows {
                            if index >= self.memory_panel.saved.len() {
                                continue;
                            }
                            let saved = self.memory_panel.saved[index].clone();
                            let selected = self.memory_panel.selected_saved.contains(&index);
                            let mut open_address = false;
                            let mut edit_value = false;
                            let mut delete = false;
                            let row = ui.horizontal(|ui| {
                                let mut checked = selected;
                                if ui.checkbox(&mut checked, "").changed() {
                                    if checked {
                                        self.memory_panel.selected_saved.insert(index);
                                    } else {
                                        self.memory_panel.selected_saved.remove(&index);
                                    }
                                }
                                ui.add_sized(
                                    [172.0, 20.0],
                                    egui::Label::new(format!("0x{:016X}", saved.address))
                                        .sense(Sense::click()),
                                );
                                ui.label(memory_type_label(saved.value_type));
                                if self.memory_panel.edit_value_index == Some(index) {
                                    let response = ui.add_sized(
                                        [120.0, 20.0],
                                        egui::TextEdit::singleline(
                                            &mut self.memory_panel.edit_value_input,
                                        ),
                                    );
                                    if response.lost_focus()
                                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                                    {
                                        self.commit_saved_memory_value(index);
                                    }
                                    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                                        self.memory_panel.edit_value_index = None;
                                    }
                                } else if ui
                                    .add_sized(
                                        [120.0, 20.0],
                                        egui::Label::new(
                                            saved
                                                .current
                                                .map(|value| {
                                                    format_scan_value(value, self.memory_panel.hex)
                                                })
                                                .unwrap_or_else(|| "?".to_owned()),
                                        )
                                        .sense(Sense::click()),
                                    )
                                    .double_clicked()
                                {
                                    edit_value = true;
                                }
                                ui.add_sized(
                                    [ui.available_width().max(80.0) - 50.0, 20.0],
                                    egui::TextEdit::singleline(
                                        &mut self.memory_panel.saved[index].description,
                                    )
                                    .hint_text("description"),
                                );
                                let mut frozen = saved.frozen.is_some();
                                if ui
                                    .checkbox(&mut frozen, "")
                                    .on_hover_text("Freeze")
                                    .changed()
                                {
                                    self.memory_panel.saved[index].frozen =
                                        if frozen { saved.current } else { None };
                                }
                            });
                            row.response.context_menu(|ui| {
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

    fn render_memory_address_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.memory_panel.address_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Change address")
            .collapsible(false)
            .resizable(false)
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
            Some(PointerSpec { base, offsets })
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
    }

    fn start_memory_action(&mut self, action: MemoryScanAction) {
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
        self.memory_panel.selection_anchor = None;
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
            if let Some(value) = saved.frozen {
                if write_scan_value(pid, saved.address, value).is_err() {
                    saved.frozen = None;
                }
            }
            saved.current = read_scan_value(pid, saved.address, saved.value_type).ok();
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
        let captured = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => hotkey::capture_from_egui(*key, *modifiers),
                _ => None,
            })
        });
        if let Some(binding) = captured {
            self.memory_panel.hotkeys.insert(action, binding);
            self.memory_panel.hotkey_was_down.insert(action, true);
            self.memory_panel.capturing_hotkey = None;
            self.persist_memory_hotkeys();
        }
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
}

fn memory_type_label(value_type: ScanValueType) -> &'static str {
    match value_type {
        ScanValueType::I32 => "4 Bytes",
        ScanValueType::F32 => "Float",
        ScanValueType::I64 => "8 Bytes",
        ScanValueType::F64 => "Double",
    }
}

fn parse_scan_value(text: &str, value_type: ScanValueType, hex: bool) -> Option<ScanValue> {
    let text = text.trim().replace('_', "");
    match value_type {
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
        ScanValueType::I32 if hex => {
            parse_hex_signed(&text, 32).map(|value| ScanValue::I32(value as i32))
        }
        ScanValueType::I64 if hex => parse_hex_signed(&text, 64).map(ScanValue::I64),
        ScanValueType::I32 => text.parse().ok().map(ScanValue::I32),
        ScanValueType::I64 => text.parse().ok().map(ScanValue::I64),
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
    Some(if bits == 32 {
        unsigned as u32 as i32 as i64
    } else {
        unsigned as i64
    })
}

fn format_scan_value(value: ScanValue, hex: bool) -> String {
    match value {
        ScanValue::I32(value) if hex => format!("0x{:08X}", value as u32),
        ScanValue::I64(value) if hex => format!("0x{:016X}", value as u64),
        ScanValue::I32(value) => value.to_string(),
        ScanValue::F32(value) => value.to_string(),
        ScanValue::I64(value) => value.to_string(),
        ScanValue::F64(value) => value.to_string(),
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
    }

    #[test]
    fn parses_decimal_and_hex_addresses() {
        assert_eq!(parse_memory_address("4096"), Some(4096));
        assert_eq!(parse_memory_address("0x1000"), Some(4096));
        assert_eq!(parse_memory_address("7FF6_ABCD"), Some(0x7FF6_ABCD));
        assert_eq!(parse_memory_address("0x1000+10-8"), Some(0x1008));
    }
}
