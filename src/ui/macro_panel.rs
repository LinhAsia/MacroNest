use crate::ai;

use crate::hotkey;

use crate::model::*;
use crate::overlay::OverlayCommand;

use crate::ui::{
    CrosshairApp, MATERIAL_ICONS_FONT, MacroActionSubmenuKind, MacroGroupFavoriteFilter,
    MacroStepDragPayload, MouseCaptureKind, MouseMoveAbsoluteCaptureTarget,
};

use eframe::egui::{self, *};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]

enum VariableValueKind {
    Neutral,

    Number,

    Text,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextHighlightMode {
    None,

    VariableTokens,

    Interpolations,
}

impl CrosshairApp {
    fn contrast_text_color(background: Color32) -> Color32 {
        let red = background.r() as f32 / 255.0;
        let green = background.g() as f32 / 255.0;
        let blue = background.b() as f32 / 255.0;
        let linearize = |channel: f32| {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        let luminance =
            0.2126 * linearize(red) + 0.7152 * linearize(green) + 0.0722 * linearize(blue);
        if luminance > 0.45 {
            Color32::from_rgb(16, 22, 30)
        } else {
            Color32::from_rgb(248, 250, 252)
        }
    }

    fn active_variable_badge_colors(has_value: bool) -> (Color32, Color32, Color32) {
        let fill = if has_value {
            Color32::from_rgb(52, 214, 255)
        } else {
            Color32::from_rgb(84, 90, 102)
        };
        let stroke = if has_value {
            Color32::from_rgb(130, 236, 255)
        } else {
            Color32::from_rgb(164, 170, 180)
        };
        let text = Self::contrast_text_color(fill);
        (fill, stroke, text)
    }

    fn parse_rgb_color(s: &str) -> Option<Color32> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() >= 3 {
            let r = parts[0].trim().parse::<u8>().ok()?;
            let g = parts[1].trim().parse::<u8>().ok()?;
            let b = parts[2].trim().parse::<u8>().ok()?;
            Some(Color32::from_rgb(r, g, b))
        } else {
            None
        }
    }

    fn loop_is_infinite(step: &MacroStep) -> bool {
        matches!(
            step.key.trim().to_ascii_lowercase().as_str(),
            "infinite" | "inf" | "forever" | "-1"
        )
    }

    fn render_macro_action_button(
        ui: &mut egui::Ui,
        language: UiLanguage,
        current: &MacroAction,
        candidate: MacroAction,
        action_hover_id: egui::Id,
        is_submenu_item: bool,
    ) -> egui::Response {
        let inner = ui.allocate_ui_with_layout(
            vec2(58.0, 42.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let label_color = if *current == candidate {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                };
                let response = ui.add_sized(
                    [34.0, 24.0],
                    Button::new(Self::macro_action_icon_text(candidate))
                        .selected(*current == candidate),
                );
                if !is_submenu_item && (response.hovered() || response.clicked()) {
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(action_hover_id, true));
                }
                ui.label(
                    RichText::new(Self::macro_action_short_label(candidate, language))
                        .size(9.0)
                        .color(label_color),
                );
                response
            },
        );
        let response = inner.inner;
        if !is_submenu_item {
            Self::show_instant_hover_tooltip(
                ui,
                &response,
                format!(
                    "{}\n{}",
                    Self::macro_action_label(candidate),
                    Self::macro_action_tooltip(candidate, language)
                ),
            );
        }
        response
    }

    fn render_macro_action_option(
        ui: &mut egui::Ui,
        language: UiLanguage,
        current: &mut MacroAction,
        candidate: MacroAction,
        live_sync: &mut bool,
        action_hover_id: egui::Id,
        is_submenu_item: bool,
    ) {
        let response = Self::render_macro_action_button(
            ui,
            language,
            current,
            candidate,
            action_hover_id,
            is_submenu_item,
        );
        if response.clicked() {
            *current = candidate;
            *live_sync = true;
            ui.close();
        }
    }

    fn render_window_control_step_comboboxes(
        window_presets: &[WindowPreset],
        window_layouts: &[WindowLayout],
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_salt: egui::Id,
        step_key: &mut String,
        live_sync: &mut bool,
        width: f32,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let is_layout = step_key.trim().starts_with("layout:");
            let mut current_mode = if is_layout { 1 } else { 0 };

            let mode_text = if current_mode == 1 {
                Self::tr_lang(language, "Layout Preset", "Bố cục").to_owned()
            } else {
                Self::tr_lang(language, "Resize", "Resize").to_owned()
            };

            // ComboBox for Mode selection
            let mode_id_salt = id_salt.with("mode");
            egui::ComboBox::from_id_salt(mode_id_salt)
                .width(width * 0.45)
                .selected_text(mode_text)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(current_mode == 0, Self::tr_lang(language, "Resize", "Resize")).clicked() {
                        if current_mode != 0 {
                            // Change to Resize mode. Default to first window preset or empty
                            *step_key = window_presets.first().map(|p| p.id.to_string()).unwrap_or_default();
                            *live_sync = true;
                        }
                    }
                    if ui.selectable_label(current_mode == 1, Self::tr_lang(language, "Layout Preset", "Bố cục")).clicked() {
                        if current_mode != 1 {
                            // Change to Layout mode. Default to layout:first_layout_id or empty layout:
                            *step_key = window_layouts.first().map(|l| format!("layout:{}", l.id)).unwrap_or_else(|| "layout:".to_string());
                            *live_sync = true;
                        }
                    }
                });

            // ComboBox for Preset selection
            let preset_id_salt = id_salt.with("preset");
            if current_mode == 1 {
                // Layout preset
                let layout_id_str = step_key.trim().strip_prefix("layout:").unwrap_or("").trim();
                let selected_id = layout_id_str.parse::<u32>().ok();
                let selected_label = selected_id
                    .and_then(|id| {
                        window_layouts
                            .iter()
                            .find(|l| l.id == id)
                            .map(|l| l.name.clone())
                    })
                    .unwrap_or_else(|| Self::tr_lang(language, "Select layout", "Chọn bố cục").to_owned());

                egui::ComboBox::from_id_salt(preset_id_salt)
                    .width(width * 0.55)
                    .selected_text(selected_label)
                    .show_ui(ui, |ui| {
                        for layout_option in window_layouts {
                            if ui
                                .selectable_label(selected_id == Some(layout_option.id), &layout_option.name)
                                .clicked()
                            {
                                *step_key = format!("layout:{}", layout_option.id);
                                *live_sync = true;
                            }
                        }
                    });
            } else {
                // Resize (Window preset)
                let selected_id = step_key.trim().parse::<u32>().ok();
                let selected_label = selected_id
                    .and_then(|id| {
                        window_presets
                            .iter()
                            .find(|preset| preset.id == id)
                            .map(|preset| preset.name.clone())
                    })
                    .unwrap_or_else(|| Self::tr_lang(language, "Select window", "Chọn cửa sổ").to_owned());

                egui::ComboBox::from_id_salt(preset_id_salt)
                    .width(width * 0.55)
                    .selected_text(selected_label)
                    .show_ui(ui, |ui| {
                        for preset_option in window_presets {
                            if ui
                                .selectable_label(selected_id == Some(preset_option.id), &preset_option.name)
                                .clicked()
                            {
                                *step_key = preset_option.id.to_string();
                                *live_sync = true;
                            }
                        }
                    });
            }
        });
    }

    fn clear_macro_action_submenus(ui: &mut egui::Ui, id_source: impl std::hash::Hash + Copy) {
        let owner_id = ui.make_persistent_id("macro-action-submenu-owner");
        let active_mouse_click_popup_key_id =
            ui.make_persistent_id((id_source, "mouse-click-active-submenu-key"));
        let mouse_popup_id = ui.make_persistent_id((id_source, "mouse-submenu-popup"));
        let image_popup_id = ui.make_persistent_id((id_source, "image-search-submenu-popup"));
        let timer_popup_id = ui.make_persistent_id((id_source, "timer-submenu-popup"));
        let if_popup_id = ui.make_persistent_id((id_source, "if-submenu-popup"));
        let geometry_popup_id = ui.make_persistent_id((id_source, "geometry-submenu-popup"));
        let audio_sense_popup_id = ui.make_persistent_id((id_source, "audiosense-submenu-popup"));
        ui.ctx().data_mut(|data| {
            data.insert_temp(owner_id, None::<MacroActionSubmenuKind>);
            data.insert_temp(active_mouse_click_popup_key_id, None::<&'static str>);
            data.insert_temp(mouse_popup_id, false);
            data.insert_temp(image_popup_id, false);
            data.insert_temp(timer_popup_id, false);
            data.insert_temp(if_popup_id, false);
            data.insert_temp(geometry_popup_id, false);
            data.insert_temp(audio_sense_popup_id, false);
        });
        egui::Popup::close_id(ui.ctx(), mouse_popup_id);
        egui::Popup::close_id(ui.ctx(), image_popup_id);
        egui::Popup::close_id(ui.ctx(), timer_popup_id);
        egui::Popup::close_id(ui.ctx(), if_popup_id);
        egui::Popup::close_id(ui.ctx(), geometry_popup_id);
        egui::Popup::close_id(ui.ctx(), audio_sense_popup_id);
        for (_, _, _, popup_key) in Self::mouse_click_action_groups().iter().copied() {
            let child_popup_id = ui.make_persistent_id((id_source, popup_key, "popup"));
            ui.ctx()
                .data_mut(|data| data.insert_temp(child_popup_id, false));
            egui::Popup::close_id(ui.ctx(), child_popup_id);
        }
        ui.ctx().request_repaint();
    }

    fn clear_mouse_click_submenus(ui: &mut egui::Ui, id_source: impl std::hash::Hash + Copy) {
        let active_mouse_click_popup_key_id =
            ui.make_persistent_id((id_source, "mouse-click-active-submenu-key"));
        ui.ctx().data_mut(|data| {
            data.insert_temp(active_mouse_click_popup_key_id, None::<&'static str>)
        });
        for (_, _, _, popup_key) in Self::mouse_click_action_groups().iter().copied() {
            let child_popup_id = ui.make_persistent_id((id_source, popup_key, "popup"));
            ui.ctx()
                .data_mut(|data| data.insert_temp(child_popup_id, false));
            egui::Popup::close_id(ui.ctx(), child_popup_id);
        }
        ui.ctx().request_repaint();
    }

    fn close_inactive_mouse_click_submenus(
        ui: &mut egui::Ui,
        id_source: impl std::hash::Hash + Copy,
        active_popup_key: Option<&'static str>,
    ) {
        for (_, _, _, popup_key) in Self::mouse_click_action_groups().iter().copied() {
            if Some(popup_key) != active_popup_key {
                let child_popup_id = ui.make_persistent_id((id_source, popup_key, "popup"));
                ui.ctx()
                    .data_mut(|data| data.insert_temp(child_popup_id, false));
            }
        }
    }

    fn render_expression_help_box(ui: &mut egui::Ui, language: UiLanguage) {
        let fill = Color32::from_rgba_unmultiplied(0, 170, 255, 18);
        let stroke = egui::Stroke::new(1.0, Color32::from_rgb(0, 170, 255));
        egui::Frame::group(ui.style())
            .fill(fill)
            .stroke(stroke)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_min_width(760.0);
                ui.horizontal(|ui| {
                    ui.label(
                        Self::material_icon_text(0xe88f, 14.0)
                            .color(Color32::from_rgb(0, 170, 255)),
                    );
                    ui.label(
                        egui::RichText::new(Self::tr_lang(
                            language,
                            "EXPRESSION HELP",
                            "HUONG DAN BIEU THUC",
                        ))
                        .strong()
                        .color(Color32::from_rgb(0, 170, 255)),
                    );
                });
                ui.add_space(2.0);
                ui.label(Self::tr_lang(
                    language,
                    "You can write math expressions and use variables in {}. Math operators + - * / and parentheses () are supported. Example: {100 + (A - B) * 2}",
                    "Ban co the viet bieu thuc toan va dung bien trong {}. Ho tro cac phep toan + - * / va dau ngoac (). Vi du: {100 + (A - B) * 2}",
                ));
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("expression_help_scroll")
                    .max_height(92.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(Self::tr_lang(
                                language,
                                "Available expressions:",
                                "Ham ho tro:",
                            ))
                            .strong(),
                        );
                        egui::Grid::new("expression-help-columns")
                            .num_columns(3)
                            .min_col_width(220.0)
                            .spacing([18.0, 0.0])
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(Self::tr_lang(language, "Functions", "Ham"))
                                            .strong(),
                                    );
                                    ui.label(egui::RichText::new("- random(min, max)").monospace());
                                    ui.label(egui::RichText::new("- choice(val1, val2, ...)").monospace());
                                    ui.label(egui::RichText::new("- min(a, b)").monospace());
                                    ui.label(egui::RichText::new("- max(a, b)").monospace());
                                    ui.label(egui::RichText::new("- abs(a)").monospace());
                                    ui.label(egui::RichText::new("- atan(a)").monospace());
                                    ui.label(egui::RichText::new("- atan2(y, x)").monospace());
                                    ui.label(egui::RichText::new("- sqrt(a)").monospace());
                                    ui.label(egui::RichText::new("- pow(a, b)").monospace());
                                    ui.label(egui::RichText::new("- ceil(a) / floor(a)").monospace());
                                    ui.label(egui::RichText::new("- factorial(n)").monospace());
                                    ui.label(egui::RichText::new("- gcd(a, b, ...) / lcm(a, b, ...)").monospace());
                                    ui.label(egui::RichText::new("- isqrt(n)").monospace());
                                    ui.label(egui::RichText::new("- comb(n, k) / perm(n, k)").monospace());
                                    ui.label(egui::RichText::new("- sin(angleDeg) * 1000").monospace());
                                    ui.label(egui::RichText::new("- cos(angleDeg) * 1000").monospace());
                                    ui.label(egui::RichText::new("- degrees(rad) / radians(deg)").monospace());
                                    ui.label(egui::RichText::new("- pi").monospace());
                                    ui.label(egui::RichText::new("- myVar.toNumber").monospace());
                                });
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(Self::tr_lang(
                                            language,
                                            "Numeric values",
                                            "Gia tri so",
                                        ))
                                        .strong(),
                                    );
                                    ui.label(egui::RichText::new("- screen.width").monospace());
                                    ui.label(egui::RichText::new("- screen.height").monospace());
                                    ui.label(egui::RichText::new("- mouse.x").monospace());
                                    ui.label(egui::RichText::new("- mouse.y").monospace());
                                    ui.label(egui::RichText::new("- mouse.sensitivity").monospace());
                                    ui.label(egui::RichText::new("- volume.level").monospace());
                                    ui.label(egui::RichText::new("- window.width").monospace());
                                    ui.label(egui::RichText::new("- window.height").monospace());
                                });
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(Self::tr_lang(
                                            language,
                                            "System and text",
                                            "He thong va text",
                                        ))
                                        .strong(),
                                    );
                                    ui.label(
                                        egui::RichText::new("- system.year / month / day").monospace(),
                                    );
                                    ui.label(
                                        egui::RichText::new("- system.hour / minute / second").monospace(),
                                    );
                                    ui.label(egui::RichText::new("- system.millisecond").monospace());
                                    ui.label(egui::RichText::new("- system.date").monospace());
                                    ui.label(egui::RichText::new("- system.time").monospace());
                                    ui.label(egui::RichText::new("- window.title").monospace());
                                    ui.label(egui::RichText::new("- clipboard.text").monospace());
                                    ui.label(
                                        egui::RichText::new("- timer1.hour ... total_sec").monospace(),
                                    );
                                    ui.label(
                                        egui::RichText::new("- TimerName.hour ... total_sec").monospace(),
                                    );
                                });
                                ui.end_row();
                            });
                    });
            });
    }

    fn sanitize_legacy_ocr_target_text(value: &mut String) -> bool {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return false;
        }
        let looks_like_mojibake = ["Ãƒ", "Ã†", "Â", "â€", "â‚¬"]
            .iter()
            .any(|marker| trimmed.contains(marker));
        if !looks_like_mojibake {
            return false;
        }
        value.clear();
        true
    }

    fn sanitize_legacy_macro_ocr_target_texts(&mut self) -> bool {
        let mut changed = false;
        for group in &mut self.state.macro_groups {
            for preset in &mut group.presets {
                changed |= Self::sanitize_legacy_ocr_target_text(
                    &mut preset.hold_stop_step.ocr_target_text,
                );
                for cond in &mut preset.hold_stop_step.extra_conditions {
                    changed |= Self::sanitize_legacy_ocr_target_text(&mut cond.ocr_target_text);
                }
                for step in &mut preset.steps {
                    changed |= Self::sanitize_legacy_ocr_target_text(&mut step.ocr_target_text);
                    for cond in &mut step.extra_conditions {
                        changed |= Self::sanitize_legacy_ocr_target_text(&mut cond.ocr_target_text);
                    }
                }
            }
        }
        changed
    }

    fn mouse_macro_actions() -> &'static [MacroAction] {
        &[
            MacroAction::MouseLeftClick,
            MacroAction::MouseLeftDown,
            MacroAction::MouseLeftUp,
            MacroAction::MouseRightClick,
            MacroAction::MouseRightDown,
            MacroAction::MouseRightUp,
            MacroAction::MouseMiddleClick,
            MacroAction::MouseMiddleDown,
            MacroAction::MouseMiddleUp,
            MacroAction::MouseX1Click,
            MacroAction::MouseX1Down,
            MacroAction::MouseX1Up,
            MacroAction::MouseX2Click,
            MacroAction::MouseX2Down,
            MacroAction::MouseX2Up,
            MacroAction::MouseWheelUp,
            MacroAction::MouseWheelDown,
            MacroAction::MouseMoveAbsolute,
            MacroAction::MouseMoveRelative,
            MacroAction::LockMouse,
            MacroAction::UnlockMouse,
            MacroAction::PlayMousePathPreset,
        ]
    }

    fn macro_action_is_mouse(action: MacroAction) -> bool {
        Self::mouse_macro_actions().contains(&action)
    }

    fn mouse_click_action_groups()
    -> &'static [(MacroAction, MacroAction, MacroAction, &'static str)] {
        &[
            (
                MacroAction::MouseLeftClick,
                MacroAction::MouseLeftDown,
                MacroAction::MouseLeftUp,
                "mouse-left-click",
            ),
            (
                MacroAction::MouseRightClick,
                MacroAction::MouseRightDown,
                MacroAction::MouseRightUp,
                "mouse-right-click",
            ),
            (
                MacroAction::MouseMiddleClick,
                MacroAction::MouseMiddleDown,
                MacroAction::MouseMiddleUp,
                "mouse-middle-click",
            ),
            (
                MacroAction::MouseX1Click,
                MacroAction::MouseX1Down,
                MacroAction::MouseX1Up,
                "mouse-x1-click",
            ),
            (
                MacroAction::MouseX2Click,
                MacroAction::MouseX2Down,
                MacroAction::MouseX2Up,
                "mouse-x2-click",
            ),
        ]
    }

    fn mouse_leaf_action_groups() -> &'static [MacroAction] {
        &[
            MacroAction::MouseWheelUp,
            MacroAction::MouseWheelDown,
            MacroAction::MouseMoveAbsolute,
            MacroAction::MouseMoveRelative,
            MacroAction::LockMouse,
            MacroAction::UnlockMouse,
            MacroAction::PlayMousePathPreset,
        ]
    }

    fn if_action_groups() -> &'static [MacroAction] {
        &[MacroAction::IfStart, MacroAction::Else, MacroAction::IfEnd]
    }

    fn render_mouse_click_action_group_option(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        current: &mut MacroAction,
        live_sync: &mut bool,
        base_action: MacroAction,
        down_action: MacroAction,
        up_action: MacroAction,
        popup_key: &'static str,
    ) {
        let selected = matches!(*current, action if action == base_action || action == down_action || action == up_action);
        let popup_id = ui.make_persistent_id((id_source, popup_key, "popup"));
        let popup_rect_id = ui.make_persistent_id((id_source, popup_key, "rect"));
        let active_mouse_click_popup_key_id =
            ui.make_persistent_id((id_source, "mouse-click-active-submenu-key"));
        let mut open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(popup_id))
            .unwrap_or(false);
        let inner = ui.allocate_ui_with_layout(
            vec2(58.0, 42.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let label_color = if selected {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                };
                let response = ui.add_sized(
                    [34.0, 24.0],
                    Button::new(Self::macro_action_icon_text(base_action)).selected(selected),
                );
                if response.hovered() || response.clicked() {
                    Self::clear_mouse_click_submenus(ui, id_source);
                    open = true;
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(active_mouse_click_popup_key_id, Some(popup_key))
                    });
                }
                if response.clicked() {
                    *current = base_action;
                    *live_sync = true;
                    ui.close();
                }
                let _popup_response = egui::Popup::from_response(&response)
                    .id(popup_id)
                    .open_bool(&mut open)
                    .align(egui::RectAlign::TOP_START)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .width(140.0)
                    .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                    .show(|ui| {
                        let rect = ui.max_rect();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(popup_rect_id, rect));
                        egui::Grid::new((id_source, popup_key, "grid"))
                            .num_columns(2)
                            .spacing([6.0, 6.0])
                            .show(ui, |ui| {
                                Self::render_macro_action_option(
                                    ui,
                                    language,
                                    current,
                                    down_action,
                                    live_sync,
                                    popup_id,
                                    true,
                                );
                                Self::render_macro_action_option(
                                    ui, language, current, up_action, live_sync, popup_id, true,
                                );
                            });
                    });
                let popup_rect: Option<egui::Rect> =
                    ui.ctx().data(|data| data.get_temp(popup_rect_id));
                if open {
                    if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
                        let mut keep_open_rect = response.rect.expand(10.0);
                        if let Some(rect) = popup_rect {
                            keep_open_rect = keep_open_rect.union(rect.expand(10.0));
                        }
                        if !keep_open_rect.contains(pointer_pos) {
                            open = false;
                            ui.ctx().request_repaint();
                        }
                    } else {
                        open = false;
                        ui.ctx().request_repaint();
                    }
                }
                let active_popup_key = ui
                    .ctx()
                    .data(|data| {
                        data.get_temp::<Option<&'static str>>(active_mouse_click_popup_key_id)
                    })
                    .flatten();
                if let Some(active_popup_key) = active_popup_key {
                    if active_popup_key != popup_key {
                        open = false;
                    }
                }
                ui.ctx().data_mut(|data| data.insert_temp(popup_id, open));
                ui.label(
                    RichText::new(Self::macro_action_short_label(base_action, language))
                        .size(9.0)
                        .color(label_color),
                );
                response
            },
        );
        let response = inner.inner;
        if !open {
            Self::show_instant_hover_tooltip(
                ui,
                &response,
                Self::macro_action_tooltip(base_action, language),
            );
        }
    }

    fn render_if_action_group_option(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        current: &mut MacroAction,
        live_sync: &mut bool,
        action_hover_id: egui::Id,
    ) {
        let selected = matches!(
            *current,
            MacroAction::IfStart | MacroAction::Else | MacroAction::IfEnd
        );
        let owner_id = ui.make_persistent_id("macro-action-submenu-owner");
        let popup_id = ui.make_persistent_id((id_source, "if-submenu-popup"));
        let popup_rect_id = ui.make_persistent_id((id_source, "if-submenu-rect"));
        let mouse_popup_id = ui.make_persistent_id((id_source, "mouse-submenu-popup"));
        let image_popup_id = ui.make_persistent_id((id_source, "image-search-submenu-popup"));
        let timer_popup_id = ui.make_persistent_id((id_source, "timer-submenu-popup"));
        let active_owner = ui
            .ctx()
            .data(|data| data.get_temp::<MacroActionSubmenuKind>(owner_id));
        let mut open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(popup_id))
            .unwrap_or(false);
        if active_owner.is_some_and(|kind| kind != MacroActionSubmenuKind::If) {
            open = false;
        }
        let inner = ui.allocate_ui_with_layout(
            vec2(58.0, 42.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let label_color = if selected {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                };
                let response = ui.add_sized(
                    [34.0, 24.0],
                    Button::new(Self::macro_action_icon_text(MacroAction::IfStart))
                        .selected(selected),
                );
                if response.hovered() || response.clicked() {
                    Self::clear_macro_action_submenus(ui, id_source);
                    open = true;
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(owner_id, MacroActionSubmenuKind::If));
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(action_hover_id, true));
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(mouse_popup_id, false));
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(image_popup_id, false));
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(timer_popup_id, false));
                }
                let _popup_response = egui::Popup::from_response(&response)
                    .id(popup_id)
                    .open_bool(&mut open)
                    .align(egui::RectAlign::BOTTOM_START)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .width(176.0)
                    .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                    .show(|ui| {
                        let rect = ui.max_rect();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(popup_rect_id, rect));
                        egui::Grid::new((id_source, "if-action-grid"))
                            .num_columns(2)
                            .spacing([6.0, 6.0])
                            .show(ui, |ui| {
                                for action in Self::if_action_groups().iter().copied() {
                                    Self::render_macro_action_option(
                                        ui,
                                        language,
                                        current,
                                        action,
                                        live_sync,
                                        action_hover_id,
                                        true,
                                    );
                                }
                            });
                    });
                let popup_rect: Option<egui::Rect> =
                    ui.ctx().data(|data| data.get_temp(popup_rect_id));
                if open {
                    if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
                        let mut keep_open_rect = response.rect.expand(10.0);
                        if let Some(rect) = popup_rect {
                            keep_open_rect = keep_open_rect.union(rect.expand(10.0));
                        }
                        if !keep_open_rect.contains(pointer_pos) {
                            open = false;
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                            });
                            ui.ctx().request_repaint();
                        }
                    } else {
                        open = false;
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                        });
                        ui.ctx().request_repaint();
                    }
                }
                ui.ctx().data_mut(|data| data.insert_temp(popup_id, open));
                ui.label(
                    RichText::new(Self::tr_lang(language, "IF", ""))
                        .size(9.0)
                        .color(label_color),
                );
                response
            },
        );
        let response = inner.inner;
        if !open {
            Self::show_instant_hover_tooltip(
                ui,
                &response,
                Self::macro_action_tooltip(MacroAction::IfStart, language),
            );
        }
    }

    fn render_mouse_action_group_option(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        current: &mut MacroAction,
        live_sync: &mut bool,
        action_hover_id: egui::Id,
    ) {
        let selected = Self::macro_action_is_mouse(*current);
        let owner_id = ui.make_persistent_id("macro-action-submenu-owner");
        let active_mouse_click_popup_key_id =
            ui.make_persistent_id((id_source, "mouse-click-active-submenu-key"));
        let popup_id = ui.make_persistent_id((id_source, "mouse-submenu-popup"));
        let image_popup_id = ui.make_persistent_id((id_source, "image-search-submenu-popup"));
        let timer_popup_id = ui.make_persistent_id((id_source, "timer-submenu-popup"));
        let active_owner = ui
            .ctx()
            .data(|data| data.get_temp::<MacroActionSubmenuKind>(owner_id));
        let top_level_hovered = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(action_hover_id))
            .unwrap_or(false);
        let mut open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(popup_id))
            .unwrap_or(false);
        if active_owner.is_some_and(|kind| kind != MacroActionSubmenuKind::Mouse) {
            open = false;
        }
        if top_level_hovered {
            open = false;
            Self::clear_macro_action_submenus(ui, id_source);
        }
        let inner = ui.allocate_ui_with_layout(
            vec2(58.0, 42.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let response = ui.add_sized(
                    [34.0, 24.0],
                    Button::new(Self::material_icon_text(0xe323, 18.0)).selected(selected),
                );
                if response.hovered() || response.clicked() {
                    Self::clear_macro_action_submenus(ui, id_source);
                    open = true;
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(owner_id, MacroActionSubmenuKind::Mouse));
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(active_mouse_click_popup_key_id, None::<&'static str>)
                    });
                }
                let popup_rect_id = ui.make_persistent_id((id_source, "mouse-submenu-rect"));
                let popup_response = egui::Popup::from_response(&response)
                    .id(popup_id)
                    .open_bool(&mut open)
                    .align(egui::RectAlign::BOTTOM_START)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .width(372.0)
                    .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                    .show(|ui| {
                        let rect = ui.max_rect();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(popup_rect_id, rect));
                        egui::Grid::new((id_source, "mouse-action-grid"))
                            .num_columns(6)
                            .spacing([6.0, 6.0])
                            .show(ui, |ui| {
                                let mut item_index = 0usize;
                                for (base_action, down_action, up_action, popup_key) in
                                    Self::mouse_click_action_groups().iter().copied()
                                {
                                    Self::render_mouse_click_action_group_option(
                                        ui,
                                        language,
                                        id_source,
                                        current,
                                        live_sync,
                                        base_action,
                                        down_action,
                                        up_action,
                                        popup_key,
                                    );
                                    item_index += 1;
                                    if item_index % 6 == 0 {
                                        ui.end_row();
                                    }
                                }
                                for action in Self::mouse_leaf_action_groups().iter().copied() {
                                    let leaf_response = Self::render_macro_action_button(
                                        ui,
                                        language,
                                        current,
                                        action,
                                        action_hover_id,
                                        true,
                                    );
                                    if leaf_response.hovered() || leaf_response.clicked() {
                                        Self::clear_mouse_click_submenus(ui, id_source);
                                        ui.ctx().data_mut(|data| {
                                            data.insert_temp(
                                                active_mouse_click_popup_key_id,
                                                None::<&'static str>,
                                            )
                                        });
                                    }
                                    if leaf_response.clicked() {
                                        *current = action;
                                        *live_sync = true;
                                        ui.close();
                                    }
                                    item_index += 1;
                                    if item_index % 6 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                let active_mouse_click_popup_key = ui
                    .ctx()
                    .data(|data| {
                        data.get_temp::<Option<&'static str>>(active_mouse_click_popup_key_id)
                    })
                    .flatten();
                Self::close_inactive_mouse_click_submenus(
                    ui,
                    id_source,
                    active_mouse_click_popup_key,
                );
                let popup_rect: Option<egui::Rect> =
                    ui.ctx().data(|data| data.get_temp(popup_rect_id));
                if open {
                    if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
                        let mut keep_open_rect = response.rect.expand(10.0);
                        if let Some(rect) = popup_rect {
                            keep_open_rect = keep_open_rect.union(rect.expand(10.0));
                            if rect.contains(pointer_pos) {
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(owner_id, MacroActionSubmenuKind::Mouse)
                                });
                            }
                        }
                        for (_, _, _, popup_key) in
                            Self::mouse_click_action_groups().iter().copied()
                        {
                            let child_popup_rect_id =
                                ui.make_persistent_id((id_source, popup_key, "rect"));
                            if let Some(rect) = ui
                                .ctx()
                                .data(|data| data.get_temp::<egui::Rect>(child_popup_rect_id))
                            {
                                keep_open_rect = keep_open_rect.union(rect.expand(10.0));
                            }
                        }
                        if !keep_open_rect.contains(pointer_pos) {
                            open = false;
                            Self::clear_macro_action_submenus(ui, id_source);
                        }
                    } else {
                        open = false;
                        Self::clear_macro_action_submenus(ui, id_source);
                    }
                }
                ui.ctx().data_mut(|data| data.insert_temp(popup_id, open));
                let label_color = if selected {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                };
                ui.label(
                    RichText::new(Self::tr_lang(language, "Mouse", "Chuột"))
                        .size(9.0)
                        .color(label_color),
                );
                response
            },
        );
        let response = inner.inner;
        if !open {
            Self::show_instant_hover_tooltip(
                ui,
                &response,
                Self::tr_lang(
                    language,
                    "Mouse\nOpen mouse click, wheel, and move actions.",
                    "Chuột\nMở các action click, lăn và di chuyển chuột.",
                ),
            );
        }
    }

    fn image_search_macro_actions() -> &'static [MacroAction] {
        &[
            MacroAction::StartVisionSearch,
            MacroAction::ScanVisionOnce,
            MacroAction::StopVision,
        ]
    }

    fn macro_action_is_image_search(action: MacroAction) -> bool {
        Self::image_search_macro_actions().contains(&action)
    }

    fn render_image_search_action_group_option(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        current: &mut MacroAction,
        live_sync: &mut bool,
        action_hover_id: egui::Id,
    ) {
        let selected = Self::macro_action_is_image_search(*current);
        let owner_id = ui.make_persistent_id("macro-action-submenu-owner");
        let popup_id = ui.make_persistent_id((id_source, "image-search-submenu-popup"));
        let mouse_popup_id = ui.make_persistent_id((id_source, "mouse-submenu-popup"));
        let timer_popup_id = ui.make_persistent_id((id_source, "timer-submenu-popup"));
        let active_owner = ui
            .ctx()
            .data(|data| data.get_temp::<MacroActionSubmenuKind>(owner_id));
        let top_level_hovered = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(action_hover_id))
            .unwrap_or(false);
        let mut open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(popup_id))
            .unwrap_or(false);
        if active_owner.is_some_and(|kind| kind != MacroActionSubmenuKind::ImageSearch) {
            open = false;
        }
        if top_level_hovered {
            open = false;
            ui.ctx()
                .data_mut(|data| data.insert_temp(owner_id, None::<MacroActionSubmenuKind>));
            ui.ctx().request_repaint();
        }
        let inner = ui.allocate_ui_with_layout(
            vec2(58.0, 42.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let response = ui.add_sized(
                    [34.0, 24.0],
                    Button::new(Self::material_icon_text(0xe8b6, 18.0)).selected(selected),
                );
                if response.hovered() || response.clicked() {
                    Self::clear_macro_action_submenus(ui, id_source);
                    open = true;
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(owner_id, MacroActionSubmenuKind::ImageSearch)
                    });
                }
                let popup_rect_id = ui.make_persistent_id((id_source, "image-search-submenu-rect"));
                let popup_response = egui::Popup::from_response(&response)
                    .id(popup_id)
                    .open_bool(&mut open)
                    .align(egui::RectAlign::BOTTOM_START)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .width(220.0)
                    .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                    .show(|ui| {
                        let rect = ui.max_rect();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(popup_rect_id, rect));
                        egui::Grid::new((id_source, "image-search-action-grid"))
                            .num_columns(3)
                            .spacing([6.0, 6.0])
                            .show(ui, |ui| {
                                for (index, action) in Self::image_search_macro_actions()
                                    .iter()
                                    .copied()
                                    .enumerate()
                                {
                                    Self::render_macro_action_option(
                                        ui,
                                        language,
                                        current,
                                        action,
                                        live_sync,
                                        action_hover_id,
                                        true,
                                    );
                                    if (index + 1) % 3 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                let popup_rect: Option<egui::Rect> =
                    ui.ctx().data(|data| data.get_temp(popup_rect_id));
                if open {
                    if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
                        let mut keep_open_rect = response.rect.expand(10.0);
                        if let Some(rect) = popup_rect {
                            keep_open_rect = keep_open_rect.union(rect.expand(10.0));
                            if rect.contains(pointer_pos) {
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(owner_id, MacroActionSubmenuKind::ImageSearch)
                                });
                            }
                        }
                        if !keep_open_rect.contains(pointer_pos) {
                            open = false;
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                            });
                            ui.ctx().request_repaint();
                        }
                    } else {
                        open = false;
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                        });
                        ui.ctx().request_repaint();
                    }
                }
                ui.ctx().data_mut(|data| data.insert_temp(popup_id, open));
                let label_color = if selected {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                };
                ui.label(
                    RichText::new(Self::tr_lang(language, "Image", "Image"))
                        .size(9.0)
                        .color(label_color),
                );
                response
            },
        );
        let response = inner.inner;
        if !open {
            Self::show_instant_hover_tooltip(
                ui,
                &response,
                Self::tr_lang(
                    language,
                    "Image\nOpen image search start, trigger, and stop actions.",
                    "Image\nMở các action bắt đầu, trigger và dừng image search.",
                ),
            );
        }
    }

    fn timer_macro_actions() -> &'static [MacroAction] {
        &[
            MacroAction::StartTimerPreset,
            MacroAction::PauseTimerPreset,
            MacroAction::StopTimerPreset,
        ]
    }

    fn macro_action_is_timer(action: MacroAction) -> bool {
        Self::timer_macro_actions().contains(&action)
    }

    fn render_timer_action_group_option(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        current: &mut MacroAction,
        live_sync: &mut bool,
        action_hover_id: egui::Id,
    ) {
        let selected = Self::macro_action_is_timer(*current);
        let owner_id = ui.make_persistent_id("macro-action-submenu-owner");
        let popup_id = ui.make_persistent_id((id_source, "timer-submenu-popup"));
        let mouse_popup_id = ui.make_persistent_id((id_source, "mouse-submenu-popup"));
        let image_popup_id = ui.make_persistent_id((id_source, "image-search-submenu-popup"));
        let active_owner = ui
            .ctx()
            .data(|data| data.get_temp::<MacroActionSubmenuKind>(owner_id));
        let top_level_hovered = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(action_hover_id))
            .unwrap_or(false);
        let mut open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(popup_id))
            .unwrap_or(false);
        if active_owner.is_some_and(|kind| kind != MacroActionSubmenuKind::Timer) {
            open = false;
        }
        if top_level_hovered {
            open = false;
            ui.ctx()
                .data_mut(|data| data.insert_temp(owner_id, None::<MacroActionSubmenuKind>));
            ui.ctx().request_repaint();
        }
        let inner = ui.allocate_ui_with_layout(
            vec2(58.0, 42.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let response = ui.add_sized(
                    [34.0, 24.0],
                    Button::new(Self::material_icon_text(0xe425, 18.0)).selected(selected),
                );
                if response.hovered() || response.clicked() {
                    Self::clear_macro_action_submenus(ui, id_source);
                    open = true;
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(owner_id, MacroActionSubmenuKind::Timer));
                }
                let popup_rect_id = ui.make_persistent_id((id_source, "timer-submenu-rect"));
                let popup_response = egui::Popup::from_response(&response)
                    .id(popup_id)
                    .open_bool(&mut open)
                    .align(egui::RectAlign::BOTTOM_START)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .width(220.0)
                    .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                    .show(|ui| {
                        let rect = ui.max_rect();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(popup_rect_id, rect));
                        egui::Grid::new((id_source, "timer-action-grid"))
                            .num_columns(3)
                            .spacing([6.0, 6.0])
                            .show(ui, |ui| {
                                for (index, action) in
                                    Self::timer_macro_actions().iter().copied().enumerate()
                                {
                                    Self::render_macro_action_option(
                                        ui,
                                        language,
                                        current,
                                        action,
                                        live_sync,
                                        action_hover_id,
                                        true,
                                    );
                                    if (index + 1) % 3 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                let popup_rect: Option<egui::Rect> =
                    ui.ctx().data(|data| data.get_temp(popup_rect_id));
                if open {
                    if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
                        let mut keep_open_rect = response.rect.expand(10.0);
                        if let Some(rect) = popup_rect {
                            keep_open_rect = keep_open_rect.union(rect.expand(10.0));
                            if rect.contains(pointer_pos) {
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(owner_id, MacroActionSubmenuKind::Timer)
                                });
                            }
                        }
                        if !keep_open_rect.contains(pointer_pos) {
                            open = false;
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                            });
                            ui.ctx().request_repaint();
                        }
                    } else {
                        open = false;
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                        });
                        ui.ctx().request_repaint();
                    }
                }
                ui.ctx().data_mut(|data| data.insert_temp(popup_id, open));
                let label_color = if selected {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                };
                ui.label(
                    RichText::new(Self::tr_lang(language, "Timer", "Hẹn giờ"))
                        .size(9.0)
                        .color(label_color),
                );
                response
            },
        );
        let response = inner.inner;
        if !open {
            Self::show_instant_hover_tooltip(
                ui,
                &response,
                Self::tr_lang(
                    language,
                    "Timer\nOpen start, pause, and stop timer actions.",
                    "Hẹn giờ\nMở các action bắt đầu, tạm dừng và dừng hẹn giờ.",
                ),
            );
        }
    }

    fn render_custom_preset_step_draft_popup(
        ui: &mut egui::Ui,
        response: &egui::Response,
        anchor_response: &egui::Response,
        step: &mut MacroStep,
        id_source: impl std::hash::Hash,
        step_index: Option<usize>,
        language: UiLanguage,
        command_presets: &[CommandPreset],
        is_generating: bool,
    ) -> (
        bool,
        Option<(Option<usize>, String, String, bool)>,
        Option<(Option<usize>, String, String, bool, bool)>,
        Option<u32>,
    ) {
        let mut changed = false;
        let mut save_request = None;
        let mut save_and_open_ai_request = None;
        let mut open_ai_preset_id = None;
        if step.action != MacroAction::TriggerCommandPreset {
            return (false, None, None, None);
        }
        let popup_id = ui.make_persistent_id((id_source, "custom-preset-draft-popup"));
        let pointer_pos = ui.input(|input| input.pointer.hover_pos());
        let mut open = response.hovered()
            || ui
                .ctx()
                .data(|data| data.get_temp::<bool>(popup_id))
                .unwrap_or(false);
        let resolved_preset = command_presets
            .iter()
            .find(|preset| {
                let key = step.key.trim();
                if key.is_empty() {
                    return false;
                }
                preset.id.to_string() == key || preset.name.trim().eq_ignore_ascii_case(key)
            })
            .cloned();
        if step.command_preset_command.trim().is_empty() {
            if let Some(preset) = resolved_preset.as_ref() {
                step.command_preset_command = preset.command.clone();
            }
        }
        let is_saved_custom_preset = resolved_preset.as_ref().is_some_and(|preset| {
            preset.command.trim() == step.command_preset_command.trim()
                && !step.command_preset_command.trim().is_empty()
                && preset.use_powershell == step.command_preset_use_powershell
        });
        if open {
            let popup_size = vec2(320.0, 132.0);
            let mut pos = anchor_response.rect.right_top() + vec2(2.0, 0.0);
            let screen_rect = ui.ctx().content_rect();
            if pos.x + popup_size.x > screen_rect.right() {
                pos.x = anchor_response.rect.left() - popup_size.x - 2.0;
            }
            let area = egui::Area::new(popup_id)
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .interactable(true);
            let area_response = area.show(ui.ctx(), |ui| {
                let mut frame = egui::Frame::popup(ui.style());
                let (fill, stroke_color) = if step.command_preset_use_powershell {
                    (
                        Color32::from_rgba_premultiplied(20, 35, 55, 245),
                        Color32::from_rgb(90, 190, 255),
                    )
                } else {
                    (
                        Color32::from_rgba_premultiplied(45, 30, 15, 245),
                        Color32::from_rgb(255, 170, 75),
                    )
                };
                frame = frame
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.5, stroke_color));
                frame.show(ui, |ui| {
                    ui.set_min_width(320.0);
                    let mut trigger_ai = false;
                    ui.horizontal(|ui| {
                        ui.label(Self::tr_lang(language, "Custom command", "Custom command"));
                        if is_generating {
                            let (rect, _resp) = ui
                                .allocate_exact_size(egui::vec2(36.0, 20.0), egui::Sense::hover());
                            Self::draw_spinning_wand(ui, rect, Color32::from_rgb(255, 220, 100));
                        } else {
                            let ai_btn = egui::Button::new(Self::ai_badge_text(true))
                                .fill(Self::ai_badge_fill())
                                .stroke(Self::ai_badge_stroke());
                            if ui
                                .add(ai_btn)
                                .on_hover_text(Self::tr_lang(
                                    language,
                                    "Generate or edit command with AI",
                                    "Tạo hoặc sửa câu lệnh bằng AI",
                                ))
                                .clicked()
                            {
                                trigger_ai = true;
                            }
                        }
                        ui.add_space(8.0);
                        changed |= ui
                            .radio_value(&mut step.command_preset_use_powershell, false, "CMD")
                            .changed();
                        ui.add_space(4.0);
                        changed |= ui
                            .radio_value(
                                &mut step.command_preset_use_powershell,
                                true,
                                "PowerShell",
                            )
                            .changed();
                    });
                    if trigger_ai {
                        if let Some(preset) = resolved_preset.as_ref() {
                            open_ai_preset_id = Some(preset.id);
                        } else {
                            let preset_name = if step.key.trim().is_empty() {
                                "Custom Command Step".to_owned()
                            } else {
                                step.key.trim().to_owned()
                            };
                            let command_text =
                                ai::normalize_command_text(&step.command_preset_command);
                            save_and_open_ai_request = Some((
                                step_index,
                                preset_name,
                                command_text,
                                step.command_preset_use_powershell,
                                true, // is_ad_hoc
                            ));
                        }
                    }
                    let is_dark_theme = ui.visuals().dark_mode;
                    let hint_color = if is_dark_theme {
                        Color32::from_rgba_unmultiplied(140, 140, 140, 150)
                    } else {
                        Color32::from_rgba_unmultiplied(100, 100, 100, 150)
                    };
                    let command_changed = Self::render_expandable_command_text_edit(
                        ui,
                        &mut step.command_preset_command,
                        ui.id().with((step_index, "command-preset-cmd")),
                        "shutdown /s /t 0",
                    )
                    .changed();
                    if command_changed {
                        changed = true;
                    }
                    if resolved_preset.is_none() {
                        ui.horizontal(|ui| {
                            ui.label(Self::tr_lang(language, "Preset name:", "Tên preset:"));
                            let name_changed = ui
                                .add(
                                    TextEdit::singleline(&mut step.key)
                                        .hint_text(Self::tr_lang(
                                            language,
                                            "Enter name...",
                                            "Nhập tên...",
                                        ))
                                        .desired_width(180.0),
                                )
                                .changed();
                            if name_changed {
                                changed = true;
                            }
                        });
                        ui.add_space(2.0);
                    }
                    if !is_saved_custom_preset {
                        ui.horizontal(|ui| {
                            let save_enabled = !step.key.trim().is_empty()
                                && !ai::normalize_command_text(&step.command_preset_command)
                                    .trim()
                                    .is_empty();
                            let btn_text = if resolved_preset.is_some() {
                                Self::tr_lang(language, "Update custom preset", "Cập nhật preset")
                            } else {
                                Self::tr_lang(
                                    language,
                                    "Save as custom preset",
                                    "Lưu thành preset mới",
                                )
                            };
                            if ui
                                .add_enabled(save_enabled, egui::Button::new(btn_text))
                                .clicked()
                            {
                                let save_name = resolved_preset
                                    .as_ref()
                                    .map(|p| p.name.clone())
                                    .unwrap_or_else(|| step.key.trim().to_owned());
                                save_request = Some((
                                    step_index,
                                    save_name,
                                    ai::normalize_command_text(&step.command_preset_command),
                                    step.command_preset_use_powershell,
                                ));
                            }
                        });
                    }
                });
            });
            let icon_pos = area_response.response.rect.right_top() + vec2(-24.0, 12.0);
            ui.painter().text(
                icon_pos,
                egui::Align2::CENTER_CENTER,
                char::from_u32(0xeb8e).unwrap_or('?').to_string(),
                egui::FontId::new(15.0, FontFamily::Name(MATERIAL_ICONS_FONT.into())),
                ui.visuals().weak_text_color(),
            );
            let popup_rect = area_response.response.rect.expand(12.0);
            let hover_popup = pointer_pos.is_some_and(|pos| popup_rect.contains(pos));
            open = response.hovered() || hover_popup;
            ui.ctx().data_mut(|data| data.insert_temp(popup_id, open));
            return (
                changed,
                save_request,
                save_and_open_ai_request,
                open_ai_preset_id,
            );
        }
        ui.ctx().data_mut(|data| data.insert_temp(popup_id, open));
        (
            changed,
            save_request,
            save_and_open_ai_request,
            open_ai_preset_id,
        )
    }

    fn render_multi_key_capture_chips(
        ui: &mut egui::Ui,
        language: UiLanguage,
        keys_str: &mut String,
        active: bool,
        mut on_capture_click: impl FnMut(),
        mut on_clear_click: impl FnMut(),
    ) {
        ui.horizontal(|ui| {
            // Nút bấm hình bàn phím để capture
            let keyboard_btn = ui
                .add_sized(
                    [18.0, 18.0],
                    egui::Button::new(Self::material_icon_text(0xe312, 14.0)) // icon bàn phím ⌨️
                        .fill(if active {
                            egui::Color32::from_rgba_premultiplied(72, 156, 116, 120)
                        } else {
                            ui.visuals().widgets.noninteractive.bg_fill
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if active {
                                egui::Color32::from_rgb(126, 224, 182)
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        )),
                )
                .on_hover_text(Self::tr_lang(
                    language,
                    "Click to capture keys/mouse (Multi-key supported)",
                    "Bấm để bắt phím/chuột (Hỗ trợ gán nhiều phím)",
                ));
            if keyboard_btn.clicked() {
                on_capture_click();
            }
            // Hiển thị các phím tắt dạng chip
            let keys: Vec<String> = keys_str
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect();
            if keys.is_empty() {
                if active {
                    ui.label(
                        egui::RichText::new(Self::tr_lang(language, "Capturing...", "Đang bắt..."))
                            .color(egui::Color32::from_rgb(255, 232, 96))
                            .strong(),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(Self::tr_lang(
                            language,
                            "No key assigned",
                            "Chưa gán phím",
                        ))
                        .weak()
                        .italics(),
                    );
                }
            } else {
                let mut remove_key = None;
                for key in &keys {
                    let chip_btn = ui
                        .add(
                            egui::Button::new(egui::RichText::new(key).monospace())
                                .min_size(egui::vec2(0.0, 20.0)),
                        )
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Click to remove this key",
                            "Bấm để xóa phím này",
                        ));
                    if chip_btn.clicked() {
                        remove_key = Some(key.clone());
                    }
                }
                if let Some(rk) = remove_key {
                    let remaining: Vec<String> = keys.into_iter().filter(|k| k != &rk).collect();
                    *keys_str = remaining.join(",");
                    on_clear_click();
                }
            }
        });
    }

    fn render_key_capture_chips(
        ui: &mut egui::Ui,
        language: UiLanguage,
        keys_str: &mut String,
        active: bool,
        mut on_capture_click: impl FnMut(),
        mut on_change: impl FnMut(),
    ) {
        const LETTERS: &[&str] = &[
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        ];
        const NUMBERS: &[&str] = &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
        const SYMBOLS: &[&str] = &[";", "=", ",", "-", ".", "/", "`", "[", "\\", "]", "'"];
        const NAVIGATION: &[&str] = &[
            "Escape",
            "Enter",
            "Space",
            "Backspace",
            "Tab",
            "Insert",
            "Delete",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Left",
            "Up",
            "Right",
            "Down",
            "PrintScreen",
            "Pause",
        ];
        const MODIFIERS: &[&str] = &[
            "Ctrl",
            "Alt",
            "Shift",
            "Win",
            "CapsLock",
            "NumLock",
            "ScrollLock",
            "Apps",
        ];
        const NUMPAD: &[&str] = &[
            "Numpad0",
            "Numpad1",
            "Numpad2",
            "Numpad3",
            "Numpad4",
            "Numpad5",
            "Numpad6",
            "Numpad7",
            "Numpad8",
            "Numpad9",
            "NumpadMultiply",
            "NumpadAdd",
            "NumpadSubtract",
            "NumpadDecimal",
            "NumpadDivide",
        ];
        let row_height = ui.spacing().interact_size.y.max(18.0);
        let icon_size = (row_height - 6.0).max(12.0);
        ui.horizontal_wrapped(|ui| {
            let capture_btn = ui
                .add_sized(
                    [row_height, row_height],
                    egui::Button::new(Self::material_icon_text(0xe312, icon_size))
                        .fill(if active {
                            egui::Color32::from_rgba_premultiplied(72, 156, 116, 120)
                        } else {
                            ui.visuals().widgets.noninteractive.bg_fill
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if active {
                                egui::Color32::from_rgb(126, 224, 182)
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        )),
                )
                .on_hover_text(Self::tr_lang(
                    language,
                    "Click to capture one key",
                    "Bấm để bắt 1 phím",
                ));
            if capture_btn.clicked() {
                on_capture_click();
            }
            let add_menu = ui.scope(|ui| {
                ui.spacing_mut().interact_size = egui::vec2(row_height, row_height);
                ui.menu_button(Self::material_icon_text(0xe5d2, icon_size), |ui| {
                    ui.set_min_width(260.0);
                    ui.set_max_width(320.0);
                    let mut add_key = |ui: &mut egui::Ui, key: &str| {
                        if ui.button(key).clicked() {
                            if Self::append_key_list_value(keys_str, key) {
                                on_change();
                            }
                            ui.close_menu();
                        }
                    };
                    egui::ScrollArea::vertical()
                        .max_height(280.0)
                        .show(ui, |ui| {
                            ui.label(Self::tr_lang(language, "Letters (A-Z)", "Chữ cái (A-Z)"));
                            ui.horizontal_wrapped(|ui| {
                                for key in LETTERS {
                                    add_key(ui, key);
                                }
                            });
                            ui.separator();
                            ui.label(Self::tr_lang(language, "Numbers & Symbols", "Số & ký tự"));
                            ui.horizontal_wrapped(|ui| {
                                for key in NUMBERS {
                                    add_key(ui, key);
                                }
                                for key in SYMBOLS {
                                    add_key(ui, key);
                                }
                            });
                            ui.separator();
                            ui.label(Self::tr_lang(language, "Navigation", "Điều hướng"));
                            ui.horizontal_wrapped(|ui| {
                                for key in NAVIGATION {
                                    add_key(ui, key);
                                }
                            });
                            ui.separator();
                            ui.label(Self::tr_lang(language, "Function", "Phím chức năng"));
                            ui.horizontal_wrapped(|ui| {
                                for num in 1..=24 {
                                    let key = format!("F{}", num);
                                    add_key(ui, &key);
                                }
                            });
                            ui.separator();
                            ui.label(Self::tr_lang(language, "Numpad", "Bàn phím số"));
                            ui.horizontal_wrapped(|ui| {
                                for key in NUMPAD {
                                    add_key(ui, key);
                                }
                            });
                            ui.separator();
                            ui.label(Self::tr_lang(
                                language,
                                "Modifiers & Locks",
                                "Phím khóa & bổ trợ",
                            ));
                            ui.horizontal_wrapped(|ui| {
                                for key in MODIFIERS {
                                    add_key(ui, key);
                                }
                            });
                        });
                })
            });
            add_menu.response.on_hover_text(Self::tr_lang(
                language,
                "Manually add a key",
                "Thêm phím thủ công",
            ));
            let keys = Self::split_key_list(keys_str);
            if keys.is_empty() {
                if active {
                    ui.label(
                        egui::RichText::new(Self::tr_lang(language, "Capturing...", "Đang bắt..."))
                            .color(egui::Color32::from_rgb(255, 232, 96))
                            .strong(),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(Self::tr_lang(
                            language,
                            "No key assigned",
                            "Chưa gán phím",
                        ))
                        .weak()
                        .italics(),
                    );
                }
            } else {
                for key in &keys {
                    let chip_btn = ui
                        .add(
                            egui::Button::new(egui::RichText::new(key).monospace())
                                .min_size(egui::vec2(0.0, row_height)),
                        )
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Click to remove this key",
                            "Bấm để xóa phím này",
                        ));
                    if chip_btn.clicked() && Self::remove_key_list_value(keys_str, key) {
                        on_change();
                    }
                }
            }
        });
    }

    fn render_extra_conditions(
        ui: &mut egui::Ui,
        extra_conditions: &mut Vec<ExtraCondition>,
        group_id: u32,
        preset_id: u32,
        step_index: usize,
        timer_names: &[String],
        ocr_preset_options: &[(u32, String)],
        image_search_preset_options: &[(u32, String)],
        all_presets: &[(u32, String)],
        all_groups: &[(u32, String, Vec<(u32, String)>)],
        language: UiLanguage,
        live_sync: &mut bool,
        timer_presets: &[crate::model::TimerPreset],
        vietnamese_input_enabled: bool,
        vietnamese_input_mode: crate::model::VietnameseInputMode,
        pending_pick_target: &mut Option<MouseMoveAbsoluteCaptureTarget>,
        capture_target_snapshot: Option<&CaptureRequest>,
        next_capture_target: &mut Option<CaptureRequest>,
        cancel_active_capture: &mut bool,
        is_hold_stop: bool,
    ) {
        let mut remove_extra_idx = None;
        for (extra_idx, cond) in extra_conditions.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let (cb_rect, _) =
                    ui.allocate_exact_size(egui::vec2(56.0, 21.0), egui::Sense::hover());
                let mut cb_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(cb_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                egui::ComboBox::from_id_salt((
                    group_id,
                    preset_id,
                    step_index,
                    extra_idx,
                    "hold-extra-join-cb",
                ))
                .width(56.0)
                .selected_text(if cond.join_operator.eq_ignore_ascii_case("OR") {
                    Self::tr_lang(language, "OR", "HOẶC")
                } else {
                    Self::tr_lang(language, "AND", "VÀ")
                })
                .show_ui(&mut cb_ui, |ui| {
                    for op in &["AND", "OR"] {
                        let label = if *op == "AND" {
                            Self::tr_lang(language, "AND", "VÀ")
                        } else {
                            Self::tr_lang(language, "OR", "HOẶC")
                        };
                        if ui
                            .selectable_label(cond.join_operator.eq_ignore_ascii_case(op), label)
                            .clicked()
                        {
                            cond.join_operator = op.to_string();
                            *live_sync = true;
                        }
                    }
                });
                let cond_text = match cond.condition_type {
                    IfConditionType::Variable => Self::tr_lang(language, "Variable", "Biến"),
                    IfConditionType::PixelColor => {
                        Self::tr_lang(language, "Pixel Color", "Màu điểm")
                    }
                    IfConditionType::VisionMatch => {
                        Self::tr_lang(language, "Vision Match", "Hình ảnh")
                    }
                    IfConditionType::KeyHeld => {
                        Self::tr_lang(language, "Input Held", "Giữ phím/chuột")
                    }
                    IfConditionType::MouseHeld => {
                        Self::tr_lang(language, "Input Held", "Giữ phím/chuột")
                    }
                    IfConditionType::MousePosition => {
                        Self::tr_lang(language, "Mouse Position", "Tọa độ chuột")
                    }
                    IfConditionType::PresetRunning => {
                        Self::tr_lang(language, "Preset Running", "Preset đang chạy")
                    }
                    IfConditionType::OcrMatch => {
                        Self::tr_lang(language, "OCR Match", "Từ tìm (OCR)")
                    }
                    _ => "-",
                };
                egui::ComboBox::from_id_salt((
                    group_id,
                    preset_id,
                    step_index,
                    extra_idx,
                    "hold-extra-cond-type-cb",
                ))
                .width(100.0)
                .selected_text(cond_text)
                .show_ui(ui, |ui| {
                    let options = [
                        (
                            IfConditionType::Variable,
                            Self::tr_lang(language, "Variable", "Biến"),
                        ),
                        (
                            IfConditionType::PixelColor,
                            Self::tr_lang(language, "Pixel Color", "Màu điểm"),
                        ),
                        (
                            IfConditionType::VisionMatch,
                            Self::tr_lang(language, "Vision Match", "Hình ảnh"),
                        ),
                        (
                            IfConditionType::KeyHeld,
                            Self::tr_lang(language, "Input Held", "Giữ phím/chuột"),
                        ),
                        (
                            IfConditionType::MousePosition,
                            Self::tr_lang(language, "Mouse Position", "Tọa độ chuột"),
                        ),
                        (
                            IfConditionType::PresetRunning,
                            Self::tr_lang(language, "Preset Running", "Preset đang chạy"),
                        ),
                    ];
                    for (opt_type, opt_label) in options {
                        if ui
                            .selectable_label(cond.condition_type == opt_type, opt_label)
                            .clicked()
                        {
                            cond.condition_type = opt_type;
                            *live_sync = true;
                        }
                    }
                });
                match cond.condition_type {
                    IfConditionType::Variable => {
                        let cond_var_id = ui.id().with((step_index, extra_idx, "extra-var-input"));
                        let response = Self::render_variable_text_edit(
                            ui,
                            &mut cond.variable_name,
                            cond_var_id,
                            76.0,
                            140.0,
                            21.0,
                            21.0,
                            Self::tr_lang(language, "value/expr", "biến/expr"),
                            false,
                        );
                        Self::apply_vietnamese_input_if_changed(
                            &response,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut cond.variable_name,
                        );
                        *live_sync |= response.changed();
                        Self::render_variable_suggestions(
                            ui,
                            &response,
                            &mut cond.variable_name,
                            timer_names,
                            language,
                        );
                        egui::ComboBox::from_id_salt((
                            group_id,
                            preset_id,
                            step_index,
                            extra_idx,
                            "hold-extra-op-cb",
                        ))
                        .width(55.0)
                        .selected_text(&cond.operator)
                        .show_ui(ui, |ui| {
                            for op in &["==", ">", "<", ">=", "<=", "!="] {
                                if ui.selectable_label(cond.operator == *op, *op).clicked() {
                                    cond.operator = op.to_string();
                                    *live_sync = true;
                                }
                            }
                        });
                        let cond_expr_id =
                            ui.id().with((step_index, extra_idx, "extra-expr-input"));
                        let response2 = Self::render_variable_text_edit(
                            ui,
                            &mut cond.expression,
                            cond_expr_id,
                            76.0,
                            180.0,
                            21.0,
                            21.0,
                            Self::tr_lang(language, "value/expr", "giá trị/expr"),
                            false,
                        );
                        Self::apply_vietnamese_input_if_changed(
                            &response2,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut cond.expression,
                        );
                        *live_sync |= response2.changed();
                        Self::render_variable_suggestions(
                            ui,
                            &response2,
                            &mut cond.expression,
                            timer_names,
                            language,
                        );
                        let left_expr = cond.variable_name.trim();
                        if !left_expr.is_empty() {
                            let left_val =
                                crate::overlay::evaluate_interpolated_math_expression(left_expr);
                            ui.add_space(2.0);
                            ui.label(
                                egui::RichText::new(format!("({})", left_val))
                                    .size(10.0)
                                    .color(egui::Color32::from_rgb(0, 191, 255)),
                            )
                            .on_hover_text(Self::tr_lang(
                                language,
                                "Evaluated left expression",
                                "Giá trị biểu thức bên trái",
                            ));
                        }
                    }
                    IfConditionType::OcrMatch => {
                        let selected_id = cond.ocr_preset_id;
                        let selected_label = selected_id
                            .and_then(|id| {
                                ocr_preset_options
                                    .iter()
                                    .find(|(preset_id, _)| *preset_id == id)
                                    .map(|(_, label)| label.clone())
                            })
                            .unwrap_or_else(|| {
                                Self::tr_lang(language, "Select OCR", "Chọn OCR").to_owned()
                            });
                        egui::ComboBox::from_id_salt((
                            group_id,
                            preset_id,
                            step_index,
                            extra_idx,
                            "hold-extra-ocr-preset-cb",
                        ))
                        .width(146.0)
                        .selected_text(selected_label)
                        .show_ui(ui, |ui| {
                            for (preset_option_id, preset_option_label) in ocr_preset_options {
                                if ui
                                    .selectable_label(
                                        selected_id == Some(*preset_option_id),
                                        preset_option_label,
                                    )
                                    .clicked()
                                {
                                    cond.ocr_preset_id = Some(*preset_option_id);
                                    *live_sync = true;
                                }
                            }
                        });
                        let var_target_id = ui.id().with((
                            step_index,
                            extra_idx,
                            "hold-stop-ocr-target-text-if-extra",
                        ));
                        *live_sync |=
                            Self::sanitize_legacy_ocr_target_text(&mut cond.ocr_target_text);
                        let response_target = Self::render_variable_text_edit(
                            ui,
                            &mut cond.ocr_target_text,
                            var_target_id,
                            76.0,
                            180.0,
                            21.0,
                            21.0,
                            Self::tr_lang(language, "Target text", "Van ban can tim"),
                            false,
                        );
                        Self::apply_vietnamese_input_if_changed(
                            &response_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut cond.ocr_target_text,
                        );
                        *live_sync |= response_target.changed();
                        Self::render_variable_suggestions(
                            ui,
                            &response_target,
                            &mut cond.ocr_target_text,
                            timer_names,
                            language,
                        );
                    }
                    IfConditionType::PixelColor => {
                        ui.label("X:");
                        let resp_x = ui.add(egui::DragValue::new(&mut cond.x));
                        *live_sync |= resp_x.changed();
                        ui.label("Y:");
                        let resp_y = ui.add(egui::DragValue::new(&mut cond.y));
                        *live_sync |= resp_y.changed();
                        let resp_col = ui.add_sized(
                            [76.0, 21.0],
                            egui::TextEdit::singleline(&mut cond.target_color).hint_text("R,G,B"),
                        );
                        *live_sync |= resp_col.changed();
                        // Swatch màu trực quan
                        let swatch_color = Self::parse_rgb_color(&cond.target_color)
                            .unwrap_or(egui::Color32::TRANSPARENT);
                        let (rect, _response) =
                            ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 3.0, swatch_color);
                        ui.label(Self::tr_lang(language, "Tol:", "Sai số:"));
                        let resp_tol =
                            ui.add(egui::DragValue::new(&mut cond.color_tolerance).range(0..=255));
                        *live_sync |= resp_tol.changed();
                        // Nút pick tọa độ & màu từ màn hình
                        let pick_btn = ui
                            .add_sized(
                                [18.0, 18.0],
                                egui::Button::new(Self::material_icon_text(0xe3b4, 14.0)),
                            )
                            .on_hover_text(Self::tr_lang(
                                language,
                                "Click on screen to capture position & color",
                                "Bấm vào màn hình để lấy tọa độ và màu sắc",
                            ));
                        if pick_btn.clicked() {
                            *pending_pick_target = Some(MouseMoveAbsoluteCaptureTarget {
                                group_id: Some(group_id),
                                preset_id,
                                step_index,
                                capture_kind: MouseCaptureKind::ExtraCondPixelColor,
                                extra_cond_index: Some(extra_idx),
                                is_hold_stop,
                            });
                        }
                    }
                    IfConditionType::VisionMatch => {
                        let selected_id = cond.vision_preset_id;
                        let selected_label = selected_id
                            .and_then(|id| {
                                image_search_preset_options
                                    .iter()
                                    .find(|(pid, _)| *pid == id)
                                    .map(|(_, name)| name.clone())
                            })
                            .unwrap_or_else(|| {
                                Self::tr_lang(language, "Select Image", "Chọn Ảnh").to_owned()
                            });
                        egui::ComboBox::from_id_salt((
                            group_id,
                            preset_id,
                            step_index,
                            extra_idx,
                            "hold-extra-vision-preset-cb",
                        ))
                        .width(146.0)
                        .selected_text(selected_label)
                        .show_ui(ui, |ui| {
                            for (pid, pname) in image_search_preset_options {
                                if ui
                                    .selectable_label(selected_id == Some(*pid), pname)
                                    .clicked()
                                {
                                    cond.vision_preset_id = Some(*pid);
                                    *live_sync = true;
                                }
                            }
                        });
                    }
                    IfConditionType::KeyHeld => {
                        let capture_target = CaptureRequest::MacroStepInput {
                            group_id,
                            preset_id,
                            step_index,
                            extra_cond_index: Some(extra_idx),
                        };
                        let active = capture_target_snapshot == Some(&capture_target);
                        Self::render_multi_key_capture_chips(
                            ui,
                            language,
                            &mut cond.key_held_name,
                            active,
                            || {
                                if active {
                                    *cancel_active_capture = true;
                                } else {
                                    *next_capture_target = Some(capture_target.clone());
                                }
                            },
                            || {
                                *live_sync = true;
                            },
                        );
                    }
                    IfConditionType::MouseHeld => {
                        let capture_target = CaptureRequest::MacroStepInput {
                            group_id,
                            preset_id,
                            step_index,
                            extra_cond_index: Some(extra_idx),
                        };
                        let active = capture_target_snapshot == Some(&capture_target);
                        Self::render_multi_key_capture_chips(
                            ui,
                            language,
                            &mut cond.mouse_button,
                            active,
                            || {
                                if active {
                                    *cancel_active_capture = true;
                                } else {
                                    *next_capture_target = Some(capture_target.clone());
                                }
                            },
                            || {
                                *live_sync = true;
                            },
                        );
                    }
                    IfConditionType::MousePosition => {
                        egui::ComboBox::from_id_salt((
                            group_id,
                            preset_id,
                            step_index,
                            extra_idx,
                            "hold-extra-mouse-pos-axis-cb",
                        ))
                        .width(55.0)
                        .selected_text(&cond.mouse_axis)
                        .show_ui(ui, |ui| {
                            for axis in &["X", "Y"] {
                                if ui
                                    .selectable_label(cond.mouse_axis == *axis, *axis)
                                    .clicked()
                                {
                                    cond.mouse_axis = axis.to_string();
                                    *live_sync = true;
                                }
                            }
                        });
                        egui::ComboBox::from_id_salt((
                            group_id,
                            preset_id,
                            step_index,
                            extra_idx,
                            "hold-extra-mouse-pos-op-cb",
                        ))
                        .width(55.0)
                        .selected_text(&cond.operator)
                        .show_ui(ui, |ui| {
                            for op in &["==", ">", "<", ">=", "<=", "!="] {
                                if ui.selectable_label(cond.operator == *op, *op).clicked() {
                                    cond.operator = op.to_string();
                                    *live_sync = true;
                                }
                            }
                        });
                        let cond_expr_id =
                            ui.id()
                                .with((step_index, extra_idx, "hold-extra-mouse-pos-val-input"));
                        let response2 = Self::render_variable_text_edit(
                            ui,
                            &mut cond.expression,
                            cond_expr_id,
                            76.0,
                            100.0,
                            21.0,
                            21.0,
                            Self::tr_lang(language, "value/expr", "giá trị/expr"),
                            false,
                        );
                        Self::apply_vietnamese_input_if_changed(
                            &response2,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            &mut cond.expression,
                        );
                        *live_sync |= response2.changed();
                        Self::render_variable_suggestions(
                            ui,
                            &response2,
                            &mut cond.expression,
                            timer_names,
                            language,
                        );
                        // Nút pick tọa độ từ màn hình
                        let pick_btn = ui
                            .add_sized(
                                [18.0, 18.0],
                                egui::Button::new(Self::material_icon_text(0xe55c, 14.0)),
                            )
                            .on_hover_text(Self::tr_lang(
                                language,
                                "Click on screen to capture coordinate",
                                "Bấm vào màn hình để lấy tọa độ",
                            ));
                        if pick_btn.clicked() {
                            *pending_pick_target = Some(MouseMoveAbsoluteCaptureTarget {
                                group_id: Some(group_id),
                                preset_id,
                                step_index,
                                capture_kind: MouseCaptureKind::ExtraCondMousePos,
                                extra_cond_index: Some(extra_idx),
                                is_hold_stop,
                            });
                        }
                    }
                    IfConditionType::PresetRunning => {
                        // Bước 1: chọn group
                        let sel_group_id = cond.running_preset_group_id;
                        let sel_group_label = sel_group_id
                            .and_then(|gid| {
                                all_groups
                                    .iter()
                                    .find(|(id, _, _)| *id == gid)
                                    .map(|(_, name, _)| name.clone())
                            })
                            .unwrap_or_else(|| Self::tr_lang(language, "Group", "Nhóm").to_owned());
                        egui::ComboBox::from_id_salt((
                            group_id,
                            preset_id,
                            step_index,
                            extra_idx,
                            "hold-extra-running-group-cb",
                        ))
                        .width(100.0)
                        .selected_text(sel_group_label)
                        .show_ui(ui, |ui| {
                            for (gid, gname, _) in all_groups {
                                if ui
                                    .selectable_label(sel_group_id == Some(*gid), gname)
                                    .clicked()
                                {
                                    cond.running_preset_group_id = Some(*gid);
                                    cond.running_preset_id = None;
                                    *live_sync = true;
                                }
                            }
                        });
                        // Bước 2: chọn preset trong group đó
                        let group_presets: &[(u32, String)] = if let Some(gid) = sel_group_id {
                            all_groups
                                .iter()
                                .find(|(id, _, _)| *id == gid)
                                .map(|(_, _, presets)| presets.as_slice())
                                .unwrap_or(&[])
                        } else {
                            &[]
                        };
                        let selected_id = cond.running_preset_id;
                        let selected_label = selected_id
                            .and_then(|id| {
                                if id == 0 {
                                    Some(Self::tr_lang(language, "Any Preset", "Bất kỳ").to_owned())
                                } else {
                                    group_presets
                                        .iter()
                                        .find(|(pid, _)| *pid == id)
                                        .map(|(_, name)| name.clone())
                                }
                            })
                            .unwrap_or_else(|| {
                                Self::tr_lang(language, "Preset", "Preset").to_owned()
                            });
                        egui::ComboBox::from_id_salt((
                            group_id,
                            preset_id,
                            step_index,
                            extra_idx,
                            "hold-extra-running-preset-cb",
                        ))
                        .width(120.0)
                        .selected_text(selected_label)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    selected_id == Some(0),
                                    Self::tr_lang(language, "Any Preset", "Bất kỳ"),
                                )
                                .clicked()
                            {
                                cond.running_preset_id = Some(0);
                                *live_sync = true;
                            }
                            for (pid, pname) in group_presets {
                                if ui
                                    .selectable_label(selected_id == Some(*pid), pname)
                                    .clicked()
                                {
                                    cond.running_preset_id = Some(*pid);
                                    *live_sync = true;
                                }
                            }
                        });
                    }
                    _ => {}
                }
                let btn_minus = egui::Button::new(egui::RichText::new("-").size(12.0).strong());
                let btn_minus_clicked = ui.scope(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
                    ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                    ui.add_sized([18.0, 18.0], btn_minus)
                        .on_hover_text(Self::tr_lang(language, "Remove condition", "Xóa điều kiện"))
                        .clicked()
                });
                if btn_minus_clicked.inner {
                    remove_extra_idx = Some(extra_idx);
                }
            });
        }
        if let Some(remove_idx) = remove_extra_idx {
            extra_conditions.remove(remove_idx);
            *live_sync = true;
        }
    }

    pub(crate) fn render_macro_panel(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        let window_presets = self.state.window_presets.clone();
        let window_layouts = self.state.window_layouts.clone();
        if self.sanitize_legacy_macro_ocr_target_texts() {
            self.sync_macro_presets();
            self.persist();
        }
        let timer_names: Vec<String> = self
            .state
            .timer_presets
            .iter()
            .map(|t| t.name.clone())
            .collect();
        let mut suggestion_names = std::collections::HashSet::new();
        for (idx, _name) in timer_names.iter().enumerate() {
            suggestion_names.insert(format!("Timer{}", idx + 1));
        }
        for name in Self::builtin_variable_suggestions() {
            suggestion_names.insert(name.to_string());
        }
        for name in self.collect_all_macro_referenced_variables() {
            if !name.contains('.') {
                suggestion_names.insert(name);
            }
        }
        {
            let vars = crate::overlay::RUNTIME_VARIABLES.lock();
            for name in vars.keys() {
                if !name.contains('.') {
                    suggestion_names.insert(name.clone());
                }
            }
        }
        let mut suggestion_names: Vec<String> = suggestion_names.into_iter().collect();
        suggestion_names.sort();
        let mut writable_suggestion_names = std::collections::HashSet::new();
        {
            let vars = crate::overlay::RUNTIME_VARIABLES.lock();
            for name in vars.keys() {
                if !name.contains('.') {
                    writable_suggestion_names.insert(name.clone());
                }
            }
        }
        for (idx, _name) in timer_names.iter().enumerate() {
            writable_suggestion_names.insert(format!("Timer{}", idx + 1));
        }
        let mut writable_suggestion_names: Vec<String> =
            writable_suggestion_names.into_iter().collect();
        writable_suggestion_names.sort();
        let mut all_vars = suggestion_names.clone();
        for (const_name, _) in &self.state.global_constants {
            if !all_vars.contains(const_name) {
                all_vars.push(const_name.clone());
            }
        }
        all_vars.sort();
        ui.memory_mut(|mem| {
            mem.data.insert_temp(
                egui::Id::new("macro_variable_suggestion_names"),
                suggestion_names,
            );
            mem.data.insert_temp(
                egui::Id::new("macro_timer_suggestion_names"),
                timer_names.clone(),
            );
            mem.data.insert_temp(
                egui::Id::new("macro_variable_writable_suggestion_names"),
                writable_suggestion_names,
            );
            mem.data
                .insert_temp(egui::Id::new("macro_variable_suggestion_committed"), false);
        });
        let any_popup_open = ui
            .memory(|mem| mem.data.get_temp::<bool>(egui::Id::new("any_popup_open")))
            .unwrap_or(false);
        let mut enter_pressed = false;
        let mut arrow_up_pressed = false;
        let mut arrow_down_pressed = false;
        if any_popup_open {
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                    enter_pressed = true;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    arrow_up_pressed = true;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    arrow_down_pressed = true;
                }
            });
        }
        ui.memory_mut(|mem| {
            mem.data
                .insert_temp(egui::Id::new("enter_pressed"), enter_pressed);
            mem.data
                .insert_temp(egui::Id::new("arrow_up_pressed"), arrow_up_pressed);
            mem.data
                .insert_temp(egui::Id::new("arrow_down_pressed"), arrow_down_pressed);
            mem.data.insert_temp(egui::Id::new("any_popup_open"), false);
        });
        let active_folder_for_controls = if self.macro_folders_panel_open {
            self.active_macro_folder_view.filter(|folder_id| {
                self.state
                    .macro_folders
                    .iter()
                    .any(|folder| folder.id == *folder_id)
            })
        } else {
            None
        };
        if self.active_macro_folder_view.is_some() && active_folder_for_controls.is_none() {
            self.active_macro_folder_view = None;
        }
        let active_folder_name = if self.macro_folders_panel_open {
            self.active_macro_folder_view.and_then(|folder_id| {
                self.state
                    .macro_folders
                    .iter()
                    .find(|folder| folder.id == folder_id)
                    .map(|folder| folder.name.clone())
            })
        } else {
            None
        };
        let paste_target_folder = if active_folder_name.is_some() {
            self.active_macro_folder_view
        } else {
            None
        };
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::symmetric(3, 3))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let master_label = if self.state.macros_master_enabled {
                        Self::tr_lang(language, "Macro On", "Macro On")
                    } else {
                        Self::tr_lang(language, "Macro Off", "Macro Off")
                    };
                    let master_fill = if self.state.macros_master_enabled {
                        Color32::from_rgb(46, 126, 76)
                    } else {
                        Color32::from_rgb(74, 78, 86)
                    };
                    let master_stroke = if self.state.macros_master_enabled {
                        Color32::from_rgb(112, 204, 142)
                    } else {
                        Color32::from_rgb(156, 162, 172)
                    };
                    if Self::with_emphasized_button_hover_radius(ui, 6, |ui| {
                        ui.add_sized(
                            [120.0, 28.0],
                            Button::new(RichText::new(master_label).color(Color32::WHITE))
                                .fill(master_fill)
                                .stroke(egui::Stroke::new(1.0, master_stroke))
                                .corner_radius(6.0),
                        )
                    })
                        .clicked()
                    {
                        self.state.macros_master_enabled = !self.state.macros_master_enabled;
                        self.sync_macro_master_enabled();
                        self.persist();
                    }
                    let macro_hotkey_capture_target = CaptureRequest::MacrosMasterHotkey;
                    let macro_hotkey_capture_active =
                        self.capture_target.as_ref() == Some(&macro_hotkey_capture_target);
                    let macro_hotkey_preview = if macro_hotkey_capture_active
                        && let Some(pending) = self.capture_hotkey_combo_keys.as_ref()
                    {
                        Some(Self::hotkey_binding_from_combo_keys(pending.clone()))
                    } else {
                        self.state.macros_master_hotkey.clone()
                    };
                    let macro_hotkey_capture_button_text = if macro_hotkey_capture_active {
                        Self::capture_button_text(language, true)
                    } else {
                        Self::material_icon_text(0xe312, 18.0)
                    };
                    if Self::with_emphasized_button_hover_radius(ui, 6, |ui| {
                        ui.add_sized(
                            if macro_hotkey_capture_active {
                                [104.0, 28.0]
                            } else {
                                [28.0, 28.0]
                            },
                            Button::new(macro_hotkey_capture_button_text)
                                .fill(if macro_hotkey_capture_active {
                                    Color32::from_rgb(46, 126, 76)
                                } else {
                                    ui.visuals().faint_bg_color
                                })
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    if macro_hotkey_capture_active {
                                        Color32::from_rgb(112, 204, 142)
                                    } else {
                                        ui.visuals().widgets.noninteractive.bg_stroke.color
                                    },
                                ))
                                .corner_radius(6.0),
                        )
                    })
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Capture macro hotkey",
                            "Bat macro hotkey",
                        ))
                        .clicked()
                    {
                        if macro_hotkey_capture_active {
                            self.cancel_capture();
                        } else {
                            self.begin_capture(
                                macro_hotkey_capture_target,
                                Self::tr_lang(
                                    language,
                                    "Press a hotkey for Macro On / Off.",
                                    "Nhan hotkey de bat / tat Macro.",
                                )
                                .to_owned(),
                            );
                        }
                    }
                    if let Some(binding) = macro_hotkey_preview.as_ref() {
                        let label = hotkey::format_binding(Some(binding));
                        if Self::with_emphasized_button_hover(ui, |ui| {
                            ui.add(
                                Button::new(RichText::new(label).monospace())
                                    .min_size(vec2(0.0, 28.0)),
                            )
                        })
                            .on_hover_text(Self::tr_lang(
                                language,
                                "Click to remove this hotkey",
                                "Bấm để xóa hotkey này",
                            ))
                            .clicked()
                        {
                            self.state.macros_master_hotkey = None;
                            self.sync_macro_master_hotkey();
                            self.persist();
                        }
                    }
                });
            if Self::with_emphasized_button_hover(ui, |ui| {
                ui.add_sized(
                    [28.0, 28.0],
                    Button::new(Self::material_icon_text(0xe145, 18.0))
                        .fill(ui.visuals().faint_bg_color)
                        .stroke(egui::Stroke::new(
                            1.0,
                            ui.visuals().widgets.noninteractive.bg_stroke.color,
                        )),
                )
            })
                .on_hover_text(Self::tr_lang(
                    language,
                    "Add macro group",
                    "Them macro group",
                ))
                .clicked()
            {
                if let Some(folder_id) = active_folder_for_controls {
                    self.add_macro_group_to_folder(folder_id);
                } else {
                    self.add_macro_group();
                }
                self.persist();
            }
            let share_icon = 0xe80d; // Material icon for share
            let share_fill = if self.show_share_buttons {
                Color32::from_rgba_premultiplied(0, 191, 255, 30)
            } else {
                ui.visuals().faint_bg_color
            };
            let share_stroke = if self.show_share_buttons {
                Color32::from_rgb(0, 191, 255)
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            };
            if Self::with_emphasized_button_hover(ui, |ui| {
                ui.add_sized(
                    [28.0, 28.0],
                    Button::new(Self::material_icon_text(share_icon, 18.0))
                        .fill(share_fill)
                        .stroke(egui::Stroke::new(1.0, share_stroke)),
                )
            })
                .on_hover_text(Self::tr_lang(
                    language,
                    "Toggle Import/Export buttons",
                    "Bật/Tắt hiển thị nút chia sẻ (Import/Export)",
                ))
                .clicked()
            {
                self.show_share_buttons = !self.show_share_buttons;
                self.refresh_macro_share_clipboard_kind(true);
            }
            if self.show_share_buttons {
                self.refresh_macro_share_clipboard_kind(false);
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
            let paste_enabled = !self.macro_group_clipboard.is_empty();
            let paste_fill = if paste_enabled {
                Color32::from_rgb(84, 90, 102)
            } else {
                ui.visuals().faint_bg_color
            };
            let paste_stroke = if paste_enabled {
                ui.visuals().widgets.active.bg_stroke.color
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            };
            if Self::with_emphasized_button_hover(ui, |ui| {
                ui.add_enabled(
                    paste_enabled,
                    Button::new(Self::material_icon_text(0xe14f, 18.0))
                        .min_size(egui::vec2(28.0, 28.0))
                        .fill(paste_fill)
                        .stroke(egui::Stroke::new(1.0, paste_stroke)),
                )
            })
                .on_hover_text(Self::tr_lang(
                    language,
                    "Paste macro groups",
                    "Dán macro group",
                ))
                .clicked()
            {
                self.paste_macro_groups_into_folder(paste_target_folder);
            }
            let copy_enabled = !self.selected_macro_groups.is_empty();
            let copy_fill = if copy_enabled {
                Color32::from_rgb(84, 90, 102)
            } else {
                ui.visuals().faint_bg_color
            };
            let copy_stroke = if copy_enabled {
                ui.visuals().widgets.active.bg_stroke.color
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            };
            if Self::with_emphasized_button_hover(ui, |ui| {
                ui.add_enabled(
                    copy_enabled,
                    Button::new(Self::material_icon_text(0xe14d, 18.0))
                        .min_size(egui::vec2(28.0, 28.0))
                        .fill(copy_fill)
                        .stroke(egui::Stroke::new(1.0, copy_stroke)),
                )
            })
                .on_hover_text(Self::tr_lang(
                    language,
                    "Copy selected macro groups",
                    "Sao chép macro group đã chọn",
                ))
                .clicked()
            {
                self.copy_selected_macro_groups();
            }
            let cut_enabled = !self.selected_macro_groups.is_empty();
            let cut_fill = if cut_enabled {
                Color32::from_rgb(84, 90, 102)
            } else {
                ui.visuals().faint_bg_color
            };
            let cut_stroke = if cut_enabled {
                ui.visuals().widgets.active.bg_stroke.color
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            };
            if Self::with_emphasized_button_hover(ui, |ui| {
                ui.add_enabled(
                    cut_enabled,
                    Button::new(Self::material_icon_text(0xe14e, 18.0))
                        .min_size(egui::vec2(28.0, 28.0))
                        .fill(cut_fill)
                        .stroke(egui::Stroke::new(1.0, cut_stroke)),
                )
            })
                .on_hover_text(Self::tr_lang(
                    language,
                    "Cut selected macro groups",
                    "Cat macro group đã chọn",
                ))
                .clicked()
            {
                self.cut_selected_macro_groups();
            }
            let trash_enabled = !self.selected_macro_groups.is_empty();
            let trash_fill = if trash_enabled {
                Color32::from_rgb(84, 90, 102)
            } else {
                ui.visuals().faint_bg_color
            };
            let trash_stroke = if trash_enabled {
                ui.visuals().widgets.active.bg_stroke.color
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            };
            if Self::with_emphasized_button_hover(ui, |ui| {
                ui.add_enabled(
                    trash_enabled,
                    Button::new(Self::material_icon_text(0xe872, 18.0))
                        .min_size(egui::vec2(28.0, 28.0))
                        .fill(trash_fill)
                        .stroke(egui::Stroke::new(1.0, trash_stroke)),
                )
            })
                .on_hover_text(Self::tr_lang(
                    language,
                    "Delete selected macro groups",
                    "Xoa cac macro group da chon",
                ))
                .clicked()
            {
                self.remove_selected_macro_groups();
            }
            let star_filter_active = matches!(
                self.macro_groups_favorite_filter,
                MacroGroupFavoriteFilter::Star
            );
            if Self::with_emphasized_button_hover(ui, |ui| {
                ui.add_sized(
                    [28.0, 28.0],
                    Button::new(Self::material_icon_text(0xe838, 18.0))
                        .fill(if star_filter_active {
                            Color32::from_rgb(124, 96, 28)
                        } else {
                            ui.visuals().faint_bg_color
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if star_filter_active {
                                Color32::from_rgb(255, 220, 96)
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        )),
                )
            })
                .on_hover_text(Self::tr_lang(
                    language,
                    "Show star macros only",
                    "Chi nhóm đã favorite",
                ))
                .clicked()
            {
                self.macro_groups_favorite_filter = if star_filter_active {
                    MacroGroupFavoriteFilter::All
                } else {
                    MacroGroupFavoriteFilter::Star
                };
            }
            // Render Global Constants on toolbar
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::symmetric(3, 3))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    if !self.state.global_constants.is_empty() {
                        let max_show = 3;
                        for (i, (name, val)) in self.state.global_constants.iter().enumerate() {
                            if i >= max_show {
                                break;
                            }
                            let text = format!("{}={}", name, val);
                            let is_dark = self.state.ui_theme == UiThemeMode::Dark;
                            let bg_color = if is_dark {
                                Color32::from_rgba_premultiplied(0, 150, 200, 30)
                            } else {
                                Color32::from_rgba_premultiplied(0, 120, 180, 20)
                            };
                            let border_color = if is_dark {
                                Color32::from_rgba_premultiplied(0, 200, 255, 120)
                            } else {
                                Color32::from_rgba_premultiplied(0, 100, 150, 120)
                            };
                            let text_color = if is_dark {
                                Color32::from_rgb(140, 230, 255)
                            } else {
                                Color32::from_rgb(0, 80, 120)
                            };
                            egui::Frame::canvas(ui.style())
                                .fill(bg_color)
                                .stroke(egui::Stroke::NONE)
                                .rounding(4.0)
                                .inner_margin(egui::Margin::symmetric(5, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(text)
                                            .monospace()
                                            .size(11.0)
                                            .color(text_color),
                                    );
                                })
                                .response
                                .on_hover_text(Self::tr_lang(
                                    language,
                                    "Global Constant (Fixed Value)",
                                    "Hang so toan cuc (Gia tri co dinh)",
                                ));
                        }
                        if self.state.global_constants.len() > max_show {
                            let remaining = self.state.global_constants.len() - max_show;
                            let mut tooltip_text = String::new();
                            for (i, (name, val)) in self.state.global_constants.iter().enumerate() {
                                if i >= max_show {
                                    if !tooltip_text.is_empty() {
                                        tooltip_text.push('\n');
                                    }
                                    tooltip_text.push_str(&format!("{} = {}", name, val));
                                }
                            }
                            ui.label(
                                RichText::new(format!("+{} more", remaining))
                                    .size(11.0)
                                    .color(ui.visuals().weak_text_color())
                                    .italics(),
                            )
                            .on_hover_text(tooltip_text);
                        }
                    }
                    let edit_icon = 0xe3c9; // edit icon (but chi cheo)
                    if Self::with_emphasized_button_hover(ui, |ui| {
                        ui.add_sized(
                            [28.0, 28.0],
                            Button::new(Self::material_icon_text(edit_icon, 18.0)) // variable edit icon
                                .fill(if self.variable_inspector_open {
                                    Color32::from_rgb(46, 126, 76)
                                } else {
                                    ui.visuals().faint_bg_color
                                })
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    if self.variable_inspector_open {
                                        Color32::from_rgb(112, 204, 142)
                                    } else {
                                        ui.visuals().widgets.noninteractive.bg_stroke.color
                                    },
                                ))
                                .corner_radius(6.0),
                        )
                    })
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Global & Local Variables Manager (Real-time)",
                            "Trinh quan ly bien toan cuc va cuc bo (Real-time)",
                        ))
                        .clicked()
                    {
                        self.variable_inspector_open = !self.variable_inspector_open;
                    }
                });
            if Self::with_emphasized_button_hover(ui, |ui| {
                ui.add_sized(
                    [28.0, 28.0],
                    Button::new(Self::folder_icon_text(self.macro_folders_panel_open, 18.0))
                        .fill(if self.macro_folders_panel_open {
                            Color32::from_rgba_premultiplied(72, 156, 116, 120)
                        } else {
                            ui.visuals().faint_bg_color
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if self.macro_folders_panel_open {
                                Color32::from_rgb(126, 224, 182)
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        )),
                )
            })
                .on_hover_text(Self::tr_lang(
                    language,
                    "Show / hide macro folders",
                    "Hien / an macro folder",
                ))
                .clicked()
            {
                self.macro_folders_panel_open = !self.macro_folders_panel_open;
                if !self.macro_folders_panel_open {
                    self.set_active_macro_folder_view(None);
                }
            }
            if self.macro_folders_panel_open {
                if let Some(folder_id) = self.active_macro_folder_view {
                    let folder_name = self
                        .state
                        .macro_folders
                        .iter()
                        .find(|f| f.id == folder_id)
                        .map(|f| f.name.clone())
                        .unwrap_or_default();
                    if ui
                        .add_sized(
                            [28.0, 28.0],
                            Button::new(Self::material_icon_text(0xe5c4, 18.0)) // arrow_back icon
                                .fill(ui.visuals().faint_bg_color)
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                                )),
                        )
                        .on_hover_text(Self::tr_lang(
                            language,
                            "Back to folder list",
                            "Quay lại danh sách thư mục",
                        ))
                        .clicked()
                    {
                        self.set_active_macro_folder_view(None);
                    }
                    ui.label(
                        RichText::new(folder_name)
                            .strong()
                            .color(ui.visuals().strong_text_color()),
                    );
                    ui.add_space(8.0);
                } else {
                    if ui
                        .add_sized(
                            [28.0, 28.0],
                            Button::new(Self::material_icon_text(0xe2cc, 18.0))
                                .fill(ui.visuals().faint_bg_color)
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                                )),
                        )
                        .on_hover_text(Self::tr_lang(language, "Add folder", "Them thu muc"))
                        .clicked()
                    {
                        self.add_macro_folder();
                        self.persist();
                        self.macro_folders_panel_open = true;
                        self.active_macro_folder_view = None;
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let response = ui.add_sized(
                    [260.0, 24.0],
                    TextEdit::singleline(&mut self.macro_preset_search_query).hint_text(
                        RichText::new(Self::tr_lang(
                            language,
                            "Search macro groups and presets",
                            "Tìm group macro và preset",
                        ))
                        .weak(),
                    ),
                );
                ui.label(Self::material_icon_text(0xe8b6, 18.0));
                Self::apply_vietnamese_input_if_changed(
                    &response,
                    self.state.vietnamese_input_enabled,
                    self.state.vietnamese_input_mode,
                    &mut self.macro_preset_search_query,
                );
            });
        });
        ui.add_space(8.0);
        if Self::is_copy_feedback_active(self.macro_group_export_feedback_until)
            || Self::is_copy_feedback_active(self.macro_preset_export_feedback_until)
            || Self::is_copy_feedback_active(self.macro_step_export_feedback_until)
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        let macro_panel_scroll_height = ui.available_height();
        let pending_macro_group_scroll_target = self.pending_macro_group_scroll_target.take();
        let scroll_target_key = ui.make_persistent_id("pending-scroll-target");
        let scroll_frames_key = ui.make_persistent_id("pending-scroll-frames");
        let last_scroll_target: Option<u32> = ui.ctx().data(|data| data.get_temp(scroll_target_key));
        
        let mut scroll_frames: usize = ui.ctx().data(|data| data.get_temp(scroll_frames_key).unwrap_or(0));
        let resolved_scroll_target = if pending_macro_group_scroll_target.is_some() {
            if pending_macro_group_scroll_target != last_scroll_target {
                scroll_frames = 0;
                ui.ctx().data_mut(|data| data.insert_temp(scroll_target_key, pending_macro_group_scroll_target.unwrap()));
                ui.ctx().data_mut(|data| data.insert_temp(scroll_frames_key, 0usize));
            }
            pending_macro_group_scroll_target
        } else {
            last_scroll_target
        };
        let pending_macro_group_scroll_target = resolved_scroll_target;
        let mut pending_macro_group_scroll_consumed = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(macro_panel_scroll_height)
            .show_viewport(ui, |ui, viewport| {
        let mut pending_macro_group_scroll_rect: Option<egui::Rect> = None;
        let mut release_folder_id = None;
        let mut delete_folder_id = None;
        let mut enter_folder_id = None;
        let mut begin_mouse_move_absolute_capture_target = None;
        let mut begin_mouse_path_draw_capture_request = None;
        let mut add_mouse_path_preset_request: Option<(u32, u32, usize, Option<u32>)> = None;
        let mut add_hold_stop_mouse_path_preset_request: Option<(u32, u32, Option<u32>)> = None;
        let mut preview_mouse_path_step_request: Option<Option<u32>> = None;
        let mut cancel_mouse_move_absolute_capture = false;
        let mut cancel_mouse_path_draw_capture = false;
        let capture_target_snapshot = self.capture_target.clone();
        let capture_hotkey_combo_keys_snapshot = self.capture_hotkey_combo_keys.clone();
        let active_folder_name = if self.macro_folders_panel_open {
            self.active_macro_folder_view.and_then(|folder_id| {
                self.state
                    .macro_folders
                    .iter()
                    .find(|folder| folder.id == folder_id)
                    .map(|folder| folder.name.clone())
            })
        } else {
            None
        };
        if !self.macro_folders_panel_open {
            self.active_macro_folder_view = None;
        } else if self.active_macro_folder_view.is_some() && active_folder_name.is_none() {
            self.active_macro_folder_view = None;
        }
        if false {
            ui.horizontal_wrapped(|ui| {
            let master_label = if self.state.macros_master_enabled {
                Self::tr_lang(language, "Macro On", "Macro On")
            } else {
                Self::tr_lang(language, "Macro Off", "Macro Off")
            };
            let master_fill = if self.state.macros_master_enabled {
                Color32::from_rgb(46, 126, 76)
            } else {
                Color32::from_rgb(74, 78, 86)
            };
            let master_stroke = if self.state.macros_master_enabled {
                Color32::from_rgb(112, 204, 142)
            } else {
                Color32::from_rgb(156, 162, 172)
            };
            if ui
                .add_sized(
                    [120.0, 28.0],
                    Button::new(RichText::new(master_label).color(Color32::WHITE))
                        .fill(master_fill)
                        .stroke(egui::Stroke::new(1.0, master_stroke))
                        .corner_radius(6.0),
                )
                .clicked()
            {
                self.state.macros_master_enabled = !self.state.macros_master_enabled;
                self.sync_macro_master_enabled();
                self.persist();
            }
            let capture_target = CaptureRequest::MacrosMasterHotkey;
            let capture_active = self.capture_target.as_ref() == Some(&capture_target);
            let hotkey_preview =
                if capture_active && let Some(pending) = self.capture_hotkey_combo_keys.as_ref() {
                    Some(Self::hotkey_binding_from_combo_keys(pending.clone()))
                } else {
                    self.state.macros_master_hotkey.clone()
                };
            let macro_hotkey_capture_button_text = if capture_active {
                Self::capture_button_text(language, true)
            } else {
                Self::material_icon_text(0xe312, 18.0)
            };
            if ui
                .add_sized(
                    if capture_active {
                        [104.0, 28.0]
                    } else {
                        [28.0, 28.0]
                    },
                    Button::new(macro_hotkey_capture_button_text)
                        .fill(if capture_active {
                            Color32::from_rgb(46, 126, 76)
                        } else {
                            ui.visuals().faint_bg_color
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if capture_active {
                                Color32::from_rgb(112, 204, 142)
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        ))
                        .corner_radius(6.0),
                )
                .on_hover_text(Self::tr_lang(
                    language,
                    "Capture macro hotkey",
                    "Bat macro hotkey",
                ))
                .clicked()
            {
                if capture_active {
                    self.cancel_capture();
                } else {
                    self.begin_capture(
                        capture_target,
                        Self::tr_lang(
                            language,
                            "Press a hotkey for Macro On / Off.",
                            "Nhan hotkey de bat / tat Macro.",
                        )
                        .to_owned(),
                    );
                }
            }
            let star_filter_active = matches!(
                self.macro_groups_favorite_filter,
                MacroGroupFavoriteFilter::Star
            );
            if ui
                .add_sized(
                    [28.0, 28.0],
                    Button::new(Self::material_icon_text(0xe838, 18.0))
                        .fill(if star_filter_active {
                            Color32::from_rgb(124, 96, 28)
                        } else {
                            ui.visuals().faint_bg_color
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if star_filter_active {
                                Color32::from_rgb(255, 220, 96)
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        )),
                )
                .on_hover_text(Self::tr_lang(
                    language,
                    "Show star macros only",
                    "Chỉ hiện nhóm đã favorite",
                ))
                .clicked()
            {
                self.macro_groups_favorite_filter = if star_filter_active {
                    MacroGroupFavoriteFilter::All
                } else {
                    MacroGroupFavoriteFilter::Star
                };
            }
            if ui
                .add_sized(
                    [28.0, 28.0],
                    Button::new(Self::folder_icon_text(self.macro_folders_panel_open, 18.0))
                        .fill(if self.macro_folders_panel_open {
                            Color32::from_rgba_premultiplied(72, 156, 116, 120)
                        } else {
                            ui.visuals().faint_bg_color
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if self.macro_folders_panel_open {
                                Color32::from_rgb(126, 224, 182)
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        )),
                )
                .on_hover_text(Self::tr_lang(
                    language,
                    "Show / hide macro folders",
                    "Hien / an macro folder",
                ))
                .clicked()
            {
                self.macro_folders_panel_open = !self.macro_folders_panel_open;
                if !self.macro_folders_panel_open {
                    self.set_active_macro_folder_view(None);
                }
            }
            // Render Global Constants on toolbar
            if !self.state.global_constants.is_empty() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let max_show = 3;
                    for (i, (name, val)) in self.state.global_constants.iter().enumerate() {
                        if i >= max_show {
                            break;
                        }
                        let text = format!("{}={}", name, val);
                        let is_dark = self.state.ui_theme == UiThemeMode::Dark;
                        let bg_color = if is_dark {
                            Color32::from_rgba_premultiplied(0, 150, 200, 30)
                        } else {
                            Color32::from_rgba_premultiplied(0, 120, 180, 20)
                        };
                        let border_color = if is_dark {
                            Color32::from_rgba_premultiplied(0, 200, 255, 120)
                        } else {
                            Color32::from_rgba_premultiplied(0, 100, 150, 120)
                        };
                        let text_color = if is_dark {
                            Color32::from_rgb(140, 230, 255)
                        } else {
                            Color32::from_rgb(0, 80, 120)
                        };
                        egui::Frame::canvas(ui.style())
                            .fill(bg_color)
                            .stroke(egui::Stroke::NONE)
                            .rounding(4.0)
                            .inner_margin(egui::Margin::symmetric(5, 2))
                            .show(ui, |ui| {
                                ui.label(RichText::new(text).monospace().size(11.0).color(text_color));
                            }).response.on_hover_text(Self::tr_lang(
                                language,
                                "Global Constant (Fixed Value)",
                                "Hằng số toàn cục (Giá trị cố định)",
                            ));
                    }
                    if self.state.global_constants.len() > max_show {
                        let remaining = self.state.global_constants.len() - max_show;
                        let mut tooltip_text = String::new();
                        for (i, (name, val)) in self.state.global_constants.iter().enumerate() {
                            if i >= max_show {
                                if !tooltip_text.is_empty() {
                                    tooltip_text.push('\n');
                                }
                                tooltip_text.push_str(&format!("{} = {}", name, val));
                            }
                        }
                        ui.label(
                            RichText::new(format!("+{} more", remaining))
                                .size(11.0)
                                .color(ui.visuals().weak_text_color())
                                .italics()
                        ).on_hover_text(tooltip_text);
                    }
                });
            }
            let variable_inspector_active = self.variable_inspector_open;
            if ui
                .add_sized(
                    [28.0, 28.0],
                    Button::new(Self::material_icon_text(0xe150, 18.0)) // variable add icon
                        .fill(if variable_inspector_active {
                            Color32::from_rgb(46, 126, 76)
                        } else {
                            ui.visuals().faint_bg_color
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if variable_inspector_active {
                                Color32::from_rgb(112, 204, 142)
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        ))
                        .corner_radius(6.0),
                )
                .on_hover_text(Self::tr_lang(
                    language,
                    "Global & Local Variables Manager (Real-time)",
                    "Trình quản lý biến toàn cục & cục bộ (Real-time)",
                ))
                .clicked()
            {
                self.variable_inspector_open = !self.variable_inspector_open;
            }
            let trash_enabled = !self.selected_macro_groups.is_empty();
            if ui
                .add_enabled(
                    trash_enabled,
                    Button::new(Self::material_icon_text(0xe872, 18.0))
                        .min_size(egui::vec2(28.0, 28.0))
                        .fill(ui.visuals().faint_bg_color)
                        .stroke(egui::Stroke::new(
                            1.0,
                            ui.visuals().widgets.noninteractive.bg_stroke.color,
                        )),
                )
                .on_hover_text(Self::tr_lang(
                    language,
                    "Delete selected macro groups",
                    "Xóa các macro group đã chọn",
                ))
                .clicked()
            {
                self.remove_selected_macro_groups();
            }
            if let Some(binding) = hotkey_preview.as_ref() {
                let label = hotkey::format_binding(Some(binding));
                if ui
                    .add(
                        Button::new(RichText::new(label).monospace())
                            .min_size(vec2(0.0, 28.0))
                            .fill(if capture_active {
                                Color32::from_rgba_premultiplied(72, 156, 116, 120)
                            } else {
                                ui.visuals().faint_bg_color
                            })
                            .stroke(egui::Stroke::new(
                                1.0,
                                if capture_active {
                                    Color32::from_rgb(126, 224, 182)
                                } else {
                                    ui.visuals().widgets.noninteractive.bg_stroke.color
                                },
                            )),
                    )
                    .on_hover_text(Self::tr_lang(
                        language,
                        "Click to remove the macro hotkey",
                        "Bam vao de xoa macro hotkey",
                    ))
                    .clicked()
                    && !capture_active
                {
                    self.state.macros_master_hotkey = None;
                    self.sync_macro_master_hotkey();
                    self.persist();
                }
            }
        });
        }
        ui.add_space(6.0);
        enum MacroDestructiveConfirmAction {
            DeleteFolder(u32),
            ReleaseFolder(u32),
            DeleteGroup(u32),
        }
        let mut destructive_confirm = None;
        if let Some(folder_id) = self.confirm_delete_folder_id {
            let group_count = self
                .state
                .macro_groups
                .iter()
                .filter(|group| group.folder_id == Some(folder_id))
                .count();
            let folder_name = self
                .state
                .macro_folders
                .iter()
                .find(|folder| folder.id == folder_id)
                .map(|folder| folder.name.clone())
                .unwrap_or_else(|| format!("Folder {folder_id}"));
            destructive_confirm = Some((
                MacroDestructiveConfirmAction::DeleteFolder(folder_id),
                Self::tr_lang(language, "Delete folder", "Delete folder"),
                format!(
                    "{} {folder_name} {} {group_count} {}?",
                    Self::tr_lang(language, "Delete", "Delete"),
                    Self::tr_lang(language, "and all", "và toàn bộ"),
                    Self::tr_lang(
                        language,
                        "macro group(s) inside it",
                        "macro group bên trong",
                    ),
                ),
                Self::tr_lang(language, "Yes, Delete All", "Yes, Delete All"),
                Self::tr_lang(language, "Cancel", "Cancel"),
            ));
        } else if let Some(folder_id) = self.confirm_release_folder_id {
            let group_count = self
                .state
                .macro_groups
                .iter()
                .filter(|group| group.folder_id == Some(folder_id))
                .count();
            let folder_name = self
                .state
                .macro_folders
                .iter()
                .find(|folder| folder.id == folder_id)
                .map(|folder| folder.name.clone())
                .unwrap_or_else(|| format!("Folder {folder_id}"));
            destructive_confirm = Some((
                MacroDestructiveConfirmAction::ReleaseFolder(folder_id),
                Self::tr_lang(language, "Release folder", "Release folder"),
                format!(
                    "{} {folder_name} {} {group_count} {}?",
                    Self::tr_lang(language, "Release", "Release"),
                    Self::tr_lang(language, "and move", "và chuyển"),
                    Self::tr_lang(
                        language,
                        "macro group(s) out of it",
                        "macro group ra khỏi nó",
                    ),
                ),
                Self::tr_lang(language, "Yes, Release", "Yes, Release"),
                Self::tr_lang(language, "Cancel", "Cancel"),
            ));
        } else if let Some(group_id) = self.confirm_delete_macro_group_id {
            if let Some(group_name) = self
                .state
                .macro_groups
                .iter()
                .find(|group| group.id == group_id)
                .map(|group| group.name.clone())
            {
                destructive_confirm = Some((
                    MacroDestructiveConfirmAction::DeleteGroup(group_id),
                    Self::tr_lang(language, "Delete macro group", "Delete macro group"),
                    format!("{group_name}?"),
                    Self::tr_lang(language, "Yes, Delete", "Yes, Delete"),
                    Self::tr_lang(language, "Cancel", "Cancel"),
                ));
            } else {
                self.confirm_delete_macro_group_id = None;
            }
        }
        if let Some((action, title, message, confirm_label, cancel_label)) = destructive_confirm {
            if let Some(result) = self.render_blocking_confirmation_modal(
                ui.ctx(),
                "macro-destructive-confirm",
                &title,
                &message,
                &confirm_label,
                &cancel_label,
            ) {
                match (action, result) {
                    (MacroDestructiveConfirmAction::DeleteFolder(folder_id), true) => {
                        self.state
                            .macro_groups
                            .retain(|group| group.folder_id != Some(folder_id));
                        self.state
                            .macro_folders
                            .retain(|folder| folder.id != folder_id);
                        self.confirm_delete_folder_id = None;
                        self.confirm_release_folder_id = None;
                        if self.active_macro_folder_view == Some(folder_id) {
                            self.set_active_macro_folder_view(None);
                        }
                        self.persist_macro_presets();
                    }
                    (MacroDestructiveConfirmAction::DeleteFolder(_), false) => {
                        self.confirm_delete_folder_id = None;
                    }
                    (MacroDestructiveConfirmAction::ReleaseFolder(folder_id), true) => {
                        self.state
                            .macro_folders
                            .retain(|folder| folder.id != folder_id);
                        for group in &mut self.state.macro_groups {
                            if group.folder_id == Some(folder_id) {
                                group.folder_id = None;
                            }
                        }
                        self.confirm_release_folder_id = None;
                        if self.active_macro_folder_view == Some(folder_id) {
                            self.set_active_macro_folder_view(None);
                        }
                        self.persist_macro_presets();
                    }
                    (MacroDestructiveConfirmAction::ReleaseFolder(_), false) => {
                        self.confirm_release_folder_id = None;
                    }
                    (MacroDestructiveConfirmAction::DeleteGroup(group_id), true) => {
                        self.state.macro_groups.retain(|group| group.id != group_id);
                        self.selected_macro_groups.remove(&group_id);
                        self.macro_group_clipboard
                            .retain(|clipboard_group_id| *clipboard_group_id != group_id);
                        self.confirm_delete_macro_group_id = None;
                        self.persist_macro_presets();
                    }
                    (MacroDestructiveConfirmAction::DeleteGroup(_), false) => {
                        self.confirm_delete_macro_group_id = None;
                    }
                }
            }
        }
        let mut remove_group = None;
        let mut live_sync = false;
        let mut geom_presets_changed = false;
        let mut audio_sense_presets_changed = false;
        let mut add_preset_to_group = None;
        let mut paste_preset_to_group: Option<u32> = None;
        ui.separator();
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum RenderItem {
            FolderHeader(u32),
            MacroGroup(usize),
        }
        let search_query = self.macro_preset_search_query.trim().to_owned();
        Self::sort_macro_groups(&mut self.state.macro_groups);
        let mut render_items = Vec::new();
        if self.macro_folders_panel_open {
            if self.state.macro_folders.is_empty() {
                ui.label(Self::tr_lang(
                    language,
                    "No folders yet. Macro groups can stay outside folders if you want.",
                    "Chưa có thư mục nào. Nếu muốn, macro group có thể nằm ngoài thư mục.",
                ));
            }
            if let Some(active_folder_id) = self.active_macro_folder_view {
                for (index, group) in self.state.macro_groups.iter().enumerate() {
                    if group.folder_id == Some(active_folder_id) {
                        if Self::macro_group_matches_search_query(group, &search_query) {
                            if match self.macro_groups_favorite_filter {
                                MacroGroupFavoriteFilter::All => true,
                                MacroGroupFavoriteFilter::Star => group.favorite,
                            } {
                                render_items.push(RenderItem::MacroGroup(index));
                            }
                        }
                    }
                }
            } else {
                for folder in &self.state.macro_folders {
                    render_items.push(RenderItem::FolderHeader(folder.id));
                }
            }
        } else {
            for (index, group) in self.state.macro_groups.iter().enumerate() {
                if group.folder_id.is_none() {
                    if Self::macro_group_matches_search_query(group, &search_query) {
                        if match self.macro_groups_favorite_filter {
                            MacroGroupFavoriteFilter::All => true,
                            MacroGroupFavoriteFilter::Star => group.favorite,
                        } {
                            render_items.push(RenderItem::MacroGroup(index));
                        }
                    }
                }
            }
        }
        if !self.macro_folders_panel_open && render_items.is_empty() {
            ui.label(Self::tr_lang(
                language,
                "No macro groups outside folders.",
                "Không có macro group nào ngoài thư mục.",
            ));
        }
        let total_render_items = render_items.len();
        let mut lazy_render_limit = self.macro_panel_render_limit.min(total_render_items);
        if let Some(target_id) = pending_macro_group_scroll_target {
            if let Some(pos) = render_items.iter().position(|item| match item {
                RenderItem::MacroGroup(index) => self.state.macro_groups[*index].id == target_id,
                _ => false,
            }) {
                if pos >= lazy_render_limit {
                    lazy_render_limit = (pos + 1).min(total_render_items);
                    self.macro_panel_render_limit = lazy_render_limit;
                }
            }
        }
        let lazy_render_active = lazy_render_limit < total_render_items;
        let mut toggle_collapsed_folder_id: Option<u32> = None;
        let mut add_group_to_folder_id: Option<u32> = None;
        let mut renamed_folder: Option<(u32, String)> = None;
        let mut toggle_folder_enabled_id: Option<u32> = None;
        let mut pending_custom_preset_save: Option<(
            u32,
            u32,
            Option<usize>,
            String,
            String,
            bool,
        )> = None;
        let mut pending_custom_preset_save_and_open_ai: Option<(
            u32,
            u32,
            Option<usize>,
            String,
            String,
            bool,
            bool, // is_ad_hoc
        )> = None;
        let mut pending_open_ai_preset_id: Option<u32> = None;
        let mut pending_ocr_step_capture: Option<(u32, u32, usize)> = None;
        let mut pending_ocr_language_settings: Option<(String, String)> = None;
        let command_presets_snapshot = self.state.command_presets.clone();
        let macro_group_virtualization_margin = 480.0;
        let all_presets: Vec<(u32, String)> = self.state.macro_groups.iter().flat_map(|g| &g.presets).map(|p| (p.id, Self::format_macro_trigger_ui(language, p))).collect();
        let all_groups: Vec<(u32, String, Vec<(u32, String)>)> = self.state.macro_groups.iter().map(|g| {
            let presets: Vec<(u32, String)> = g.presets.iter().map(|p| (p.id, Self::format_macro_trigger_ui(language, p))).collect();
            (g.id, g.name.clone(), presets)
        }).collect();
        let all_trigger_macro_warnings: Vec<(u32, bool)> = self.state.macro_groups
            .iter()
            .flat_map(|g| g.presets.iter())
            .map(|preset| {
                let has_infinite_loop = self.state.macro_infinite_loop_warning_enabled
                    && preset.enabled
                    && preset.steps.iter().any(|s| {
                        s.action == MacroAction::LoopStart && s.is_infinite_loop()
                    });
                let has_vision_leak = preset.enabled
                    && preset.steps.iter().any(|s| {
                        s.action == MacroAction::StartVisionSearch && s.enabled
                    })
                    && !preset
                        .steps
                        .iter()
                        .any(|s| s.action == MacroAction::StopVision && s.enabled);
                (preset.id, has_infinite_loop || has_vision_leak)
            })
            .collect();
        let geometry_preset_options: Vec<(u32, String)> = self
            .state
            .geometry_presets
            .iter()
            .map(|preset| (preset.id, preset.name.clone()))
            .collect();
        let audio_sense_preset_options: Vec<(u32, String)> = self
            .state
            .audio_sense_presets
            .iter()
            .map(|preset| (preset.id, preset.name.clone()))
            .collect();
        for item in render_items.into_iter().take(lazy_render_limit) {
            match item {
                RenderItem::FolderHeader(folder_id) => {
                    let folder = self.state.macro_folders.iter().find(|f| f.id == folder_id).unwrap();
                    let folder_group_count = self
                        .state
                        .macro_groups
                        .iter()
                        .filter(|group| group.folder_id == Some(folder.id))
                        .count();
                    let folder_has_enabled_content = self.state.macro_groups.iter().any(|group| {
                        group.folder_id == Some(folder.id)
                            && group.enabled
                            && group.presets.iter().any(|preset| preset.enabled)
                    });
                    let mut folder_name = folder.name.clone();
                    let rect_key = ui.make_persistent_id((folder_id, "folder-rect"));
                    let last_rect: Option<egui::Rect> = ui.ctx().data(|data| data.get_temp(rect_key));
                    let hovered = last_rect.map_or(false, |rect| ui.rect_contains_pointer(rect));
                    let mut delete_clicked = false;
                    let (inner_res, frame_response) = Self::show_folder_card(ui, folder_has_enabled_content, hovered, |ui| {
                        ui.horizontal(|ui| {
                            let icon_btn = ui.add_sized(
                                [28.0, 21.0],
                                Button::new(Self::folder_icon_text(false, 18.0)),
                            );
                            let name_response =
                                ui.add_sized([220.0, 21.0], TextEdit::singleline(&mut folder_name));
                            Self::apply_vietnamese_input_if_changed(
                                &name_response,
                                self.state.vietnamese_input_enabled,
                                self.state.vietnamese_input_mode,
                                &mut folder_name,
                            );
                            if name_response.changed() {
                                renamed_folder = Some((folder_id, folder_name.clone()));
                            }
                            ui.add_sized(
                                [96.0, 21.0],
                                egui::Label::new(match language {
                                    UiLanguage::Vietnamese => format!("{folder_group_count} nhóm"),
                                    _ => format!("{folder_group_count} group(s)"),
                                }),
                            );
                            let delete_response = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let btn = Self::sound_style_remove_button(ui);
                                if btn.clicked() {
                                    delete_clicked = true;
                                }
                                btn
                            }).inner;
                            (icon_btn, name_response, delete_response)
                        })
                    });
                    let (icon_btn, name_response, delete_response) = inner_res.inner;
                    ui.ctx().data_mut(|data| data.insert_temp(rect_key, frame_response.rect));
                    let card_hovered = frame_response.hovered();
                    let pointer_in_widgets = ui.rect_contains_pointer(name_response.rect) || ui.rect_contains_pointer(delete_response.rect);
                    let card_clicked = card_hovered && ui.input(|i| i.pointer.any_click());
                    if card_hovered && !pointer_in_widgets {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if ((card_clicked && !pointer_in_widgets && !delete_clicked && !name_response.has_focus()) || icon_btn.clicked()) && !self.confirm_delete_folder_id.is_some() {
                        enter_folder_id = Some(folder_id);
                    }
                    if delete_clicked {
                        if folder_group_count > 0 {
                            self.confirm_delete_folder_id = Some(folder_id);
                        } else {
                            delete_folder_id = Some(folder_id);
                        }
                    }
                    ui.add_space(4.0);
                }
                RenderItem::MacroGroup(group_index) => {
                    let group_id = self.state.macro_groups[group_index].id;
                    let group_is_collapsed = self.state.macro_groups[group_index].collapsed;
                    let should_scroll_to_group =
                        pending_macro_group_scroll_target == Some(group_id);
                    let macro_group_height_key =
                        ui.make_persistent_id((group_id, "macro-group-cached-height"));
                    let cached_group_height: Option<f32> =
                        ui.ctx().data(|data| data.get_temp(macro_group_height_key));
                    let group_top = ui.cursor().min.y;
                    if group_is_collapsed && let Some(cached_height) = cached_group_height {
                        let visible_bottom = viewport.bottom() + macro_group_virtualization_margin;
                        if group_top > visible_bottom && !should_scroll_to_group {
                            ui.add_space(cached_height);
                            continue;
                        }
                    }
                    let mut next_capture_target = None;
                    let mut cancel_active_capture = false;
                    let mut remove_step = None;
                    let mut insert_step_after = None;
                    let mut move_step_to: Option<(u32, Vec<usize>, usize)> = None;
                    let mut remove_preset = None;
                    let mut pending_step_selection = None;
                    let mut clear_step_selection = None;
                    let mut copy_selected_steps = None;
                    let mut delete_selected_steps = None;
                    let mut paste_step_after: Option<(u32, u32, usize)> = None;
                    let mut paste_steps_at_start: Option<(u32, u32)> = None;
                    let mut copy_single_step: Option<(u32, u32, usize)> = None;
                    let mut export_step: Option<(u32, usize)> = None;
                    let mut import_step_to: Option<(u32, u32, Option<usize>)> = None; // (group_id, preset_id, Option<step_index>)
                    let mut export_preset: Option<u32> = None;
                    let mut import_preset_to_group: Option<(u32, Option<u32>)> = None; // (group_id, Option<insert_after_preset_id>)
                    let mut export_group: Option<u32> = None;
                    let mut import_group_after: Option<u32> = None; // insert_after_group_id
                    let selected_steps_snapshot = self.selected_macro_steps.clone();
                let render_preset_indices = {
                    let group = &self.state.macro_groups[group_index];
                    let query = search_query.as_str();
                    if query.is_empty() || Self::contains_case_insensitive(&group.name, query) {
                        (0..group.presets.len()).collect::<Vec<_>>()
                    } else {
                        group
                            .presets
                            .iter()
                            .enumerate()
                            .filter(|(_, preset)| {
                                Self::macro_preset_matches_search_query(group, preset, query)
                            })
                            .map(|(index, _)| index)
                            .collect::<Vec<_>>()
                    }
                };
                if render_preset_indices.is_empty() {
                    continue;
                }
                {
                    // Pre-compute group list for TriggerMacroPreset (avoids borrow conflict)
                    let current_folder_id = self.state.macro_groups[group_index].folder_id;
                    let all_groups_for_trigger: Vec<(u32, String, Vec<(u32, String)>)> = self.state.macro_groups
                        .iter()
                        .enumerate()
                        .filter(|(_, g)| g.folder_id == current_folder_id)
                        .map(|(idx, _)| {
                            all_groups[idx].clone()
                        })
                        .collect();
                    let mut geometry_manual_color = self.vision_manual_color;
                    let mut geometry_manual_color_hex = self.vision_manual_color_hex.clone();
                    let mut request_geometry_screen_color_pick = false;
                    let mut pending_geometry_macro_step_color_pick: Option<(u32, u32, usize, bool, bool)> = None;
                    let group = &mut self.state.macro_groups[group_index];
                    let folder_enabled = true;
                    let group_scroll_rect_top = ui.cursor().min.y;
                    Self::show_preset_card(ui, group.enabled && folder_enabled, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                            let star_icon = if group.favorite { 0xe838 } else { 0xe83a };
                            let star_fill = if group.favorite {
                                Color32::from_rgb(104, 82, 18)
                            } else {
                                Color32::from_rgba_premultiplied(52, 58, 70, 190)
                            };
                            let star_stroke = if group.favorite {
                                Color32::from_rgb(255, 220, 96)
                            } else {
                                Color32::from_rgb(102, 110, 122)
                            };
                            if Self::with_emphasized_button_hover(ui, |ui| {
                                ui.add_sized(
                                    [28.0, 21.0],
                                    Button::new(Self::material_icon_text(star_icon, 15.0).color(
                                        if group.favorite {
                                            Color32::from_rgb(255, 224, 110)
                                        } else {
                                            Color32::from_rgb(208, 214, 224)
                                        },
                                    ))
                                    .fill(star_fill)
                                    .stroke(egui::Stroke::new(1.0, star_stroke)),
                                )
                            })
                                .on_hover_text(Self::tr_lang(language, "Star group", "Nhom sao"))
                                .clicked()
                            {
                                group.favorite = !group.favorite;
                                live_sync = true;
                            }
                            let mut selected = self.selected_macro_groups.contains(&group.id);
                            if ui.checkbox(&mut selected, "").changed() {
                                if selected {
                                    self.selected_macro_groups.insert(group.id);
                                } else {
                                    self.selected_macro_groups.remove(&group.id);
                                }
                            }
                            let has_group_inf_loop = self.state.macro_infinite_loop_warning_enabled
                                && group.enabled
                                && group.presets.iter().any(|preset| {
                                    preset.enabled
                                        && (
                                            (preset.trigger_mode == MacroTriggerMode::Press && !preset.stop_on_retrigger_immediate)
                                            || preset.trigger_mode == MacroTriggerMode::Release
                                        )
                                        && preset.steps.iter().any(|s| s.action == MacroAction::LoopStart && s.is_infinite_loop())
                                });
                            let has_group_vision_leak = group.enabled
                                && group.presets.iter().any(|preset| {
                                    preset.enabled
                                        && (preset.trigger_mode == MacroTriggerMode::Press || preset.trigger_mode == MacroTriggerMode::Release)
                                        && preset.steps.iter().any(|s| s.action == MacroAction::StartVisionSearch && s.enabled)
                                        && !preset.steps.iter().any(|s| s.action == MacroAction::StopVision && s.enabled)
                                });
                            if has_group_inf_loop || has_group_vision_leak {
                                ui.add_space(2.0);
                                let response = ui.add_sized([24.0, 21.0], egui::Button::new(
                                    Self::material_icon_text(0xe002, 18.0).color(Color32::from_rgb(255, 10, 10))
                                ).frame(false));
                                if response.contains_pointer() {
                                    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.id.with("group-tip"), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(Self::material_icon_text(0xe002, 14.0).color(Color32::from_rgb(255, 10, 10)));
                                            ui.label(RichText::new(Self::tr_lang(language, "CRITICAL WARNING", "CẢNH BÁO NGUY HIỂM")).strong().color(Color32::from_rgb(255, 10, 10)));
                                        });
                                        if has_group_inf_loop {
                                            ui.label(Self::tr_lang(
                                                language,
                                                "This group contains one or more enabled infinite loop macros! Enabling this group could lead to persistent looping upon keypress.",
                                                "Nhóm macro này chứa một hoặc nhiều macro bị lặp vô tận đang bật! Kích hoạt nhóm này có thể dẫn tới lặp vĩnh viễn khi bấm phím."
                                            ));
                                        }
                                        if has_group_vision_leak {
                                            ui.label(Self::tr_lang(
                                                language,
                                                "This group contains one or more macros that start image search (Press/Release trigger) but never stop it! This could cause background CPU thread leaks.",
                                                "Nhóm macro này chứa một hoặc nhiều macro bắt đầu tìm ảnh (kích hoạt bằng Nhấn/Thả) nhưng không dừng lại! Điều này có thể gây chạy luồng ngầm liên tục hao CPU."
                                            ));
                                        }
                                    });
                                }
                                ui.add_space(2.0);
                            }
                            let name_width = Self::preset_header_name_width(ui);
                            if group.collapsed {
                                ui.add_sized(
                                    [name_width, 21.0],
                                    egui::Label::new(
                                        RichText::new(&group.name).size(17.0).strong(),
                                    ),
                                );
                            } else {
                                let response = ui.add_sized(
                                    [name_width, 21.0],
                                    TextEdit::singleline(&mut group.name)
                                        .font(egui::FontId::proportional(17.0))
                                        .margin(egui::Margin {
                                            left: 5,
                                            right: 3,
                                            top: 1,
                                            bottom: 0,
                                        })
                                        .text_color(ui.visuals().strong_text_color())
                                        .horizontal_align(egui::Align::Center),
                                );
                                Self::apply_vietnamese_input_if_changed(
                                    &response,
                                    self.state.vietnamese_input_enabled,
                                    self.state.vietnamese_input_mode,
                                    &mut group.name,
                                );
                                live_sync |= response.changed();
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let is_active = group.enabled && folder_enabled;
                                    let enabled_icon = if is_active { 0xe5ca } else { 0xe835 };
                                    let enabled_fill = if is_active {
                                        Color32::from_rgba_premultiplied(72, 156, 116, 120)
                                    } else {
                                        ui.visuals().faint_bg_color
                                    };
                                    let enabled_stroke = if is_active {
                                        Color32::from_rgb(126, 224, 182)
                                    } else {
                                        ui.visuals().widgets.noninteractive.bg_stroke.color
                                    };
                                    if Self::with_emphasized_button_hover(ui, |ui| {
                                        ui.add_sized(
                                            [36.0, 24.0],
                                            Button::new(Self::material_icon_text(
                                                enabled_icon,
                                                18.0,
                                            ))
                                            .fill(enabled_fill)
                                            .stroke(egui::Stroke::new(1.0, enabled_stroke)),
                                        )
                                    })
                                        .on_hover_text(Self::tr_lang(
                                            language,
                                            if folder_enabled { "Enable / disable group" } else { "Folder containing this group is disabled" }, "",
                                        ))
                                        .clicked()
                                    {
                                        group.enabled = !group.enabled;
                                        live_sync = true;
                                    }
                                    if Self::sound_style_remove_button(ui).clicked() {
                                        remove_group = Some(group.id);
                                    }
                                    if Self::sound_style_toggle_button(
                                        ui,
                                        if group.collapsed {
                                            Self::tr_lang(language, "Show", "Show")
                                        } else {
                                            Self::tr_lang(language, "Hide", "Hide")
                                        },
                                    )
                                    .clicked()
                                    {
                                        group.collapsed = !group.collapsed;
                                        if group.collapsed {
                                            for preset in &mut group.presets {
                                                for step in &mut preset.steps {
                                                    if step.action == MacroAction::DrawGeometry {
                                                        step.geometry_collapsed = true;
                                                    }
                                                    if step.action == MacroAction::StartAudioSensePreset {
                                                        step.audio_sense_collapsed = true;
                                                    }
                                                }
                                            }
                                            if let Some((preview_group_id, _, _, _)) = self.draw_geometry_step_preview_target {
                                                if preview_group_id == group.id {
                                                    self.draw_geometry_step_preview_target = None;
                                                    let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometrySpec(None));
                                                }
                                            }
                                        }
                                        ui.ctx().data_mut(|data| {
                                            data.remove::<bool>(ui.make_persistent_id((
                                                (group.id, "macro-group-window-target"),
                                                "extra-target-windows-expanded",
                                            )));
                                        });
                                        live_sync = true;
                                    }
                                    if self.show_share_buttons {
                                        let group_export_feedback =
                                            self.macro_group_export_feedback_target == Some(group.id)
                                                && Self::is_copy_feedback_active(
                                                    self.macro_group_export_feedback_until,
                                                );
                                        if ui
                                            .add_enabled(
                                                self.macro_share_clipboard_kind
                                                    == crate::ui::MacroShareCodeKind::Group,
                                                Button::new(Self::tr_lang(
                                                    language,
                                                    "Import",
                                                    "Import",
                                                ))
                                                .min_size(egui::vec2(72.0, 24.0)),
                                            )
                                            .on_hover_text(Self::tr_lang(
                                                language,
                                                "Import Group Code",
                                                "Nhap ma nhom",
                                            ))
                                            .clicked()
                                        {
                                            import_group_after = Some(group.id);
                                        }
                                        let group_export_label = if group_export_feedback {
                                            Self::tr_lang(language, "Copied", "Copied")
                                        } else {
                                            Self::tr_lang(language, "Export", "Export")
                                        };
                                        if ui
                                            .add_sized(
                                                [72.0, 24.0],
                                                Button::new(group_export_label)
                                                    .fill(if group_export_feedback {
                                                        Color32::from_rgba_premultiplied(
                                                            72, 156, 116, 140,
                                                        )
                                                    } else {
                                                        ui.visuals().widgets.inactive.bg_fill
                                                    })
                                                    .stroke(egui::Stroke::new(
                                                        1.0,
                                                        if group_export_feedback {
                                                            Color32::from_rgb(126, 224, 182)
                                                        } else {
                                                            ui.visuals()
                                                                .widgets
                                                                .inactive
                                                                .bg_stroke
                                                                .color
                                                        },
                                                    )),
                                            )
                                            .on_hover_text(Self::tr_lang(
                                                language,
                                                "Copy Group Code",
                                                "Sao chep ma nhom",
                                            ))
                                            .clicked()
                                        {
                                            export_group = Some(group.id);
                                        }
                                    }
                                    if !group.collapsed {
                                        let folder_popup_id =
                                            ui.make_persistent_id((group.id, "macro-group-folder-popup"));
                                        let mut folder_popup_open = ui
                                            .ctx()
                                            .data(|data| data.get_temp::<bool>(folder_popup_id))
                                            .unwrap_or(false);
                                        let folder_button = Self::sound_style_icon_button(
                                            ui,
                                            Self::folder_icon_text(group.folder_id.is_some(), 18.0).color(
                                                if group.folder_id.is_some() {
                                                    Color32::from_rgb(248, 214, 102)
                                                } else {
                                                    ui.visuals().widgets.inactive.fg_stroke.color
                                                },
                                            ),
                                        );
                                        if folder_button.clicked() {
                                            folder_popup_open = true;
                                        }
                                        let mut selected_folder_after_popup: Option<Option<u32>> = None;
                                        let popup_response = egui::Popup::from_response(&folder_button)
                                            .id(folder_popup_id)
                                            .open_bool(&mut folder_popup_open)
                                            .align(egui::RectAlign::BOTTOM_END)
                                            .layout(egui::Layout::top_down_justified(egui::Align::Min))
                                            .width(220.0)
                                            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                                            .show(|ui| {
                                                ui.set_min_width(220.0);
                                                ui.label(Self::tr_lang(language, "Folder", "Folder"));
                                                ui.separator();
                                                if ui
                                                    .selectable_label(
                                                        group.folder_id.is_none(),
                                                        Self::tr_lang(language, "No folder", "No folder"),
                                                    )
                                                    .clicked()
                                                {
                                                    selected_folder_after_popup = Some(None);
                                                }
                                                for folder in &self.state.macro_folders {
                                                    if ui
                                                        .selectable_label(
                                                            group.folder_id == Some(folder.id),
                                                            &folder.name,
                                                        )
                                                        .clicked()
                                                    {
                                                        selected_folder_after_popup = Some(Some(folder.id));
                                                    }
                                                }
                                            });
                                        if let Some(selected_folder) = selected_folder_after_popup {
                                            group.folder_id = selected_folder;
                                            live_sync = true;
                                            folder_popup_open = false;
                                        }
                                        if folder_popup_open
                                            && let Some(pointer_pos) = ui.ctx().pointer_hover_pos()
                                        {
                                            let mut keep_open_rect = folder_button.rect.expand(10.0);
                                            if let Some(popup) = &popup_response {
                                                keep_open_rect =
                                                    keep_open_rect.union(popup.response.rect.expand(10.0));
                                            }
                                            if !keep_open_rect.contains(pointer_pos) {
                                                folder_popup_open = false;
                                            }
                                        }
                                        ui.ctx().data_mut(|data| {
                                            data.insert_temp(folder_popup_id, folder_popup_open)
                                        });
                                        if Self::sound_style_icon_button(
                                            ui,
                                            Self::material_icon_text(0xe145, 18.0),
                                        )
                                        .on_hover_text(Self::tr_lang(language, "Add Preset", "Thêm Preset"))
                                        .clicked()
                                        {
                                            add_preset_to_group = Some(group.id);
                                        }
                                        ui.add_space(4.0);
                                         live_sync |= Self::render_multi_window_targets_with_duplicate_mode(
                                             ui,
                                             language,
                                             (group.id, "macro-group-window-target"),
                                             Self::tr_lang(language, "Any focused window", ""),
                                             &mut group.target_window_title,
                                             &mut group.extra_target_window_titles,
                                             &mut group.match_duplicate_window_titles,
                                             &self.open_windows,
                                         );
                                    }
                                },
                            );
                        });
                        if group.collapsed {
                            return;
                        }
                        let binding_labels = Self::macro_group_binding_labels(group);
                        let group_preset_step_counts = group
                            .presets
                            .iter()
                            .map(|preset| (preset.id, preset.steps.len() as u32))
                            .collect::<Vec<_>>();
                        let group_preset_options = group
                            .presets
                            .iter()
                            .map(|preset_option| {
                                (
                                    preset_option.id,
                                    binding_labels
                                        .get(&preset_option.id)
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            Self::format_macro_trigger_ui(language, preset_option)
                                        }),
                                )
                            })
                            .collect::<Vec<_>>();
                                                let image_search_preset_options = self
                            .state
                            .vision_presets
                            .iter()
                            .map(|preset_option| (preset_option.id, preset_option.name.clone()))
                            .collect::<Vec<_>>();
                        let ocr_preset_options = self
                            .state
                            .ocr_presets
                            .iter()
                            .map(|preset_option| (preset_option.id, preset_option.name.clone()))
                            .collect::<Vec<_>>();
                        for preset_index in render_preset_indices.iter().copied() {
                            let preset = &mut group.presets[preset_index];
                            // Convert old MouseHeld to KeyHeld
                            for step in &mut preset.steps {
                                if step.if_condition_type == IfConditionType::MouseHeld {
                                    step.if_condition_type = IfConditionType::KeyHeld;
                                }
                                for cond in &mut step.extra_conditions {
                                    if cond.condition_type == IfConditionType::MouseHeld {
                                        cond.condition_type = IfConditionType::KeyHeld;
                                        if !cond.mouse_button.is_empty() {
                                            if cond.key_held_name.is_empty() {
                                                cond.key_held_name = cond.mouse_button.clone();
                                            } else {
                                                let mut existing = cond.key_held_name.split(',').map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned).collect::<Vec<_>>();
                                                for part in cond.mouse_button.split(',') {
                                                    let part_trimmed = part.trim();
                                                    if !part_trimmed.is_empty() && !existing.contains(&part_trimmed.to_owned()) {
                                                        existing.push(part_trimmed.to_owned());
                                                    }
                                                }
                                                cond.key_held_name = existing.join(",");
                                            }
                                            cond.mouse_button.clear();
                                        }
                                    }
                                }
                            }
                            {
                                let step = &mut preset.hold_stop_step;
                                if step.if_condition_type == IfConditionType::MouseHeld {
                                    step.if_condition_type = IfConditionType::KeyHeld;
                                }
                                for cond in &mut step.extra_conditions {
                                    if cond.condition_type == IfConditionType::MouseHeld {
                                        cond.condition_type = IfConditionType::KeyHeld;
                                        if !cond.mouse_button.is_empty() {
                                            if cond.key_held_name.is_empty() {
                                                cond.key_held_name = cond.mouse_button.clone();
                                            } else {
                                                let mut existing = cond.key_held_name.split(',').map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned).collect::<Vec<_>>();
                                                for part in cond.mouse_button.split(',') {
                                                    let part_trimmed = part.trim();
                                                    if !part_trimmed.is_empty() && !existing.contains(&part_trimmed.to_owned()) {
                                                        existing.push(part_trimmed.to_owned());
                                                    }
                                                }
                                                cond.key_held_name = existing.join(",");
                                            }
                                            cond.mouse_button.clear();
                                        }
                                    }
                                }
                            }
                            Self::show_macro_preset_card(ui, group.enabled && folder_enabled, preset.enabled, |ui| {
                                ui.horizontal_top(|ui| {
                                    let available_width = ui.available_width();
                                    let right_width = 540.0;
                                    let left_width =
                                        (available_width - right_width - 8.0).max(260.0);
                                    let trigger_label_width = ui
                                        .painter()
                                        .layout_no_wrap(
                                            Self::tr_lang(language, "Trigger", "Kich hoat").to_owned(),
                                            egui::FontId::proportional(14.0),
                                            ui.visuals().text_color(),
                                        )
                                        .size()
                                        .x
                                        .max(
                                            ui.painter()
                                                .layout_no_wrap(
                                                    Self::tr_lang(language, "Release", "Tha").to_owned(),
                                                    egui::FontId::proportional(14.0),
                                                    ui.visuals().text_color(),
                                                )
                                                .size()
                                                .x,
                                        )
                                        + 10.0;
                                    let label_width = trigger_label_width.max(48.0);
                                    let binding_width = (left_width - label_width - 6.0).max(160.0);
                                    let referenced_vars =
                                        Self::collect_preset_referenced_variables(preset);
                                    ui.allocate_ui_with_layout(
                                        vec2(left_width, 0.0),
                                        egui::Layout::left_to_right(egui::Align::TOP),
                                        |ui| {
                                            ui.allocate_ui_with_layout(
                                                vec2(label_width, 0.0),
                                                egui::Layout::top_down(egui::Align::LEFT),
                                                |ui| {
                                                    ui.label(Self::tr_lang(
                                                         language,
                                                         if preset.trigger_mode
                                                             == MacroTriggerMode::Release
                                                         {
                                                             "Release"
                                                         } else {
                                                             "Trigger"
                                                         },
                                                         if preset.trigger_mode
                                                             == MacroTriggerMode::Release
                                                         {
                                                             "Thả"
                                                         } else {
                                                             "Kích hoạt"
                                                         },
                                                     ));
                                                },
                                            );
                                            ui.allocate_ui_with_layout(
                                                vec2(binding_width, 0.0),
                                                egui::Layout::top_down(egui::Align::LEFT),
                                                |ui| {
                                                    live_sync |= Self::render_macro_trigger_chips(
                                                        ui,
                                                        language,
                                                        group.id,
                                                        preset,
                                                        capture_target_snapshot.as_ref(),
                                                        capture_hotkey_combo_keys_snapshot.as_ref(),
                                                    );
                                                    if !preset.collapsed && !referenced_vars.is_empty() {
                                                        let active_vars_expanded_id = ui.make_persistent_id(
                                                            (group.id, preset.id, "active-vars-expanded"),
                                                        );
                                                        let active_vars_label = Self::tr_lang(
                                                            language,
                                                            "Active Variables:",
                                                            "Active Variables:",
                                                        );
                                                        let label_font =
                                                            egui::FontId::new(11.0, egui::FontFamily::Proportional);
                                                        let badge_font =
                                                            egui::FontId::new(11.0, egui::FontFamily::Proportional);
                                                        let label_width = ui
                                                            .painter()
                                                            .layout_no_wrap(
                                                                active_vars_label.to_owned(),
                                                                label_font,
                                                                ui.visuals().weak_text_color(),
                                                            )
                                                            .size()
                                                            .x;
                                                        let vars_map =
                                                            crate::overlay::RUNTIME_VARIABLES.lock();
                                                        let badge_texts: Vec<(String, bool)> = referenced_vars
                                                            .iter()
                                                            .map(|var_name| {
                                                                let val = vars_map.get(var_name).copied();
                                                                let val_str = val
                                                                    .map(|v| v.to_string())
                                                                    .unwrap_or_else(|| "?".to_string());
                                                                (format!("{} = {}", var_name, val_str), val.is_some())
                                                            })
                                                            .collect();
                                                        let badge_widths: Vec<f32> = badge_texts
                                                            .iter()
                                                            .map(|(badge_text, _)| {
                                                                ui.painter()
                                                                    .layout_no_wrap(
                                                                        badge_text.clone(),
                                                                        badge_font.clone(),
                                                                        ui.visuals().text_color(),
                                                                    )
                                                                    .size()
                                                                    .x
                                                                    + 18.0
                                                            })
                                                            .collect();
                                                        let badge_width_sum: f32 = badge_widths
                                                            .iter()
                                                            .copied()
                                                            .sum();
                                                        drop(vars_map);
                                                        let badge_gap_width = if referenced_vars.is_empty() {
                                                            0.0
                                                        } else {
                                                            4.0 * referenced_vars.len().saturating_sub(1) as f32
                                                        };
                                                        let available_vars_width = ui.available_width().max(0.0);
                                                        let toggle_width = 24.0;
                                                        let active_vars_overflow =
                                                            label_width
                                                                + 8.0
                                                                + badge_width_sum
                                                                + badge_gap_width
                                                                > available_vars_width;
                                                        let mut active_vars_expanded = ui
                                                            .ctx()
                                                            .data(|data| {
                                                                data.get_temp::<bool>(
                                                                    active_vars_expanded_id,
                                                                )
                                                            })
                                                            .unwrap_or(!active_vars_overflow);
                                                        if !active_vars_overflow {
                                                            active_vars_expanded = true;
                                                        }
                                                        ui.add_space(4.0);
                                                        ui.horizontal(|ui| {
                                                            ui.spacing_mut().item_spacing =
                                                                vec2(4.0, 4.0);
                                                            ui.label(
                                                                RichText::new(active_vars_label)
                                                                .size(11.0)
                                                                .weak(),
                                                            );
                                                            let mut consumed_width = label_width + 4.0;
                                                            let preview_width_limit =
                                                                (available_vars_width
                                                                    - if active_vars_overflow {
                                                                        toggle_width + 4.0
                                                                    } else {
                                                                        0.0
                                                                    })
                                                                    .max(0.0);
                                                            for ((badge_text, has_value), badge_width) in badge_texts
                                                                .iter()
                                                                .zip(badge_widths.iter())
                                                            {
                                                                if active_vars_overflow
                                                                    && !active_vars_expanded
                                                                    && consumed_width + badge_width
                                                                        > preview_width_limit
                                                                {
                                                                    break;
                                                                }
                                                                let (
                                                                    fill_color,
                                                                    stroke_color,
                                                                    text_color,
                                                                ) = Self::active_variable_badge_colors(
                                                                    *has_value,
                                                                );
                                                                egui::Frame::none()
                                                                    .fill(fill_color)
                                                                    .stroke(egui::Stroke::new(
                                                                        1.0,
                                                                        stroke_color,
                                                                    ))
                                                                    .inner_margin(
                                                                        egui::Margin::symmetric(6, 2),
                                                                    )
                                                                    .rounding(4.0)
                                                                    .show(ui, |ui| {
                                                                        ui.add(
                                                                            egui::Label::new(
                                                                                RichText::new(
                                                                                    badge_text.clone(),
                                                                                )
                                                                                .size(11.0)
                                                                                .strong()
                                                                                .color(text_color),
                                                                            )
                                                                            .wrap_mode(
                                                                                egui::TextWrapMode::Truncate,
                                                                            ),
                                                                        );
                                                                    });
                                                                consumed_width += badge_width + 4.0;
                                                            }
                                                            if active_vars_overflow {
                                                                let toggle_icon = if active_vars_expanded {
                                                                    0xe5cf
                                                                } else {
                                                                    0xe5cc
                                                                };
                                                                if ui
                                                                    .add_sized(
                                                                        [18.0, 18.0],
                                                                        Button::new(
                                                                            Self::material_icon_text(
                                                                                toggle_icon,
                                                                                14.0,
                                                                            ),
                                                                        ),
                                                                    )
                                                                    .on_hover_text(Self::tr_lang(
                                                                        language,
                                                                        "Show or hide active variables.",
                                                                        "Show or hide active variables.",
                                                                    ))
                                                                    .clicked()
                                                                {
                                                                    active_vars_expanded =
                                                                        !active_vars_expanded;
                                                                }
                                                            }
                                                        });
                                                        ui.ctx().data_mut(|data| {
                                                            data.insert_temp(
                                                                active_vars_expanded_id,
                                                                active_vars_expanded,
                                                            );
                                                        });
                                                        if active_vars_overflow && active_vars_expanded {
                                                            ui.horizontal_wrapped(|ui| {
                                                                ui.spacing_mut().item_spacing =
                                                                    vec2(4.0, 4.0);
                                                                for (badge_text, has_value) in &badge_texts {
                                                                    let (
                                                                        fill_color,
                                                                        stroke_color,
                                                                        text_color,
                                                                    ) = Self::active_variable_badge_colors(
                                                                        *has_value,
                                                                    );
                                                                    egui::Frame::none()
                                                                        .fill(fill_color)
                                                                        .stroke(egui::Stroke::new(
                                                                            1.0,
                                                                            stroke_color,
                                                                        ))
                                                                        .inner_margin(
                                                                            egui::Margin::symmetric(
                                                                                6,
                                                                                2,
                                                                            ),
                                                                        )
                                                                        .rounding(4.0)
                                                                        .show(ui, |ui| {
                                                                            ui.add(
                                                                                egui::Label::new(
                                                                                    RichText::new(
                                                                                        badge_text.clone(),
                                                                                    )
                                                                                    .size(11.0)
                                                                                    .strong()
                                                                                    .color(text_color),
                                                                                )
                                                                                .wrap_mode(
                                                                                    egui::TextWrapMode::Truncate,
                                                                                ),
                                                                            );
                                                                        });
                                                                }
                                                            });
                                                        }
                                                    }
                                                },
                                            );
                                        },
                                    );
                                    let right_spacer =
                                        (ui.available_width() - right_width).max(0.0);
                                    if right_spacer > 0.0 {
                                        ui.add_space(right_spacer);
                                    }
                                    ui.allocate_ui_with_layout(
                                        vec2(right_width, 0.0),
                                        egui::Layout::right_to_left(egui::Align::TOP),
                                        |ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            if Self::sound_style_remove_button(ui).clicked() {
                                                remove_preset = Some(preset.id);
                                            }
                                            let paste_response = ui.add_enabled_ui(self.macro_preset_clipboard.is_some(), |ui| {
                                                ui.add_sized([60.0, 24.0], Button::new(Self::tr_lang(language, "Paste", "Paste")))
                                            }).inner;
                                            if paste_response.clicked() {
                                                paste_preset_to_group = Some(group.id);
                                            }
                                            if Self::sized_button(
                                                 ui,
                                                 60.0,
                                                 Self::tr_lang(language, "Copy", "Copy"),
                                             )
                                             .clicked()
                                             {
                                                 self.macro_preset_clipboard = Some(preset.clone());
                                                 self.status = "Copied macro preset.".to_owned();
                                             }
                                             if self.show_share_buttons {
                                                 let preset_export_feedback =
                                                     self.macro_preset_export_feedback_target
                                                         == Some(preset.id)
                                                         && Self::is_copy_feedback_active(
                                                          self.macro_preset_export_feedback_until,
                                                      );
                                              let preset_export_label = if preset_export_feedback {
                                                  Self::tr_lang(language, "Copied", "Copied")
                                              } else {
                                                  Self::tr_lang(language, "Exp", "Exp")
                                              };
                                              let preset_export_button = ui.add_sized(
                                                  [60.0, 24.0],
                                                  Button::new(preset_export_label).fill(if preset_export_feedback {
                                                      Color32::from_rgba_premultiplied(72, 156, 116, 140)
                                                  } else {
                                                      ui.visuals().widgets.inactive.bg_fill
                                                  })
                                                  .stroke(egui::Stroke::new(
                                                      1.0,
                                                      if preset_export_feedback {
                                                          Color32::from_rgb(126, 224, 182)
                                                      } else {
                                                          ui.visuals().widgets.inactive.bg_stroke.color
                                                      },
                                                  )),
                                              );
                                              if preset_export_button
                                                  .on_hover_text(Self::tr_lang(
                                                      language,
                                                      "Export Preset Code",
                                                      "Sao chép mã preset",
                                                  ))
                                                  .clicked()
                                              {
                                                  export_preset = Some(preset.id);
                                              }
                                             if ui
                                                 .add_enabled(
                                                     self.macro_share_clipboard_kind
                                                         == crate::ui::MacroShareCodeKind::Preset,
                                                     Button::new(Self::tr_lang(language, "Imp", "Imp"))
                                                         .min_size(egui::vec2(46.0, 24.0)),
                                                 )
                                                 .on_hover_text(Self::tr_lang(
                                                     language,
                                                     "Import Preset from Clipboard",
                                                     "Nhap Preset tu clipboard",
                                                 ))
                                                 .clicked()
                                             {
                                                 import_preset_to_group = Some((group.id, Some(preset.id)));
                                             }
                                             }
                                            if false {
                                            let mouse_trigger_options = [
                                                (
                                                    "MouseLeft",
                                                    Self::tr_lang(language, "Left Click", "Click Trái"),
                                                ),
                                                (
                                                    "MouseRight",
                                                    Self::tr_lang(language, "Right Click", "Click Phải"),
                                                ),
                                                (
                                                    "MouseMiddle",
                                                    Self::tr_lang(language, "Middle Click", "Click Giữa"),
                                                ),
                                                ("MouseX1", Self::tr_lang(language, "Mouse X1", "Nút Phụ 1 (X1)")),
                                                ("MouseX2", Self::tr_lang(language, "Mouse X2", "Nút Phụ 2 (X2)")),
                                                (
                                                    "MouseWheelUp",
                                                    Self::tr_lang(language, "Wheel Up", "Cuộn Lên"),
                                                ),
                                                (
                                                    "MouseWheelDown",
                                                    Self::tr_lang(language, "Wheel Down", "Cuộn Xuống"),
                                                ),
                                            ];
                                            let selected_mouse_key =
                                                Self::macro_trigger_bindings(preset)
                                                    .into_iter()
                                                    .rev()
                                                    .find(|binding| {
                                                        binding.combo_keys.iter().any(|key| {
                                                            hotkey::is_mouse_key_name(key)
                                                        })
                                                    })
                                                    .and_then(|binding| {
                                                        binding.combo_keys.into_iter().find(|key| {
                                                            hotkey::is_mouse_key_name(key)
                                                        })
                                                    });
                                            let selected_mouse_label = selected_mouse_key
                                                .as_deref()
                                                .and_then(|key| {
                                                    mouse_trigger_options.iter().find(
                                                        |(option_key, _)| {
                                                            option_key.eq_ignore_ascii_case(key)
                                                        },
                                                    )
                                                })
                                                .map(|(_, label)| *label)
                                                .unwrap_or_else(|| {
                                                    Self::tr_lang(language, "Mouse", "Mouse")
                                                });
                                            let mouse_trigger_response = ui
                                                .scope(|ui| {
                                                    ui.spacing_mut().interact_size.y = 21.0;
                                                    egui::ComboBox::from_id_salt((
                                                        group.id,
                                                        preset.id,
                                                        "mouse-trigger-dropdown",
                                                    ))
                                                    .width(96.0)
                                                    .selected_text(selected_mouse_label)
                                                    .show_ui(ui, |ui| {
                                                        for (option_key, option_label) in
                                                            mouse_trigger_options
                                                        {
                                                            if ui
                                                                .selectable_label(
                                                                    selected_mouse_key
                                                                        .as_ref()
                                                                        .is_some_and(|current| {
                                                                            current
                                                                            .eq_ignore_ascii_case(
                                                                                option_key,
                                                                            )
                                                                        }),
                                                                    option_label,
                                                                )
                                                                .clicked()
                                                            {
                                                                let binding =
                                                                    hotkey::parse_binding(
                                                                        option_key,
                                                                    )
                                                                    .unwrap_or_else(|| {
                                                                        HotkeyBinding {
                                                                            ctrl: false,
                                                                            alt: false,
                                                                            shift: false,
                                                                            win: false,
                                                                            key: option_key
                                                                                .to_owned(),
                                                                            combo_keys: vec![
                                                                                option_key
                                                                                    .to_owned(),
                                                                            ],
                                                                        }
                                                                    });
                                                                live_sync |=
                                                                    Self::macro_trigger_add_binding(
                                                                        preset, binding,
                                                                    );
                                                            }
                                                        }
                                                    })
                                                })
                                                .inner;
                                            mouse_trigger_response
                                                .response
                                                .on_hover_text(selected_mouse_label);
                                            }
                                            let capture_target = CaptureRequest::MacroPresetHotkey(
                                                group.id, preset.id,
                                            );
                                            if ui
                                                .add_sized(
                                                    [64.0, 24.0],
                                                    Button::new(Self::capture_button_text(
                                                        language,
                                                        capture_target_snapshot.as_ref()
                                                            == Some(&capture_target),
                                                    )),
                                                )
                                                .clicked()
                                            {
                                                if capture_target_snapshot.as_ref()
                                                    == Some(&capture_target)
                                                {
                                                    cancel_active_capture = true;
                                                } else {
                                                    next_capture_target = Some(capture_target);
                                                }
                                            }
                                            if Self::sized_button(
                                                ui,
                                                56.0,
                                                if preset.collapsed {
                                                    Self::tr_lang(language, "Show", "Show")
                                                } else {
                                                    Self::tr_lang(language, "Hide", "Hide")
                                                },
                                            )
                                            .clicked()
                                            {
                                                preset.collapsed = !preset.collapsed;
                                                if preset.collapsed {
                                                    for step in &mut preset.steps {
                                                        if step.action == MacroAction::DrawGeometry {
                                                            step.geometry_collapsed = true;
                                                        }
                                                        if step.action == MacroAction::StartAudioSensePreset {
                                                            step.audio_sense_collapsed = true;
                                                        }
                                                    }
                                                    if let Some((_, preview_preset_id, _, _)) = self.draw_geometry_step_preview_target {
                                                        if preview_preset_id == preset.id {
                                                            self.draw_geometry_step_preview_target = None;
                                                            let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometrySpec(None));
                                                        }
                                                    }
                                                }
                                                live_sync = true;
                                            }
                                            let is_preset_enabled = preset.enabled;
                                            let is_preset_active = is_preset_enabled && group.enabled && folder_enabled;
                                            let enabled_icon = if is_preset_enabled { 0xe5ca } else { 0xe835 };
                                            let enabled_fill = if is_preset_active {
                                                Color32::from_rgba_premultiplied(72, 156, 116, 120)
                                            } else if is_preset_enabled {
                                                Color32::from_rgba_premultiplied(72, 156, 116, 50)
                                            } else {
                                                ui.visuals().faint_bg_color
                                            };
                                            let enabled_stroke = if is_preset_active {
                                                Color32::from_rgb(126, 224, 182)
                                            } else if is_preset_enabled {
                                                Color32::from_rgb(110, 180, 142)
                                            } else {
                                                ui.visuals().widgets.noninteractive.bg_stroke.color
                                            };
                                            if ui
                                                .add_sized(
                                                    [36.0, 24.0],
                                                    Button::new(Self::material_icon_text(
                                                        enabled_icon,
                                                        18.0,
                                                    ))
                                                    .fill(enabled_fill)
                                                    .stroke(egui::Stroke::new(1.0, enabled_stroke)),
                                                )
                                                .on_hover_text(Self::tr_lang(
                                                    language,
                                                    if !folder_enabled {
                                                        "Folder containing this preset is disabled"
                                                    } else if !group.enabled {
                                                        "Group containing this preset is disabled"
                                                    } else {
                                                        "Enable / disable preset"
                                                    },
                                                    "",
                                                ))
                                                .clicked()
                                            {
                                                preset.enabled = !preset.enabled;
                                                live_sync = true;
                                            }
                                             let has_preset_inf_loop = self.state.macro_infinite_loop_warning_enabled
                                                 && preset.enabled
                                                 && (
                                                     (preset.trigger_mode == MacroTriggerMode::Press && !preset.stop_on_retrigger_immediate)
                                                     || preset.trigger_mode == MacroTriggerMode::Release
                                                 )
                                                 && preset.steps.iter().any(|s| s.action == MacroAction::LoopStart && s.is_infinite_loop());
                                             let has_preset_vision_leak = preset.enabled
                                                 && (preset.trigger_mode == MacroTriggerMode::Press || preset.trigger_mode == MacroTriggerMode::Release)
                                                 && preset.steps.iter().any(|s| s.action == MacroAction::StartVisionSearch && s.enabled)
                                                 && !preset.steps.iter().any(|s| s.action == MacroAction::StopVision && s.enabled);
                                             let has_preset_audio_leak = preset.enabled
                                                 && (preset.trigger_mode == MacroTriggerMode::Press || preset.trigger_mode == MacroTriggerMode::Release)
                                                 && preset.steps.iter().any(|s| matches!(
                                                     s.action,
                                                     MacroAction::StartAudioSensePreset
                                                 ) && s.enabled)
                                                 && !preset.steps.iter().any(|s| matches!(
                                                     s.action,
                                                     MacroAction::StopAudioSense
                                                 ) && s.enabled);
                                             if has_preset_inf_loop || has_preset_vision_leak || has_preset_audio_leak {
                                                 ui.add_space(4.0);
                                                 let response = ui.add_sized([24.0, 21.0], egui::Button::new(
                                                     Self::material_icon_text(0xe002, 18.0).color(Color32::from_rgb(255, 90, 0))
                                                 ).frame(false));
                                                 if response.contains_pointer() {
                                                     egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.id.with("preset-tip"), |ui| {
                                                         ui.horizontal(|ui| {
                                                             ui.label(Self::material_icon_text(0xe002, 14.0).color(Color32::from_rgb(255, 90, 0)));
                                                             ui.label(RichText::new(Self::tr_lang(language, "MACRO WARNING", "CẢNH BÁO MACRO")).strong().color(Color32::from_rgb(255, 90, 0)));
                                                         });
                                                         if has_preset_inf_loop {
                                                             ui.label(Self::tr_lang(
                                                                 language,
                                                                 "This macro contains an infinite loop and is active. Ensure you know how to stop it to avoid system hang!",
                                                                "Macro này chứa vòng lặp vô hạn và đang ở chế độ tự kích hoạt. Hãy đảm bảo bạn đã biết cách dừng nó để tránh treo máy!"
                                                             ));
                                                         }
                                                         if has_preset_vision_leak {
                                                             ui.label(Self::tr_lang(
                                                                 language,
                                                                 "This macro starts image search (Press/Release trigger) but does not contain a 'StopImageSearch' action! This could lead to a persistent background CPU thread. Add a 'StopImageSearch' step or change trigger to 'Hold'.",
                                                                "Macro này bắt đầu tìm kiếm hình ảnh (chế độ Nhấn/Thả) nhưng không có bước dừng tìm ảnh! Điều này có thể dẫn tới luồng chạy ngầm liên tục gây hao CPU. Hãy thêm bước dừng tìm ảnh hoặc đổi trigger sang Giữ (Hold)."
                                                             ));
                                                         }
                                                         if has_preset_audio_leak {
                                                             ui.label(Self::tr_lang(
                                                                 language,
                                                                 "This macro starts AudioSense under Press/Release trigger, but does not contain a stop-audio action! This could leave a background audio monitor running. Add a 'StopAudio' step or change trigger to 'Hold'.",
                                                                 "Macro nay bat dau AudioSense o trigger Nhan/Tha nhung khong co buoc dung audio! Dieu nay co the de lai mot bo giam sat am thanh chay ngam. Hay them buoc 'StopAudio' hoac doi trigger sang Giu (Hold)."
                                                             ));
                                                         }
                                                     });
                                                 }
                                             }
                                            },
                                    );
                                });
                                if !preset.collapsed {
                                    ui.push_id((group.id, preset.id, "preset-steps-container"), |ui| {
                                        ui.horizontal(|ui| {
                            ui.label(Self::tr_lang(language, "Mode", "Mode"));
                            egui::ComboBox::from_id_salt((group.id, preset.id, "trigger-mode"))
                                .width(108.0)
                                .selected_text(Self::macro_trigger_mode_label(preset.trigger_mode, language))
                                .show_ui(ui, |ui| {
                                    for mode in [
                                        MacroTriggerMode::Press,
                                        MacroTriggerMode::Hold,
                                        MacroTriggerMode::Release,
                                    ] {
                                        if ui
                                            .selectable_label(
                                                preset.trigger_mode == mode,
                                                Self::macro_trigger_mode_label(mode, language),
                                            )
                                            .clicked()
                                        {
                                            preset.trigger_mode = mode;
                                            live_sync = true;
                                        }
                                    }
                                });
                            if preset.trigger_mode == MacroTriggerMode::Press {
                                live_sync |= ui
                                    .checkbox(
                                        &mut preset.pass_through_press,
                                        Self::tr_lang(language, "Pass trigger through", "Cho phép phím đi qua"),
                                    )
                                    .on_hover_text(
                                        Self::tr_lang(
                                            language,
                                            "Let the trigger input keep going to the game or app while it also starts this macro.",
                                            "Cho phép phím kích hoạt tiếp tục truyền đến trò chơi hoặc ứng dụng trong khi cũng bắt đầu chạy macro này.",
                                        ),
                                    )
                                    .changed();
                                live_sync |= ui
                                    .checkbox(
                                        &mut preset.stop_on_retrigger_immediate,
                                        Self::tr_lang(language, "Stop on trigger again", "Dừng khi kích hoạt lại"),
                                    )
                                    .on_hover_text(
                                        Self::tr_lang(
                                            language,
                                            "Press the trigger again to stop this macro immediately, without waiting for a StopIfTriggerPressedAgain step.",
                                            "Nhấn lại phím kích hoạt để dừng macro này ngay lập tức, không cần chờ bước StopIfTriggerPressedAgain.",
                                        ),
                                     )
                                        .changed();
                                } else {
                                preset.stop_on_retrigger_immediate = false;
                            }
                            if preset.trigger_mode == MacroTriggerMode::Hold {
                                live_sync |= ui
                                    .checkbox(
                                        &mut preset.pass_through_hold,
                                        Self::tr_lang(language, "Pass trigger through", "Cho phép phím đi qua"),
                                    )
                                    .on_hover_text(
                                        Self::tr_lang(
                                            language,
                                            "While this hold trigger is active, let the same input keep going to the game or app too.",
                                            "Trong khi phím giữ đang hoạt động, cho phép phím đó tiếp tục truyền đến trò chơi hoặc ứng dụng.",
                                        ),
                                    )
                                    .changed();
                                live_sync |= ui
                                    .checkbox(
                                        &mut preset.hold_stop_step_enabled,
                                        Self::tr_lang(
                                            language,
                                            "Run one action if hold stops early",
                                            "Chạy một action nếu hold dừng sớm",
                                        ),
                                    )
                                    .on_hover_text(
                                        Self::tr_lang(
                                            language,
                                            "If this hold macro is interrupted before it finishes all steps, run this extra action once on stop.",
                                            "Nếu macro hold này bị ngắt trước khi chạy hết các bước, hãy chạy thêm action này một lần khi dừng.",
                                        ),
                                     )
                                        .changed();
                                } else {
                                preset.hold_stop_step_enabled = false;
                            }
                            if preset.trigger_mode == MacroTriggerMode::Release {
                                ui.add_space(8.0);
                                live_sync |= ui
                                    .checkbox(
                                        &mut preset.release_requires_all_inputs_released,
                                        Self::tr_lang(
                                            language,
                                            "Wait for other keys to release before triggering",
                                            "Đợi các phím khác nhả ra rồi mới kích hoạt",
                                        ),
                                    )
                                    .on_hover_text(
                                        Self::tr_lang(
                                            language,
                                            "If enabled, releasing the trigger key or mouse button will not fire while any other key or mouse button is still held down.",
                                            "Nếu bật, khi bạn thả phím kích hoạt ra, macro sẽ chưa chạy ngay nếu vẫn còn các phím/nút chuột khác đang được giữ. Nó sẽ đề£i cho đến khi toàn bộ các phím đó được nhả ra hết rồi mới chính thức kích hoạt.",
                                        ),
                                     )
                                        .changed();
                                } else {
                                preset.release_requires_all_inputs_released = false;
                            }
                        });
                                    if preset.trigger_mode == MacroTriggerMode::Release {
                                        if preset.release_requires_all_inputs_released {
                                            ui.horizontal(|ui| {
                                                live_sync |= Self::render_key_list_chips(
                                                    ui,
                                                    language,
                                                    &mut preset.release_wait_key,
                                                    Self::tr_lang(language, "Not set", "Not set"),
                                                );
                                                let wait_capture_target =
                                                    CaptureRequest::MacroPresetReleaseWaitKey(
                                                        group.id, preset.id,
                                                    );
                                                if ui
                                                    .add_sized(
                                                        [64.0, 20.0],
                                                        Button::new(Self::capture_button_text(
                                                            language,
                                                            capture_target_snapshot.as_ref()
                                                                == Some(&wait_capture_target),
                                                        )),
                                                    )
                                                    .clicked()
                                                {
                                                    if capture_target_snapshot.as_ref()
                                                        == Some(&wait_capture_target)
                                                    {
                                                        cancel_active_capture = true;
                                                    } else {
                                                        next_capture_target =
                                                            Some(wait_capture_target);
                                                    }
                                                }
                                            });
                                        }
                                    }
                                    if preset.trigger_mode == MacroTriggerMode::Hold && preset.hold_stop_step_enabled {
                                        Frame::group(ui.style())
                                .inner_margin(egui::Margin::symmetric(6, 4))
                                .show(ui, |ui| {
                                        let mut clear_hold_stop_step = false;
                                        let step = &mut preset.hold_stop_step;
                                        let is_dark_theme = self.state.ui_theme == UiThemeMode::Dark;
                                        let hint_color = if is_dark_theme {
                                            Color32::from_rgba_unmultiplied(140, 140, 140, 150)
                                        } else {
                                            Color32::from_rgba_unmultiplied(100, 100, 100, 150)
                                        };
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(Self::tr_lang(language, "On Stop", "On Stop"));
                                            let hold_stop_combo = egui::ComboBox::from_id_salt((
                                                group.id,
                                                preset.id,
                                                "hold-stop-action",
                                            ))
                                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                            .width(168.0)
                                            .height(420.0)
                                            .selected_text(Self::macro_action_selected_widget_text(step.action, language))
                                            .show_ui(ui, |ui| {
                                                ui.set_min_width(520.0);
                                                live_sync |= ui.checkbox(&mut step.toggle_enabled_on_run, Self::tr_lang(
                                                    language,
                                                    "Toggle self enabled on run",
                                                    "Tự bật/tắt chính nó khi chạy",
                                                )).changed();
                                                ui.separator();
                                                let action_hover_id = ui.make_persistent_id((
                                                    group.id,
                                                    preset.id,
                                                    "hold-stop-action-hover",
                                                ));
                                                ui.ctx().data_mut(|data| {
                                                    data.insert_temp(action_hover_id, false);
                                                });
                                                egui::Grid::new((group.id, preset.id, "hold-stop-action-grid"))
                                                    .num_columns(8)
                                                    .spacing([6.0, 6.0])
                                                    .show(ui, |ui| {
                                                        let mut grid_col = 0;
                                                        for action in [
                                                            MacroAction::KeyPress,
                                                            MacroAction::KeyDown,
                                                            MacroAction::KeyUp,
                                                            MacroAction::TypeText,
                                                            MacroAction::ApplyWindowPreset,
                                                            MacroAction::FocusWindowPreset,
                                                            MacroAction::TriggerMacroPreset,
                                                            MacroAction::TriggerCommandPreset,
                                                            MacroAction::EnableCrosshairProfile,
                                                            MacroAction::DisableCrosshair,
                                                            MacroAction::EnablePinPreset,
                                                            MacroAction::DisablePin,
                                                            MacroAction::PlaySoundPreset,
                                                            MacroAction::PlayVideoPreset,
                                                            MacroAction::ApplyMouseSensitivityPreset,
                                                            MacroAction::LoopStart,
                                                            MacroAction::LoopEnd,
                                                            MacroAction::StopIfKeyPressed,
                                                            MacroAction::ShowHud,
                                                            MacroAction::HideHud,
                                                            MacroAction::LockKeys,
                                                            MacroAction::UnlockKeys,
                                                            MacroAction::EnableMacroPreset,
                                                            MacroAction::DisableMacroPreset,
                                                                MacroAction::EnableStep,
                                                                MacroAction::DisableStep,
                                                             MacroAction::SetVariable,
                                                             MacroAction::OcrSearch,
                                                             MacroAction::JumpToStep,
                                                        ]
                                                        {
                                                            Self::render_macro_action_option(
                                                                ui,
                                                                language,
                                                                &mut step.action,
                                                                action,
                                                                &mut live_sync,
                                                                action_hover_id,
                                                                false,
                                                            );
                                                            grid_col += 1;
                                                            if grid_col % 8 == 0 {
                                                                ui.end_row();
                                                            }
                                                        }
                                                        Self::render_mouse_action_group_option(
                                                            ui,
                                                            language,
                                                            (group.id, preset.id, "hold-stop-mouse-group"),
                                                            &mut step.action,
                                                            &mut live_sync,
                                                            action_hover_id,
                                                        );
                                                        grid_col += 1;
                                                        if grid_col % 8 == 0 { ui.end_row(); }
                                                        Self::render_image_search_action_group_option(
                                                            ui,
                                                            language,
                                                            (group.id, preset.id, "hold-stop-image-search-group"),
                                                            &mut step.action,
                                                            &mut live_sync,
                                                            action_hover_id,
                                                        );
                                                        grid_col += 1;
                                                        if grid_col % 8 == 0 { ui.end_row(); }
                                                        Self::render_timer_action_group_option(
                                                             ui,
                                                             language,
                                                             (group.id, preset.id, "hold-stop-timer-group"),
                                                             &mut step.action,
                                                             &mut live_sync,
                                                             action_hover_id,
                                                         );
                                                        grid_col += 1;
                                                        if grid_col % 8 == 0 { ui.end_row(); }
                                                        Self::render_if_action_group_option(
                                                            ui,
                                                            language,
                                                            (group.id, preset.id, "hold-stop-if-group"),
                                                            &mut step.action,
                                                            &mut live_sync,
                                                            action_hover_id,
                                                        );
                                                        grid_col += 1;
                                                        if grid_col % 8 == 0 { ui.end_row(); }
                                                        Self::render_geometry_action_group_option(
                                                            ui,
                                                            language,
                                                            (group.id, preset.id, "hold-stop-geometry-group"),
                                                            &mut step.action,
                                                            &mut live_sync,
                                                            action_hover_id,
                                                        );
                                                        grid_col += 1;
                                                        if grid_col % 8 == 0 { ui.end_row(); }
                                                        Self::render_audio_sense_action_group_option(
                                                            ui,
                                                            language,
                                                            (group.id, preset.id, "hold-stop-audiosense-group"),
                                                            &mut step.action,
                                                            &mut live_sync,
                                                            action_hover_id,
                                                        );
                                                        grid_col += 1;
                                                        if grid_col % 8 == 0 { ui.end_row(); }
                                                    });
                            });
                                            let action_uses_key = Self::macro_action_uses_key(step.action);
                                            let action_supports_capture =
                                                Self::macro_action_supports_capture(step.action);
                                            if action_uses_key {
                                                if step.action == MacroAction::ApplyWindowPreset {
                                                    let id_salt = ui.make_persistent_id((group.id, preset.id, "hold-stop-window-preset"));
                                                    Self::render_window_control_step_comboboxes(
                                                        &window_presets,
                                                        &window_layouts,
                                                        ui,
                                                        language,
                                                        id_salt,
                                                        &mut step.key,
                                                        &mut live_sync,
                                                        160.0,
                                                    );
                                                } else if step.action == MacroAction::FocusWindowPreset {
                                                    let mut selected_window = if step.key.trim().is_empty() {
                                                        None
                                                    } else {
                                                        Some(step.key.clone())
                                                    };
                                                    let mut duplicate_mode = false;
                                                    if Self::render_window_target_combo_with_duplicate_mode(
                                                        ui,
                                                        (group.id, preset.id, "hold-stop-focus-window-preset"),
                                                        &Self::tr_lang(language, "Select window", "Chọn cửa sổ"),
                                                        &mut selected_window,
                                                        &mut duplicate_mode,
                                                        &self.open_windows,
                                                        160.0,
                                                        false,
                                                    ) {
                                                        step.key = selected_window.unwrap_or_default();
                                                        live_sync = true;
                                                    }
                                                } else if step.action == MacroAction::TriggerMacroPreset {
                                                    // Auto-init group if None
                                                    if step.trigger_macro_group_id.is_none() {
                                                        step.trigger_macro_group_id = Some(group.id);
                                                        live_sync = true;
                                                    }
                                                    let trig_group_id = step.trigger_macro_group_id.unwrap_or(group.id);
                                                    let trig_group_name = all_groups_for_trigger
                                                        .iter()
                                                        .find(|(gid, _, _)| *gid == trig_group_id)
                                                        .map(|(_, gname, _)| gname.clone())
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select group", "Chọn group").to_owned());
                                                    // ComboBox 1: Select group
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-trigger-macro-group"))
                                                        .width(110.0)
                                                        .selected_text(&trig_group_name)
                                                        .show_ui(ui, |ui| {
                                                            for (gid, gname, gpresets) in &all_groups_for_trigger {
                                                                if ui
                                                                    .selectable_label(*gid == trig_group_id, &{
                                                                    if gname.starts_with("Macro Group") && !gpresets.is_empty() {
                                                                        let preset_labels: Vec<String> = gpresets.iter().map(|(_, lbl)| lbl.clone()).collect();
                                                                        format!("{} [{}]", gname, preset_labels.join(", "))
                                                                    } else {
                                                                        gname.clone()
                                                                    }
                                                                })
                                                                    .clicked()
                                                                {
                                                                    step.trigger_macro_group_id = Some(*gid);
                                                                    if *gid != trig_group_id {
                                                                        step.key = String::new();
                                                                    }
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    // ComboBox 2: Select preset from chosen group
                                                    let trig_presets: Vec<(u32, String)> = all_groups_for_trigger
                                                        .iter()
                                                        .find(|(gid, _, _)| *gid == trig_group_id)
                                                        .map(|(gid, _, gpresets)| {
                                                            gpresets.iter()
                                                                .filter(|(pid, _)| true)
                                                                .cloned()
                                                                .collect()
                                                        })
                                                        .unwrap_or_default();
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| trig_presets.iter().find(|(pid, _)| *pid == id).map(|(_, lbl)| lbl.clone()))
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select macro", "Chọn macro").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-trigger-macro"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for (preset_option_id, preset_option_label) in &trig_presets {
                                                                if ui
                                                                    .selectable_label(selected_id == Some(*preset_option_id), preset_option_label)
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option_id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                        ui.add_space(4.0);
                                                        let selected_trigger_warning = selected_id
                                                            .and_then(|id| {
                                                                all_trigger_macro_warnings
                                                                    .iter()
                                                                    .find(|(preset_id, _)| *preset_id == id)
                                                                    .map(|(_, warning)| *warning)
                                                            })
                                                            .unwrap_or(false);
                                                        if selected_trigger_warning {
                                                            let warn_color = Color32::from_rgb(255, 90, 0);
                                                            let response = ui.add_sized([18.0, 18.0], egui::Button::new(
                                                                Self::material_icon_text(0xe002, 16.0).color(warn_color)
                                                            ).frame(false));
                                                            if response.contains_pointer() {
                                                                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.id.with("trigger-macro-warning-tip"), |ui| {
                                                                    ui.horizontal(|ui| {
                                                                        ui.label(Self::material_icon_text(0xe002, 14.0).color(warn_color));
                                                                        ui.label(RichText::new(Self::tr_lang(language, "TRIGGER WARNING", "CẢNH BÁO KÍCH HOẠT")).strong().color(warn_color));
                                                                    });
                                                                    ui.label(Self::tr_lang(
                                                                        language,
                                                                        "This macro preset can run continuously or keep working in the background. Triggering it from another macro can make it run immediately without a hold key.",
                                                                        "Macro này có thể chạy liên tục hoặc tiếp tục chạy trong nền. Kích hoạt nó từ macro khác có thể làm nó chạy ngay mà không cần giữ phím.",
                                                                    ));
                                                                });
                                                            }
                                                        }
                                                        let cb_text = Self::tr_lang(language, "Wait for completion", "Đợi chạy xong");
                                                        if ui.checkbox(&mut step.wait_for_completion, cb_text).changed() {
                                                            live_sync = true;
                                                        }
                                                } else if step.action == MacroAction::TriggerCommandPreset {
                                                    let selected_id = step
                                                        .key
                                                        .trim()
                                                        .parse::<u32>()
                                                        .ok()
                                                        .or_else(|| {
                                                            self.state
                                                                .command_presets
                                                                .iter()
                                                                .find(|preset| preset.name.trim().eq_ignore_ascii_case(step.key.trim()))
                                                                .map(|preset| preset.id)
                                                        });
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .command_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            if step.key.trim().is_empty() {
                                                                Self::tr_lang(language, "Select command", "Chọn câu lệnh")
                                                                .to_owned()
                                                            } else {
                                                                step.key.clone()
                                                            }
                                                        });
                                                    let custom_preset_combo = egui::ComboBox::from_id_salt((group.id, preset.id, "trigger-custom-preset-step"))
                                                        .width(160.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.command_presets {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(preset_option.id),
                                                                        &preset_option.name,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    let is_generating = self.command_ai_job.as_ref()
                                                         .map(|job| job.preset_id == 999999)
                                                         .unwrap_or(false)
                                                         && self.command_ai_step_target.as_ref()
                                                             .map(|target| target.0 == group.id && target.1 == preset.id && target.2.is_none())
                                                             .unwrap_or(false);
                                                     let (custom_draft_changed, custom_save_request, custom_save_and_open_ai_request, open_ai_preset_id) = Self::render_custom_preset_step_draft_popup(
                                                          ui,
                                                          &custom_preset_combo.response,
                                                          &custom_preset_combo.response,
                                                          step,
                                                          (group.id, preset.id, "hold-stop"),
                                                          None,
                                                          language,
                                                          &command_presets_snapshot,
                                                          is_generating,
                                                     );
                                                     live_sync |= custom_draft_changed;
                                                     if let Some((step_index, name, command, use_powershell)) = custom_save_request {
                                                         pending_custom_preset_save = Some((
                                                             group.id,
                                                             preset.id,
                                                             step_index,
                                                             name,
                                                             command,
                                                             use_powershell,
                                                         ));
                                                     }
                                                     if let Some((step_index, name, command, use_powershell, is_ad_hoc)) = custom_save_and_open_ai_request {
                                                         pending_custom_preset_save_and_open_ai = Some((
                                                             group.id,
                                                             preset.id,
                                                             step_index,
                                                             name,
                                                             command,
                                                             use_powershell,
                                                             is_ad_hoc,
                                                         ));
                                                     }
                                                     if let Some(preset_id) = open_ai_preset_id {
                                                         pending_open_ai_preset_id = Some(preset_id);
                                                     }
                                                } else if matches!(
                                                    step.action,
                                                    MacroAction::EnableMacroPreset
                                                        | MacroAction::DisableMacroPreset
                                                ) {
                                                    if step.trigger_macro_group_id.is_none() {
                                                        step.trigger_macro_group_id = Some(group.id);
                                                        live_sync = true;
                                                    }
                                                    let trig_group_id = step.trigger_macro_group_id.unwrap_or(group.id);
                                                    let trig_group_name = all_groups_for_trigger
                                                        .iter()
                                                        .find(|(gid, _, _)| *gid == trig_group_id)
                                                        .map(|(_, gname, _)| gname.clone())
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select group", "Chọn group").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-enable-disable-macro-group"))
                                                        .width(110.0)
                                                        .selected_text(&trig_group_name)
                                                        .show_ui(ui, |ui| {
                                                            for (gid, gname, gpresets) in &all_groups_for_trigger {
                                                                if ui
                                                                    .selectable_label(*gid == trig_group_id, &{
                                                                    if gname.starts_with("Macro Group") && !gpresets.is_empty() {
                                                                        let preset_labels: Vec<String> = gpresets.iter().map(|(_, lbl)| lbl.clone()).collect();
                                                                        format!("{} [{}]", gname, preset_labels.join(", "))
                                                                    } else {
                                                                        gname.clone()
                                                                    }
                                                                })
                                                                    .clicked()
                                                                {
                                                                    step.trigger_macro_group_id = Some(*gid);
                                                                    if *gid != trig_group_id {
                                                                        step.key = String::new();
                                                                    }
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    let trig_presets: Vec<(u32, String)> = all_groups_for_trigger
                                                        .iter()
                                                        .find(|(gid, _, _)| *gid == trig_group_id)
                                                        .map(|(_, _, gpresets)| gpresets.clone())
                                                        .unwrap_or_default();
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| trig_presets.iter().find(|(pid, _)| *pid == id).map(|(_, lbl)| lbl.clone()))
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select macro", "Chọn macro").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-enable-disable-macro-preset"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for (preset_option_id, preset_option_label) in &trig_presets {
                                                                if ui
                                                                    .selectable_label(selected_id == Some(*preset_option_id), preset_option_label)
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option_id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                } else if matches!(step.action, MacroAction::EnableStep | MacroAction::DisableStep) {
                                                    // Parse `preset_id|1,2,3` or `1,2,3` for legacy fallback
                                                    let (selected_preset_id, mut selected_steps) = {
                                                        let parts: Vec<&str> = step.key.split('|').collect();
                                                        if parts.len() == 2 {
                                                            let p_id = parts[0].trim().parse::<u32>().ok();
                                                            let s_list = parts[1].split(',').filter_map(|s| s.trim().parse::<u32>().ok()).collect::<Vec<u32>>();
                                                            (p_id, s_list)
                                                        } else {
                                                            let s_list = step.key.split(',').filter_map(|s| s.trim().parse::<u32>().ok()).collect::<Vec<u32>>();
                                                            (None, s_list)
                                                        }
                                                    };
                                                    let current_preset_id = selected_preset_id.unwrap_or(preset.id);
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        let preset_label = group_preset_options.iter()
                                                            .find(|(id, _)| *id == current_preset_id)
                                                            .map(|(_, label)| label.clone())
                                                            .unwrap_or_else(|| Self::tr_lang(language, "Select preset", "Chọn preset").to_owned());
                                                        egui::ComboBox::from_id_salt((group.id, preset.id, 0, "step-preset-select"))
                                                            .width(100.0)
                                                            .selected_text(preset_label)
                                                            .show_ui(ui, |ui| {
                                                                for (preset_option_id, preset_option_label) in &group_preset_options {
                                                                    if ui.selectable_label(current_preset_id == *preset_option_id, preset_option_label).clicked() {
                                                                        if current_preset_id != *preset_option_id {
                                                                            step.key = format!("{}|", preset_option_id);
                                                                            live_sync = true;
                                                                        }
                                                                    }
                                                                }
                                                            });
                                                        let target_step_count = group_preset_step_counts.iter()
                                                            .find(|(id, _)| *id == current_preset_id)
                                                            .map(|(_, count)| *count)
                                                            .unwrap_or(0);
                                                        let original_len = selected_steps.len();
                                                        selected_steps.retain(|&x| x <= target_step_count);
                                                        if selected_steps.len() != original_len {
                                                            let steps_str = selected_steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
                                                            step.key = format!("{}|{}", current_preset_id, steps_str);
                                                            live_sync = true;
                                                        }
                                                        let steps_label = if selected_steps.is_empty() {
                                                            Self::tr_lang(language, "Select steps", "Chọn steps").to_owned()
                                                        } else {
                                                            selected_steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
                                                        };
                                                        egui::ComboBox::from_id_salt((group.id, preset.id, 0, "step-multi-select"))
                                                            .width(100.0)
                                                            .selected_text(steps_label)
                                                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                                            .show_ui(ui, |ui| {
                                                                for i in 1..=target_step_count {
                                                                    let mut is_selected = selected_steps.contains(&i);
                                                                    if ui.checkbox(&mut is_selected, format!("Step {}", i)).changed() {
                                                                        if is_selected {
                                                                            selected_steps.push(i);
                                                                        } else {
                                                                            selected_steps.retain(|x| *x != i);
                                                                        }
                                                                        selected_steps.sort_unstable();
                                                                        let steps_str = selected_steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
                                                                        step.key = format!("{}|{}", current_preset_id, steps_str);
                                                                        live_sync = true;
                                                                    }
                                                                }
                                                            });
                                                    });
                                                } else if matches!(step.action, MacroAction::StartTimerPreset | MacroAction::PauseTimerPreset | MacroAction::StopTimerPreset) {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state.timer_presets.iter()
                                                                .find(|p| p.id == id)
                                                                .map(|p| p.name.clone())
                                                        })
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select timer", "Chọn hẹn giờ").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-timer-preset"))
                                                        .width(160.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for timer in &self.state.timer_presets {
                                                                if ui.selectable_label(selected_id == Some(timer.id), &timer.name).clicked() {
                                                                    step.key = timer.id.to_string();
                                                                    step.timer_preset_id = Some(timer.id);
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                } else if step.action == MacroAction::EnableCrosshairProfile {
                                                    let selected_label = if step.key.trim().is_empty() {
                                                        Self::tr_lang(language, "Select crosshair", "Chọn tâm ngắm").to_owned()
                                                    } else {
                                                        step.key.clone()
                                                    };
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-crosshair"))
                                                        .width(134.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for profile in &self.state.profiles {
                                                                if ui
                                                                    .selectable_label(step.key == profile.name, &profile.name)
                                                                    .clicked()
                                                                {
                                                                    step.key = profile.name.clone();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    Self::render_overlay_eye_button(
                                                        ui,
                                                        language,
                                                        step,
                                                        group.id,
                                                        preset.id,
                                                        0,
                                                        true,
                                                        &self.state.hud_presets,
                                                        &mut self.draw_geometry_step_preview_target,
                                                        &self.overlay_tx,
                                                        [18.0, 18.0],
                                                        12.0,
                                                    );
                                                } else if step.action == MacroAction::EnablePinPreset {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .pin_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            Self::tr_lang(language, "Select pin", "Chọn ghim").to_owned()
                                                        });
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-pin-preset"))
                                                            .width(134.0)
                                                            .selected_text(selected_label)
                                                            .show_ui(ui, |ui| {
                                                                for preset_option in &self.state.pin_presets {
                                                                    if ui
                                                                        .selectable_label(selected_id == Some(preset_option.id), &preset_option.name)
                                                                        .clicked()
                                                                    {
                                                                        step.key = preset_option.id.to_string();
                                                                        live_sync = true;
                                                                    }
                                                                }
                                                            });
                                                        Self::render_overlay_eye_button(
                                                            ui,
                                                            language,
                                                            step,
                                                            group.id,
                                                            preset.id,
                                                            0,
                                                            true,
                                                            &self.state.hud_presets,
                                                            &mut self.draw_geometry_step_preview_target,
                                                            &self.overlay_tx,
                                                            [18.0, 18.0],
                                                            12.0,
                                                        );
                                                    });
                                                } else if step.action == MacroAction::PlayMousePathPreset {
                                                    let parsed_selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_id = parsed_selected_id.and_then(|id| {
                                                        self.state
                                                            .mouse_path_presets
                                                            .iter()
                                                            .find(|preset| preset.id == id)
                                                            .map(|preset| preset.id)
                                                    });
                                                    if parsed_selected_id.is_some() && selected_id.is_none() {
                                                        step.key.clear();
                                                        live_sync = true;
                                                    }
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .mouse_path_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            Self::tr_lang(language, "Select path", "Chọn đường chuột").to_owned()
                                                        });
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-mouse-path"))
                                                        .width(160.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.mouse_path_presets {
                                                                if ui
                                                                    .selectable_label(selected_id == Some(preset_option.id), &preset_option.name)
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                            ui.separator();
                                                            if ui
                                                                .selectable_label(
                                                                    false,
                                                                    Self::tr_lang(
                                                                        language,
                                                                        "+ Add preset",
                                                                        "+ Them preset",
                                                                    ),
                                                                )
                                                                .clicked()
                                                            {
                                                                add_hold_stop_mouse_path_preset_request =
                                                                    Some((
                                                                        group.id,
                                                                        preset.id,
                                                                        selected_id,
                                                                    ));
                                                                ui.close();
                                                            }
                                                        });
                                                    if ui
                                                        .button(Self::tr_lang(
                                                            language,
                                                            "Preview",
                                                            "Xem truoc",
                                                        ))
                                                        .on_hover_text(Self::tr_lang(
                                                            language,
                                                            "Open the same path preview canvas used in the Mouse Path panel.",
                                                            "Mo khung xem truoc giong trong tab Mouse Path.",
                                                        ))
                                                        .clicked()
                                                    {
                                                        if let Some(path_preset_id) = selected_id {
                                                            preview_mouse_path_step_request =
                                                                Some(Some(path_preset_id));
                                                        } else {
                                                            self.status = Self::tr_lang(
                                                                language,
                                                                "Select a Mouse Path preset first.",
                                                                "Hay chon Mouse Path truoc.",
                                                            )
                                                            .to_owned();
                                                        }
                                                    }
                                                    let wait_label = Self::tr_lang(
                                                        language,
                                                        "Wait for completion",
                                                        "Doi chay xong",
                                                    );
                                                    if ui
                                                        .checkbox(
                                                            &mut step.wait_for_completion,
                                                            wait_label,
                                                        )
                                                        .on_hover_text(Self::tr_lang(
                                                            language,
                                                            "Wait until this Mouse Path finishes before the macro continues to the next step.",
                                                            "Doi Mouse Path nay chay xong roi macro moi chuyen sang step tiep theo.",
                                                        ))
                                                        .changed()
                                                    {
                                                        live_sync = true;
                                                    }
                                                } else if matches!(
                                                    step.action,
                                                    MacroAction::StartVisionSearch
                                                         | MacroAction::ScanVisionOnce
                                                         | MacroAction::StopVision
                                                ) {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            image_search_preset_options
                                                                .iter()
                                                                .find(|(preset_id, _)| *preset_id == id)
                                                                .map(|(_, label)| label.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            Self::tr_lang(
                                                                language,
                                                                "Select image search preset",
                                                                "Chọn preset image search",
                                                            )
                                                            .to_owned()
                                                        });
                                                egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-image-search"))
    .width(160.0)

    .selected_text(selected_label)

    .show_ui(ui, |ui| {

                    let (image_presets, color_presets, pixel_presets): (Vec<_>, Vec<_>, Vec<_>) = self.state.vision_presets.iter().fold(
                        (Vec::new(), Vec::new(), Vec::new()),
                        |(mut img, mut col, mut pix), p| {
                            if p.is_pixel_counter {
                                pix.push(p);
                            } else if p.use_color_matching {
                                col.push(p);
                            } else {
                                img.push(p);
                            }
                            (img, col, pix)
                        }
                    );
                    if !image_presets.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0, 191, 255), "Image Detect");
                        ui.separator();
                        for p in &image_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.key = p.id.to_string();
                                live_sync = true;
                            }
                        }
                    }
                    if !color_presets.is_empty() {
                        if !image_presets.is_empty() {
                            ui.add_space(4.0);
                        }
                        ui.colored_label(egui::Color32::from_rgb(0, 250, 154), "Color Detect");
                        ui.separator();
                        for p in &color_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.key = p.id.to_string();
                                live_sync = true;
                            }
                        }
                    }
                    let show_pixel_counter = step.action == MacroAction::ScanVisionOnce;
                    if show_pixel_counter && !pixel_presets.is_empty() {
                        if !image_presets.is_empty() || !color_presets.is_empty() {
                            ui.add_space(4.0);
                        }
                        ui.colored_label(egui::Color32::from_rgb(255, 165, 0), "Pixel Counter");
                        ui.separator();
                        for p in &pixel_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.key = p.id.to_string();
                                live_sync = true;
                            }
                        }
                    }
    });

                                                 let is_pixel = selected_id.and_then(|id| {
                                                        self.state.vision_presets.iter().find(|p| p.id == id)
                                                    }).map(|p| p.is_pixel_counter).unwrap_or(false);
                                                 let is_single_pixel = selected_id.and_then(|id| {
                                                        self.state.vision_presets.iter().find(|p| p.id == id)
                                                    }).map(|p| p.use_color_matching && p.search_region_is_single_pixel && !p.is_pixel_counter).unwrap_or(false);
                                                 let supports_move_mouse = selected_id.is_some() && !is_pixel && !is_single_pixel;
                                                    if matches!(step.action, MacroAction::StartVisionSearch | MacroAction::StopVision) {
                                                        ui.add_space(4.0);
                                                        ui.weak(Self::tr_lang(language, "(Mouse move only)", "(Chức năng di chuột)"));
                                                    }
                                                                                                   if step.action == MacroAction::ScanVisionOnce {
                                                      ui.add_space(4.0);
                                                      if is_single_pixel {
                                                          ui.label(Self::tr_lang(language, "Color Var:", "Biến màu:"));
                                                          let prev_override = ui.visuals().override_text_color;
                                                          ui.visuals_mut().override_text_color = None;
                                                          let resp = ui.add(egui::TextEdit::singleline(&mut step.if_variable_name).hint_text("color_var").desired_width(100.0));
                                                          ui.visuals_mut().override_text_color = prev_override;
                                                          Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.if_variable_name);
                                                          live_sync |= resp.changed();
                                                      } else {
                                                          let outputs_label = Self::tr_lang(language, "Outputs", "Đầu ra").to_owned();
                                                          egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-vision-outputs"))
                                                              .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                                              .width(110.0)
                                                              .selected_text(outputs_label)
                                                               .show_ui(ui, |ui| {
                                                                   ui.set_min_width(200.0);
                                                                   egui::Grid::new("vision_outputs_grid_hold_stop")
                                                                      .num_columns(2)
                                                                      .spacing([8.0, 6.0])
                                                                      .show(ui, |ui| {
                                                                          if !is_pixel {
                                                                              ui.label("Found:");
                                                                              let prev_override = ui.visuals().override_text_color;
                                                                              ui.visuals_mut().override_text_color = None;
                                                                              let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_found_var).hint_text("found_var"));
                                                                              ui.visuals_mut().override_text_color = prev_override;
                                                                              Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_found_var);
                                                                              live_sync |= resp.changed();
                                                                              ui.end_row();
                                                                              ui.label("Pos X:");
                                                                              let prev_override = ui.visuals().override_text_color;
                                                                              ui.visuals_mut().override_text_color = None;
                                                                              let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_pos_var_x).hint_text("result_x_var"));
                                                                              ui.visuals_mut().override_text_color = prev_override;
                                                                              Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_pos_var_x);
                                                                              live_sync |= resp.changed();
                                                                              ui.end_row();
                                                                              ui.label("Pos Y:");
                                                                              let prev_override = ui.visuals().override_text_color;
                                                                              ui.visuals_mut().override_text_color = None;
                                                                              let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_pos_var_y).hint_text("result_y_var"));
                                                                              ui.visuals_mut().override_text_color = prev_override;
                                                                              Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_pos_var_y);
                                                                              live_sync |= resp.changed();
                                                                              ui.end_row();
                                                                          } else {
                                                                              ui.label("Found:");
                                                                              let prev_override = ui.visuals().override_text_color;
                                                                              ui.visuals_mut().override_text_color = None;
                                                                              let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_found_var).hint_text("found_var"));
                                                                              ui.visuals_mut().override_text_color = prev_override;
                                                                              Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_found_var);
                                                                              live_sync |= resp.changed();
                                                                              ui.end_row();
                                                                              ui.label(Self::tr_lang(language, "Count Var:", "Biến kết quả:"));
                                                                              let prev_override = ui.visuals().override_text_color;
                                                                              ui.visuals_mut().override_text_color = None;
                                                                              let resp = ui.add(egui::TextEdit::singleline(&mut step.if_variable_name).hint_text("count_var"));
                                                                              ui.visuals_mut().override_text_color = prev_override;
                                                                              Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.if_variable_name);
                                                                              live_sync |= resp.changed();
                                                                              ui.end_row();
                                                                          }
                                                                       });
                                                               });
                                                      }
                                                      if supports_move_mouse {
                                                          ui.add_space(4.0);
                                                          let resp = ui.checkbox(
                                                              &mut step.vision_move_cursor_on_match,
                                                              Self::tr_lang(language, "Move Mouse", "Di chuột"),
                                                          );
                                                          live_sync |= resp.changed();
                                                      }
                                                  }
                                                } else if step.action == MacroAction::ApplyMouseSensitivityPreset {
                                                    live_sync |= ui.checkbox(&mut step.manual_mouse_sensitivity, Self::tr_lang(language, "Manual", "Nhập tay")).changed();
                                                    if step.manual_mouse_sensitivity {
                                                         ui.vertical(|ui| {
                                                             let response = Self::render_variable_text_edit(
                                                                 ui,
                                                                 &mut step.key,
                                                                 ui.id().with("sensitivity-manual-key"),
                                                                 110.0,
                                                                 160.0,
                                                                 21.0,
                                                                 21.0,
                                                                 &Self::tr_lang(language, "value/expr", "giá trị"),
                                                                 false,
                                                             );
                                                            Self::apply_vietnamese_input_if_changed(
                                                                &response,
                                                                self.state.vietnamese_input_enabled,
                                                                self.state.vietnamese_input_mode,
                                                                &mut step.key,
                                                            );
                                                            live_sync |= response.changed();
                                                            let interpolated = crate::overlay::interpolate_variables(&step.key);
                                                            let evaluated = crate::overlay::evaluate_math_expression(&interpolated);
                                                            let clamped = evaluated.clamp(1, 20);
                                                            let tooltip_text = match language {
                                                                UiLanguage::Vietnamese => format!("Kết quả: {} (giới hạn: {} trong 1..20)", evaluated, clamped),
                                                                _ => format!("Evaluated: {} (clamped to: {} within 1..20)", evaluated, clamped),
                                                            };
                                                            let response = response.on_hover_text(tooltip_text);
                                                            Self::render_variable_suggestions(ui, &response, &mut step.key, &timer_names, language);
                                                        });
                                                    } else {
                                                        let selected_id = step.key.trim().parse::<u32>().ok();
                                                        let selected_label = selected_id
                                                            .and_then(|id| {
                                                                self.state
                                                                    .mouse_sensitivity_presets
                                                                    .iter()
                                                                    .find(|preset| preset.id == id)
                                                                    .map(|preset| preset.name.clone())
                                                            })
                                                            .unwrap_or_else(|| {
                                                                Self::tr_lang(language, "Select sens", "Chọn độ nhạy")
                                                                .to_owned()
                                                            });
                                                        ui.push_id((group.id, preset.id, "mouse-sensitivity-preset-step"), |ui| {
                                                            egui::ComboBox::from_id_salt("mouse-sensitivity-preset-step-combo")
                                                                .width(110.0)
                                                                .selected_text(selected_label)
                                                                .show_ui(ui, |ui| {
                                                                    for preset_option in &self.state.mouse_sensitivity_presets {
                                                                        if ui
                                                                            .selectable_label(
                                                                                selected_id == Some(preset_option.id),
                                                                                &preset_option.name,
                                                                            )
                                                                            .clicked()
                                                                        {
                                                                            step.key = preset_option.id.to_string();
                                                                            live_sync = true;
                                                                        }
                                                                    }
                                                                });
                                                        });
                                                    }
                                                } else if step.action == MacroAction::EnableZoomPreset {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .zoom_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            Self::tr_lang(language, "Select zoom preset", "Select zoom preset").to_owned()
                                                        });
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-zoom"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.zoom_presets {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(preset_option.id),
                                                                        &preset_option.name,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                } else if step.action == MacroAction::PlaySoundPreset {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .audio_settings
                                                                .presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            Self::tr_lang(language, "Select sound", "Chọn âm thanh").to_owned()
                                                        });
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-sound"))
                                                        .width(160.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.audio_settings.presets {
                                                                if ui
                                                                    .selectable_label(selected_id == Some(preset_option.id), &preset_option.name)
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                } else if step.action == MacroAction::PlayVideoPreset {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .audio_settings
                                                                .video_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            Self::tr_lang(language, "Select video", "Chọn video").to_owned()
                                                        });
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-video"))
                                                        .width(134.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.audio_settings.video_presets {
                                                                if ui
                                                                    .selectable_label(selected_id == Some(preset_option.id), &preset_option.name)
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    Self::render_overlay_eye_button(
                                                        ui,
                                                        language,
                                                        step,
                                                        group.id,
                                                        preset.id,
                                                        0,
                                                        true,
                                                        &self.state.hud_presets,
                                                        &mut self.draw_geometry_step_preview_target,
                                                        &self.overlay_tx,
                                                        [18.0, 18.0],
                                                        12.0,
                                                    );
                                                } else if step.action == MacroAction::UnlockKeys {
                                                    let capture_target =
                                                        CaptureRequest::MacroPresetHoldStopInput(group.id, preset.id);
                                                    let active = capture_target_snapshot == Some(capture_target.clone());
                                                    Self::render_key_capture_chips(
                                                        ui,
                                                        language,
                                                        &mut step.key,
                                                        active,
                                                        || {
                                                            if active {
                                                                cancel_active_capture = true;
                                                            } else {
                                                                next_capture_target = Some(capture_target.clone());
                                                            }
                                                        },
                                                        || {
                                                            live_sync = true;
                                                        },
                                                    );
                                                } else if step.action == MacroAction::LockKeys {
                                                    let capture_target =
                                                        CaptureRequest::MacroPresetHoldStopInput(group.id, preset.id);
                                                    let active = capture_target_snapshot == Some(capture_target.clone());
                                                    Self::render_key_capture_chips(
                                                        ui,
                                                        language,
                                                        &mut step.key,
                                                        active,
                                                        || {
                                                            if active {
                                                                cancel_active_capture = true;
                                                            } else {
                                                                next_capture_target = Some(capture_target.clone());
                                                            }
                                                        },
                                                        || {
                                                            live_sync = true;
                                                        },
                                                    );
                                                    ui.add_space(4.0);
                                                    let unlock_resp = ui.checkbox(&mut step.unlock_on_exit, Self::tr_lang(language, "Unlock when macro ends", ""));
                                                    if unlock_resp.changed() {
                                                        live_sync = true;
                                                    }
                                                 } else if step.action == MacroAction::LoopStart {
                                                    let mut infinite = Self::loop_is_infinite(step);
                                                     if ui
                                                         .checkbox(
                                                             &mut infinite,
                                                             RichText::new(Self::tr_lang(
                                                                 language,
                                                                 "Infinite",
                                                                 "Infinite",
                                                             ))
                                                             .color(Color32::WHITE)
                                                             .strong(),
                                                         )
                                                         .changed()
                                                     {
                                                         step.key = if infinite {
                                                             "infinite".to_owned()
                                                         } else {
                                                             "1".to_owned()
                                                         };
                                                         live_sync = true;
                                                      }
                                                      if !infinite {
                                                          ui.vertical(|ui| {
                                                              let key_id = ui.id().with(("hold-stop-loop-count",));
                                                              let response = Self::render_variable_text_edit(
                                                                  ui,
                                                                  &mut step.key,
                                                                  key_id,
                                                                  96.0,
                                                                  180.0,
                                                                    21.0, 21.0,
                                                                  &Self::tr_lang(language, "Loop count", "Số lần lặp"),
                                                                  false,
                                                              );
                                                              Self::apply_vietnamese_input_if_changed(
                                                                  &response,
                                                                  self.state.vietnamese_input_enabled,
                                                                  self.state.vietnamese_input_mode,
                                                                  &mut step.key,
                                                              );
                                                              live_sync |= response.changed();
                                                              Self::render_variable_suggestions(ui, &response, &mut step.key, &timer_names, language);
                                                          });
                                                      }
                                                } else if step.action == MacroAction::StopIfKeyPressed {
                                                     ui.scope(|ui| {
                                                         ui.spacing_mut().item_spacing.x = 2.0;
                                                         ui.spacing_mut().interact_size.y = 18.0;
                                                         ui.spacing_mut().button_padding.y = 0.0;
                                                         ui.allocate_ui_with_layout(
                                                             vec2(ui.available_width(), 20.0),
                                                             egui::Layout::top_down(egui::Align::Min),
                                                             |ui| {
                                                             ui.horizontal(|ui| {
                                                                 let current_mode = step.get_break_loop_mode().to_string();
                                                                 let mode_label = match current_mode.as_str() {
                                                                     "VarCompare" => Self::tr_lang(language, "Var compare", "So sánh biến"),
                                                                     "StopKey" => Self::tr_lang(language, "Stop key", "Nút đã nhấn"),
                                                                     _ => Self::tr_lang(language, "Break Loop", "Ngắt lặp"),
                                                                 };
                                                                 egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-loop-break-mode"))
                                                                     .width(100.0)
                                                                     .selected_text(mode_label)
                                                                     .show_ui(ui, |ui| {
                                                                         if ui.selectable_label(current_mode == "Immediate", Self::tr_lang(language, "Break Loop", "Ngắt lặp")).clicked() {
                                                                             step.break_loop_mode = "Immediate".to_string();
                                                                             step.break_loop_by_variable = false;
                                                                             live_sync = true;
                                                                         }
                                                                         if ui.selectable_label(current_mode == "VarCompare", Self::tr_lang(language, "Var compare", "So sánh biến")).clicked() {
                                                                             step.break_loop_mode = "VarCompare".to_string();
                                                                             step.break_loop_by_variable = true;
                                                                             live_sync = true;
                                                                         }
                                                                         if ui.selectable_label(current_mode == "StopKey", Self::tr_lang(language, "Stop key", "Nút đã nhấn")).clicked() {
                                                                             step.break_loop_mode = "StopKey".to_string();
                                                                             step.break_loop_by_variable = false;
                                                                             live_sync = true;
                                                                         }
                                                                     });
                                                                 let mode = step.get_break_loop_mode();
                                                                 if mode == "VarCompare" {
                                                                     let var_name_id = ui.id().with("hold-stop-loop-break-var-name");
                                                                     let response = Self::render_variable_text_edit(
                                                                         ui,
                                                                         &mut step.if_variable_name,
                                                                         var_name_id,
                                                                         76.0,
                                                                         140.0,
                                                                         21.0, 21.0,
                                                                         Self::tr_lang(language, "variable", "biến"),
                                                                         false,
                                                                     );
                                                                     Self::apply_vietnamese_input_if_changed(
                                                                         &response,
                                                                         self.state.vietnamese_input_enabled,
                                                                         self.state.vietnamese_input_mode,
                                                                         &mut step.if_variable_name,
                                                                     );
                                                                     live_sync |= response.changed();
                                                                     Self::render_variable_suggestions(ui, &response, &mut step.if_variable_name, &timer_names, language);
                                                                     egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-loop-if-op"))
                                                                         .width(40.0)
                                                                         .selected_text(&step.if_operator)
                                                                         .show_ui(ui, |ui| {
                                                                             for op in &["==", ">", "<", ">=", "<=", "!="] {
                                                                                 if ui.selectable_label(step.if_operator == *op, *op).clicked() {
                                                                                     step.if_operator = op.to_string();
                                                                                     live_sync = true;
                                                                                 }
                                                                             }
                                                                         });
                                                                     let key_val_id = ui.id().with("hold-stop-loop-break-var-val");
                                                                     let response2 = Self::render_variable_text_edit(
                                                                         ui,
                                                                         &mut step.key,
                                                                         key_val_id,
                                                                         76.0,
                                                                         140.0,
                                                                         21.0, 21.0,
                                                                         Self::tr_lang(language, "value/expr", "giá trị"),
                                                                         false,
                                                                     );
                                                                     Self::apply_vietnamese_input_if_changed(
                                                                         &response2,
                                                                         self.state.vietnamese_input_enabled,
                                                                         self.state.vietnamese_input_mode,
                                                                         &mut step.key,
                                                                     );
                                                                     live_sync |= response2.changed();
                                                                     Self::render_variable_suggestions(ui, &response2, &mut step.key, &timer_names, language);
                                                                     let var_name = step.if_variable_name.trim();
                                                                     if !var_name.is_empty() {
                                                                         let current_val = crate::overlay::RUNTIME_VARIABLES.lock().get(var_name).copied();
                                                                         let val_str = current_val.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string());
                                                                         ui.add_space(2.0);
                                                                         ui.label(
                                                                             RichText::new(format!("({})", val_str))
                                                                                 .size(10.0)
                                                                                 .color(Color32::from_rgb(0, 191, 255))
                                                                         ).on_hover_text(Self::tr_lang(language, "Current runtime value", "Giá trị chạy hiện tại"));
                                                                     }
                                                                     let btn_plus_clicked = ui.scope(|ui| {
                                                                         ui.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
                                                                         ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                                                                          ui.add_sized([18.0, 18.0], egui::Button::new(egui::RichText::new("+").size(12.0).strong()))
                                                                             .on_hover_text(Self::tr_lang(language, "Add condition", "Thêm điều kiện"))
                                                                             .clicked()
                                                                     });
                                                                     if btn_plus_clicked.inner {
                                                                         step.extra_conditions.push(ExtraCondition::default());
                                                                         live_sync = true;
                                                                     }
                                                                 } else if mode == "StopKey" {
                                                                     let capture_target =
                                                                         CaptureRequest::MacroPresetHoldStopInput(group.id, preset.id);
                                                                     let active = capture_target_snapshot == Some(capture_target.clone());
                                                                     Self::render_key_capture_chips(
                                                                         ui,
                                                                         language,
                                                                         &mut step.key,
                                                                         active,
                                                                         || {
                                                                             if active {
                                                                                 cancel_active_capture = true;
                                                                             } else {
                                                                                 next_capture_target = Some(capture_target.clone());
                                                                             }
                                                                         },
                                                                         || {
                                                                             live_sync = true;
                                                                         },
                                                                     );
                                                                 }
                                                             });
                                                             if step.get_break_loop_mode() == "VarCompare" {
                                                                 Self::render_extra_conditions(
                                                                 ui,
                                                                 &mut step.extra_conditions,
                                                                 group.id,
                                                                 preset.id,
                                                                 0,
                                                                 &timer_names,
                                                                 &ocr_preset_options,
                                                                 &image_search_preset_options,
                                                                 &all_presets,
                                                                 &all_groups,
                                                                 language,
                                                                 &mut live_sync,
                                                                 &self.state.timer_presets,
                                                                 self.state.vietnamese_input_enabled,
                                                                 self.state.vietnamese_input_mode,
                                                                 &mut begin_mouse_move_absolute_capture_target,
                                                                 capture_target_snapshot.as_ref(),
                                                                 &mut next_capture_target,
                                                                 &mut cancel_active_capture,
                                                                  true,
                                                                 );
                                                             }
                                                         });
                                                     });
                                                } else if step.action == MacroAction::ShowHud {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .hud_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            if step.key.trim().is_empty() {
                                                                Self::tr_lang(
                                                                    language,
                                                                    "Select HUD",
                                                                    "Chọn HUD",
                                                                )
                                                                .to_owned()
                                                            } else {
                                                                format!("Cũ: {}", step.key)
                                                            }
                                                        });
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-toolbox-preset"))
                                                            .width(88.0)
                                                            .selected_text(selected_label)
                                                            .show_ui(ui, |ui| {
                                                                for toolbox_preset in &self.state.hud_presets {
                                                                    if ui
                                                                        .selectable_label(
                                                                            selected_id == Some(toolbox_preset.id),
                                                                            &toolbox_preset.name,
                                                                        )
                                                                        .clicked()
                                                                    {
                                                                        step.key = toolbox_preset.id.to_string();
                                                                        live_sync = true;
                                                                    }
                                                                }
                                                            });
                                                        Self::render_overlay_eye_button(
                                                            ui,
                                                            language,
                                                            step,
                                                            group.id,
                                                            preset.id,
                                                            0,
                                                            true,
                                                            &self.state.hud_presets,
                                                            &mut self.draw_geometry_step_preview_target,
                                                            &self.overlay_tx,
                                                            [18.0, 18.0],
                                                            12.0,
                                                        );
                                                        let text_id = ui.id().with(("hold-stop-showhud-text-override",));
                                                        let response = Self::render_variable_text_edit(
                                                            ui,
                                                            &mut step.text_override,
                                                            text_id,
                                                            110.0,
                                                            200.0,
                                                              21.0, 21.0,
                                                            &Self::tr_lang(language, "Text override", "Ghi đè văn bản"),
                                                            false,
                                                        );
                                                        Self::apply_vietnamese_input_if_changed(
                                                            &response,
                                                            self.state.vietnamese_input_enabled,
                                                            self.state.vietnamese_input_mode,
                                                            &mut step.text_override,
                                                        );
                                                        live_sync |= response.changed();
                                                        Self::render_variable_suggestions(
                                                            ui,
                                                            &response,
                                                            &mut step.text_override,
                                                            &timer_names,
                                                            language,
                                                        );
                                                    });
                                                } else if step.action == MacroAction::TypeText {
                                                    ui.vertical(|ui| {
                                                        let response = Self::render_interpolated_text_edit(ui, &mut step.key, ui.id().with("hold-stop-type-text-key"),
                                                            220.0,
                                                            360.0,
                                                            17.0,
                                                            44.0,
                                                            Self::tr_lang(language, "Text to type", "Văn bảnh cần gõ"),
                                                            true,
                                                        );
                                                        Self::apply_vietnamese_input_if_changed(
                                                            &response,
                                                            self.state.vietnamese_input_enabled,
                                                            self.state.vietnamese_input_mode,
                                                            &mut step.key,
                                                        );
                                                        live_sync |= response.changed();
                                                        Self::render_variable_suggestions_braced(ui, &response, &mut step.key, &timer_names, language);
                                                    });
                                                } else if step.action == MacroAction::DisableCrosshair {
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.horizontal(|ui| {
                                                            let response = ui.checkbox(&mut step.lock_mouse_left, Self::tr_lang(language, "All", ""));
                                                            live_sync |= response.changed();
                                                            if !step.lock_mouse_left {
                                                                let selected_label = if step.key.trim().is_empty() {
                                                                    Self::tr_lang(language, "Select profile", "Chọn profile").to_owned()
                                                                } else {
                                                                    step.key.clone()
                                                                };
                                                                egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-disable-crosshair"))
                                                                    .width(110.0)
                                                                    .selected_text(selected_label)
                                                                    .show_ui(ui, |ui| {
                                                                        for profile in &self.state.profiles {
                                                                            if ui
                                                                                .selectable_label(step.key == profile.name, &profile.name)
                                                                                .clicked()
                                                                            {
                                                                                step.key = profile.name.clone();
                                                                                live_sync = true;
                                                                            }
                                                                        }
                                                                    });
                                                            }
                                                        });
                                                    });
                                                } else if step.action == MacroAction::DisablePin {
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.horizontal(|ui| {
                                                            let response = ui.checkbox(&mut step.lock_mouse_left, Self::tr_lang(language, "All", ""));
                                                            live_sync |= response.changed();
                                                            if !step.lock_mouse_left {
                                                                let selected_id = step.key.trim().parse::<u32>().ok();
                                                                let selected_label = selected_id
                                                                    .and_then(|id| {
                                                                        self.state
                                                                            .pin_presets
                                                                            .iter()
                                                                            .find(|p| p.id == id)
                                                                            .map(|p| p.name.clone())
                                                                    })
                                                                    .unwrap_or_else(|| {
                                                                        Self::tr_lang(language, "Select pin", "Chọn preset ghim").to_owned()
                                                                    });
                                                                egui::ComboBox::from_id_salt((group.id, preset.id, "hold-stop-disable-pin"))
                                                                    .width(110.0)
                                                                    .selected_text(selected_label)
                                                                    .show_ui(ui, |ui| {
                                                                        for preset_option in &self.state.pin_presets {
                                                                            if ui
                                                                                .selectable_label(selected_id == Some(preset_option.id), &preset_option.name)
                                                                                .clicked()
                                                                            {
                                                                                step.key = preset_option.id.to_string();
                                                                                live_sync = true;
                                                                            }
                                                                        }
                                                                    });
                                                            }
                                                        });
                                                    });
                                                } else if matches!(step.action, MacroAction::Else | MacroAction::IfEnd) {
                                                     ui.add_sized(
                                                         [110.0, 21.0],
                                                         egui::Label::new(""),
                                                     );
                                                } else if matches!(step.action, MacroAction::DisableZoom | MacroAction::HideHud) {
                                                     ui.add_sized(
                                                         [110.0, 21.0],
                                                         egui::Label::new(Self::tr_lang(language, "No input", "No input")),
                                                     );
                                                } else if step.action == MacroAction::LockMouse {
                                                    Self::render_mouse_move_direction_controls(
                                                        ui,
                                                        step,
                                                        language,
                                                        &mut live_sync,
                                                        true,
                                                        "lockmouse-compact-warning-tip",
                                                    );
                                                } else if step.action == MacroAction::UnlockMouse {
                                                    Self::render_mouse_move_direction_controls(
                                                        ui,
                                                        step,
                                                        language,
                                                        &mut live_sync,
                                                        false,
                                                        "unlockmouse-compact-warning-tip",
                                                    );
                                                } else if step.action == MacroAction::IfStart {
                                                     ui.scope(|ui| {
                                                         ui.spacing_mut().item_spacing.x = 2.0;
                                                         ui.spacing_mut().interact_size.y = 18.0;
                                                         ui.spacing_mut().button_padding.y = 0.0;
                                                         ui.allocate_ui_with_layout(
                                                             vec2(ui.available_width(), 20.0),
                                                             egui::Layout::top_down(egui::Align::Min),
                                                             |ui| {
                                                             ui.horizontal(|ui| {
                                                                   ui.add_sized(
                                                                       [56.0, 20.0],
                                                                       egui::Label::new(Self::tr_lang(language, "IF", "NẾU")),
                                                                   );
                                                                   let cond_text = match step.if_condition_type {
                                                                        IfConditionType::Variable => Self::tr_lang(language, "Variable", "Biến"),
                                                                        IfConditionType::PixelColor => Self::tr_lang(language, "Pixel Color", "Màu điểm"),
                                                                        IfConditionType::VisionMatch => Self::tr_lang(language, "Vision Match", "Hình ảnh"),
                                                                        IfConditionType::KeyHeld => Self::tr_lang(language, "Input Held", "Giữ phím/chuột"),
                                                                        IfConditionType::MouseHeld => Self::tr_lang(language, "Input Held", "Giữ phím/chuột"),
                                                                        IfConditionType::MousePosition => Self::tr_lang(language, "Mouse Position", "Tọa độ chuột"),
                                                                        IfConditionType::PresetRunning => Self::tr_lang(language, "Preset Running", "Preset đang chạy"),
                                                                        IfConditionType::OcrMatch => Self::tr_lang(language, "OCR Match", "Từ tìm (OCR)"),
                                                                        _ => "-",
                                                                    };
                                                                   egui::ComboBox::from_id_salt((group.id, preset.id, 0, "hold-stop-if-condition-type"))
                                                                       .width(100.0)
                                                                       .selected_text(cond_text)
                                                                       .show_ui(ui, |ui| {
                                                                           let options = [
                                                                               (IfConditionType::Variable, Self::tr_lang(language, "Variable", "Biến")),
                                                                               (IfConditionType::PixelColor, Self::tr_lang(language, "Pixel Color", "Màu điểm")),
                                                                               (IfConditionType::VisionMatch, Self::tr_lang(language, "Vision Match", "Hình ảnh")),
                                                                               (IfConditionType::KeyHeld, Self::tr_lang(language, "Input Held", "Giữ phím/chuột")),
                                                                               (IfConditionType::MousePosition, Self::tr_lang(language, "Mouse Position", "Tọa độ chuột")),
                                                                               (IfConditionType::PresetRunning, Self::tr_lang(language, "Preset Running", "Preset đang chạy")),
                                                                           ];
                                                                           for (opt_type, opt_label) in options {
                                                                               if ui.selectable_label(step.if_condition_type == opt_type, opt_label).clicked() {
                                                                                   step.if_condition_type = opt_type;
                                                                                   live_sync = true;
                                                                               }
                                                                           }
                                                                       });
                                                                   if step.if_condition_type == IfConditionType::Variable {
                                                                    let var_name_id = ui.id().with("hold-stop-if-var-name");
                                                                   let response = Self::render_variable_text_edit(
                                                                       ui,
                                                                       &mut step.if_variable_name,
                                                                       var_name_id,
                                                                       76.0,
                                                                       140.0,
                                                                       21.0, 21.0,
                                                                       Self::tr_lang(language, "value/expr", "biến/expr"),
                                                                       false,
                                                                   );
                                                                   Self::apply_vietnamese_input_if_changed(
                                                                       &response,
                                                                       self.state.vietnamese_input_enabled,
                                                                       self.state.vietnamese_input_mode,
                                                                       &mut step.if_variable_name,
                                                                   );
                                                                   live_sync |= response.changed();
                                                                   Self::render_variable_suggestions(ui, &response, &mut step.if_variable_name, &timer_names, language);
                                                                   egui::ComboBox::from_id_salt((group.id, preset.id, 0, "hold-stop-if-op"))
                                                                       .width(55.0)
                                                                       .selected_text(&step.if_operator)
                                                                       .show_ui(ui, |ui| {
                                                                           for op in &["==", ">", "<", ">=", "<=", "!="] {
                                                                               if ui.selectable_label(step.if_operator == *op, *op).clicked() {
                                                                                   step.if_operator = op.to_string();
                                                                                   live_sync = true;
                                                                               }
                                                                           }
                                                                       });
                                                                   let var_val_id = ui.id().with("hold-stop-if-var-val");
                                                                   let response2 = Self::render_variable_text_edit(
                                                                       ui,
                                                                       &mut step.key,
                                                                       var_val_id,
                                                                       76.0,
                                                                       180.0,
                                                                       21.0, 21.0,
                                                                       Self::tr_lang(language, "value/expr", "giá trị/expr"),
                                                                       false,
                                                                   );
                                                                   Self::apply_vietnamese_input_if_changed(
                                                                       &response2,
                                                                       self.state.vietnamese_input_enabled,
                                                                       self.state.vietnamese_input_mode,
                                                                       &mut step.key,
                                                                   );
                                                                   live_sync |= response2.changed();
                                                                   Self::render_variable_suggestions(ui, &response2, &mut step.key, &timer_names, language);
                                                                      let left_expr = step.if_variable_name.trim();
                                                                      if !left_expr.is_empty() {
                                                                          let left_val = crate::overlay::evaluate_interpolated_math_expression(left_expr);
                                                                          ui.add_space(2.0);
                                                                          ui.label(
                                                                              RichText::new(format!("({})", left_val))
                                                                                  .size(10.0)
                                                                                  .color(Color32::from_rgb(0, 191, 255))
                                                                          ).on_hover_text(Self::tr_lang(language, "Evaluated left expression", "Gia tri bieu thuc ben trai"));
                                                                      }
                                                                   } else if step.if_condition_type == IfConditionType::PixelColor {
                                                                       ui.label("X:");
                                                                       let resp_x = ui.add(egui::DragValue::new(&mut step.x));
                                                                       live_sync |= resp_x.changed();
                                                                       ui.label("Y:");
                                                                       let resp_y = ui.add(egui::DragValue::new(&mut step.y));
                                                                       live_sync |= resp_y.changed();
                                                                       let resp_col = ui.add_sized(
                                                                           [64.0, 21.0], TextEdit::singleline(&mut step.if_target_color)
                                                                               .hint_text(RichText::new("#RRGGBB").color(hint_color).weak()),
                                                                       );
                                                                       live_sync |= resp_col.changed();
                                                                       // Swatch màu trực quan
                                                                       let swatch_color = Self::parse_rgb_color(&step.if_target_color).unwrap_or(egui::Color32::TRANSPARENT);
                                                                       let (rect, _response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                                                       ui.painter().rect_filled(rect, 3.0, swatch_color);
                                                                       ui.label(Self::tr_lang(language, "Tol:", "Sai số:"));
                                                                       let resp_tol = ui.add(egui::DragValue::new(&mut step.if_color_tolerance).range(0..=255));
                                                                       live_sync |= resp_tol.changed();
                                                                       // Nút pick màu + vị trí từ màn hình
                                                                       let pick_btn = ui.add_sized(
                                                                           [18.0, 18.0],
                                                                           egui::Button::new(Self::material_icon_text(0xe3b4, 14.0)),
                                                                       ).on_hover_text(Self::tr_lang(language, "Click on screen to pick color & position", "Bấm vào màn hình để lấy màu và tọa độ"));
                                                                       if pick_btn.clicked() {
                                                                           begin_mouse_move_absolute_capture_target = Some(MouseMoveAbsoluteCaptureTarget {
                                                                               group_id: Some(group.id),
                                                                               preset_id: preset.id,
                                                                               step_index: 0,
                                                                               capture_kind: MouseCaptureKind::IfStartPixelColor,
                                                                               extra_cond_index: None,
                                                                               is_hold_stop: true,
                                                                           });
                                                                       }
                                                                   } else if step.if_condition_type == IfConditionType::VisionMatch {
                                                                       let selected_id = step.if_vision_preset_id;
                                                                       let selected_label = selected_id
                                                                           .and_then(|id| {
                                                                               self.state.vision_presets.iter().find(|p| p.id == id).map(|p| p.name.clone())
                                                                           })
                                                                           .unwrap_or_else(|| Self::tr_lang(language, "Select preset", "Chọn preset").to_owned());
                                                                       egui::ComboBox::from_id_salt((group.id, preset.id, 0, "hold-stop-if-vision-preset"))
    .width(146.0)

    .selected_text(selected_label)

    .show_ui(ui, |ui| {

                    let (image_presets, color_presets): (Vec<_>, Vec<_>) = self.state.vision_presets.iter().filter(|p| !p.is_pixel_counter).fold(
                        (Vec::new(), Vec::new()),
                        |(mut img, mut col), p| {
                            if p.use_color_matching {
                                col.push(p);
                            } else {
                                img.push(p);
                            }
                            (img, col)
                        }
                    );
                    if !image_presets.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0, 191, 255), "Image Detect");
                        ui.separator();
                        for p in &image_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.if_vision_preset_id = Some(p.id);
                                live_sync = true;
                            }
                        }
                    }
                    if !color_presets.is_empty() {
                        if !image_presets.is_empty() {
                            ui.add_space(4.0);
                        }
                        ui.colored_label(egui::Color32::from_rgb(0, 250, 154), "Color Detect");
                        ui.separator();
                        for p in &color_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.if_vision_preset_id = Some(p.id);
                                live_sync = true;
                            }
                        }
                    }
    });

                                                                   } else if step.if_condition_type == IfConditionType::KeyHeld {
                                                                        let capture_target = CaptureRequest::MacroStepInput {
                                                                            group_id: group.id,
                                                                            preset_id: preset.id,
                                                                            step_index: 0,
                                                                            extra_cond_index: None,
                                                                        };
                                                                        let active = capture_target_snapshot.as_ref() == Some(&capture_target);
                                                                        Self::render_multi_key_capture_chips(
                                                                            ui,
                                                                            language,
                                                                            &mut step.key,
                                                                            active,
                                                                            || {
                                                                                if active {
                                                                                    cancel_active_capture = true;
                                                                                } else {
                                                                                    next_capture_target = Some(capture_target.clone());
                                                                                }
                                                                            },
                                                                            || {
                                                                                live_sync = true;
                                                                            }
                                                                        );
                                                                    } else if step.if_condition_type == IfConditionType::MousePosition {
                                                                       egui::ComboBox::from_id_salt((group.id, preset.id, 0, "hold-stop-if-mouse-axis"))
                                                                           .width(50.0)
                                                                           .selected_text(&step.if_mouse_axis)
                                                                           .show_ui(ui, |ui| {
                                                                               for axis in &["X", "Y"] {
                                                                                   if ui.selectable_label(step.if_mouse_axis == *axis, *axis).clicked() {
                                                                                       step.if_mouse_axis = axis.to_string();
                                                                                       live_sync = true;
                                                                                   }
                                                                               }
                                                                           });
                                                                       egui::ComboBox::from_id_salt((group.id, preset.id, 0, "hold-stop-if-mouse-pos-op"))
                                                                           .width(40.0)
                                                                           .selected_text(&step.if_operator)
                                                                           .show_ui(ui, |ui| {
                                                                               for op in &["==", ">", "<", ">=", "<=", "!="] {
                                                                                   if ui.selectable_label(step.if_operator == *op, *op).clicked() {
                                                                                       step.if_operator = op.to_string();
                                                                                       live_sync = true;
                                                                                   }
                                                                               }
                                                                           });
                                                                       let mut variable_layouter = |ui: &egui::Ui, string: &dyn TextBuffer, wrap_width: f32| {
                                                                           let job = Self::interpolation_highlight_job(
                                                                               ui,
                                                                               string.as_str(),
                                                                               wrap_width,
                                                                               egui::TextStyle::Body,
                                                                           );
                                                                           ui.fonts_mut(|fonts| fonts.layout_job(job))
                                                                       };
                                                                       let response2 = ui.add_sized(
                                                                            [76.0, 21.0], TextEdit::singleline(&mut step.key)
                                                                                .layouter(&mut variable_layouter)
                                                                                .hint_text(RichText::new(Self::tr_lang(language, "value/expr", "giá trị/expr")).color(hint_color).weak()),
                                                                        );
                                                                       Self::apply_vietnamese_input_if_changed(
                                                                           &response2,
                                                                           self.state.vietnamese_input_enabled,
                                                                           self.state.vietnamese_input_mode,
                                                                           &mut step.key,
                                                                       );
                                                                       live_sync |= response2.changed();
                                                                       Self::render_variable_suggestions(ui, &response2, &mut step.key, &timer_names, language);
                                                                       // Nút pick tọa độ chuột từ màn hình
                                                                       let pick_btn = ui.add_sized(
                                                                           [18.0, 18.0],
                                                                           egui::Button::new(Self::material_icon_text(0xe55c, 14.0)),
                                                                       ).on_hover_text(Self::tr_lang(language, "Click on screen to capture coordinate", "Bấm vào màn hình để lấy tọa độ"));
                                                                       if pick_btn.clicked() {
                                                                           begin_mouse_move_absolute_capture_target = Some(MouseMoveAbsoluteCaptureTarget {
                                                                               group_id: Some(group.id),
                                                                               preset_id: preset.id,
                                                                               step_index: 0,
                                                                               capture_kind: MouseCaptureKind::IfStartMousePos,
                                                                               extra_cond_index: None,
                                                                               is_hold_stop: true,
                                                                           });
                                                                       }
                                                                   } else if step.if_condition_type == IfConditionType::PresetRunning {
                                                                       // Bước 1: chọn group
                                                                       let sel_group_id = step.if_running_preset_group_id;
                                                                       let sel_group_label = sel_group_id
                                                                           .and_then(|gid| all_groups.iter().find(|(id, _, _)| *id == gid).map(|(_, name, _)| name.clone()))
                                                                           .unwrap_or_else(|| Self::tr_lang(language, "Group", "Nhóm").to_owned());
                                                                       egui::ComboBox::from_id_salt((group.id, preset.id, 0, "hold-stop-if-running-group"))
                                                                           .width(100.0)
                                                                           .selected_text(sel_group_label)
                                                                           .show_ui(ui, |ui| {
                                                                               for (gid, gname, _) in &all_groups {
                                                                                   if ui.selectable_label(sel_group_id == Some(*gid), gname).clicked() {
                                                                                       step.if_running_preset_group_id = Some(*gid);
                                                                                       step.if_running_preset_id = None;
                                                                                       live_sync = true;
                                                                                   }
                                                                               }
                                                                           });
                                                                       // Bước 2: chọn preset trong group đó
                                                                       let group_presets: &[(u32, String)] = if let Some(gid) = sel_group_id {
                                                                           all_groups.iter()
                                                                               .find(|(id, _, _)| *id == gid)
                                                                               .map(|(_, _, presets)| presets.as_slice())
                                                                               .unwrap_or(&[])
                                                                       } else {
                                                                           &[]
                                                                       };
                                                                       let selected_id = step.if_running_preset_id;
                                                                       let selected_label = selected_id
                                                                           .and_then(|id| group_presets.iter().find(|(pid, _)| *pid == id).map(|(_, name)| name.clone()))
                                                                           .unwrap_or_else(|| Self::tr_lang(language, "Preset", "Preset").to_owned());
                                                                       egui::ComboBox::from_id_salt((group.id, preset.id, 0, "hold-stop-if-running-preset"))
                                                                           .width(120.0)
                                                                           .selected_text(selected_label)
                                                                           .show_ui(ui, |ui| {
                                                                               for (pid, pname) in group_presets {
                                                                                   if ui.selectable_label(selected_id == Some(*pid), pname).clicked() {
                                                                                       step.if_running_preset_id = Some(*pid);
                                                                                       live_sync = true;
                                                                                   }
                                                                               }
                                                                           });
                                                                   } else if step.if_condition_type == IfConditionType::OcrMatch {
                                                                        let selected_id = step.if_ocr_preset_id;
                                                                        let selected_label = selected_id
                                                                            .and_then(|id| {
                                                                                ocr_preset_options
                                                                                    .iter()
                                                                                    .find(|(preset_id, _)| *preset_id == id)
                                                                                    .map(|(_, label)| label.clone())
                                                                            })
                                                                            .unwrap_or_else(|| Self::tr_lang(language, "Select OCR", "Chọn OCR").to_owned());
                                                                        egui::ComboBox::from_id_salt((group.id, preset.id, 0, "hold-stop-if-ocr-preset"))
                                                                            .width(146.0)
                                                                            .selected_text(selected_label)
                                                                            .show_ui(ui, |ui| {
                                                                                for (preset_option_id, preset_option_label) in &ocr_preset_options {
                                                                                    if ui
                                                                                        .selectable_label(selected_id == Some(*preset_option_id), preset_option_label)
                                                                                        .clicked()
                                                                                    {
                                                                                        step.if_ocr_preset_id = Some(*preset_option_id);
                                                                                        live_sync = true;
                                                                                    }
                                                                                }
                                                                            });
                                                                        let var_target_id = ui.id().with("hold-stop-ocr-target-text-if");
                                                                        live_sync |= Self::sanitize_legacy_ocr_target_text(&mut step.ocr_target_text);
                                                                        live_sync |= Self::sanitize_legacy_ocr_target_text(&mut step.ocr_target_text);
                                                                        let response_target = Self::render_variable_text_edit(
                                                                            ui,
                                                                            &mut step.ocr_target_text,
                                                                            var_target_id,
                                                                            76.0,
                                                                            180.0,
                                                                            21.0, 21.0,
                                                                            Self::tr_lang(language, "Target text", "Van ban can tim"),
                                                                            false,
                                                                        );
                                                                        Self::apply_vietnamese_input_if_changed(
                                                                            &response_target,
                                                                            self.state.vietnamese_input_enabled,
                                                                            self.state.vietnamese_input_mode,
                                                                            &mut step.ocr_target_text,
                                                                        );
                                                                        live_sync |= response_target.changed();
                                                                        Self::render_variable_suggestions(ui, &response_target, &mut step.ocr_target_text, &timer_names, language);
                                                                    }
                                                                     let btn_plus_clicked = ui.scope(|ui| {
                                                                         ui.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
                                                                         ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                                                                          ui.add_sized([18.0, 18.0], egui::Button::new(egui::RichText::new("+").size(12.0).strong()))
                                                                             .on_hover_text(Self::tr_lang(language, "Add condition", "Thêm điều kiện"))
                                                                             .clicked()
                                                                     });
                                                                     if btn_plus_clicked.inner {
                                                                         step.extra_conditions.push(ExtraCondition::default());
                                                                         live_sync = true;
                                                                     }
                                                                });
                                                              Self::render_extra_conditions(
                                                              ui,
                                                              &mut step.extra_conditions,
                                                              group.id,
                                                              preset.id,
                                                              0,
                                                              &timer_names,
                                                              &ocr_preset_options,
                                                              &image_search_preset_options,
                                                              &all_presets,
                                                              &all_groups,
                                                              language,
                                                              &mut live_sync,
                                                              &self.state.timer_presets,
                                                              self.state.vietnamese_input_enabled,
                                                              self.state.vietnamese_input_mode,
                                                              &mut begin_mouse_move_absolute_capture_target,
                                                              capture_target_snapshot.as_ref(),
                                                              &mut next_capture_target,
                                                              &mut cancel_active_capture,
                                                                  true,
                                                              );
                                                          });
                                                      });} else if step.action == MacroAction::SetVariable {
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.vertical(|ui| {
                                                            ui.horizontal(|ui| {
                                                                  let var_name_id = ui.id().with("hold-stop-set-var-name");
                                                                  let response = Self::render_variable_text_edit(
                                                                      ui,
                                                                      &mut step.if_variable_name,
                                                                      var_name_id,
                                                                      76.0,
                                                                      140.0,
                                                                      21.0, 21.0,
                                                                      Self::tr_lang(language, "variable", "biến"),
                                                                      false,
                                                                  );
                                                                  Self::apply_vietnamese_input_if_changed(
                                                                      &response,
                                                                      self.state.vietnamese_input_enabled,
                                                                      self.state.vietnamese_input_mode,
                                                                      &mut step.if_variable_name,
                                                                  );
                                                                  live_sync |= response.changed();
                                                                  ui.label(" = ");
                                                                  let var_val_id = ui.id().with("hold-stop-set-var-val");
                                                                  let response2 = Self::render_plain_text_edit(
                                                                      ui,
                                                                      &mut step.key,
                                                                      var_val_id,
                                                                      76.0,
                                                                      180.0,   21.0, 21.0,
                                                                      Self::tr_lang(language, "value/expr", "giá trị"),
                                                                      false,
                                                                  );
                                                                  Self::apply_vietnamese_input_if_changed(
                                                                      &response2,
                                                                      self.state.vietnamese_input_enabled,
                                                                      self.state.vietnamese_input_mode,
                                                                      &mut step.key,
                                                                  );
                                                                  live_sync |= response2.changed();
                                                                Self::render_variable_suggestions_raw(ui, &response, &mut step.if_variable_name, &timer_names, language);
                                                                Self::render_variable_suggestions(ui, &response2, &mut step.key, &timer_names, language);
                                                                let var_name = step.if_variable_name.trim();
                                                                if !var_name.is_empty() {
                                                                    let current_val = crate::overlay::RUNTIME_VARIABLES.lock().get(var_name).copied();
                                                                    let val_str = current_val.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string());
                                                                    ui.add_space(2.0);
                                                                    ui.label(
                                                                        RichText::new(format!("({})", val_str))
                                                                            .size(10.0)
                                                                            .color(Color32::from_rgb(0, 191, 255))
                                                                    ).on_hover_text(Self::tr_lang(language, "Current runtime value", "Giá trị chạy hiện tại"));
                                                                }
                                                            });
                                                        });
                                                    });
                                                } else {
                                                    let id = ui.id().with("hold-stop-default-key");
                                                    let response = Self::render_expandable_text_edit(
                                                        ui,
                                                        &mut step.key,
                                                        id,
                                                        160.0,
                                                        240.0,
                                                          21.0, 21.0,
                                                        "...",
                                                        false,
                                                     );
                                                     live_sync |= response.changed();
                                                }
                                            } else if matches!(
                                                step.action,
                                                MacroAction::DrawGeometry
                                                    | MacroAction::ShowGeometryPreset
                                                    | MacroAction::HideGeometryPreset
                                                    | MacroAction::StartAudioSensePreset
                                                    | MacroAction::StopAudioSense
                                            ) {
                                                if matches!(
                                                    step.action,
                                                    MacroAction::DrawGeometry
                                                        | MacroAction::ShowGeometryPreset
                                                        | MacroAction::HideGeometryPreset
                                                ) {
                                                    Self::render_geometry_macro_step_editor(
                                                        ui,
                                                        language,
                                                        (group.id, preset.id, "hold-stop-geometry-step"),
                                                        group.id,
                                                        preset.id,
                                                        0,
                                                        true,
                                                        &geometry_preset_options,
                                                        &mut geometry_manual_color,
                                                        &mut geometry_manual_color_hex,
                                                        &mut request_geometry_screen_color_pick,
                                                        &mut begin_mouse_move_absolute_capture_target,
                                                        &mut pending_geometry_macro_step_color_pick,
                                                        &mut self.draw_geometry_step_preview_target,
                                                        &self.overlay_tx,
                                                        step,
                                                        &mut live_sync,
                                                        self.state.vietnamese_input_enabled,
                                                        self.state.vietnamese_input_mode,
                                                        &timer_names,
                                                        &mut self.state.geometry_presets,
                                                        &mut geom_presets_changed,
                                                    );
                                                } else {
                                                    Self::render_audio_sense_step_editor(
                                                        ui,
                                                        language,
                                                        (group.id, preset.id, "hold-stop-audiosense-step"),
                                                        group.id,
                                                        preset.id,
                                                        0,
                                                        true,
                                                        &audio_sense_preset_options,
                                                        &mut self.state.audio_sense_presets,
                                                        &self.audio_sense_devices,
                                                        &timer_names,
                                                        step,
                                                        &mut live_sync,
                                                        &mut audio_sense_presets_changed,
                                                    );
                                                }
                                            } else if Self::macro_action_uses_position(step.action) {
                                                ui.add_space(2.0);
                                            } else {
                                                ui.add_sized([70.0, 20.0], egui::Label::new(""));
                                            }
                                            if Self::macro_action_uses_position(step.action) {
                                                if matches!(
                                                    step.action,
                                                    MacroAction::MouseMoveAbsolute
                                                        | MacroAction::MouseMoveRelative
                                                ) {
                                                    Self::ensure_coordinate_exprs(step);
                                                    let x_hint = if step.action
                                                        == MacroAction::MouseMoveRelative
                                                    {
                                                        "X delta or {var}"
                                                    } else {
                                                        "X value or {var}"
                                                    };
                                                    let y_hint = if step.action
                                                        == MacroAction::MouseMoveRelative
                                                    {
                                                        "Y delta or {var}"
                                                    } else {
                                                        "Y value or {var}"
                                                    };
                                                    let x_id = ui.id().with((group.id, preset.id, "hold-stop-move-abs-x-22"));
                                                    let y_id = ui.id().with((group.id, preset.id, "hold-stop-move-abs-y-22"));
                                                    let (x_response, y_response) =
                                                        Self::render_coordinate_compact_pair(
                                                            ui,
                                                            &mut step.x_expr,
                                                            x_id,
                                                            x_hint,
                                                            &mut step.y_expr,
                                                            y_id,
                                                            y_hint,
                                                            58.0,
                                                            20.0,
                                                        );
                                                    Self::apply_vietnamese_input_if_changed(
                                                        &x_response,
                                                        self.state.vietnamese_input_enabled,
                                                        self.state.vietnamese_input_mode,
                                                        &mut step.x_expr,
                                                    );
                                                    if x_response.changed() {
                                                        Self::sync_coordinate_expr_to_value(&step.x_expr, &mut step.x);
                                                        live_sync = true;
                                                    }
                                                    Self::render_variable_suggestions(
                                                        ui,
                                                        &x_response,
                                                        &mut step.x_expr,
                                                        &timer_names,
                                                        language,
                                                    );
                                                    Self::apply_vietnamese_input_if_changed(
                                                        &y_response,
                                                        self.state.vietnamese_input_enabled,
                                                        self.state.vietnamese_input_mode,
                                                        &mut step.y_expr,
                                                    );
                                                    if y_response.changed() {
                                                        Self::sync_coordinate_expr_to_value(&step.y_expr, &mut step.y);
                                                        live_sync = true;
                                                    }
                                                    Self::render_variable_suggestions(
                                                        ui,
                                                        &y_response,
                                                        &mut step.y_expr,
                                                        &timer_names,
                                                        language,
                                                    );
                                                }
                                            } else if matches!(
                                                step.action,
                                                MacroAction::ShowHud
                                                    | MacroAction::EnableCrosshairProfile
                                                    | MacroAction::EnablePinPreset
                                                    | MacroAction::PlayVideoPreset
                                            ) {
                                                let duration_id = ui.id().with((group.id, preset.id, "duration-expr-hold-stop"));
                                                ui.scope(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 2.0;
                                                    ui.spacing_mut().interact_size.y = 21.0;
                                                    ui.spacing_mut().button_padding.y = 0.0;
                                                    let is_video = step.action == MacroAction::PlayVideoPreset;
                                                    let mut is_permanent = !is_video && (step.duration_expr.trim() == "0" || step.duration_expr.trim().is_empty());
                                                    if !is_video {
                                                        let cb_response = ui.checkbox(&mut is_permanent, Self::tr_lang(language, "Permanent", "Vĩnh viễn"));
                                                        if cb_response.changed() {
                                                            if is_permanent {
                                                                step.duration_expr = "0".to_string();
                                                            } else {
                                                                step.duration_expr = "1500".to_string();
                                                            }
                                                            live_sync = true;
                                                        }
                                                    }
                                                    if is_video || !is_permanent {
                                                        let response = Self::render_variable_text_edit(ui, &mut step.duration_expr, duration_id, 74.0, 150.0,  21.0, 21.0, "0", false);
                                                        ui.weak("ms");
                                                        Self::apply_vietnamese_input_if_changed(
                                                            &response,
                                                            self.state.vietnamese_input_enabled,
                                                            self.state.vietnamese_input_mode,
                                                            &mut step.duration_expr,
                                                        );
                                                        live_sync |= response.changed();
                                                        Self::render_variable_suggestions(
                                                            ui,
                                                            &response,
                                                            &mut step.duration_expr,
                                                            &timer_names,
                                                            language,
                                                        );
                                                        let tooltip_text = Self::tr_lang(
                                                            language,
                                                            "Display duration (0 = show until macro/overlay ends, supports variables/math)",
                                                            "Thời gian hiển thị (0 = hiện đến khi dừng macro/overlay, hỗ trợ biến/phép tính)",
                                                        );
                                                        if response.hovered() {
                                                            egui::show_tooltip_text(ui.ctx(), ui.layer_id(), response.id, tooltip_text);
                                                        }
                                                    }
                                                });
                                            } else {
                                                ui.add_sized([24.0, 20.0], egui::Label::new(""));
                                                ui.add_sized([24.0, 20.0], egui::Label::new(""));
                                            }
                                            if action_supports_capture
                                                && !matches!(
                                                    step.action,
                                                    MacroAction::LockKeys
                                                        | MacroAction::UnlockKeys
                                                        | MacroAction::StopIfKeyPressed
                                                )
                                            {
                                                let hold_stop_capture_target =
                                                    CaptureRequest::MacroPresetHoldStopInput(group.id, preset.id);
                                                let hold_stop_capture_active =
                                                    capture_target_snapshot.as_ref() == Some(&hold_stop_capture_target);
                                                let hold_stop_capture_width =
                                                    if hold_stop_capture_active { 92.0 } else { 28.0 };
                                                let hold_stop_capture_button = if hold_stop_capture_active {
                                                    Button::new(Self::capture_button_text(language, true))
                                                        .min_size(vec2(hold_stop_capture_width, 18.0))
                                                        .fill(Color32::from_rgb(88, 84, 44))
                                                } else {
                                                    Button::new(Self::material_icon_text(0xe312, 18.0))
                                                        .min_size(vec2(hold_stop_capture_width, 18.0))
                                                };
                                                if ui
                                                    .add_sized([hold_stop_capture_width, 18.0], hold_stop_capture_button)
                                                    .on_hover_text(Self::tr_lang(
                                                        language,
                                                        "Capture hold stop key",
                                                        "",
                                                    ))
                                                    .clicked()
                                                {
                                                    if hold_stop_capture_active {
                                                        cancel_active_capture = true;
                                                    } else {
                                                        next_capture_target = Some(hold_stop_capture_target);
                                                    }
                                                }
                                                // Dropdown right here for hold stop
                                                let hs_menu_response = ui.menu_button(Self::material_icon_text(0xe5d2, 14.0), |ui| {
                                                    ui.set_max_width(200.0);
                                                     ui.menu_button(Self::tr_lang(language, "Letters (A-Z)", "Chữ cái (A-Z)"), |ui| {
                                                         ui.set_max_width(120.0);
                                                         egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                             for ch in b'A'..=b'Z' {
                                                                 let key_str = (ch as char).to_string();
                                                                 if ui.button(&key_str).clicked() {
                                                                     step.key = key_str;
                                                                     live_sync = true;
                                                                     ui.close_menu();
                                                                 }
                                                             }
                                                         });
                                                     });
                                                     ui.menu_button(Self::tr_lang(language, "Numbers & Symbols", "Số & Kí tự"), |ui| {
                                                         ui.set_max_width(140.0);
                                                         egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                             for num in ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"] {
                                                                 if ui.button(num).clicked() {
                                                                     step.key = num.to_string();
                                                                     live_sync = true;
                                                                     ui.close_menu();
                                                                 }
                                                             }
                                                             ui.separator();
                                                             for sym in [";", "=", ",", "-", ".", "/", "`", "[", "\\", "]", "'"] {
                                                                 if ui.button(sym).clicked() {
                                                                     step.key = sym.to_string();
                                                                     live_sync = true;
                                                                     ui.close_menu();
                                                                 }
                                                             }
                                                         });
                                                     });
                                                     ui.menu_button(Self::tr_lang(language, "Navigation", "Điều hướng & Phím tắt"), |ui| {
                                                         ui.set_max_width(160.0);
                                                         for key in ["Escape", "Enter", "Space", "Backspace", "Tab", "Insert", "Delete", "Home", "End", "PageUp", "PageDown", "Left", "Up", "Right", "Down", "PrintScreen", "Pause"] {
                                                             if ui.button(key).clicked() {
                                                                 step.key = key.to_string();
                                                                 live_sync = true;
                                                                 ui.close_menu();
                                                             }
                                                         }
                                                     });
                                                     ui.menu_button(Self::tr_lang(language, "Function (F1-F24)", "Phím chức năng"), |ui| {
                                                         ui.set_max_width(100.0);
                                                         egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                             for num in 1..=24 {
                                                                 let key_str = format!("F{}", num);
                                                                 if ui.button(&key_str).clicked() {
                                                                     step.key = key_str;
                                                                     live_sync = true;
                                                                     ui.close_menu();
                                                                 }
                                                             }
                                                         });
                                                     });
                                                     ui.menu_button(Self::tr_lang(language, "Numpad", "Bàn phím số phụ"), |ui| {
                                                         ui.set_max_width(160.0);
                                                         for key in ["Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4", "Numpad5", "Numpad6", "Numpad7", "Numpad8", "Numpad9", "NumpadMultiply", "NumpadAdd", "NumpadSubtract", "NumpadDecimal", "NumpadDivide"] {
                                                             if ui.button(key).clicked() {
                                                                 step.key = key.to_string();
                                                                 live_sync = true;
                                                                 ui.close_menu();
                                                             }
                                                         }
                                                     });
                                                     ui.menu_button(Self::tr_lang(language, "Modifiers & Locks", "Phím khóa & bổ trợ"), |ui| {
                                                         ui.set_max_width(150.0);
                                                         for key in ["Ctrl", "Alt", "Shift", "Win", "CapsLock", "NumLock", "ScrollLock", "Apps"] {
                                                             if ui.button(key).clicked() {
                                                                 step.key = key.to_string();
                                                                 live_sync = true;
                                                                 ui.close_menu();
                                                             }
                                                         }
                                                     });
                                                 });
                                                 hs_menu_response.response.on_hover_text(Self::tr_lang(
                                                     language,
                                                     "Manually select key",
                                                     "Chọn phím thủ công"
                                                 ));
                                            } else {
                                                ui.add_sized([28.0, 20.0], egui::Label::new(""));
                                            }
                                            if ui.button(Self::tr_lang(language, "Clear", "Clear")).clicked() {
                                                clear_hold_stop_step = true;
                                            }
                                        });
                                        if clear_hold_stop_step {
                                            preset.hold_stop_step = MacroStep::default();
                                            live_sync = true;
                                        }
                                });
                                    }
                                    ui.scope(|ui| {
                            Frame::new()
                                .inner_margin(egui::Margin::symmetric(4, 2))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        let capture_target = CaptureRequest::MacroPresetRecordHotkey(group.id, preset.id);
                                        let has_rec_hotkey = preset.record_hotkey.is_some();
                                        let capture_active = self.capture_target.as_ref() == Some(&capture_target);
                                        let (rect, _) = ui.allocate_exact_size(egui::vec2(118.0, 18.0), egui::Sense::hover());
                                         let mut child_ui = ui.new_child(
                                             egui::UiBuilder::new()
                                                 .max_rect(rect)
                                                 .layout(egui::Layout::left_to_right(egui::Align::Center))
                                         );
                                         child_ui.spacing_mut().item_spacing.x = 2.0;
                                         if child_ui
                                             .add_sized([18.0, 18.0], Button::new(Self::material_icon_text(0xe145, 12.0)))
                                             .on_hover_text(Self::tr_lang(
                                                 language,
                                                 "Add step",
                                                 "Thêm một bước vào đầu preset này",
                                             ))
                                             .clicked()
                                         {
                                             preset.steps.insert(0, MacroStep::default());
                                             live_sync = true;
                                         }
                                         let is_recording_this = self.active_macro_record_preset_id == Some(preset.id);
                                         let record_icon = if is_recording_this { 0xe047 } else { 0xe061 }; // stop square or solid circle
                                         let mut dot_color = Color32::from_rgb(255, 60, 60);
                                         if is_recording_this {
                                             let ms = std::time::SystemTime::now()
                                                 .duration_since(std::time::SystemTime::UNIX_EPOCH)
                                                 .unwrap_or_default()
                                                 .as_millis();
                                             if (ms / 500) % 2 == 0 {
                                                 dot_color = Color32::from_rgba_unmultiplied(255, 60, 60, 80);
                                             }
                                             child_ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                                         }
                                         let record_text = if is_recording_this {
                                             Self::tr_lang(language, "Stop", "")
                                         } else {
                                             Self::tr_lang(language, "Record", "")
                                         };
                                         let record_btn = Button::new(
                                             RichText::new(format!("{} {}", Self::material_icon_text(record_icon, 10.0).text(), record_text))
                                                 .color(dot_color)
                                                 .strong()
                                         );
                                         if child_ui.add_sized([70.0, 20.0], record_btn)
                                             .on_hover_text(Self::tr_lang(
                                                 language,
                                                 "Record your keyboard and mouse clicks globally to automatically generate macro steps",
                                                "Ghi lại thao tác phím và click chuột toàn màn hình để tự động tạo bước macro",
                                             ))
                                             .clicked()
                                         {
                                             let _ = self.overlay_tx.send(crate::overlay::OverlayCommand::ToggleMacroRecording(
                                                 group.id,
                                                 preset.id,
                                                 group.name.clone(),
                                             ));
                                         }
                                         // Keyboard Trigger Hotkey Capture UI
                                         let pulse = if capture_active {
                                             let capture_time = child_ui.ctx().input(|input| input.time) as f32;
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
                                             child_ui.visuals().widgets.inactive.bg_fill
                                         };
                                          let kbd_btn_text = if capture_active {
                                              if let Some(pending) = self.capture_hotkey_combo_keys.as_ref() {
                                                  if !pending.is_empty() {
                                                      let preview = Self::hotkey_binding_from_combo_keys(pending.clone());
                                                      Self::format_binding_ui(language, Some(&preview))
                                                  } else {
                                                      Self::tr_lang(language, "capturing", "capturing").to_owned()
                                                  }
                                              } else {
                                                  Self::tr_lang(language, "capturing", "capturing").to_owned()
                                              }
                                          } else if let Some(binding) = &preset.record_hotkey {
                                              Self::format_binding_ui(language, Some(binding))
                                          } else {
                                              Self::material_icon_text(0xe312, 10.0).text().to_owned()
                                          };
                                          let text_color = if capture_active {
                                              Color32::WHITE
                                          } else if has_rec_hotkey {
                                              Color32::from_rgb(96, 232, 255)
                                          } else {
                                              child_ui.visuals().widgets.inactive.text_color()
                                          };
                                          let hover_text = if let Some(binding) = &preset.record_hotkey {
                                              let key_ui = Self::format_binding_ui(language, Some(binding));
                                              let fmt = Self::tr_lang(
                                                  language,
                                                  "Bound trigger key: {} (Click to clear)",
                                                 "Phím tắt đã gán: {} (Nhấp để xóa)",
                                              );
                                              fmt.replace("{}", &key_ui)
                                          } else {
                                              Self::tr_lang(
                                                  language,
                                                  "Click to bind a keyboard key to start/stop macro recording dynamically",
                                                 "Nhấp để gán phím tắt bắt đầu/dừng ghi macro nhanh",
                                              ).to_string()
                                          };
                                          let clicked = child_ui.scope(|ui| {
                                              ui.spacing_mut().button_padding = egui::vec2(6.0, 0.0);
                                              let kbd_btn = Button::new(
                                                  RichText::new(kbd_btn_text)
                                                      .color(text_color)
                                                      .strong()
                                                      .size(10.0)
                                              )
                                              .fill(capture_fill)
                                              .min_size(egui::vec2(18.0, 18.0));
                                              ui.add_sized([18.0, 18.0], kbd_btn)
                                          }).inner.on_hover_text(hover_text).clicked();
                                          if clicked {
                                              if capture_active {
                                                  cancel_active_capture = true;
                                              } else if has_rec_hotkey {
                                                  preset.record_hotkey = None;
                                                  live_sync = true;
                                              } else {
                                                  next_capture_target = Some(capture_target.clone());
                                              }
                                          }
                                         let is_recording_this = self.active_macro_record_preset_id == Some(preset.id);
                                         let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 20.0), egui::Sense::hover());
                                         let mut child_ui = ui.new_child(
                                             egui::UiBuilder::new()
                                                 .max_rect(rect)
                                                 .layout(egui::Layout::left_to_right(egui::Align::Center))
                                         );
                                         child_ui.spacing_mut().item_spacing.x = 2.0;
                                         if is_recording_this {
                                             let label_color = Color32::from_rgb(255, 96, 96);
                                             let is_even = (std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis() / 500) % 2 == 0;
                                             let dot_color = if is_even { label_color } else { label_color.linear_multiply(0.3) };
                                             child_ui.add_sized([18.0, 18.0], egui::Label::new(
                                                 RichText::new("● REC")
                                                     .color(dot_color)
                                                     .size(9.0)
                                                     .strong()
                                             )).on_hover_text(Self::tr_lang(language, "Macro recording is active", ""));
                                         }
                                         ui.add_sized(
                                             [18.0, 18.0],
                                             egui::Label::new(RichText::new("#").strong())
                                                 .halign(egui::Align::Center),
                                         );
                                          let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 20.0), egui::Sense::hover());
                                          let mut child_ui = ui.new_child(
                                              egui::UiBuilder::new()
                                                  .max_rect(rect)
                                                  .layout(egui::Layout::top_down(egui::Align::Center))
                                          );
                                          child_ui.horizontal(|ui| {
                                               ui.add_space(26.0);
                                               ui.label(RichText::new(Self::tr_lang(language, "Delay", "Delay")).strong());
                                           });
                                         ui.add_sized([148.0, 20.0], egui::Label::new(RichText::new(Self::tr_lang(language, "Action", "Action")).strong()));
                                         ui.add_sized([146.0, 21.0], egui::Label::new(""));
                                        let has_selected_steps = selected_steps_snapshot.iter().any(|(g_id, p_id, _)| *g_id == group.id && *p_id == preset.id);
                                        let selected_copy_feedback_active =
                                            self.macro_selected_steps_copy_feedback_target
                                                == Some((group.id, preset.id))
                                                && Self::is_copy_feedback_active(
                                                    self.macro_selected_steps_copy_feedback_until,
                                                );
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let paste_btn = Button::new(Self::tr_lang(language, "Paste", "Paste"))
                                                .min_size(egui::vec2(42.0, 20.0));
                                            if ui
                                                .add_enabled(!self.macro_step_clipboard.is_empty(), paste_btn)
                                                .on_hover_text(Self::tr_lang(
                                                    language,
                                                    "Paste copied step(s) at the top of this preset.",
                                                    "Paste copied step(s) at the top of this preset.",
                                                ))
                                                .clicked()
                                            {
                                                paste_steps_at_start = Some((group.id, preset.id));
                                            }
                                            if has_selected_steps {
                                                if selected_copy_feedback_active {
                                                    ui.add_sized(
                                                        [56.0, 20.0],
                                                        egui::Label::new(
                                                            RichText::new(Self::tr_lang(
                                                                language,
                                                                "Copied",
                                                                "Copied",
                                                            ))
                                                            .color(Color32::from_rgb(126, 224, 182))
                                                            .strong(),
                                                        ),
                                                    );
                                                } else {
                                                    let copy_btn = Button::new(Self::tr_lang(language, "Copy", "Copy"))
                                                        .min_size(egui::vec2(56.0, 20.0));
                                                    if ui
                                                        .add(copy_btn)
                                                        .on_hover_text(Self::tr_lang(
                                                            language,
                                                            "Copy the selected steps in this preset.",
                                                            "Copy selected steps in this preset.",
                                                        ))
                                                        .clicked()
                                                    {
                                                        copy_selected_steps = Some((group.id, preset.id));
                                                    }
                                                }
                                                let delete_btn = Button::new(Self::tr_lang(language, "Delete", "Xoa"))
                                                    .min_size(egui::vec2(64.0, 20.0));
                                                if ui
                                                    .add(delete_btn)
                                                    .on_hover_text(Self::tr_lang(
                                                        language,
                                                        "Delete selected steps",
                                                        "Xoa cac buoc da chon",
                                                    ))
                                                    .clicked()
                                                {
                                                    delete_selected_steps = Some((group.id, preset.id));
                                                }
                                            }
                                        });
                                        if has_rec_hotkey && !capture_active {
                                            let clear_btn = Button::new(RichText::new(Self::material_icon_text(0xe14c, 10.0).text()).color(Color32::LIGHT_RED));
                                            if ui.add_sized([18.0, 18.0], clear_btn)
                                                .on_hover_text(Self::tr_lang(language, "Clear hotkey", "Xóa phím tắt"))
                                                .clicked()
                                            {
                                                preset.record_hotkey = None;
                                                live_sync = true;
                                            }
                                        }
                                        });
                                    });
                            let steps_len = preset.steps.len();
                            let has_stop_vision = preset.steps.iter().any(|s| s.action == MacroAction::StopVision && s.enabled);
                            let drag_payload = egui::DragAndDrop::payload::<MacroStepDragPayload>(ui.ctx())
                                .filter(|payload| payload.group_id == group.id && payload.preset_id == preset.id);
                            let pointer_y = ui.ctx().pointer_interact_pos().map(|pointer| pointer.y);
                            let drag_preview_index_id =
                                ui.make_persistent_id((group.id, preset.id, "step-drag-preview-index"));
                            let expand_drag_drop_index =
                                |compact_index: usize, dragged_indices: &[usize]| {
                                    let mut compact_cursor = 0usize;
                                    for boundary in 0..=steps_len {
                                        if compact_cursor == compact_index {
                                            return boundary;
                                        }
                                        if boundary < steps_len
                                            && !dragged_indices.contains(&boundary)
                                        {
                                            compact_cursor += 1;
                                        }
                                    }
                                    steps_len
                                };
                            let mut preview_drop_index = steps_len;
                            let mut visual_step_order: Vec<usize> = (0..steps_len).collect();
                            let mut active_dragged_indices: Vec<usize> = Vec::new();
                            let mut active_current_compact_index: Option<usize> = None;
                            if let Some(payload) = drag_payload.as_ref() {
                                active_dragged_indices = payload.indices.clone();
                                active_dragged_indices.sort_unstable();
                                active_dragged_indices.dedup();
                                let compact_steps_len =
                                    steps_len.saturating_sub(active_dragged_indices.len());
                                let default_compact_index = active_dragged_indices
                                    .first()
                                    .copied()
                                    .map(|first_dragged| {
                                        first_dragged
                                            .saturating_sub(
                                                active_dragged_indices
                                                    .iter()
                                                    .filter(|index| **index < first_dragged)
                                                    .count(),
                                            )
                                    })
                                    .unwrap_or(0)
                                    .min(compact_steps_len);
                                let stored_compact_index = ui.memory(|mem| {
                                    mem.data
                                        .get_temp::<usize>(drag_preview_index_id)
                                        .unwrap_or(default_compact_index)
                                });
                                let current_compact_index =
                                    stored_compact_index.min(compact_steps_len);
                                active_current_compact_index = Some(current_compact_index);
                                let mut compact_order = (0..steps_len)
                                    .filter(|index| !active_dragged_indices.contains(index))
                                    .collect::<Vec<_>>();
                                for (offset, dragged_index) in
                                    active_dragged_indices.iter().copied().enumerate()
                                {
                                    compact_order.insert(
                                        (current_compact_index + offset).min(compact_order.len()),
                                        dragged_index,
                                    );
                                }
                                preview_drop_index = expand_drag_drop_index(
                                    current_compact_index,
                                    &active_dragged_indices,
                                );
                                visual_step_order = compact_order;
                            } else {
                                ui.memory_mut(|mem| {
                                    mem.data.remove::<usize>(drag_preview_index_id);
                                });
                            }
                            let mut step_rects = vec![Rect::ZERO; steps_len];
                            let mut next_compact_preview_index = steps_len;
                            let mut compact_cursor = 0usize;
                            let mut hovered_region = None;
                            for (display_index, step_index) in
                                visual_step_order.iter().copied().enumerate()
                            {
                                let step_is_being_dragged =
                                    active_dragged_indices.contains(&step_index);
                                let has_step_break_loop_warning = {
                                    let current_step = &preset.steps[step_index];
                                    current_step.action == MacroAction::StopIfKeyPressed
                                        && current_step.enabled
                                        && !{
                                            let mut depth = 0;
                                            let mut inside = false;
                                            for (idx, s) in preset.steps.iter().enumerate() {
                                                if idx == step_index {
                                                    if depth > 0 {
                                                        inside = true;
                                                    }
                                                    break;
                                                }
                                                if s.enabled {
                                                    if s.action == MacroAction::LoopStart {
                                                        depth += 1;
                                                    } else if s.action == MacroAction::LoopEnd {
                                                        if depth > 0 {
                                                            depth -= 1;
                                                        }
                                                    }
                                                }
                                            }
                                            inside
                                        }
                                };
                                let is_step_executing = crate::overlay::ACTIVE_MACRO_STEPS.lock()
                                    .get(&preset.id)
                                    .map(|set| set.contains(&step_index))
                                    .unwrap_or(false);
                                let step_ref = &preset.steps[step_index];
                                let is_vision_active = step_ref.action == MacroAction::StartVisionSearch && {
                                    crate::overlay::is_vision_following_active_by_spec(&step_ref.key)
                                };
                                let is_timer_active = step_ref.action == MacroAction::StartTimerPreset && {
                                    let t_id = step_ref.timer_preset_id.or_else(|| step_ref.key.trim().parse::<u32>().ok());
                                    crate::overlay::is_timer_preset_active(t_id)
                                };
                                let is_loop_end_active = step_ref.action == MacroAction::LoopEnd && {
                                    let mut matching_start_idx = None;
                                    let mut depth = 0usize;
                                    for idx in (0..=step_index).rev() {
                                        let s = &preset.steps[idx];
                                        match s.action {
                                            MacroAction::LoopEnd => depth += 1,
                                            MacroAction::LoopStart => {
                                                depth = depth.saturating_sub(1);
                                                if depth == 0 {
                                                    matching_start_idx = Some(idx);
                                                    break;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    if let Some(start_idx) = matching_start_idx {
                                        crate::overlay::ACTIVE_MACRO_STEPS.lock()
                                            .get(&preset.id)
                                            .map(|set| set.contains(&start_idx))
                                            .unwrap_or(false)
                                    } else {
                                        false
                                    }
                                };
                                let is_active = is_step_executing || is_vision_active || is_timer_active || is_loop_end_active;
                                let has_stop_audio = preset.steps.iter().any(|s| {
                                    matches!(
                                        s.action,
                                        MacroAction::StopAudioSense
                                    ) && s.enabled
                                });
                                let step = &mut preset.steps[step_index];
                                let is_selected = selected_steps_snapshot
                                    .contains(&(group.id, preset.id, step_index));
                                let drag_indices = if is_selected {
                                    let mut indices = selected_steps_snapshot
                                        .iter()
                                        .filter_map(|(selected_group, selected_preset, selected_index)| {
                                            (*selected_group == group.id
                                                && *selected_preset == preset.id)
                                                .then_some(*selected_index)
                                        })
                                        .collect::<Vec<_>>();
                                    indices.sort_unstable();
                                    if indices.is_empty() {
                                        vec![step_index]
                                    } else {
                                        indices
                                    }
                                } else {
                                    vec![step_index]
                                };
                                let mut row_fill = if is_selected {
                                    if step.enabled {
                                        Color32::from_rgba_premultiplied(88, 148, 220, 130)
                                    } else {
                                        Color32::from_rgba_premultiplied(68, 118, 180, 130)
                                    }
                                } else {
                                    ui.visuals().faint_bg_color
                                };
                                if !step.enabled && !is_selected {
                                    row_fill = Color32::from_rgba_unmultiplied(62, 62, 62, 220);
                                }
                                let has_infinite_loop_warning = self.state.macro_infinite_loop_warning_enabled
                                    && preset.enabled
                                    && (
                                        (preset.trigger_mode == MacroTriggerMode::Press && !preset.stop_on_retrigger_immediate)
                                        || preset.trigger_mode == MacroTriggerMode::Release
                                    )
                                    && step.action == MacroAction::LoopStart
                                    && step.is_infinite_loop();
                                let has_step_vision_leak = preset.enabled
                                    && (preset.trigger_mode == MacroTriggerMode::Press || preset.trigger_mode == MacroTriggerMode::Release)
                                    && step.action == MacroAction::StartVisionSearch
                                    && step.enabled
                                    && !has_stop_vision;
                                let has_step_audio_leak = preset.enabled
                                    && (preset.trigger_mode == MacroTriggerMode::Press || preset.trigger_mode == MacroTriggerMode::Release)
                                    && matches!(
                                        step.action,
                                        MacroAction::StartAudioSensePreset
                                    )
                                    && !has_stop_audio;
                                if has_infinite_loop_warning || has_step_vision_leak || has_step_audio_leak {
                                    row_fill = Color32::from_rgba_unmultiplied(255, 90, 0, 25);
                                } else if has_step_break_loop_warning {
                                    row_fill = Color32::from_rgba_unmultiplied(255, 200, 0, 15);
                                }
                                if is_active {
                                    row_fill = Color32::from_rgba_unmultiplied(0, 255, 170, 35);
                                }
                                if step_is_being_dragged {
                                    row_fill = Color32::from_rgba_unmultiplied(120, 235, 190, 95);
                                }
                                let row_drag_payload = MacroStepDragPayload {
                                    group_id: group.id,
                                    preset_id: preset.id,
                                    indices: drag_indices,
                                };
                                let border_stroke = if step_is_being_dragged {
                                    egui::Stroke::new(2.5, Color32::from_rgb(170, 255, 220))
                                } else if is_active {
                                    egui::Stroke::new(1.5, Color32::from_rgb(0, 255, 170))
                                } else {
                                    egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
                                };
                                let row_response = Frame::group(ui.style())
                                    .fill(row_fill)
                                    .stroke(border_stroke)
                                    .inner_margin(egui::Margin::symmetric(4, 2))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 2.0;
                                            ui.spacing_mut().interact_size.y = 18.0;
                                            let is_dark_theme = self.state.ui_theme == UiThemeMode::Dark;
                                            let hint_color = if is_dark_theme {
                                                Color32::from_rgba_unmultiplied(140, 140, 140, 150)
                                            } else {
                                                Color32::from_rgba_unmultiplied(100, 100, 100, 150)
                                            };
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(118.0, 18.0), egui::Sense::hover());
                                            let mut child_ui = ui.new_child(
                                                egui::UiBuilder::new()
                                                    .max_rect(rect)
                                                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                                            );
                                            child_ui.spacing_mut().item_spacing.x = 2.0;
                                            if child_ui
                                                .add_sized([18.0, 18.0], Button::new(Self::material_icon_text(0xe145, 12.0)))
                                                .on_hover_text(Self::tr_lang(language, "Add a new step below this one", "Thêm một bước mới phía dưới"))
                                                .clicked()
                                            {
                                                insert_step_after = Some((preset.id, step_index));
                                            }
                                            let select_icon = if is_selected {
                                                Self::material_icon_text(0xe5ca, 12.0).color(Color32::from_rgb(96, 232, 255))
                                            } else {
                                                RichText::new("")
                                            };
                                            if child_ui
                                                .add_sized(
                                                    [18.0, 18.0],
                                                    Button::new(select_icon),
                                                )
                                                .on_hover_text(Self::tr_lang(language, "Select step", "Chọn bước này"))
                                                .clicked()
                                            {
                                                pending_step_selection = Some((
                                                    group.id,
                                                    preset.id,
                                                    step_index,
                                                    child_ui.input(|input| input.modifiers.ctrl),
                                                    child_ui.input(|input| input.modifiers.shift),
                                                ));
                                            }
                                            child_ui.scope(|ui| {
                                                ui.visuals_mut().widgets.inactive.bg_fill = Color32::TRANSPARENT;
                                                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                                                ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
                                                let hover_bg = if is_dark_theme {
                                                    Color32::from_rgba_unmultiplied(255, 255, 255, 20)
                                                } else {
                                                    Color32::from_rgba_unmultiplied(0, 0, 0, 15)
                                                };
                                                let hover_stroke = if is_dark_theme {
                                                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 40))
                                                } else {
                                                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 30))
                                                };
                                                let active_bg = if is_dark_theme {
                                                    Color32::from_rgba_unmultiplied(255, 255, 255, 35)
                                                } else {
                                                    Color32::from_rgba_unmultiplied(0, 0, 0, 25)
                                                };
                                                let active_stroke = if is_dark_theme {
                                                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 60))
                                                } else {
                                                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 45))
                                                };
                                                ui.visuals_mut().widgets.hovered.bg_fill = hover_bg;
                                                ui.visuals_mut().widgets.hovered.bg_stroke = hover_stroke;
                                                ui.visuals_mut().widgets.active.bg_fill = active_bg;
                                                ui.visuals_mut().widgets.active.bg_stroke = active_stroke;
                                                let (toggle_changed, new_enabled) = ui.scope(|ui| {
                                                    let icon = if step.enabled { 0xe5ca } else { 0xe835 };
                                                    let fill = if step.enabled {
                                                        Color32::from_rgba_premultiplied(72, 156, 116, 120)
                                                    } else {
                                                        ui.visuals().faint_bg_color
                                                    };
                                                    let stroke = if step.enabled {
                                                        Color32::from_rgb(126, 224, 182)
                                                    } else {
                                                        ui.visuals().widgets.noninteractive.bg_stroke.color
                                                    };
                                                    let resp = ui.add_sized(
                                                        [18.0, 18.0],
                                                        Button::new(Self::material_icon_text(icon, 12.0))
                                                            .fill(fill)
                                                            .stroke(egui::Stroke::new(1.0, stroke)),
                                                    ).on_hover_text(Self::tr_lang(language, "Toggle step enabled", "Bật/Tắt bước này"));
                                                    (resp.clicked(), !step.enabled)
                                                }).inner;
                                                if toggle_changed {
                                                    step.enabled = new_enabled;
                                                    live_sync = true;
                                                }
                                                if ui
                                                    .add_sized(
                                                        [18.0, 18.0],
                                                        Button::new(Self::material_icon_text(0xe872, 16.0)),
                                                    )
                                                    .on_hover_text(Self::tr_lang(
                                                        language,
                                                        "Remove this step",
                                                        "Xóa bước này",
                                                    ))
                                                    .clicked()
                                                {
                                                    remove_step = Some((preset.id, step_index));
                                                }
                                                let drag_handle = ui
                                                    .add_sized(
                                                        [18.0, 18.0],
                                                        Button::new(Self::material_icon_text(0xe25d, 16.0))
                                                            .sense(Sense::drag()),
                                                    )
                                                    .on_hover_cursor(egui::CursorIcon::Grab);
                                                drag_handle.dnd_set_drag_payload(row_drag_payload.clone());
                                            });
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 20.0), egui::Sense::hover());
                                            let mut child_ui = ui.new_child(
                                                egui::UiBuilder::new()
                                                    .max_rect(rect)
                                                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                                            );
                                            child_ui.spacing_mut().item_spacing.x = 2.0;
                                            if has_infinite_loop_warning || has_step_vision_leak || has_step_audio_leak || has_step_break_loop_warning {
                                                let warn_color = if has_infinite_loop_warning || has_step_vision_leak || has_step_audio_leak {
                                                    Color32::from_rgb(255, 90, 0)
                                                } else {
                                                    Color32::from_rgb(255, 200, 0)
                                                };
                                                let response = child_ui.add_sized([18.0, 18.0], egui::Button::new(
                                                    Self::material_icon_text(0xe002, 16.0).color(warn_color)
                                                ).frame(false));
                                                if response.contains_pointer() {
                                                    egui::show_tooltip_at_pointer(child_ui.ctx(), child_ui.layer_id(), response.id.with("step-tip"), |ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.label(Self::material_icon_text(0xe002, 14.0).color(warn_color));
                                                            ui.label(RichText::new(Self::tr_lang(language, "STEP WARNING", "CẢNH BÁO BƯỚC")).strong().color(warn_color));
                                                        });
                                                        if has_infinite_loop_warning {
                                                            ui.label(Self::tr_lang(
                                                                language,
                                                                "This step starts an infinite loop without an end point. The macro will run forever until you manually stop it.",
                                                                "Bước này khởi đầu một vòng lặp vô tận mà không có điểm dừng, macro sẽ chạy mãi mãi cho đến khi bạn chủ động bấm dừng."
                                                            ));
                                                        }
                                                        if has_step_vision_leak {
                                                            ui.label(Self::tr_lang(
                                                                language,
                                                                "This step starts image search under Press/Release trigger, but there is no 'StopImageSearch' action in this macro! This could lead to a persistent background CPU thread. Add a 'StopImageSearch' step or change trigger to 'Hold'.",
                                                                "Bước này bắt đầu tìm ảnh (chế độ Nhấn/Thả) nhưng macro không có bước dừng tìm ảnh! Điều này có thể gây chạy ngầm hao CPU. Hãy thêm bước dừng tìm ảnh hoặc đổi trigger sang Giữ (Hold)."
                                                            ));
                                                        }
                                                        if has_step_audio_leak {
                                                            ui.label(Self::tr_lang(
                                                                language,
                                                                "This step starts AudioSense under Press/Release trigger, but there is no 'StopAudio' action in this macro! This could leave a background audio monitor running. Add a 'StopAudio' step or change trigger to 'Hold'.",
                                                                "Buoc nay bat dau AudioSense trong trigger Nhan/Tha nhung macro khong co buoc dung audio! Dieu nay co the de lai monitor audio chay ngam. Hay them buoc 'StopAudio' hoac doi trigger sang Hold."
                                                            ));
                                                        }
                                                        if has_step_break_loop_warning {
                                                            ui.label(Self::tr_lang(
                                                                language,
                                                                "This step breaks a loop, but it is not placed inside any Loop Start / Loop End block! It will have no effect.",
                                                                "Bước này thoát vòng lặp, nhưng nó hiện không nằm trong cặp khối Lặp (Loop Start) / Hết lặp (Loop End) nào! Nó sẽ không có tác dụng."
                                                            ));
                                                        }
                                                    });
                                                }
                                            }
                                            if is_active {
                                                child_ui.add_sized([18.0, 18.0], egui::Label::new(
                                                    RichText::new("\u{25CF} ")
                                                        .color(Color32::from_rgb(0, 255, 170))
                                                        .size(12.0)
                                                ))
                                                .on_hover_text(Self::tr_lang(language, "Step is running/active", "Bước này đang chạy/hoạt động"));
                                            } else {
                                                child_ui.add_sized([18.0, 18.0], egui::Label::new(""));
                                            }
                                            let step_num_text = format!("{}", display_index + 1);
                                            let label_width = if has_infinite_loop_warning || has_step_vision_leak || has_step_audio_leak || has_step_break_loop_warning { 20.0 } else { 20.0 };
                                            ui.add_sized(
                                                [label_width, 20.0],
                                                egui::Label::new(
                                                    if is_active {
                                                        RichText::new(step_num_text)
                                                            .monospace()
                                                            .color(Color32::from_rgb(0, 255, 170))
                                                            .strong()
                                                    } else {
                                                        RichText::new(step_num_text).monospace()
                                                    },
                                                )
                                                .halign(egui::Align::Center),
                                            );
                                            if step.delay_expr.is_empty() && step.delay_ms > 0 {
                                                step.delay_expr = step.delay_ms.to_string();
                                            }
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 20.0), egui::Sense::hover());
                                            let mut child_ui = ui.new_child(
                                                egui::UiBuilder::new()
                                                    .max_rect(rect)
                                                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                                            );
                                            child_ui.spacing_mut().item_spacing.x = 0.0;
                                            child_ui.spacing_mut().button_padding = egui::vec2(2.0, 0.0);
                                            child_ui.spacing_mut().interact_size.y = 21.0;
                                            child_ui.spacing_mut().interact_size.x = 36.0;
                                            let left_rounding = egui::CornerRadius { nw: 4, ne: 0, se: 0, sw: 4 };
                                            let right_rounding = egui::CornerRadius { nw: 0, ne: 4, se: 4, sw: 0 };
                                            child_ui.visuals_mut().widgets.inactive.corner_radius = left_rounding;
                                            child_ui.visuals_mut().widgets.hovered.corner_radius = left_rounding;
                                            child_ui.visuals_mut().widgets.active.corner_radius = left_rounding;
                                            child_ui.visuals_mut().widgets.open.corner_radius = left_rounding;
                                            child_ui.visuals_mut().widgets.noninteractive.corner_radius = left_rounding;
                                            let edit_id = child_ui.make_persistent_id((group.id, preset.id, step_index, "delay-edit-state"));
                                            let is_editing = child_ui.memory(|mem| mem.data.get_temp::<bool>(edit_id).unwrap_or(false));
                                            if is_editing {
                                                let delay_id = child_ui.id().with((step_index, "delay"));
                                                let response = Self::render_variable_text_edit(
                                                    &mut child_ui,
                                                    &mut step.delay_expr,
                                                    delay_id,
                                                    78.0,
                                                    130.0,
                                                      20.0, 20.0,
                                                    "0",
                                                    false,
                                                );
                                                Self::apply_vietnamese_input_if_changed(
                                                    &response,
                                                    self.state.vietnamese_input_enabled,
                                                    self.state.vietnamese_input_mode,
                                                    &mut step.delay_expr,
                                                );
                                                let just_started_id = edit_id.with("just_started");
                                                let just_started = child_ui.memory(|mem| mem.data.get_temp::<bool>(just_started_id).unwrap_or(false));
                                                if just_started {
                                                    response.request_focus();
                                                    child_ui.memory_mut(|mem| mem.data.insert_temp(just_started_id, false));
                                                }
                                                if response.changed() {
                                                    if let Ok(val) = step.delay_expr.trim().parse::<u64>() {
                                                        step.delay_ms = val;
                                                    } else {
                                                        step.delay_ms = 0;
                                                    }
                                                    live_sync = true;
                                                }
                                                if response.lost_focus() || child_ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                                    child_ui.memory_mut(|mem| mem.data.insert_temp(edit_id, false));
                                                }
                                            } else {
                                                let display_text = if step.delay_expr.is_empty() {
                                                    "0".to_string()
                                                } else {
                                                    step.delay_expr.clone()
                                                };
                                                let response = child_ui.add_sized(
                                                    [78.0, 20.0],
                                                    egui::Button::new(display_text)
                                                        .wrap_mode(egui::TextWrapMode::Truncate)
                                                        .sense(egui::Sense::click_and_drag()),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                                                let has_dragged_id = edit_id.with("has-dragged");
                                                if response.dragged() {
                                                    child_ui.memory_mut(|mem| mem.data.insert_temp(has_dragged_id, true));
                                                    let accum_id = edit_id.with("drag-accum");
                                                    let mut accum = child_ui.memory(|mem| mem.data.get_temp::<f32>(accum_id).unwrap_or(0.0));
                                                    accum += response.drag_delta().x;
                                                    let step_size = if child_ui.input(|i| i.modifiers.shift) {
                                                        10.0
                                                    } else {
                                                        1.0
                                                    };
                                                    let pixels_per_unit = 2.0;
                                                    let delta_units = (accum / pixels_per_unit).trunc();
                                                    if delta_units != 0.0 {
                                                        accum -= delta_units * pixels_per_unit;
                                                        let delta_int = (delta_units * step_size).round() as i32;
                                                        if delta_int != 0 {
                                                            step.delay_expr = Self::adjust_expression_by_delta(&step.delay_expr, delta_int);
                                                            if let Ok(val) = step.delay_expr.trim().parse::<u64>() {
                                                                step.delay_ms = val;
                                                            } else {
                                                                step.delay_ms = 0;
                                                            }
                                                            live_sync = true;
                                                        }
                                                    }
                                                    child_ui.memory_mut(|mem| mem.data.insert_temp(accum_id, accum));
                                                } else {
                                                    if !child_ui.input(|i| i.pointer.any_down()) {
                                                        let accum_id = edit_id.with("drag-accum");
                                                        child_ui.memory_mut(|mem| {
                                                            mem.data.insert_temp(has_dragged_id, false);
                                                            mem.data.insert_temp(accum_id, 0.0);
                                                        });
                                                    }
                                                }
                                                if response.clicked() {
                                                    let has_dragged = child_ui.memory(|mem| mem.data.get_temp::<bool>(has_dragged_id).unwrap_or(false));
                                                    if !has_dragged {
                                                        child_ui.memory_mut(|mem| {
                                                            mem.data.insert_temp(edit_id, true);
                                                            mem.data.insert_temp(edit_id.with("just_started"), true);
                                                        });
                                                    }
                                                }
                                            }
                                            child_ui.visuals_mut().widgets.inactive.corner_radius = right_rounding;
                                            child_ui.visuals_mut().widgets.hovered.corner_radius = right_rounding;
                                            child_ui.visuals_mut().widgets.active.corner_radius = right_rounding;
                                            child_ui.visuals_mut().widgets.open.corner_radius = right_rounding;
                                            child_ui.visuals_mut().widgets.noninteractive.corner_radius = right_rounding;
                                            let unit_text = if step.wait_time_unit.is_empty() { "ms" } else { &step.wait_time_unit };
                                            let popup_rounding = right_rounding;
                                            let popup_style_modifier = egui::style::StyleModifier::new(move |style| {
                                                style.visuals.widgets.noninteractive.corner_radius = popup_rounding;
                                            });
                                            egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "delay-unit"))
                                                .width(42.0)
                                                .selected_text(unit_text)
                                                .popup_style(popup_style_modifier)
                                                .show_ui(&mut child_ui, |ui| {
                                                    for unit in &["ms", "s", "m", "h"] {
                                                        let label = *unit;
                                                        let val = if label == "ms" { "" } else { label };
                                                        if ui.selectable_label(step.wait_time_unit == val, label).clicked() {
                                                            step.wait_time_unit = val.to_string();
                                                            live_sync = true;
                                                        }
                                                    }
                                                });
                                            let previous_action = step.action;
                                            let action_combo = egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "action"))
                                                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                                .width(146.0)
                                                .height(420.0)
                                                .selected_text(Self::macro_action_selected_widget_text(step.action, language))
                                                .show_ui(ui, |ui| {
                                                    ui.set_min_width(520.0);
                                                    live_sync |= ui.checkbox(&mut step.toggle_enabled_on_run, Self::tr_lang(
                                                        language,
                                                        "Toggle self enabled on run",
                                                        "Tự bật/tắt chính nó khi chạy",
                                                    )).changed();
                                                    ui.separator();
                                                    let action_hover_id = ui.make_persistent_id((
                                                        group.id,
                                                        preset.id,
                                                        step_index,
                                                        "action-hover",
                                                    ));
                                                    ui.ctx().data_mut(|data| {
                                                        data.insert_temp(action_hover_id, false);
                                                    });
                                                    egui::Grid::new((group.id, preset.id, step_index, "action-grid"))
                                                        .num_columns(8)
                                                        .spacing([6.0, 6.0])
                                                        .show(ui, |ui| {
                                                            let mut grid_col = 0;
                                                            for action in [
                                                                MacroAction::KeyPress,
                                                                MacroAction::KeyDown,
                                                                MacroAction::KeyUp,
                                                                MacroAction::TypeText,
                                                                MacroAction::ApplyWindowPreset,
                                                                MacroAction::FocusWindowPreset,
                                                                MacroAction::TriggerMacroPreset,
                                                                MacroAction::TriggerCommandPreset,
                                                                MacroAction::EnableCrosshairProfile,
                                                                MacroAction::DisableCrosshair,
                                                                MacroAction::EnablePinPreset,
                                                                MacroAction::DisablePin,
                                                                MacroAction::PlaySoundPreset,
                                                                MacroAction::PlayVideoPreset,
                                                                MacroAction::ApplyMouseSensitivityPreset,
                                                                MacroAction::LoopStart,
                                                                MacroAction::LoopEnd,
                                                                MacroAction::StopIfKeyPressed,
                                                                MacroAction::ShowHud,
                                                                MacroAction::HideHud,
                                                                MacroAction::LockKeys,
                                                                MacroAction::UnlockKeys,
                                                                MacroAction::EnableMacroPreset,
                                                                MacroAction::DisableMacroPreset,
                                                                MacroAction::EnableStep,
                                                                MacroAction::DisableStep,
                                                                MacroAction::SetVariable,
                                                                MacroAction::OcrSearch,
                                                                MacroAction::JumpToStep,
                                                            ] {
                                                                Self::render_macro_action_option(
                                                                    ui,
                                                                    language,
                                                                    &mut step.action,
                                                                    action,
                                                                    &mut live_sync,
                                                                    action_hover_id,
                                                                    false,
                                                                );
                                                                grid_col += 1;
                                                                if grid_col % 8 == 0 {
                                                                    ui.end_row();
                                                                }
                                                            }
                                                            Self::render_mouse_action_group_option(
                                                                ui,
                                                                language,
                                                                (group.id, preset.id, step_index, "mouse-group"),
                                                                &mut step.action,
                                                                &mut live_sync,
                                                                action_hover_id,
                                                            );
                                                            grid_col += 1;
                                                            if grid_col % 8 == 0 { ui.end_row(); }
                                                            Self::render_image_search_action_group_option(
                                                                ui,
                                                                language,
                                                                (group.id, preset.id, step_index, "image-search-group"),
                                                                &mut step.action,
                                                                &mut live_sync,
                                                                action_hover_id,
                                                            );
                                                            grid_col += 1;
                                                            if grid_col % 8 == 0 { ui.end_row(); }
                                                            Self::render_timer_action_group_option(
                                                                ui,
                                                                language,
                                                                (group.id, preset.id, step_index, "timer-group"),
                                                                &mut step.action,
                                                                &mut live_sync,
                                                                action_hover_id,
                                                            );
                                                            grid_col += 1;
                                                            if grid_col % 8 == 0 { ui.end_row(); }
                                                            Self::render_if_action_group_option(
                                                                ui,
                                                                language,
                                                                (group.id, preset.id, step_index, "if-group"),
                                                                &mut step.action,
                                                                &mut live_sync,
                                                                action_hover_id,
                                                            );
                                                            grid_col += 1;
                                                            if grid_col % 8 == 0 { ui.end_row(); }
                                                            Self::render_geometry_action_group_option(
                                                                ui,
                                                                language,
                                                                (group.id, preset.id, step_index, "geometry-group"),
                                                                &mut step.action,
                                                                &mut live_sync,
                                                                action_hover_id,
                                                            );
                                                            grid_col += 1;
                                                            if grid_col % 8 == 0 { ui.end_row(); }
                                                            Self::render_audio_sense_action_group_option(
                                                                ui,
                                                                language,
                                                                (group.id, preset.id, step_index, "audiosense-group"),
                                                                &mut step.action,
                                                                &mut live_sync,
                                                                action_hover_id,
                                                            );
                                                            grid_col += 1;
                                                            if grid_col % 8 == 0 { ui.end_row(); }
                                                        });
                                                });
                                            let action_uses_key = Self::macro_action_uses_key(step.action);
                                            let action_supports_capture =
                                                Self::macro_action_supports_capture(step.action);
                                            if action_uses_key {
                                                 if step.action == MacroAction::ApplyWindowPreset {
                                                     let id_salt = ui.make_persistent_id((group.id, preset.id, step_index, "window-preset-step"));
                                                     Self::render_window_control_step_comboboxes(
                                                         &window_presets,
                                                         &window_layouts,
                                                         ui,
                                                         language,
                                                         id_salt,
                                                         &mut step.key,
                                                         &mut live_sync,
                                                         146.0,
                                                     );
                                                 } else if step.action == MacroAction::FocusWindowPreset {
                                                    let mut selected_window = if step.key.trim().is_empty() {
                                                        None
                                                    } else {
                                                        Some(step.key.clone())
                                                    };
                                                    let mut duplicate_mode = false;
                                                    if Self::render_window_target_combo_with_duplicate_mode(
                                                        ui,
                                                        (group.id, preset.id, "hold-stop-focus-window-preset"),
                                                        &Self::tr_lang(language, "Select window", "Chọn cửa sổ"),
                                                        &mut selected_window,
                                                        &mut duplicate_mode,
                                                        &self.open_windows,
                                                        160.0,
                                                        false,
                                                    ) {
                                                        step.key = selected_window.unwrap_or_default();
                                                        live_sync = true;
                                                    }
                                                } else if step.action == MacroAction::TriggerMacroPreset {
                                                    // Auto-init group if None
                                                    if step.trigger_macro_group_id.is_none() {
                                                        step.trigger_macro_group_id = Some(group.id);
                                                        live_sync = true;
                                                    }
                                                    let trig_group_id = step.trigger_macro_group_id.unwrap_or(group.id);
                                                    let trig_group_name = all_groups_for_trigger
                                                        .iter()
                                                        .find(|(gid, _, _)| *gid == trig_group_id)
                                                        .map(|(_, gname, _)| gname.clone())
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select group", "Chọn group").to_owned());
                                                    // ComboBox 1: Select group
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "trigger-macro-group-step"))
                                                        .width(110.0)
                                                        .selected_text(&trig_group_name)
                                                        .show_ui(ui, |ui| {
                                                            for (gid, gname, gpresets) in &all_groups_for_trigger {
                                                                if ui
                                                                    .selectable_label(*gid == trig_group_id, &{
                                                                    if gname.starts_with("Macro Group") && !gpresets.is_empty() {
                                                                        let preset_labels: Vec<String> = gpresets.iter().map(|(_, lbl)| lbl.clone()).collect();
                                                                        format!("{} [{}]", gname, preset_labels.join(", "))
                                                                    } else {
                                                                        gname.clone()
                                                                    }
                                                                })
                                                                    .clicked()
                                                                {
                                                                    step.trigger_macro_group_id = Some(*gid);
                                                                    if *gid != trig_group_id {
                                                                        step.key = String::new();
                                                                    }
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    // ComboBox 2: Select preset from chosen group
                                                    let trig_presets: Vec<(u32, String)> = all_groups_for_trigger
                                                        .iter()
                                                        .find(|(gid, _, _)| *gid == trig_group_id)
                                                        .map(|(gid, _, gpresets)| {
                                                            gpresets.iter()
                                                                .filter(|(pid, _)| true)
                                                                .cloned()
                                                                .collect()
                                                        })
                                                        .unwrap_or_default();
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| trig_presets.iter().find(|(pid, _)| *pid == id).map(|(_, lbl)| lbl.clone()))
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select macro", "Chọn macro").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "trigger-macro-preset-step"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for (preset_option_id, preset_option_label) in &trig_presets {
                                                                if ui
                                                                    .selectable_label(selected_id == Some(*preset_option_id), preset_option_label)
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option_id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                        ui.add_space(4.0);
                                                        let selected_trigger_warning = selected_id
                                                            .and_then(|id| {
                                                                all_trigger_macro_warnings
                                                                    .iter()
                                                                    .find(|(preset_id, _)| *preset_id == id)
                                                                    .map(|(_, warning)| *warning)
                                                            })
                                                            .unwrap_or(false);
                                                        if selected_trigger_warning {
                                                            let warn_color = Color32::from_rgb(255, 90, 0);
                                                            let response = ui.add_sized([18.0, 18.0], egui::Button::new(
                                                                Self::material_icon_text(0xe002, 16.0).color(warn_color)
                                                            ).frame(false));
                                                            if response.contains_pointer() {
                                                                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), response.id.with("trigger-macro-warning-tip"), |ui| {
                                                                    ui.horizontal(|ui| {
                                                                        ui.label(Self::material_icon_text(0xe002, 14.0).color(warn_color));
                                                                        ui.label(RichText::new(Self::tr_lang(language, "TRIGGER WARNING", "CẢNH BÁO KÍCH HOẠT")).strong().color(warn_color));
                                                                    });
                                                                    ui.label(Self::tr_lang(
                                                                        language,
                                                                        "This macro preset can run continuously or keep working in the background. Triggering it from another macro can make it run immediately without a hold key.",
                                                                        "Macro này có thể chạy liên tục hoặc tiếp tục chạy trong nền. Kích hoạt nó từ macro khác có thể làm nó chạy ngay mà không cần giữ phím.",
                                                                    ));
                                                                });
                                                            }
                                                        }
                                                        let cb_text = Self::tr_lang(language, "Wait for completion", "Đợi chạy xong");
                                                        if ui.checkbox(&mut step.wait_for_completion, cb_text).changed() {
                                                            live_sync = true;
                                                        }
                                                } else if step.action == MacroAction::TriggerCommandPreset {
                                                    let selected_id = step
                                                        .key
                                                        .trim()
                                                        .parse::<u32>()
                                                        .ok()
                                                        .or_else(|| {
                                                            self.state
                                                                .command_presets
                                                                .iter()
                                                                .find(|preset| preset.name.trim().eq_ignore_ascii_case(step.key.trim()))
                                                                .map(|preset| preset.id)
                                                        });
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .command_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            if step.key.trim().is_empty() {
                                                                Self::tr_lang(language, "Select command", "Chọn câu lệnh")
                                                                .to_owned()
                                                            } else {
                                                                step.key.clone()
                                                            }
                                                        });
                                                    let custom_preset_combo = egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "trigger-custom-preset-step"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.command_presets {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(preset_option.id),
                                                                        &preset_option.name,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    let is_generating = self.command_ai_job.as_ref()
                                                         .map(|job| job.preset_id == 999999)
                                                         .unwrap_or(false)
                                                         && self.command_ai_step_target.as_ref()
                                                             .map(|target| target.0 == group.id && target.1 == preset.id && target.2 == Some(step_index))
                                                             .unwrap_or(false);
                                                     let (custom_draft_changed, custom_save_request, custom_save_and_open_ai_request, open_ai_preset_id) = Self::render_custom_preset_step_draft_popup(
                                                          ui,
                                                          &custom_preset_combo.response,
                                                          &custom_preset_combo.response,
                                                          step,
                                                          (group.id, preset.id, step_index),
                                                          Some(step_index),
                                                          language,
                                                          &command_presets_snapshot,
                                                          is_generating,
                                                     );
                                                     live_sync |= custom_draft_changed;
                                                     if let Some((save_step_index, name, command, use_powershell)) = custom_save_request {
                                                         pending_custom_preset_save = Some((
                                                             group.id,
                                                             preset.id,
                                                             save_step_index,
                                                             name,
                                                             command,
                                                             use_powershell,
                                                         ));
                                                     }
                                                     if let Some((save_step_index, name, command, use_powershell, is_ad_hoc)) = custom_save_and_open_ai_request {
                                                         pending_custom_preset_save_and_open_ai = Some((
                                                             group.id,
                                                             preset.id,
                                                             save_step_index,
                                                             name,
                                                             command,
                                                             use_powershell,
                                                             is_ad_hoc,
                                                         ));
                                                     }
                                                     if let Some(preset_id) = open_ai_preset_id {
                                                         pending_open_ai_preset_id = Some(preset_id);
                                                     }
                                                } else if matches!(
                                                    step.action,
                                                    MacroAction::EnableMacroPreset
                                                        | MacroAction::DisableMacroPreset
                                                ) {
                                                    if step.trigger_macro_group_id.is_none() {
                                                        step.trigger_macro_group_id = Some(group.id);
                                                        live_sync = true;
                                                    }
                                                    let trig_group_id = step.trigger_macro_group_id.unwrap_or(group.id);
                                                    let trig_group_name = all_groups_for_trigger
                                                        .iter()
                                                        .find(|(gid, _, _)| *gid == trig_group_id)
                                                        .map(|(_, gname, _)| gname.clone())
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select group", "Chọn group").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "enable-disable-macro-group-step"))
                                                        .width(110.0)
                                                        .selected_text(&trig_group_name)
                                                        .show_ui(ui, |ui| {
                                                            for (gid, gname, gpresets) in &all_groups_for_trigger {
                                                                if ui
                                                                    .selectable_label(*gid == trig_group_id, &{
                                                                    if gname.starts_with("Macro Group") && !gpresets.is_empty() {
                                                                        let preset_labels: Vec<String> = gpresets.iter().map(|(_, lbl)| lbl.clone()).collect();
                                                                        format!("{} [{}]", gname, preset_labels.join(", "))
                                                                    } else {
                                                                        gname.clone()
                                                                    }
                                                                })
                                                                    .clicked()
                                                                {
                                                                    step.trigger_macro_group_id = Some(*gid);
                                                                    if *gid != trig_group_id {
                                                                        step.key = String::new();
                                                                    }
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    let trig_presets: Vec<(u32, String)> = all_groups_for_trigger
                                                        .iter()
                                                        .find(|(gid, _, _)| *gid == trig_group_id)
                                                        .map(|(_, _, gpresets)| gpresets.clone())
                                                        .unwrap_or_default();
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| trig_presets.iter().find(|(pid, _)| *pid == id).map(|(_, lbl)| lbl.clone()))
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select macro", "Chọn macro").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "enable-disable-macro-preset-step"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for (preset_option_id, preset_option_label) in &trig_presets {
                                                                if ui
                                                                    .selectable_label(selected_id == Some(*preset_option_id), preset_option_label)
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option_id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                } else if matches!(step.action, MacroAction::EnableStep | MacroAction::DisableStep) {
                                                    // Parse `preset_id|1,2,3` or `1,2,3` for legacy fallback
                                                    let (selected_preset_id, mut selected_steps) = {
                                                        let parts: Vec<&str> = step.key.split('|').collect();
                                                        if parts.len() == 2 {
                                                            let p_id = parts[0].trim().parse::<u32>().ok();
                                                            let s_list = parts[1].split(',').filter_map(|s| s.trim().parse::<u32>().ok()).collect::<Vec<u32>>();
                                                            (p_id, s_list)
                                                        } else {
                                                            let s_list = step.key.split(',').filter_map(|s| s.trim().parse::<u32>().ok()).collect::<Vec<u32>>();
                                                            (None, s_list)
                                                        }
                                                    };
                                                    let current_preset_id = selected_preset_id.unwrap_or(preset.id);
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        let preset_label = group_preset_options.iter()
                                                            .find(|(id, _)| *id == current_preset_id)
                                                            .map(|(_, label)| label.clone())
                                                            .unwrap_or_else(|| Self::tr_lang(language, "Select preset", "Chọn preset").to_owned());
                                                        egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "step-preset-select"))
                                                            .width(146.0)
                                                            .selected_text(preset_label)
                                                            .show_ui(ui, |ui| {
                                                                for (preset_option_id, preset_option_label) in &group_preset_options {
                                                                    if ui.selectable_label(current_preset_id == *preset_option_id, preset_option_label).clicked() {
                                                                        if current_preset_id != *preset_option_id {
                                                                            step.key = format!("{}|", preset_option_id);
                                                                            live_sync = true;
                                                                        }
                                                                    }
                                                                }
                                                            });
                                                        let target_step_count = group_preset_step_counts.iter()
                                                            .find(|(id, _)| *id == current_preset_id)
                                                            .map(|(_, count)| *count)
                                                            .unwrap_or(0);
                                                        let original_len = selected_steps.len();
                                                        selected_steps.retain(|&x| x <= target_step_count);
                                                        if selected_steps.len() != original_len {
                                                            let steps_str = selected_steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
                                                            step.key = format!("{}|{}", current_preset_id, steps_str);
                                                            live_sync = true;
                                                        }
                                                        let steps_label = if selected_steps.is_empty() {
                                                            Self::tr_lang(language, "Select steps", "Chọn steps").to_owned()
                                                        } else {
                                                            selected_steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
                                                        };
                                                        egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "step-multi-select"))
                                                            .width(146.0)
                                                            .selected_text(steps_label)
                                                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                                            .show_ui(ui, |ui| {
                                                                for i in 1..=target_step_count {
                                                                    let mut is_selected = selected_steps.contains(&i);
                                                                    if ui.checkbox(&mut is_selected, format!("Step {}", i)).changed() {
                                                                        if is_selected {
                                                                            selected_steps.push(i);
                                                                        } else {
                                                                            selected_steps.retain(|x| *x != i);
                                                                        }
                                                                        selected_steps.sort_unstable();
                                                                        let steps_str = selected_steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
                                                                        step.key = format!("{}|{}", current_preset_id, steps_str);
                                                                        live_sync = true;
                                                                    }
                                                                }
                                                            });
                                                    });
                                                } else if matches!(step.action, MacroAction::StartTimerPreset | MacroAction::PauseTimerPreset | MacroAction::StopTimerPreset) {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state.timer_presets.iter()
                                                                .find(|p| p.id == id)
                                                                .map(|p| p.name.clone())
                                                        })
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select timer", "Chọn hẹn giờ").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "step-timer-preset-select"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for timer in &self.state.timer_presets {
                                                                if ui.selectable_label(selected_id == Some(timer.id), &timer.name).clicked() {
                                                                    step.key = timer.id.to_string();
                                                                    step.timer_preset_id = Some(timer.id);
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                } else if step.action == MacroAction::EnableCrosshairProfile {
                                                    let selected_label = if step.key.trim().is_empty() {
                                                        Self::tr_lang(language, "Select crosshair", "Chọn tâm ngắm").to_owned()
                                                    } else {
                                                        step.key.clone()
                                                    };
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "crosshair-profile-step"))
                                                        .width(120.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for profile in &self.state.profiles {
                                                                if ui
                                                                    .selectable_label(step.key == profile.name, &profile.name)
                                                                    .clicked()
                                                                {
                                                                    step.key = profile.name.clone();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    Self::render_overlay_eye_button(
                                                        ui,
                                                        language,
                                                        step,
                                                        group.id,
                                                        preset.id,
                                                        step_index,
                                                        false,
                                                        &self.state.hud_presets,
                                                        &mut self.draw_geometry_step_preview_target,
                                                        &self.overlay_tx,
                                                        [18.0, 18.0],
                                                        12.0,
                                                    );
                                                } else if step.action == MacroAction::EnablePinPreset {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .pin_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select pin", "Chọn ghim").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "pin-preset-step"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.pin_presets {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(preset_option.id),
                                                                        &preset_option.name,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        Self::render_overlay_eye_button(
                                                            ui,
                                                            language,
                                                            step,
                                                            group.id,
                                                            preset.id,
                                                            step_index,
                                                            false,
                                                            &self.state.hud_presets,
                                                            &mut self.draw_geometry_step_preview_target,
                                                            &self.overlay_tx,
                                                            [18.0, 18.0],
                                                            12.0,
                                                        );
                                                    });
                                                } else if step.action == MacroAction::PlayMousePathPreset {
                                                    let parsed_selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_id = parsed_selected_id.and_then(|id| {
                                                        self.state
                                                            .mouse_path_presets
                                                            .iter()
                                                            .find(|preset| preset.id == id)
                                                            .map(|preset| preset.id)
                                                    });
                                                    if parsed_selected_id.is_some() && selected_id.is_none() {
                                                        step.key.clear();
                                                        live_sync = true;
                                                    }
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .mouse_path_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select path", "Chọn đường chuột").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "mouse-path-preset-step"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.mouse_path_presets {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(preset_option.id),
                                                                        &preset_option.name,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                            ui.separator();
                                                            if ui
                                                                .selectable_label(
                                                                    false,
                                                                    Self::tr_lang(
                                                                        language,
                                                                        "+ Add preset",
                                                                        "+ Them preset",
                                                                    ),
                                                                )
                                                                .clicked()
                                                            {
                                                                add_mouse_path_preset_request = Some((
                                                                    group.id,
                                                                    preset.id,
                                                                    step_index,
                                                                    selected_id,
                                                                ));
                                                                ui.close();
                                                            }
                                                        });
                                                    let draw_capture_active = selected_id
                                                        .is_some_and(|path_preset_id| {
                                                            self.mouse_path_draw_capture_preset_id
                                                                == Some(path_preset_id)
                                                        });
                                                    let draw_button_icon = if draw_capture_active {
                                                        0xe047
                                                    } else {
                                                        0xe061
                                                    };
                                                    let draw_button_label = if draw_capture_active {
                                                        Self::tr_lang(language, "Stop", "Dung")
                                                    } else {
                                                        Self::tr_lang(language, "Draw", "Ve")
                                                    };
                                                    let mut draw_button_color =
                                                        Color32::from_rgb(255, 76, 76);
                                                    if draw_capture_active {
                                                        let ms = std::time::SystemTime::now()
                                                            .duration_since(
                                                                std::time::SystemTime::UNIX_EPOCH,
                                                            )
                                                            .unwrap_or_default()
                                                            .as_millis();
                                                        if (ms / 500) % 2 == 0 {
                                                            draw_button_color =
                                                                Color32::from_rgba_unmultiplied(
                                                                    255, 76, 76, 96,
                                                                );
                                                        }
                                                        ui.ctx().request_repaint_after(
                                                            std::time::Duration::from_millis(250),
                                                        );
                                                    }
                                                    let draw_button = Button::new(
                                                        RichText::new(format!(
                                                            "{} {}",
                                                            Self::material_icon_text(
                                                                draw_button_icon,
                                                                10.0
                                                            )
                                                            .text(),
                                                            draw_button_label
                                                        ))
                                                        .color(draw_button_color)
                                                        .strong(),
                                                    );
                                                    let draw_tooltip = if selected_id.is_some() || draw_capture_active {
                                                        Self::tr_lang(
                                                            language,
                                                            "Hide the app, hold left mouse to draw this path, then release to save into the selected Mouse Path preset.",
                                                            "An app, giu chuot trai de ve duong, roi tha ra de luu vao Mouse Path da chon.",
                                                        )
                                                    } else {
                                                        Self::tr_lang(
                                                            language,
                                                            "Press Draw to create a new Mouse Path preset and record into it.",
                                                            "Bam Draw de tao Mouse Path preset moi va ghi vao do.",
                                                        )
                                                    };
                                                    let draw_response = ui
                                                        .add(draw_button)
                                                        .on_hover_text(draw_tooltip);
                                                    if draw_response.clicked() {
                                                        if draw_capture_active {
                                                            cancel_mouse_path_draw_capture = true;
                                                        } else {
                                                            begin_mouse_path_draw_capture_request =
                                                                Some((
                                                                    group.id,
                                                                    preset.id,
                                                                    step_index,
                                                                    selected_id,
                                                                ));
                                                        }
                                                    }
                                                    let preview_active = selected_id
                                                        .is_some_and(|path_preset_id| {
                                                            self.mouse_path_step_preview_preset_id
                                                                == Some(path_preset_id)
                                                        });
                                                    let preview_response = ui
                                                        .button(Self::tr_lang(
                                                            language,
                                                            if preview_active {
                                                                "Hide preview"
                                                            } else {
                                                                "Preview"
                                                            },
                                                            if preview_active {
                                                                "Tat preview"
                                                            } else {
                                                                "Xem truoc"
                                                            },
                                                        ))
                                                        .on_hover_text(Self::tr_lang(
                                                            language,
                                                            "Show this path on the real screen at its recorded size and position.",
                                                            "Hien duong chuot nay tren man hinh that dung vi tri va kich thuoc da ghi.",
                                                        ));
                                                    if preview_response.clicked() {
                                                        if let Some(path_preset_id) = selected_id {
                                                            preview_mouse_path_step_request =
                                                                Some(if preview_active {
                                                                    None
                                                                } else {
                                                                    Some(path_preset_id)
                                                                });
                                                        } else {
                                                            self.status = Self::tr_lang(
                                                                language,
                                                                "Select a Mouse Path preset first.",
                                                                "Hay chon Mouse Path truoc.",
                                                            )
                                                            .to_owned();
                                                        }
                                                    }
                                                    let wait_label = Self::tr_lang(
                                                        language,
                                                        "Wait for completion",
                                                        "Doi chay xong",
                                                    );
                                                    if ui
                                                        .checkbox(
                                                            &mut step.wait_for_completion,
                                                            wait_label,
                                                        )
                                                        .on_hover_text(Self::tr_lang(
                                                            language,
                                                            "Wait until this Mouse Path finishes before the macro continues to the next step.",
                                                            "Doi Mouse Path nay chay xong roi macro moi chuyen sang step tiep theo.",
                                                        ))
                                                        .changed()
                                                    {
                                                        live_sync = true;
                                                    }
                                                } else if matches!(
                                                    step.action,
                                                    MacroAction::StartVisionSearch
                                                         | MacroAction::ScanVisionOnce
                                                         | MacroAction::StopVision
                                                ) {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            image_search_preset_options
                                                                .iter()
                                                                .find(|(preset_id, _)| *preset_id == id)
                                                                .map(|(_, label)| label.clone())
                                                        })
                                                        .unwrap_or_else(|| "Select image".to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "image-search-preset-step"))
    .width(146.0)

    .selected_text(selected_label)

    .show_ui(ui, |ui| {

                    let (image_presets, color_presets, pixel_presets): (Vec<_>, Vec<_>, Vec<_>) = self.state.vision_presets.iter().fold(
                        (Vec::new(), Vec::new(), Vec::new()),
                        |(mut img, mut col, mut pix), p| {
                            if p.is_pixel_counter {
                                pix.push(p);
                            } else if p.use_color_matching {
                                col.push(p);
                            } else {
                                img.push(p);
                            }
                            (img, col, pix)
                        }
                    );
                    if !image_presets.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0, 191, 255), "Image Detect");
                        ui.separator();
                        for p in &image_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.key = p.id.to_string();
                                live_sync = true;
                            }
                        }
                    }
                    if !color_presets.is_empty() {
                        if !image_presets.is_empty() {
                            ui.add_space(4.0);
                        }
                        ui.colored_label(egui::Color32::from_rgb(0, 250, 154), "Color Detect");
                        ui.separator();
                        for p in &color_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.key = p.id.to_string();
                                live_sync = true;
                            }
                        }
                    }
                    let show_pixel_counter = step.action == MacroAction::ScanVisionOnce;
                    if show_pixel_counter && !pixel_presets.is_empty() {
                        if !image_presets.is_empty() || !color_presets.is_empty() {
                            ui.add_space(4.0);
                        }
                        ui.colored_label(egui::Color32::from_rgb(255, 165, 0), "Pixel Counter");
                        ui.separator();
                        for p in &pixel_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.key = p.id.to_string();
                                live_sync = true;
                            }
                        }
                    }
    });

                                                     let is_pixel = selected_id.and_then(|id| {
                                                        self.state.vision_presets.iter().find(|p| p.id == id)
                                                    }).map(|p| p.is_pixel_counter).unwrap_or(false);
                                                     let is_single_pixel = selected_id.and_then(|id| {
                                                        self.state.vision_presets.iter().find(|p| p.id == id)
                                                    }).map(|p| p.use_color_matching && p.search_region_is_single_pixel && !p.is_pixel_counter).unwrap_or(false);
                                                     let supports_move_mouse = selected_id.is_some() && !is_pixel && !is_single_pixel;
                                                    if matches!(step.action, MacroAction::StartVisionSearch | MacroAction::StopVision) {
                                                        ui.add_space(4.0);
                                                        ui.weak(Self::tr_lang(language, "(Mouse move only)", "(Chức năng di chuột)"));
                                                    }
                                                                                                           if step.action == MacroAction::ScanVisionOnce {
                                                          ui.add_space(4.0);
                                                          if is_single_pixel {
                                                              ui.label(Self::tr_lang(language, "Color Var:", "Biến màu:"));
                                                              let prev_override = ui.visuals().override_text_color;
                                                              ui.visuals_mut().override_text_color = None;
                                                              let resp = ui.add(egui::TextEdit::singleline(&mut step.if_variable_name).hint_text("color_var").desired_width(100.0));
                                                              ui.visuals_mut().override_text_color = prev_override;
                                                              Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.if_variable_name);
                                                              live_sync |= resp.changed();
                                                          } else {
                                                              let outputs_label = Self::tr_lang(language, "Outputs", "Đầu ra").to_owned();
                                                              egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "vision-outputs-rt"))
                                                                  .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                                                  .width(110.0)
                                                                  .selected_text(outputs_label)
                                                                  .show_ui(ui, |ui| {
                                                                      ui.set_min_width(200.0);
                                                                      egui::Grid::new("vision_outputs_grid_rt")
                                                                          .num_columns(2)
                                                                          .spacing([8.0, 6.0])
                                                                          .show(ui, |ui| {
                                                                              if !is_pixel {
                                                                                  ui.label("Found:");
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_found_var).hint_text("found_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_found_var);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                                  ui.label("Pos X:");
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_pos_var_x).hint_text("result_x_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_pos_var_x);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                                  ui.label("Pos Y:");
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_pos_var_y).hint_text("result_y_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_pos_var_y);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                              } else {
                                                                                  ui.label("Found:");
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_found_var).hint_text("found_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_found_var);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                                  ui.label(Self::tr_lang(language, "Count Var:", "Biến kết quả:"));
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.if_variable_name).hint_text("count_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.if_variable_name);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                              }
                                                                          });
                                                                  });
                                                          }
                                                          if supports_move_mouse {
                                                              ui.add_space(4.0);
                                                              let resp = ui.checkbox(
                                                                  &mut step.vision_move_cursor_on_match,
                                                                  Self::tr_lang(language, "Move Mouse", "Di chuột"),
                                                              );
                                                              live_sync |= resp.changed();
                                                          }
                                                      }
                                                } else if step.action == MacroAction::EnableZoomPreset {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .zoom_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| "Select zoom".to_owned());
                                                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "zoom-preset-step"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.zoom_presets {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(preset_option.id),
                                                                        &preset_option.name,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                } else if step.action == MacroAction::OcrSearch {
                                                    let is_custom = step.key.trim().is_empty() || step.key.trim() == "0";
                                                    let selected_id = if is_custom { None } else { step.key.trim().parse::<u32>().ok() };
                                                    let selected_label = if is_custom {
                                                        Self::tr_lang(language, "Custom OCR", "OCR tuỳ chỉnh").to_owned()
                                                    } else {
                                                        selected_id
                                                            .and_then(|id| {
                                                                ocr_preset_options
                                                                    .iter()
                                                                    .find(|(preset_id, _)| *preset_id == id)
                                                                    .map(|(_, label)| label.clone())
                                                            })
                                                            .unwrap_or_else(|| Self::tr_lang(language, "Select OCR", "Chọn OCR").to_owned())
                                                    };
                                                    live_sync |= Self::sanitize_legacy_ocr_target_text(&mut step.ocr_target_text);
                                                    // 1. OCR Preset ComboBox (Width 110.0)
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "ocr-preset-step"))
                                                        .width(110.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            if ui.selectable_label(is_custom, Self::tr_lang(language, "Custom OCR", "OCR tuỳ chỉnh")).clicked() {
                                                                step.key = "0".to_string();
                                                                live_sync = true;
                                                            }
                                                            ui.separator();
                                                            for (preset_option_id, preset_option_label) in &ocr_preset_options {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(*preset_option_id),
                                                                        preset_option_label,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option_id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    Self::render_ocr_outputs_selector(
                                                        ui,
                                                        language,
                                                        self.state.vietnamese_input_enabled,
                                                        self.state.vietnamese_input_mode,
                                                        group.id,
                                                        preset.id,
                                                        step_index,
                                                        step,
                                                        &mut live_sync,
                                                    );
                                                    if is_custom {
                                                        Self::render_custom_ocr_inline_controls(
                                                            ui,
                                                            language,
                                                            self.state.vietnamese_input_enabled,
                                                            self.state.vietnamese_input_mode,
                                                            group.id,
                                                            preset.id,
                                                            step_index,
                                                            step,
                                                            &mut live_sync,
                                                            &mut pending_ocr_step_capture,
                                                            &mut pending_ocr_language_settings,
                                                        );
                                                    }
                                                                                                } else if step.action == MacroAction::PlaySoundPreset {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .audio_settings
                                                                .presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select sound", "Chọn âm thanh").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "sound-preset-step"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.audio_settings.presets {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(preset_option.id),
                                                                        &preset_option.name,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                } else if step.action == MacroAction::PlayVideoPreset {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .audio_settings
                                                                .video_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| Self::tr_lang(language, "Select video", "Chọn video").to_owned());
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "video-preset-step"))
                                                        .width(120.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for preset_option in &self.state.audio_settings.video_presets {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(preset_option.id),
                                                                        &preset_option.name,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = preset_option.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    Self::render_overlay_eye_button(
                                                        ui,
                                                        language,
                                                        step,
                                                        group.id,
                                                        preset.id,
                                                        step_index,
                                                        false,
                                                        &self.state.hud_presets,
                                                        &mut self.draw_geometry_step_preview_target,
                                                        &self.overlay_tx,
                                                        [18.0, 18.0],
                                                        12.0,
                                                    );
                                                } else if step.action == MacroAction::ApplyMouseSensitivityPreset {
                                                    live_sync |= ui.checkbox(&mut step.manual_mouse_sensitivity, Self::tr_lang(language, "Manual", "Nhập tay")).changed();
                                                    if step.manual_mouse_sensitivity {
                                                         ui.vertical(|ui| {
                                                             let response = Self::render_variable_text_edit(
                                                                 ui,
                                                                 &mut step.key,
                                                                 ui.id().with("sensitivity-manual-key-hold-stop"),
                                                                 96.0,
                                                                 160.0,
                                                                 21.0,
                                                                 21.0,
                                                                 &Self::tr_lang(language, "value/expr", "giá trị"),
                                                                 false,
                                                             );
                                                            Self::apply_vietnamese_input_if_changed(
                                                                &response,
                                                                self.state.vietnamese_input_enabled,
                                                                self.state.vietnamese_input_mode,
                                                                &mut step.key,
                                                            );
                                                            live_sync |= response.changed();
                                                            let interpolated = crate::overlay::interpolate_variables(&step.key);
                                                            let evaluated = crate::overlay::evaluate_math_expression(&interpolated);
                                                            let clamped = evaluated.clamp(1, 20);
                                                            let tooltip_text = match language {
                                                                UiLanguage::Vietnamese => format!("Kết quả: {} (giới hạn: {} trong 1..20)", evaluated, clamped),
                                                                _ => format!("Evaluated: {} (clamped to: {} within 1..20)", evaluated, clamped),
                                                            };
                                                            let response = response.on_hover_text(tooltip_text);
                                                            Self::render_variable_suggestions(ui, &response, &mut step.key, &timer_names, language);
                                                        });
                                                    } else {
                                                        let selected_id = step.key.trim().parse::<u32>().ok();
                                                        let selected_label = selected_id
                                                            .and_then(|id| {
                                                                self.state
                                                                    .mouse_sensitivity_presets
                                                                    .iter()
                                                                    .find(|preset| preset.id == id)
                                                                    .map(|preset| preset.name.clone())
                                                            })
                                                            .unwrap_or_else(|| {
                                                                Self::tr_lang(language, "Select sens", "Chọn độ nhạy")
                                                                .to_owned()
                                                            });
                                                        egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "mouse-sensitivity-preset-step"))
                                                            .width(96.0)
                                                            .selected_text(selected_label)
                                                            .show_ui(ui, |ui| {
                                                                for preset_option in &self.state.mouse_sensitivity_presets {
                                                                    if ui
                                                                        .selectable_label(
                                                                            selected_id == Some(preset_option.id),
                                                                            &preset_option.name,
                                                                        )
                                                                        .clicked()
                                                                    {
                                                                        step.key = preset_option.id.to_string();
                                                                        live_sync = true;
                                                                    }
                                                                }
                                                            });
                                                    }
                                                } else if step.action == MacroAction::UnlockKeys {
                                                    let capture_target = CaptureRequest::MacroStepInput {
                                                        group_id: group.id,
                                                        preset_id: preset.id,
                                                        step_index,
                                                        extra_cond_index: None,
                                                    };
                                                    let active = capture_target_snapshot == Some(capture_target.clone());
                                                    Self::render_key_capture_chips(
                                                        ui,
                                                        language,
                                                        &mut step.key,
                                                        active,
                                                        || {
                                                            if active {
                                                                cancel_active_capture = true;
                                                            } else {
                                                                next_capture_target = Some(capture_target.clone());
                                                            }
                                                        },
                                                        || {
                                                            live_sync = true;
                                                        },
                                                    );
                                                } else if step.action == MacroAction::LockKeys {
                                                    let capture_target = CaptureRequest::MacroStepInput {
                                                        group_id: group.id,
                                                        preset_id: preset.id,
                                                        step_index,
                                                        extra_cond_index: None,
                                                    };
                                                    let active = capture_target_snapshot == Some(capture_target.clone());
                                                    Self::render_key_capture_chips(
                                                        ui,
                                                        language,
                                                        &mut step.key,
                                                        active,
                                                        || {
                                                            if active {
                                                                cancel_active_capture = true;
                                                            } else {
                                                                next_capture_target = Some(capture_target.clone());
                                                            }
                                                        },
                                                        || {
                                                            live_sync = true;
                                                        },
                                                    );
                                                    ui.add_space(4.0);
                                                    let unlock_resp = ui.checkbox(&mut step.unlock_on_exit, Self::tr_lang(language, "Unlock when macro ends", ""));
                                                    if unlock_resp.changed() {
                                                        live_sync = true;
                                                    }
                                                } else if step.action == MacroAction::LoopStart {
                                                     let mut infinite = Self::loop_is_infinite(step);
                                                     if ui
                                                         .checkbox(
                                                             &mut infinite,
                                                             RichText::new(Self::tr_lang(
                                                                 language,
                                                                 "Infinite",
                                                                 "Infinite",
                                                             ))
                                                             .color(Color32::WHITE),
                                                         )
                                                         .changed()
                                                     {
                                                         step.key = if infinite {
                                                             "infinite".to_owned()
                                                         } else {
                                                             "1".to_owned()
                                                         };
                                                         live_sync = true;
                                                      }
                                                      if !infinite {
                                                          ui.vertical(|ui| {
                                                              let key_id = ui.id().with((step_index, "loop-count"));
                                                              let response = Self::render_variable_text_edit(
                                                                  ui,
                                                                  &mut step.key,
                                                                  key_id,
                                                                  80.0,
                                                                  180.0,
                                                                    21.0, 21.0,
                                                                  &Self::tr_lang(language, "Loop count", "Số lần lặp"),
                                                                  false,
                                                              );
                                                              Self::apply_vietnamese_input_if_changed(
                                                                  &response,
                                                                  self.state.vietnamese_input_enabled,
                                                                  self.state.vietnamese_input_mode,
                                                                  &mut step.key,
                                                              );
                                                              live_sync |= response.changed();
                                                              Self::render_variable_suggestions(ui, &response, &mut step.key, &timer_names, language);
                                                          });
                                                      }
                                                } else if step.action == MacroAction::StopIfKeyPressed {
                                                     ui.scope(|ui| {
                                                         ui.spacing_mut().item_spacing.x = 2.0;
                                                         ui.spacing_mut().interact_size.y = 18.0;
                                                         ui.spacing_mut().button_padding.y = 0.0;
                                                         ui.allocate_ui_with_layout(
                                                             vec2(ui.available_width(), 20.0),
                                                             egui::Layout::top_down(egui::Align::Min),
                                                             |ui| {
                                                             ui.horizontal(|ui| {
                                                                 let current_mode = step.get_break_loop_mode().to_string();
                                                                 let mode_label = match current_mode.as_str() {
                                                                     "VarCompare" => Self::tr_lang(language, "Var compare", "So sánh biến"),
                                                                     "StopKey" => Self::tr_lang(language, "Stop key", "Nút đã nhấn"),
                                                                     _ => Self::tr_lang(language, "Break Loop", "Ngắt lặp"),
                                                                 };
                                                                 egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "stop-loop-break-mode"))
                                                                     .width(100.0)
                                                                     .selected_text(mode_label)
                                                                     .show_ui(ui, |ui| {
                                                                         if ui.selectable_label(current_mode == "Immediate", Self::tr_lang(language, "Break Loop", "Ngắt lặp")).clicked() {
                                                                             step.break_loop_mode = "Immediate".to_string();
                                                                             step.break_loop_by_variable = false;
                                                                             live_sync = true;
                                                                         }
                                                                         if ui.selectable_label(current_mode == "VarCompare", Self::tr_lang(language, "Var compare", "So sánh biến")).clicked() {
                                                                             step.break_loop_mode = "VarCompare".to_string();
                                                                             step.break_loop_by_variable = true;
                                                                             live_sync = true;
                                                                         }
                                                                         if ui.selectable_label(current_mode == "StopKey", Self::tr_lang(language, "Stop key", "Nút đã nhấn")).clicked() {
                                                                             step.break_loop_mode = "StopKey".to_string();
                                                                             step.break_loop_by_variable = false;
                                                                             live_sync = true;
                                                                         }
                                                                     });
                                                                 let mode = step.get_break_loop_mode();
                                                                 if mode == "VarCompare" {
                                                                     let var_name_id = ui.id().with((step_index, "loop-break-var-name"));
                                                                     let response = Self::render_variable_text_edit(
                                                                         ui,
                                                                         &mut step.if_variable_name,
                                                                         var_name_id,
                                                                         64.0,
                                                                         140.0,
                                                                         21.0, 21.0,
                                                                         Self::tr_lang(language, "variable", "biến"),
                                                                         false,
                                                                     );
                                                                     Self::apply_vietnamese_input_if_changed(
                                                                         &response,
                                                                         self.state.vietnamese_input_enabled,
                                                                         self.state.vietnamese_input_mode,
                                                                         &mut step.if_variable_name,
                                                                     );
                                                                     live_sync |= response.changed();
                                                                     Self::render_variable_suggestions(ui, &response, &mut step.if_variable_name, &timer_names, language);
                                                                     egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "stop-loop-if-op"))
                                                                         .width(40.0)
                                                                         .selected_text(&step.if_operator)
                                                                         .show_ui(ui, |ui| {
                                                                             for op in &["==", ">", "<", ">=", "<=", "!="] {
                                                                                 if ui.selectable_label(step.if_operator == *op, *op).clicked() {
                                                                                     step.if_operator = op.to_string();
                                                                                     live_sync = true;
                                                                                 }
                                                                             }
                                                                         });
                                                                     let var_val_id = ui.id().with((step_index, "loop-break-var-val"));
                                                                     let response2 = Self::render_variable_text_edit(
                                                                         ui,
                                                                         &mut step.key,
                                                                         var_val_id,
                                                                         76.0,
                                                                         180.0,
                                                                         21.0, 21.0,
                                                                         Self::tr_lang(language, "value/expr", "giá trị"),
                                                                         false,
                                                                     );
                                                                     Self::apply_vietnamese_input_if_changed(
                                                                         &response2,
                                                                         self.state.vietnamese_input_enabled,
                                                                         self.state.vietnamese_input_mode,
                                                                         &mut step.key,
                                                                     );
                                                                     live_sync |= response2.changed();
                                                                     Self::render_variable_suggestions(ui, &response2, &mut step.key, &timer_names, language);
                                                                     let var_name = step.if_variable_name.trim();
                                                                     if !var_name.is_empty() {
                                                                         let current_val = crate::overlay::RUNTIME_VARIABLES.lock().get(var_name).copied();
                                                                         let val_str = current_val.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string());
                                                                         ui.add_space(2.0);
                                                                         ui.label(
                                                                             RichText::new(format!("({})", val_str))
                                                                                 .size(10.0)
                                                                                 .color(Color32::from_rgb(0, 191, 255))
                                                                         ).on_hover_text(Self::tr_lang(language, "Current runtime value", "Giá trị chạy hiện tại"));
                                                                     }
                                                                     let btn_plus_clicked = ui.scope(|ui| {
                                                                         ui.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
                                                                         ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                                                                          ui.add_sized([18.0, 18.0], egui::Button::new(egui::RichText::new("+").size(12.0).strong()))
                                                                             .on_hover_text(Self::tr_lang(language, "Add condition", "Thêm điều kiện"))
                                                                             .clicked()
                                                                     });
                                                                     if btn_plus_clicked.inner {
                                                                         step.extra_conditions.push(ExtraCondition::default());
                                                                         live_sync = true;
                                                                     }
                                                                 } else if mode == "StopKey" {
                                                                     let capture_target = CaptureRequest::MacroStepInput {
                                                                         group_id: group.id,
                                                                         preset_id: preset.id,
                                                                         step_index,
                                                                         extra_cond_index: None,
                                                                     };
                                                                     let active = capture_target_snapshot == Some(capture_target.clone());
                                                                     Self::render_key_capture_chips(
                                                                         ui,
                                                                         language,
                                                                         &mut step.key,
                                                                         active,
                                                                         || {
                                                                             if active {
                                                                                 cancel_active_capture = true;
                                                                             } else {
                                                                                 next_capture_target = Some(capture_target.clone());
                                                                             }
                                                                         },
                                                                         || {
                                                                             live_sync = true;
                                                                         },
                                                                     );
                                                                 }
                                                             });
                                                             if step.get_break_loop_mode() == "VarCompare" {
                                                                 Self::render_extra_conditions(
                                                                 ui,
                                                                 &mut step.extra_conditions,
                                                                 group.id,
                                                                 preset.id,
                                                                 step_index,
                                                                 &timer_names,
                                                                 &ocr_preset_options,
                                                                 &image_search_preset_options,
                                                                 &all_presets,
                                                                 &all_groups,
                                                                 language,
                                                                 &mut live_sync,
                                                                 &self.state.timer_presets,
                                                                 self.state.vietnamese_input_enabled,
                                                                 self.state.vietnamese_input_mode,
                                                                 &mut begin_mouse_move_absolute_capture_target,
                                                                 capture_target_snapshot.as_ref(),
                                                                 &mut next_capture_target,
                                                                 &mut cancel_active_capture,
                                                                  false,
                                                                 );
                                                             }
                                                         });
                                                     });
                                                } else if step.action == MacroAction::ShowHud {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .hud_presets
                                                                .iter()
                                                                .find(|preset| preset.id == id)
                                                                .map(|preset| preset.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            if step.key.trim().is_empty() {
                                                                Self::tr_lang(
                                                                    language,
                                                                    "Select HUD",
                                                                    "Chọn HUD",
                                                                )
                                                                .to_owned()
                                                            } else {
                                                                match language {
                                                                    UiLanguage::Vietnamese => format!("Cũ: {}", step.key),
                                                                    _ => format!("Legacy: {}", step.key),
                                                                }
                                                            }
                                                        });
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "toolbox-preset-step"))
                                                        .width(146.0)
                                                        .selected_text(selected_label)
                                                        .show_ui(ui, |ui| {
                                                            for toolbox_preset in &self.state.hud_presets {
                                                                if ui
                                                                    .selectable_label(
                                                                        selected_id == Some(toolbox_preset.id),
                                                                        &toolbox_preset.name,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    step.key = toolbox_preset.id.to_string();
                                                                    live_sync = true;
                                                                }
                                                            }
                                                        });
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                            Self::render_overlay_eye_button(
                                                                ui,
                                                                language,
                                                                step,
                                                                group.id,
                                                                preset.id,
                                                                step_index,
                                                                false,
                                                                &self.state.hud_presets,
                                                                &mut self.draw_geometry_step_preview_target,
                                                                &self.overlay_tx,
                                                                [18.0, 18.0],
                                                                12.0,
                                                            );
                                                            let text_id = ui.id().with((step_index, "showhud-text-override"));
                                                            let response = Self::render_variable_text_edit(
                                                                ui,
                                                                &mut step.text_override,
                                                                text_id,
                                                                122.0,
                                                                240.0,
                                                                  21.0, 21.0,
                                                                &Self::tr_lang(language, "Text override", "Ghi de van ban"),
                                                                false,
                                                            );
                                                            Self::apply_vietnamese_input_if_changed(
                                                                &response,
                                                                self.state.vietnamese_input_enabled,
                                                                self.state.vietnamese_input_mode,
                                                                &mut step.text_override,
                                                            );
                                                            live_sync |= response.changed();
                                                            Self::render_variable_suggestions(
                                                                ui,
                                                                &response,
                                                                &mut step.text_override,
                                                                &timer_names,
                                                                language,
                                                            );
                                                            let mut is_permanent = step.duration_expr.trim() == "0" || step.duration_expr.trim().is_empty();
                                                            let cb_response = ui.checkbox(&mut is_permanent, Self::tr_lang(language, "Permanent", "Vĩnh viễn"));
                                                            if cb_response.changed() {
                                                                if is_permanent {
                                                                    step.duration_expr = "0".to_string();
                                                                } else {
                                                                    step.duration_expr = "1500".to_string();
                                                                }
                                                                live_sync = true;
                                                            }
                                                            if !is_permanent {
                                                                let duration_id = ui.id().with((group.id, preset.id, step_index, "showhud-duration-expr"));
                                                                let duration_response = Self::render_variable_text_edit(
                                                                    ui,
                                                                    &mut step.duration_expr,
                                                                    duration_id,
                                                                    72.0,
                                                                    150.0,
                                                                      21.0, 21.0,
                                                                    "0",
                                                                    false,
                                                                );
                                                                ui.add_sized(
                                                                    [20.0, 21.0],
                                                                    egui::Label::new(
                                                                        egui::RichText::new("ms").weak(),
                                                                    ),
                                                                );
                                                                Self::apply_vietnamese_input_if_changed(
                                                                    &duration_response,
                                                                    self.state.vietnamese_input_enabled,
                                                                    self.state.vietnamese_input_mode,
                                                                    &mut step.duration_expr,
                                                                );
                                                                live_sync |= duration_response.changed();
                                                                Self::render_variable_suggestions(
                                                                    ui,
                                                                    &duration_response,
                                                                    &mut step.duration_expr,
                                                                    &timer_names,
                                                                    language,
                                                                );
                                                                let tooltip_text = Self::tr_lang(
                                                                    language,
                                                                    "Display duration (0 = show until macro/overlay ends, supports variables/math)",
                                                                    "Thời gian hiển thị (0 = hiện đến khi dừng macro/overlay, hỗ trợ biến/phép tính)",
                                                                );
                                                                if duration_response.hovered() {
                                                                    egui::show_tooltip_text(ui.ctx(), ui.layer_id(), duration_response.id, tooltip_text);
                                                                }
                                                            }                                                        });
                                                    });
                                                } else if step.action == MacroAction::TypeText {
                                                     ui.vertical(|ui| {
                                                         let response = Self::render_interpolated_text_edit(ui, &mut step.key, ui.id().with((step_index, "type-text-key")),
                                                             146.0,
                                                             260.0,
                                                                21.0, 36.0,
                                                             Self::tr_lang(language, "Text to type", "Văn bảnh cần gõ"),
                                                             true,
                                                         );
                                                         Self::apply_vietnamese_input_if_changed(
                                                             &response,
                                                             self.state.vietnamese_input_enabled,
                                                             self.state.vietnamese_input_mode,
                                                             &mut step.key,
                                                         );
                                                         live_sync |= response.changed();
                                                         Self::render_variable_suggestions_braced(ui, &response, &mut step.key, &timer_names, language);
                                                     });
                                                } else if step.action == MacroAction::DisableCrosshair {
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.horizontal(|ui| {
                                                            let response = ui.checkbox(&mut step.lock_mouse_left, Self::tr_lang(language, "All", ""));
                                                            live_sync |= response.changed();
                                                            if !step.lock_mouse_left {
                                                                let selected_label = if step.key.trim().is_empty() {
                                                                    Self::tr_lang(language, "Select profile", "Chọn profile").to_owned()
                                                                } else {
                                                                    step.key.clone()
                                                                };
                                                                egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "main-disable-crosshair"))
                                                                    .width(96.0)
                                                                    .selected_text(selected_label)
                                                                    .show_ui(ui, |ui| {
                                                                        for profile in &self.state.profiles {
                                                                            if ui
                                                                                .selectable_label(step.key == profile.name, &profile.name)
                                                                                .clicked()
                                                                            {
                                                                                step.key = profile.name.clone();
                                                                                live_sync = true;
                                                                            }
                                                                        }
                                                                    });
                                                            }
                                                        });
                                                    });
                                                } else if step.action == MacroAction::DisablePin {
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.horizontal(|ui| {
                                                            let response = ui.checkbox(&mut step.lock_mouse_left, Self::tr_lang(language, "All", ""));
                                                            live_sync |= response.changed();
                                                            if !step.lock_mouse_left {
                                                                let selected_id = step.key.trim().parse::<u32>().ok();
                                                                let selected_label = selected_id
                                                                    .and_then(|id| {
                                                                        self.state
                                                                            .pin_presets
                                                                            .iter()
                                                                            .find(|p| p.id == id)
                                                                            .map(|p| p.name.clone())
                                                                    })
                                                                    .unwrap_or_else(|| {
                                                                        Self::tr_lang(language, "Select pin", "Chọn preset ghim").to_owned()
                                                                    });
                                                                egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "main-disable-pin"))
                                                                    .width(96.0)
                                                                    .selected_text(selected_label)
                                                                    .show_ui(ui, |ui| {
                                                                        for preset_option in &self.state.pin_presets {
                                                                            if ui
                                                                                .selectable_label(selected_id == Some(preset_option.id), &preset_option.name)
                                                                                .clicked()
                                                                            {
                                                                                step.key = preset_option.id.to_string();
                                                                                live_sync = true;
                                                                            }
                                                                        }
                                                                    });
                                                            }
                                                        });
                                                    });
                                                } else if matches!(step.action, MacroAction::Else | MacroAction::IfEnd) {
                                                     ui.add_sized(
                                                         [146.0, 21.0],
                                                         egui::Label::new(""),
                                                     );
                                                } else if matches!(step.action, MacroAction::DisableZoom | MacroAction::HideHud) {
                                                     ui.add_sized(
                                                         [146.0, 21.0],
                                                         egui::Label::new(Self::tr_lang(language, "No input", "No input")),
                                                     );
                                                } else if step.action == MacroAction::LockMouse {
                                                    Self::render_mouse_move_direction_controls(
                                                        ui,
                                                        step,
                                                        language,
                                                        &mut live_sync,
                                                        true,
                                                        "lockmouse-warning-tip",
                                                    );
                                                } else if step.action == MacroAction::UnlockMouse {
                                                    Self::render_mouse_move_direction_controls(
                                                        ui,
                                                        step,
                                                        language,
                                                        &mut live_sync,
                                                        false,
                                                        "unlockmouse-warning-tip",
                                                    );
                                                } else if step.action == MacroAction::IfStart {
                                                      ui.scope(|ui| {
                                                          ui.spacing_mut().item_spacing.x = 2.0;
                                                          ui.spacing_mut().interact_size.y = 18.0;
                                                          ui.spacing_mut().button_padding.y = 0.0;
                                                          ui.allocate_ui_with_layout(
                                                              vec2(ui.available_width(), 20.0),
                                                              egui::Layout::top_down(egui::Align::Min),
                                                              |ui| {
                                                              ui.horizontal(|ui| {
                                                                    ui.add_sized(
                                                                        [28.0, 20.0],
                                                                        egui::Label::new(Self::tr_lang(language, "IF", "NẾU")),
                                                                    );
                                                                    let cond_text = match step.if_condition_type {
                                                                         IfConditionType::Variable => Self::tr_lang(language, "Variable", "Biến"),
                                                                         IfConditionType::PixelColor => Self::tr_lang(language, "Pixel Color", "Màu điểm"),
                                                                         IfConditionType::VisionMatch => Self::tr_lang(language, "Vision Match", "Hình ảnh"),
                                                                         IfConditionType::KeyHeld => Self::tr_lang(language, "Input Held", "Giữ phím/chuột"),
                                                                         IfConditionType::MouseHeld => Self::tr_lang(language, "Input Held", "Giữ phím/chuột"),
                                                                         IfConditionType::MousePosition => Self::tr_lang(language, "Mouse Position", "Tọa độ chuột"),
                                                                         IfConditionType::PresetRunning => Self::tr_lang(language, "Preset Running", "Preset đang chạy"),
                                                                         IfConditionType::OcrMatch => Self::tr_lang(language, "OCR Match", "Từ tìm (OCR)"),
                                                                         _ => "-",
                                                                     };
                                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "if-condition-type"))
                                                                        .width(100.0)
                                                                        .selected_text(cond_text)
                                                                        .show_ui(ui, |ui| {
                                                                            let options = [
                                                                                (IfConditionType::Variable, Self::tr_lang(language, "Variable", "Biến")),
                                                                                (IfConditionType::PixelColor, Self::tr_lang(language, "Pixel Color", "Màu điểm")),
                                                                                (IfConditionType::VisionMatch, Self::tr_lang(language, "Vision Match", "Hình ảnh")),
                                                                                (IfConditionType::KeyHeld, Self::tr_lang(language, "Input Held", "Giữ phím/chuột")),
                                                                                (IfConditionType::MousePosition, Self::tr_lang(language, "Mouse Position", "Tọa độ chuột")),
                                                                                (IfConditionType::PresetRunning, Self::tr_lang(language, "Preset Running", "Preset đang chạy")),
                                                                            ];
                                                                            for (opt_type, opt_label) in options {
                                                                                if ui.selectable_label(step.if_condition_type == opt_type, opt_label).clicked() {
                                                                                    step.if_condition_type = opt_type;
                                                                                    live_sync = true;
                                                                                }
                                                                            }
                                                                        });
                                                                    if step.if_condition_type == IfConditionType::Variable {
                                                                     let var_name_id = ui.id().with((step_index, "regular-if-var-name"));
                                                                    let response = Self::render_variable_text_edit(
                                                                        ui,
                                                                        &mut step.if_variable_name,
                                                                        var_name_id,
                                                                        76.0,
                                                                        140.0,
                                                                        21.0, 21.0,
                                                                        Self::tr_lang(language, "value/expr", "biến/expr"),
                                                                        false,
                                                                    );
                                                                    Self::apply_vietnamese_input_if_changed(
                                                                        &response,
                                                                        self.state.vietnamese_input_enabled,
                                                                        self.state.vietnamese_input_mode,
                                                                        &mut step.if_variable_name,
                                                                    );
                                                                    live_sync |= response.changed();
                                                                    Self::render_variable_suggestions(ui, &response, &mut step.if_variable_name, &timer_names, language);
                                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "if-op"))
                                                                        .width(55.0)
                                                                        .selected_text(&step.if_operator)
                                                                        .show_ui(ui, |ui| {
                                                                            for op in &["==", ">", "<", ">=", "<=", "!="] {
                                                                                if ui.selectable_label(step.if_operator == *op, *op).clicked() {
                                                                                    step.if_operator = op.to_string();
                                                                                    live_sync = true;
                                                                                }
                                                                            }
                                                                        });
                                                                    let var_val_id = ui.id().with((step_index, "regular-if-var-val"));
                                                                    let response2 = Self::render_variable_text_edit(
                                                                        ui,
                                                                        &mut step.key,
                                                                        var_val_id,
                                                                        76.0,
                                                                        180.0,
                                                                        21.0, 21.0,
                                                                        Self::tr_lang(language, "value/expr", "giá trị/expr"),
                                                                        false,
                                                                    );
                                                                    Self::apply_vietnamese_input_if_changed(
                                                                        &response2,
                                                                        self.state.vietnamese_input_enabled,
                                                                        self.state.vietnamese_input_mode,
                                                                        &mut step.key,
                                                                    );
                                                                    live_sync |= response2.changed();
                                                                    Self::render_variable_suggestions(ui, &response2, &mut step.key, &timer_names, language);
                                                                       let left_expr = step.if_variable_name.trim();
                                                                       if !left_expr.is_empty() {
                                                                           let left_val = crate::overlay::evaluate_interpolated_math_expression(left_expr);
                                                                           ui.add_space(2.0);
                                                                           ui.label(
                                                                               RichText::new(format!("({})", left_val))
                                                                                   .size(10.0)
                                                                                   .color(Color32::from_rgb(0, 191, 255))
                                                                           ).on_hover_text(Self::tr_lang(language, "Evaluated left expression", "Gia tri bieu thuc ben trai"));
                                                                       }
                                                                    } else if step.if_condition_type == IfConditionType::PixelColor {
                                                                        ui.label("X:");
                                                                        let resp_x = ui.add(egui::DragValue::new(&mut step.x));
                                                                        live_sync |= resp_x.changed();
                                                                        ui.label("Y:");
                                                                        let resp_y = ui.add(egui::DragValue::new(&mut step.y));
                                                                        live_sync |= resp_y.changed();
                                                                        let resp_col = Self::render_plain_text_edit(ui, &mut step.if_target_color, ui.id().with("regular-if-target-color"), 64.0, 100.0, 21.0, 21.0, "#RRGGBB", false);
                                                                        live_sync |= resp_col.changed();
                                                                        // Swatch màu trực quan
                                                                        let swatch_color = Self::parse_rgb_color(&step.if_target_color).unwrap_or(egui::Color32::TRANSPARENT);
                                                                        let (rect, _response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                                                        ui.painter().rect_filled(rect, 3.0, swatch_color);
                                                                        ui.label(Self::tr_lang(language, "Tol:", "Sai số:"));
                                                                        let resp_tol = ui.add(egui::DragValue::new(&mut step.if_color_tolerance).range(0..=255));
                                                                        live_sync |= resp_tol.changed();
                                                                        // Nút pick màu + vị trí từ màn hình
                                                                        let pick_btn = ui.add_sized(
                                                                            [18.0, 18.0],
                                                                            egui::Button::new(Self::material_icon_text(0xe3b4, 14.0)),
                                                                        ).on_hover_text(Self::tr_lang(language, "Click on screen to pick color & position", "Bấm vào màn hình để lấy màu và tọa độ"));
                                                                        if pick_btn.clicked() {
                                                                            begin_mouse_move_absolute_capture_target = Some(MouseMoveAbsoluteCaptureTarget {
                                                                                group_id: Some(group.id),
                                                                                preset_id: preset.id,
                                                                                step_index,
                                                                                capture_kind: MouseCaptureKind::IfStartPixelColor,
                                                                                extra_cond_index: None,
                                                                                is_hold_stop: false,
                                                                            });
                                                                        }
                                                                    } else if step.if_condition_type == IfConditionType::VisionMatch {
                                                                        let selected_id = step.if_vision_preset_id;
                                                                        let selected_label = selected_id
                                                                            .and_then(|id| {
                                                                                self.state.vision_presets.iter().find(|p| p.id == id).map(|p| p.name.clone())
                                                                            })
                                                                            .unwrap_or_else(|| Self::tr_lang(language, "Select preset", "Chọn preset").to_owned());
                                                                        egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "if-vision-preset"))
    .width(146.0)

    .selected_text(selected_label)

    .show_ui(ui, |ui| {

                    let (image_presets, color_presets): (Vec<_>, Vec<_>) = self.state.vision_presets.iter().filter(|p| !p.is_pixel_counter).fold(
                        (Vec::new(), Vec::new()),
                        |(mut img, mut col), p| {
                            if p.use_color_matching {
                                col.push(p);
                            } else {
                                img.push(p);
                            }
                            (img, col)
                        }
                    );
                    if !image_presets.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0, 191, 255), "Image Detect");
                        ui.separator();
                        for p in &image_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.if_vision_preset_id = Some(p.id);
                                live_sync = true;
                            }
                        }
                    }
                    if !color_presets.is_empty() {
                        if !image_presets.is_empty() {
                            ui.add_space(4.0);
                        }
                        ui.colored_label(egui::Color32::from_rgb(0, 250, 154), "Color Detect");
                        ui.separator();
                        for p in &color_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.if_vision_preset_id = Some(p.id);
                                live_sync = true;
                            }
                        }
                    }
    });

                                                                    } else if step.if_condition_type == IfConditionType::KeyHeld {
                                                                        let capture_target = CaptureRequest::MacroStepInput {
                                                                            group_id: group.id,
                                                                            preset_id: preset.id,
                                                                            step_index,
                                                                            extra_cond_index: None,
                                                                        };
                                                                        let active = capture_target_snapshot.as_ref() == Some(&capture_target);
                                                                        Self::render_multi_key_capture_chips(
                                                                            ui,
                                                                            language,
                                                                            &mut step.key,
                                                                            active,
                                                                            || {
                                                                                if active {
                                                                                    cancel_active_capture = true;
                                                                                } else {
                                                                                    next_capture_target = Some(capture_target.clone());
                                                                                }
                                                                            },
                                                                            || {
                                                                                live_sync = true;
                                                                            }
                                                                        );
                                                                    } else if step.if_condition_type == IfConditionType::MousePosition {
                                                                        egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "if-mouse-axis"))
                                                                            .width(50.0)
                                                                            .selected_text(&step.if_mouse_axis)
                                                                            .show_ui(ui, |ui| {
                                                                                for axis in &["X", "Y"] {
                                                                                    if ui.selectable_label(step.if_mouse_axis == *axis, *axis).clicked() {
                                                                                        step.if_mouse_axis = axis.to_string();
                                                                                        live_sync = true;
                                                                                    }
                                                                                }
                                                                            });
                                                                        egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "if-mouse-pos-op"))
                                                                            .width(40.0)
                                                                            .selected_text(&step.if_operator)
                                                                            .show_ui(ui, |ui| {
                                                                                for op in &["==", ">", "<", ">=", "<=", "!="] {
                                                                                    if ui.selectable_label(step.if_operator == *op, *op).clicked() {
                                                                                        step.if_operator = op.to_string();
                                                                                        live_sync = true;
                                                                                    }
                                                                                }
                                                                            });
                                                                        let mut variable_layouter = |ui: &egui::Ui, string: &dyn TextBuffer, wrap_width: f32| {
                                                                            let job = Self::interpolation_highlight_job(
                                                                                ui,
                                                                                string.as_str(),
                                                                                wrap_width,
                                                                                egui::TextStyle::Body,
                                                                            );
                                                                            ui.fonts_mut(|fonts| fonts.layout_job(job))
                                                                        };
                                                                        let response2 = Self::render_variable_text_edit(ui, &mut step.key, ui.id().with("regular-ocr-search-val"), 76.0, 150.0, 21.0, 21.0, &Self::tr_lang(language, "value/expr", "giá trị/expr"), false);
                                                                        Self::apply_vietnamese_input_if_changed(
                                                                            &response2,
                                                                            self.state.vietnamese_input_enabled,
                                                                            self.state.vietnamese_input_mode,
                                                                            &mut step.key,
                                                                        );
                                                                        live_sync |= response2.changed();
                                                                        Self::render_variable_suggestions(ui, &response2, &mut step.key, &timer_names, language);
                                                                        // Nút pick tọa độ chuột từ màn hình
                                                                        let pick_btn = ui.add_sized(
                                                                            [18.0, 18.0],
                                                                            egui::Button::new(Self::material_icon_text(0xe55c, 14.0)),
                                                                        ).on_hover_text(Self::tr_lang(language, "Click on screen to capture coordinate", "Bấm vào màn hình để lấy tọa độ"));
                                                                        if pick_btn.clicked() {
                                                                            begin_mouse_move_absolute_capture_target = Some(MouseMoveAbsoluteCaptureTarget {
                                                                                group_id: Some(group.id),
                                                                                preset_id: preset.id,
                                                                                step_index,
                                                                                capture_kind: MouseCaptureKind::IfStartMousePos,
                                                                                extra_cond_index: None,
                                                                                is_hold_stop: false,
                                                                            });
                                                                        }
                                                                    } else if step.if_condition_type == IfConditionType::PresetRunning {
                                                                        // Bước 1: chọn group
                                                                        let sel_group_id = step.if_running_preset_group_id;
                                                                        let sel_group_label = sel_group_id
                                                                            .and_then(|gid| all_groups.iter().find(|(id, _, _)| *id == gid).map(|(_, name, _)| name.clone()))
                                                                            .unwrap_or_else(|| Self::tr_lang(language, "Group", "Nhóm").to_owned());
                                                                        egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "if-running-group"))
                                                                            .width(100.0)
                                                                            .selected_text(sel_group_label)
                                                                            .show_ui(ui, |ui| {
                                                                                for (gid, gname, _) in &all_groups {
                                                                                    if ui.selectable_label(sel_group_id == Some(*gid), gname).clicked() {
                                                                                        step.if_running_preset_group_id = Some(*gid);
                                                                                        step.if_running_preset_id = None;
                                                                                        live_sync = true;
                                                                                    }
                                                                                }
                                                                            });
                                                                        // Bước 2: chọn preset trong group đó
                                                                        let group_presets: &[(u32, String)] = if let Some(gid) = sel_group_id {
                                                                            all_groups.iter()
                                                                                .find(|(id, _, _)| *id == gid)
                                                                                .map(|(_, _, presets)| presets.as_slice())
                                                                                .unwrap_or(&[])
                                                                        } else {
                                                                            &[]
                                                                        };
                                                                        let selected_id = step.if_running_preset_id;
                                                                        let selected_label = selected_id
                                                                            .and_then(|id| {
                                                                                if id == 0 {
                                                                                    Some(Self::tr_lang(language, "Any Preset", "Bất kỳ").to_owned())
                                                                                } else {
                                                                                    group_presets.iter().find(|(pid, _)| *pid == id).map(|(_, name)| name.clone())
                                                                                }
                                                                            })
                                                                            .unwrap_or_else(|| Self::tr_lang(language, "Preset", "Preset").to_owned());
                                                                        egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "if-running-preset"))
                                                                            .width(120.0)
                                                                            .selected_text(selected_label)
                                                                            .show_ui(ui, |ui| {
                                                                                if ui.selectable_label(selected_id == Some(0), Self::tr_lang(language, "Any Preset", "Bất kỳ")).clicked() {
                                                                                    step.if_running_preset_id = Some(0);
                                                                                    live_sync = true;
                                                                                }
                                                                                for (pid, pname) in group_presets {
                                                                                    if ui.selectable_label(selected_id == Some(*pid), pname).clicked() {
                                                                                        step.if_running_preset_id = Some(*pid);
                                                                                        live_sync = true;
                                                                                    }
                                                                                }
                                                                            });
                                                                    } else if step.if_condition_type == IfConditionType::OcrMatch {
                                                                        let selected_id = step.if_ocr_preset_id;
                                                                        let selected_label = selected_id
                                                                            .and_then(|id| {
                                                                                ocr_preset_options
                                                                                    .iter()
                                                                                    .find(|(preset_id, _)| *preset_id == id)
                                                                                    .map(|(_, label)| label.clone())
                                                                            })
                                                                            .unwrap_or_else(|| Self::tr_lang(language, "Select OCR", "Chọn OCR").to_owned());
                                                                        egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "ocr-preset-cb-if"))
                                                                            .width(146.0)
                                                                            .selected_text(selected_label)
                                                                            .show_ui(ui, |ui| {
                                                                                for (preset_option_id, preset_option_label) in &ocr_preset_options {
                                                                                    if ui
                                                                                        .selectable_label(selected_id == Some(*preset_option_id), preset_option_label)
                                                                                        .clicked()
                                                                                    {
                                                                                        step.if_ocr_preset_id = Some(*preset_option_id);
                                                                                        live_sync = true;
                                                                                    }
                                                                                }
                                                                            });
                                                                        let var_target_id = ui.id().with("ocr-target-text-if");
                                                                        let response_target = Self::render_variable_text_edit(
                                                                            ui,
                                                                            &mut step.ocr_target_text,
                                                                            var_target_id,
                                                                            76.0,
                                                                            180.0,
                                                                            21.0, 21.0,
                                                                            Self::tr_lang(language, "Target text", "Van ban can tim"),
                                                                            false,
                                                                        );
                                                                        Self::apply_vietnamese_input_if_changed(
                                                                            &response_target,
                                                                            self.state.vietnamese_input_enabled,
                                                                            self.state.vietnamese_input_mode,
                                                                            &mut step.ocr_target_text,
                                                                        );
                                                                        live_sync |= response_target.changed();
                                                                        Self::render_variable_suggestions(ui, &response_target, &mut step.ocr_target_text, &timer_names, language);
                                                                    }
                                                                     let btn_plus_clicked = ui.scope(|ui| {
                                                                         ui.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
                                                                         ui.spacing_mut().interact_size = egui::vec2(18.0, 18.0);
                                                                          ui.add_sized([18.0, 18.0], egui::Button::new(egui::RichText::new("+").size(12.0).strong()))
                                                                             .on_hover_text(Self::tr_lang(language, "Add condition", "Thêm điều kiện"))
                                                                             .clicked()
                                                                     });
                                                                     if btn_plus_clicked.inner {
                                                                         step.extra_conditions.push(ExtraCondition::default());
                                                                         live_sync = true;
                                                                     }
                                                                });
                                                              Self::render_extra_conditions(
                                                              ui,
                                                              &mut step.extra_conditions,
                                                              group.id,
                                                              preset.id,
                                                              step_index,
                                                              &timer_names,
                                                              &ocr_preset_options,
                                                              &image_search_preset_options,
                                                              &all_presets,
                                                              &all_groups,
                                                              language,
                                                              &mut live_sync,
                                                              &self.state.timer_presets,
                                                              self.state.vietnamese_input_enabled,
                                                              self.state.vietnamese_input_mode,
                                                              &mut begin_mouse_move_absolute_capture_target,
                                                              capture_target_snapshot.as_ref(),
                                                              &mut next_capture_target,
                                                              &mut cancel_active_capture,
                                                                false,
                                                            );
                                                        });
                                            if step.action != previous_action
                                                && matches!(step.action, MacroAction::ScanVisionOnce)
                                            {
                                                step.vision_move_cursor_on_match = false;
                                            }
                                                      });} else if step.action == MacroAction::SetVariable {
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.vertical(|ui| {
                                                            ui.horizontal(|ui| {
                                                                  let var_name_id = ui.id().with((step_index, "regular-set-var-name"));
                                                                  let response = Self::render_variable_text_edit(
                                                                      ui,
                                                                      &mut step.if_variable_name,
                                                                      var_name_id,
                                                                      76.0,
                                                                      140.0,
                                                                      21.0, 21.0,
                                                                      Self::tr_lang(language, "variable", "biến"),
                                                                      false,
                                                                  );
                                                                  Self::apply_vietnamese_input_if_changed(
                                                                      &response,
                                                                      self.state.vietnamese_input_enabled,
                                                                      self.state.vietnamese_input_mode,
                                                                      &mut step.if_variable_name,
                                                                  );
                                                                  live_sync |= response.changed();
                                                                  ui.label(" = ");
                                                                  let var_val_id = ui.id().with((step_index, "regular-set-var-val"));
                                                                  let response2 = Self::render_plain_text_edit(
                                                                      ui,
                                                                      &mut step.key,
                                                                      var_val_id,
                                                                      76.0,
                                                                      180.0,   21.0, 21.0,
                                                                      Self::tr_lang(language, "value/expr", "giá trị"),
                                                                      false,
                                                                  );
                                                                  Self::apply_vietnamese_input_if_changed(
                                                                      &response2,
                                                                      self.state.vietnamese_input_enabled,
                                                                      self.state.vietnamese_input_mode,
                                                                      &mut step.key,
                                                                  );
                                                                  live_sync |= response2.changed();
                                                                Self::render_variable_suggestions_raw(ui, &response, &mut step.if_variable_name, &timer_names, language);
                                                                Self::render_variable_suggestions(ui, &response2, &mut step.key, &timer_names, language);
                                                                let var_name = step.if_variable_name.trim();
                                                                if !var_name.is_empty() {
                                                                    let current_val = crate::overlay::RUNTIME_VARIABLES.lock().get(var_name).copied();
                                                                    let val_str = current_val.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string());
                                                                    ui.add_space(2.0);
                                                                    ui.label(
                                                                        RichText::new(format!("({})", val_str))
                                                                            .size(10.0)
                                                                            .color(Color32::from_rgb(0, 191, 255))
                                                                    ).on_hover_text(Self::tr_lang(language, "Current runtime value", "Giá trị chạy hiện tại"));
                                                                }
                                                            });
                                                        });
                                                    });
                                                } else if step.action == MacroAction::JumpToStep {
                                                    ui.scope(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        ui.spacing_mut().interact_size.y = 18.0;
                                                        ui.spacing_mut().button_padding.y = 0.0;
                                                        ui.horizontal(|ui| {
                                                            let key_id = ui.id().with((step_index, "jump-to-step"));
                                                            let response = Self::render_variable_text_edit(
                                                                ui,
                                                                &mut step.key,
                                                                key_id,
                                                                146.0,
                                                                260.0,
                                                                  21.0, 21.0,
                                                                &Self::tr_lang(language, "Step number or expression", "Số thứ tự bước hoặc biểu thức"),
                                                                false,
                                                            );
                                                            Self::apply_vietnamese_input_if_changed(
                                                                &response,
                                                                self.state.vietnamese_input_enabled,
                                                                self.state.vietnamese_input_mode,
                                                                &mut step.key,
                                                            );
                                                            live_sync |= response.changed();
                                                            Self::render_variable_suggestions(ui, &response, &mut step.key, &timer_names, language);
                                                        });
                                                    });
                                                } else if matches!(step.action, MacroAction::StartVisionSearch
                                                         | MacroAction::ScanVisionOnce
                                                         | MacroAction::StopVision | MacroAction::StopVisionWait) {
                                                    let selected_id = step.key.trim().parse::<u32>().ok();
                                                    let selected_label = selected_id
                                                        .and_then(|id| {
                                                            self.state
                                                                .vision_presets
                                                                .iter()
                                                                .find(|p| p.id == id)
                                                                .map(|p| p.name.clone())
                                                        })
                                                        .unwrap_or_else(|| {
                                                            if step.key.trim().is_empty() {
                                                                Self::tr_lang(
                                                                    language,
                                                                    "Select vision",
                                                                    "Chọn hiển thị",
                                                                )
                                                                .to_owned()
                                                            } else {
                                                                format!("ID: {}", step.key)
                                                            }
                                                        });
                                                    egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "vision-preset-step"))
    .width(146.0)

    .selected_text(selected_label)

    .show_ui(ui, |ui| {

                    let (image_presets, color_presets, pixel_presets): (Vec<_>, Vec<_>, Vec<_>) = self.state.vision_presets.iter().fold(
                        (Vec::new(), Vec::new(), Vec::new()),
                        |(mut img, mut col, mut pix), p| {
                            if p.is_pixel_counter {
                                pix.push(p);
                            } else if p.use_color_matching {
                                col.push(p);
                            } else {
                                img.push(p);
                            }
                            (img, col, pix)
                        }
                    );
                    if !image_presets.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0, 191, 255), "Image Detect");
                        ui.separator();
                        for p in &image_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.key = p.id.to_string();
                                live_sync = true;
                            }
                        }
                    }
                    if !color_presets.is_empty() {
                        if !image_presets.is_empty() {
                            ui.add_space(4.0);
                        }
                        ui.colored_label(egui::Color32::from_rgb(0, 250, 154), "Color Detect");
                        ui.separator();
                        for p in &color_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.key = p.id.to_string();
                                live_sync = true;
                            }
                        }
                    }
                    let show_pixel_counter = step.action == MacroAction::ScanVisionOnce;
                    if show_pixel_counter && !pixel_presets.is_empty() {
                        if !image_presets.is_empty() || !color_presets.is_empty() {
                            ui.add_space(4.0);
                        }
                        ui.colored_label(egui::Color32::from_rgb(255, 165, 0), "Pixel Counter");
                        ui.separator();
                        for p in &pixel_presets {
                            if ui.selectable_label(selected_id == Some(p.id), &p.name).clicked() {
                                step.key = p.id.to_string();
                                live_sync = true;
                            }
                        }
                    }
    });

                                                     let selected_preset = selected_id.and_then(|id| {
                                                         self.state.vision_presets.iter().find(|p| p.id == id)
                                                     });
                                                     let is_pixel = selected_preset.map(|p| p.is_pixel_counter).unwrap_or(false);
                                                     let is_single_pixel = selected_preset.map(|p| p.use_color_matching && p.search_region_is_single_pixel && !p.is_pixel_counter).unwrap_or(false);
                                                     let supports_move_mouse = selected_preset.is_some() && !is_pixel && !is_single_pixel;
                                                     if matches!(step.action, MacroAction::StartVisionSearch | MacroAction::StopVision) {
                                                         ui.add_space(4.0);
                                                         ui.weak(Self::tr_lang(language, "(Mouse move only)", "(Chức năng di chuột)"));
                                                     }
                                                                                                           if step.action == MacroAction::ScanVisionOnce {
                                                          ui.add_space(4.0);
                                                          if is_single_pixel {
                                                              ui.label(Self::tr_lang(language, "Color Var:", "Biến màu:"));
                                                              let prev_override = ui.visuals().override_text_color;
                                                              ui.visuals_mut().override_text_color = None;
                                                              let resp = ui.add(egui::TextEdit::singleline(&mut step.if_variable_name).hint_text("color_var").desired_width(100.0));
                                                              ui.visuals_mut().override_text_color = prev_override;
                                                              Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.if_variable_name);
                                                              live_sync |= resp.changed();
                                                          } else {
                                                              let outputs_label = Self::tr_lang(language, "Outputs", "Đầu ra").to_owned();
                                                              egui::ComboBox::from_id_salt((group.id, preset.id, step_index, "vision-outputs-reg"))
                                                                  .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                                                  .width(110.0)
                                                                  .selected_text(outputs_label)
                                                                  .show_ui(ui, |ui| {
                                                                      ui.set_min_width(200.0);
                                                                      egui::Grid::new("vision_outputs_grid_reg")
                                                                          .num_columns(2)
                                                                          .spacing([8.0, 6.0])
                                                                          .show(ui, |ui| {
                                                                              if !is_pixel {
                                                                                  ui.label("Found:");
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_found_var).hint_text("found_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_found_var);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                                  ui.label("Pos X:");
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_pos_var_x).hint_text("result_x_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_pos_var_x);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                                  ui.label("Pos Y:");
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_pos_var_y).hint_text("result_y_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_pos_var_y);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                              } else {
                                                                                  ui.label("Found:");
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.vision_found_var).hint_text("found_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.vision_found_var);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                                  ui.label(Self::tr_lang(language, "Count Var:", "Biến kết quả:"));
                                                                                  let prev_override = ui.visuals().override_text_color;
                                                                                  ui.visuals_mut().override_text_color = None;
                                                                                  let resp = ui.add(egui::TextEdit::singleline(&mut step.if_variable_name).hint_text("count_var"));
                                                                                  ui.visuals_mut().override_text_color = prev_override;
                                                                                  Self::apply_vietnamese_input_if_changed(&resp, self.state.vietnamese_input_enabled, self.state.vietnamese_input_mode, &mut step.if_variable_name);
                                                                                  live_sync |= resp.changed();
                                                                                  ui.end_row();
                                                                              }
                                                                          });
                                                                  });
                                                          }
                                                          if supports_move_mouse {
                                                              ui.add_space(4.0);
                                                              let resp = ui.checkbox(
                                                                  &mut step.vision_move_cursor_on_match,
                                                                  Self::tr_lang(language, "Move Mouse", "Di chuột"),
                                                              );
                                                              live_sync |= resp.changed();
                                                          }
                                                      }
                                                } else {
                                                    let step_capture_target = CaptureRequest::MacroStepInput {
                                                        group_id: group.id,
                                                        preset_id: preset.id,
                                                        step_index,
                                                        extra_cond_index: None,
                                                    };
                                                    let step_capture_active =
                                                        capture_target_snapshot.as_ref() == Some(&step_capture_target);
                                                    let mut display_key = if step_capture_active {
                                                        Self::tr_lang(
                                                            language,
                                                            "Capturing...",
                                                            "Đang lấy phím...",
                                                        ).to_owned()
                                                    } else {
                                                        step.key.clone()
                                                    };
                                                                          let response = if step_capture_active {
                                                        let text_edit = TextEdit::singleline(&mut display_key).hint_text(Self::tr_lang(
                                                            language,
                                                            "Capturing...",
                                                            "Đang lấy phím...",
                                                        ));
                                                        ui.add_sized([146.0, 21.0], text_edit)
                                                    } else {
                                                        let key_id = ui.id().with((step_index, "regular-default-key"));
                                                        Self::render_expandable_text_edit(
                                                            ui,
                                                            &mut display_key,
                                                            key_id,
                                                            146.0,
                                                            240.0,
                                                              21.0, 21.0,
                                                            "...",
                                                            false,
                                                        )
                                                    };
                                                    if !step_capture_active {
                                                        Self::apply_vietnamese_input_if_changed(
                                                            &response,
                                                            self.state.vietnamese_input_enabled,
                                                            self.state.vietnamese_input_mode,
                                                            &mut display_key,
                                                        );
                                                        if response.changed() || step.key != display_key {
                                                            step.key = display_key;
                                                            live_sync = true;
                                                        }
                                                    }
                                                }
                                            } else if matches!(
                                                step.action,
                                                MacroAction::DrawGeometry
                                                    | MacroAction::ShowGeometryPreset
                                                    | MacroAction::HideGeometryPreset
                                                    | MacroAction::StartAudioSensePreset
                                                    | MacroAction::StopAudioSense
                                            ) {
                                                ui.add_space(2.0);
                                            } else if Self::macro_action_uses_position(step.action) {
                                                ui.add_space(2.0);
                                            } else {
                                                ui.add_sized([146.0, 21.0], egui::Label::new("-"));
                                            }
                                            let action_uses_position =
                                                Self::macro_action_uses_position(step.action);
                                            if action_uses_position {
                                                if matches!(
                                                    step.action,
                                                    MacroAction::MouseMoveAbsolute
                                                        | MacroAction::MouseMoveRelative
                                                ) {
                                                    Self::ensure_coordinate_exprs(step);
                                                    let x_hint = if step.action
                                                        == MacroAction::MouseMoveRelative
                                                    {
                                                        "X delta or {var}"
                                                    } else {
                                                        "X value or {var}"
                                                    };
                                                    let y_hint = if step.action
                                                        == MacroAction::MouseMoveRelative
                                                    {
                                                        "Y delta or {var}"
                                                    } else {
                                                        "Y value or {var}"
                                                    };
                                                    let x_id =
                                                        ui.id().with((group.id, preset.id, step_index, "move-abs-x-18"));
                                                    let y_id =
                                                        ui.id().with((group.id, preset.id, step_index, "move-abs-y-18"));
                                                    let (x_response, y_response) =
                                                        Self::render_coordinate_compact_pair(
                                                            ui,
                                                            &mut step.x_expr,
                                                            x_id,
                                                            x_hint,
                                                            &mut step.y_expr,
                                                            y_id,
                                                            y_hint,
                                                            48.0,
                                                            20.0,
                                                        );
                                                    Self::apply_vietnamese_input_if_changed(
                                                        &x_response,
                                                        self.state.vietnamese_input_enabled,
                                                        self.state.vietnamese_input_mode,
                                                        &mut step.x_expr,
                                                    );
                                                    if x_response.changed() {
                                                        Self::sync_coordinate_expr_to_value(&step.x_expr, &mut step.x);
                                                        live_sync = true;
                                                    }
                                                    Self::render_variable_suggestions(
                                                        ui,
                                                        &x_response,
                                                        &mut step.x_expr,
                                                        &timer_names,
                                                        language,
                                                    );
                                                    Self::apply_vietnamese_input_if_changed(
                                                        &y_response,
                                                        self.state.vietnamese_input_enabled,
                                                        self.state.vietnamese_input_mode,
                                                        &mut step.y_expr,
                                                    );
                                                    if y_response.changed() {
                                                        Self::sync_coordinate_expr_to_value(&step.y_expr, &mut step.y);
                                                        live_sync = true;
                                                    }
                                                    Self::render_variable_suggestions(
                                                        ui,
                                                        &y_response,
                                                        &mut step.y_expr,
                                                        &timer_names,
                                                        language,
                                                    );
                                                }
                                                if step.action == MacroAction::MouseMoveAbsolute {
                                                    let capture_target = MouseMoveAbsoluteCaptureTarget {
                                                        group_id: Some(group.id),
                                                        preset_id: preset.id,
                                                        step_index,
                                                        capture_kind: MouseCaptureKind::MoveMouseAbsolute,
                                                        extra_cond_index: None,
                                                        is_hold_stop: false,
                                                    };
                                                    let capture_active = self
                                                        .mouse_move_absolute_capture_target
                                                        == Some(capture_target);
                                                    if ui
                                                        .add_sized(
                                                            [62.0, 18.0],
                                                            Button::new(Self::pick_point_button_text(
                                                                language,
                                                                capture_active,
                                                            )),
                                                        )
                                                        .on_hover_text(Self::tr_lang(
                                                            language,
                                                            "Minimize the app and click anywhere on screen to capture screen X/Y.",
                                                            "Thu nhỏ app rồi bấm vào bất kỳ vị trí nào trên màn hình để lấy X/Y.",
                                                        ))
                                                    .clicked()
                                                    {
                                                        if capture_active {
                                                            cancel_mouse_move_absolute_capture = true;
                                                        } else {
                                                            begin_mouse_move_absolute_capture_target =
                                                                Some(capture_target);
                                                        }
                                                    }
                                                }
                                            } else if step.action == MacroAction::PlayMousePathPreset {
                                                if step.mouse_speed_expr.trim().is_empty() {
                                                    step.mouse_speed_expr =
                                                        MacroStep::format_mouse_speed_multiplier(
                                                            step.get_mouse_speed_multiplier(),
                                                        );
                                                    live_sync = true;
                                                }
                                                live_sync |= ui
                                                    .checkbox(
                                                        &mut step.smooth_mouse_path,
                                                        Self::tr_lang(language, "Smooth", "Smooth"),
                                                    )
                                                    .on_hover_text(Self::tr_lang(
                                                        language,
                                                        "Replay the path with smooth constant-speed movement.",
                                                        "Di chuyển chuột với tốc độ đều",
                                                    ))
                                                    .changed();
                                                ui.add_space(6.0);
                                                ui.label(
                                                    RichText::new("⚡")
                                                        .strong()
                                                        .color(Color32::from_rgb(255, 210, 90)),
                                                )
                                                .on_hover_text(Self::tr_lang(
                                                    language,
                                                    "Smooth multiplier. Use values like x1, x1.2, x2, or {var}.",
                                                    "He so Smooth. Dung cac gia tri nhu x1, x1.2, x2, hoac {var}.",
                                                ));
                                                live_sync |= ui
                                                    .add_enabled_ui(step.smooth_mouse_path, |ui| {
                                                        let speed_id =
                                                            ui.id().with((step_index, "mouse-speed"));
                                                        let response = Self::render_interpolated_text_edit(
                                                            ui,
                                                            &mut step.mouse_speed_expr,
                                                            speed_id,
                                                            72.0,
                                                            120.0,
                                                              21.0, 21.0,
                                                            "x1",
                                                            false,
                                                        );
                                                        Self::apply_vietnamese_input_if_changed(
                                                            &response,
                                                            self.state.vietnamese_input_enabled,
                                                            self.state.vietnamese_input_mode,
                                                            &mut step.mouse_speed_expr,
                                                        );
                                                        if response.changed() {
                                                            if let Some(multiplier) =
                                                                MacroStep::resolve_mouse_speed_multiplier(
                                                                    &step.mouse_speed_expr,
                                                                )
                                                            {
                                                                step.mouse_speed_percent =
                                                                    (multiplier * 100.0)
                                                                        .round()
                                                                        .clamp(10.0, 10_000.0)
                                                                        as u32;
                                                            }
                                                        }
                                                        response
                                                    })
                                                    .inner
                                                    .on_hover_text(Self::tr_lang(
                                                        language,
                                                        "Only changes Smooth playback for this Mouse Path step. x1 = normal speed, x2 = 2x faster, x0.5 = half speed. Supports {var}.",
                                                        "Chi doi toc do Smooth cua step Mouse Path nay. x1 = toc do mac dinh, x2 = nhanh gap doi, x0.5 = mot nua. Ho tro {var}.",
                                                    ) )
                                        .changed();
                                } else if matches!(
                                                step.action,
                                                MacroAction::DrawGeometry
                                                    | MacroAction::ShowGeometryPreset
                                                    | MacroAction::HideGeometryPreset
                                                    | MacroAction::StartAudioSensePreset
                                                    | MacroAction::StopAudioSense
                                            ) {
                                                if matches!(
                                                    step.action,
                                                    MacroAction::DrawGeometry
                                                        | MacroAction::ShowGeometryPreset
                                                        | MacroAction::HideGeometryPreset
                                                ) {
                                                    Self::render_geometry_macro_step_editor(
                                                        ui,
                                                        language,
                                                        (group.id, preset.id, step_index, "normal-geometry-step"),
                                                        group.id,
                                                        preset.id,
                                                        step_index,
                                                        false,
                                                        &geometry_preset_options,
                                                        &mut geometry_manual_color,
                                                        &mut geometry_manual_color_hex,
                                                        &mut request_geometry_screen_color_pick,
                                                        &mut begin_mouse_move_absolute_capture_target,
                                                        &mut pending_geometry_macro_step_color_pick,
                                                        &mut self.draw_geometry_step_preview_target,
                                                        &self.overlay_tx,
                                                        step,
                                                        &mut live_sync,
                                                        self.state.vietnamese_input_enabled,
                                                        self.state.vietnamese_input_mode,
                                                        &timer_names,
                                                        &mut self.state.geometry_presets,
                                                        &mut geom_presets_changed,
                                                    );
                                                } else {
                                                    Self::render_audio_sense_step_editor(
                                                        ui,
                                                        language,
                                                        (group.id, preset.id, step_index, "normal-audiosense-step"),
                                                        group.id,
                                                        preset.id,
                                                        step_index,
                                                        false,
                                                        &audio_sense_preset_options,
                                                        &mut self.state.audio_sense_presets,
                                                        &self.audio_sense_devices,
                                                        &timer_names,
                                                        step,
                                                        &mut live_sync,
                                                        &mut audio_sense_presets_changed,
                                                    );
                                                }
                                            } else if matches!(
                                                step.action,
                                                MacroAction::DrawGeometry
                                                    | MacroAction::EnableCrosshairProfile
                                                    | MacroAction::EnablePinPreset
                                                    | MacroAction::PlayVideoPreset
                                            ) {
                                                let duration_id = ui.id().with((group.id, preset.id, step_index, "duration-expr-full"));
                                                ui.scope(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 2.0;
                                                    ui.spacing_mut().interact_size.y = 20.0;
                                                    ui.spacing_mut().button_padding.y = 0.0;
                                                    let is_video = step.action == MacroAction::PlayVideoPreset;
                                                    let mut is_permanent = !is_video && (step.duration_expr.trim() == "0" || step.duration_expr.trim().is_empty());
                                                    let width_val = if is_video { 96.0 } else if is_permanent { 96.0 } else { 190.0 };
                                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width_val, 21.0), egui::Sense::hover());
                                                    let mut child_ui = ui.new_child(
                                                        egui::UiBuilder::new()
                                                            .max_rect(rect)
                                                            .layout(egui::Layout::left_to_right(egui::Align::Center))
                                                    );
                                                    child_ui.spacing_mut().item_spacing.x = 2.0;
                                                    child_ui.spacing_mut().button_padding.y = 0.0;
                                                    child_ui.spacing_mut().interact_size.y = 21.0;
                                                    if !is_video {
                                                        let cb_response = child_ui.checkbox(&mut is_permanent, Self::tr_lang(language, "Permanent", "Vĩnh viễn"));
                                                        if cb_response.changed() {
                                                            if is_permanent {
                                                                step.duration_expr = "0".to_string();
                                                            } else {
                                                                step.duration_expr = "1500".to_string();
                                                            }
                                                            live_sync = true;
                                                        }
                                                    }
                                                    if is_video || !is_permanent {
                                                        let response = Self::render_variable_text_edit(
                                                            &mut child_ui,
                                                            &mut step.duration_expr,
                                                            duration_id,
                                                            72.0,
                                                            150.0,
                                                              21.0, 21.0,
                                                            "0",
                                                            false,
                                                        );
                                                        child_ui.add_sized([20.0, 21.0], egui::Label::new(egui::RichText::new("ms").weak()));
                                                        Self::apply_vietnamese_input_if_changed(
                                                            &response,
                                                            self.state.vietnamese_input_enabled,
                                                            self.state.vietnamese_input_mode,
                                                            &mut step.duration_expr,
                                                        );
                                                        live_sync |= response.changed();
                                                        Self::render_variable_suggestions(
                                                            ui,
                                                            &response,
                                                            &mut step.duration_expr,
                                                            &timer_names,
                                                            language,
                                                        );
                                                        let tooltip_text = Self::tr_lang(
                                                            language,
                                                            "Display duration (0 = show until macro/overlay ends, supports variables/math)",
                                                            "Thời gian hiển thị (0 = hiện đến khi dừng macro/overlay, hỗ trợ biến/phép tính)",
                                                        );
                                                        if response.hovered() {
                                                            egui::show_tooltip_text(ui.ctx(), ui.layer_id(), response.id, tooltip_text);
                                                        }
                                                    }
                                                });
                                            } else if action_supports_capture
                                                && !matches!(
                                                    step.action,
                                                    MacroAction::LockKeys
                                                        | MacroAction::UnlockKeys
                                                        | MacroAction::StopIfKeyPressed
                                                )
                                            {
                                                let step_capture_target = CaptureRequest::MacroStepInput {
                                                    group_id: group.id,
                                                    preset_id: preset.id,
                                                    step_index,
                                                    extra_cond_index: None,
                                                };
                                                let step_capture_active =
                                                    capture_target_snapshot.as_ref() == Some(&step_capture_target);
                                                let step_capture_width = if step_capture_active { 84.0 } else { 22.0 };
                                                let step_capture_button = if step_capture_active {
                                                    Button::new(Self::capture_button_text(language, true))
                                                        .fill(Color32::from_rgb(88, 84, 44))
                                                } else {
                                                    Button::new(Self::material_icon_text(0xe312, 12.0))
                                                };
                                                if ui
                                                    .add_sized([step_capture_width, 18.0], step_capture_button)
                                                    .on_hover_text(Self::tr_lang(
                                                        language,
                                                        "Capture input",
                                                        "",
                                                    ))
                                                    .clicked()
                                                {
                                                    if step_capture_active {
                                                        cancel_active_capture = true;
                                                    } else {
                                                        next_capture_target = Some(step_capture_target);
                                                    }
                                                }
                                                // Dropdown right here (equal size: 22.0 wide, 18.0 high)
                                                let menu_response = ui.menu_button(Self::material_icon_text(0xe5d2, 12.0), |ui| {
                                                    ui.set_max_width(200.0);
                                                    ui.menu_button(Self::tr_lang(language, "Letters (A-Z)", "Chữ cái (A-Z)"), |ui| {
                                                        ui.set_max_width(120.0);
                                                        egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                            for ch in b'A'..=b'Z' {
                                                                let key_str = (ch as char).to_string();
                                                                if ui.button(&key_str).clicked() {
                                                                    step.key = key_str;
                                                                    live_sync = true;
                                                                    ui.close_menu();
                                                                }
                                                            }
                                                        });
                                                    });
                                                    ui.menu_button(Self::tr_lang(language, "Numbers & Symbols", "Số & Kí tự"), |ui| {
                                                        ui.set_max_width(140.0);
                                                        egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                            for num in ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"] {
                                                                if ui.button(num).clicked() {
                                                                    step.key = num.to_string();
                                                                    live_sync = true;
                                                                    ui.close_menu();
                                                                }
                                                            }
                                                            ui.separator();
                                                            for sym in [";", "=", ",", "-", ".", "/", "`", "[", "\\", "]", "'"] {
                                                                if ui.button(sym).clicked() {
                                                                    step.key = sym.to_string();
                                                                    live_sync = true;
                                                                    ui.close_menu();
                                                                }
                                                            }
                                                        });
                                                    });
                                                    ui.menu_button(Self::tr_lang(language, "Navigation", "Điều hướng & Phím tắt"), |ui| {
                                                        ui.set_max_width(160.0);
                                                        for key in ["Escape", "Enter", "Space", "Backspace", "Tab", "Insert", "Delete", "Home", "End", "PageUp", "PageDown", "Left", "Up", "Right", "Down", "PrintScreen", "Pause"] {
                                                            if ui.button(key).clicked() {
                                                                step.key = key.to_string();
                                                                live_sync = true;
                                                                ui.close_menu();
                                                            }
                                                        }
                                                    });
                                                    ui.menu_button(Self::tr_lang(language, "Function (F1-F24)", "Phím chức năng"), |ui| {
                                                        ui.set_max_width(100.0);
                                                        egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                            for num in 1..=24 {
                                                                let key_str = format!("F{}", num);
                                                                if ui.button(&key_str).clicked() {
                                                                    step.key = key_str;
                                                                    live_sync = true;
                                                                    ui.close_menu();
                                                                }
                                                            }
                                                        });
                                                    });
                                                    ui.menu_button(Self::tr_lang(language, "Numpad", "Bàn phím số phụ"), |ui| {
                                                        ui.set_max_width(160.0);
                                                        for key in ["Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4", "Numpad5", "Numpad6", "Numpad7", "Numpad8", "Numpad9", "NumpadMultiply", "NumpadAdd", "NumpadSubtract", "NumpadDecimal", "NumpadDivide"] {
                                                            if ui.button(key).clicked() {
                                                                step.key = key.to_string();
                                                                live_sync = true;
                                                                ui.close_menu();
                                                            }
                                                        }
                                                    });
                                                    ui.menu_button(Self::tr_lang(language, "Modifiers & Locks", "Phím khóa & bổ trợ"), |ui| {
                                                        ui.set_max_width(150.0);
                                                        for key in ["Ctrl", "Alt", "Shift", "Win", "CapsLock", "NumLock", "ScrollLock", "Apps"] {
                                                            if ui.button(key).clicked() {
                                                                step.key = key.to_string();
                                                                live_sync = true;
                                                                ui.close_menu();
                                                            }
                                                        }
                                                    });
                                                });
                                                menu_response.response.on_hover_text(Self::tr_lang(
                                                    language,
                                                    "Manually select key",
                                                    "Chọn phím thủ công"
                                                ));
                                                // Trailing spacers placed after buttons to align columns with other rows having X/Y coords
                                                ui.add_sized([48.0, 20.0], egui::Label::new(""));
                                                ui.add_sized([48.0, 20.0], egui::Label::new(""));
                                            } else {
                                                ui.add_sized([28.0, 20.0], egui::Label::new(""));
                                            }
                                            let is_dark_theme = self.state.ui_theme == UiThemeMode::Dark;
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                  let copy_feedback_active =
                                                      self.macro_step_copy_feedback_target
                                                          == Some((group.id, preset.id, step_index))
                                                          && Self::is_copy_feedback_active(
                                                              self.macro_step_copy_feedback_until,
                                                          );
                                                  if ui
                                                      .add(
                                                          Button::new(Self::tr_lang(language, "Paste", "Paste"))
                                                              .min_size(vec2(42.0, 18.0)),
                                                      )
                                                      .on_hover_text(Self::tr_lang(
                                                          language,
                                                          "Paste copied step(s) below this step.",
                                                          "Paste copied step(s) below this step.",
                                                      ))
                                                      .clicked()
                                                  {
                                                      paste_step_after =
                                                          Some((group.id, preset.id, step_index));
                                                  }
                                                  if copy_feedback_active {
                                                      ui.add_sized(
                                                          [48.0, 20.0],
                                                          egui::Label::new(
                                                              RichText::new(Self::tr_lang(
                                                                  language,
                                                                  "Copied",
                                                                  "Copied",
                                                              ))
                                                              .color(Color32::from_rgb(126, 224, 182))
                                                              .strong(),
                                                          ),
                                                      );
                                                  } else if ui
                                                      .add(
                                                          Button::new(Self::tr_lang(language, "Copy", "Copy"))
                                                              .min_size(vec2(40.0, 18.0)),
                                                      )
                                                      .on_hover_text(Self::tr_lang(
                                                          language,
                                                          "Copy this step.",
                                                          "Copy this step.",
                                                      ))
                                                      .clicked()
                                                  {
                                                      copy_single_step =
                                                          Some((group.id, preset.id, step_index));
                                                  }
                                                if self.show_share_buttons {
                                                  let step_export_feedback =
                                                      self.macro_step_export_feedback_target
                                                          == Some((preset.id, step_index))
                                                          && Self::is_copy_feedback_active(
                                                              self.macro_step_export_feedback_until,
                                                          );
                                                  if ui
                                                      .add(
                                                          Button::new(if step_export_feedback {
                                                              Self::tr_lang(language, "Copied", "Copied")
                                                          } else {
                                                              Self::tr_lang(language, "Exp", "Exp")
                                                          })
                                                          .min_size(vec2(50.0, 18.0)),
                                                      )
                                                      .on_hover_text(Self::tr_lang(
                                                          language,
                                                          "Copy step code to clipboard.",
                                                          "Sao chep ma step vao clipboard.",
                                                      ))
                                                      .clicked()
                                                  {
                                                      export_step = Some((preset.id, step_index));
                                                  }
                                                  if ui
                                                      .add_enabled(
                                                          self.macro_share_clipboard_kind
                                                              == crate::ui::MacroShareCodeKind::Step,
                                                          Button::new(Self::tr_lang(language, "Imp", "Imp"))
                                                              .min_size(vec2(32.0, 18.0)),
                                                      )
                                                      .on_hover_text(Self::tr_lang(
                                                          language,
                                                          "Import step from clipboard below this step.",
                                                          "Nhap step tu clipboard nam duoi step nay.",
                                                      ))
                                                      .clicked()
                                                  {
                                                      import_step_to = Some((group.id, preset.id, Some(step_index)));
                                                  }
                                                }
                                                if step.toggle_enabled_on_run {
                                                    ui.add_space(4.0);
                                                    ui.scope(|ui| {
                                                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::TRANSPARENT;
                                                        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                                                        ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
                                                        let hover_bg = if is_dark_theme {
                                                            Color32::from_rgba_unmultiplied(255, 255, 255, 20)
                                                        } else {
                                                            Color32::from_rgba_unmultiplied(0, 0, 0, 15)
                                                        };
                                                        let hover_stroke = if is_dark_theme {
                                                            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 40))
                                                        } else {
                                                            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 30))
                                                        };
                                                        let active_bg = if is_dark_theme {
                                                            Color32::from_rgba_unmultiplied(255, 255, 255, 35)
                                                        } else {
                                                            Color32::from_rgba_unmultiplied(0, 0, 0, 25)
                                                        };
                                                        let active_stroke = if is_dark_theme {
                                                            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 60))
                                                        } else {
                                                            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 45))
                                                        };
                                                        ui.visuals_mut().widgets.hovered.bg_fill = hover_bg;
                                                        ui.visuals_mut().widgets.hovered.bg_stroke = hover_stroke;
                                                        ui.visuals_mut().widgets.active.bg_fill = active_bg;
                                                        ui.visuals_mut().widgets.active.bg_stroke = active_stroke;
                                                        let toggle_icon_color = Color32::from_rgb(0, 220, 255);
                                                        let toggle_icon = Self::material_icon_text(0xe040, 16.0).color(toggle_icon_color);
                                                        if ui
                                                            .add_sized([18.0, 18.0], Button::new(toggle_icon))
                                                            .on_hover_text(Self::tr_lang(
                                                                language,
                                                                "Toggle self enabled on run (run-loop/refresh state)",
                                                                "Tự bật/tắt chính nó khi chạy (trạng thái chạy lại/cập nhật)"
                                                            ))
                                                            .clicked()
                                                        {
                                                            step.toggle_enabled_on_run = !step.toggle_enabled_on_run;
                                                            live_sync = true;
                                                        }
                                                    });
                                                }
                                            });
                                        });
                                    })
                                    .response;
                                let is_row_hovered = row_response.hovered();
                                let mut hover_regions = Vec::new();
                                 let mut has_hover_support = false;
                                 match step.action {
                                     MacroAction::OcrSearch => {
                                         has_hover_support = true;
                                         hover_regions.push(crate::overlay::VisionRegion {
                                             left: step.x,
                                             top: step.y,
                                             width: step.ocr_width.max(10),
                                             height: step.ocr_height.max(10),
                                             is_circle: false,
                                             angle_offset_deg: None,
                                             angle_span_deg: None,
                                         });
                                     }
                                     MacroAction::StartVisionSearch | MacroAction::ScanVisionOnce | MacroAction::StopVision => {
                                         has_hover_support = true;
                                         if let Some(preset) = self.state.vision_presets.iter().find(|p| p.id.to_string() == step.key.trim()) {
                                             if let (Some(rx), Some(ry), Some(rw), Some(rh)) = (preset.search_region_screen_x, preset.search_region_screen_y, preset.search_region_width, preset.search_region_height) {
                                                 let bounds = crate::window_list::virtual_screen_bounds();
                                                 let left = rx.max(bounds.0);
                                                 let top = ry.max(bounds.1);
                                                 let right = (rx + rw).min(bounds.0 + bounds.2);
                                                 let bottom = (ry + rh).min(bounds.1 + bounds.3);
                                                 let width = right - left;
                                                 let height = bottom - top;
                                                 if width > 0 && height > 0 {
                                                     hover_regions.push(crate::overlay::VisionRegion {
                                                         left,
                                                         top,
                                                         width,
                                                         height,
                                                         is_circle: preset.search_region_is_circle,
                                                         angle_offset_deg: None,
                                                         angle_span_deg: None,
                                                     });
                                                 }
                                             }
                                         }
                                     }
                                     MacroAction::EnablePinPreset | MacroAction::DisablePin => {
                                         has_hover_support = true;
                                         if let Some(preset) = step.key.trim().parse::<u32>().ok().and_then(|pid| self.state.pin_presets.iter().find(|p| p.id == pid)) {
                                             if preset.use_source_crop {
                                                 hover_regions.push(crate::overlay::VisionRegion {
                                                     left: preset.source_x,
                                                     top: preset.source_y,
                                                     width: preset.source_width.max(10),
                                                     height: preset.source_height.max(10),
                                                     is_circle: false,
                                                     angle_offset_deg: None,
                                                     angle_span_deg: None,
                                                 });
                                             }
                                             if preset.use_custom_bounds {
                                                 hover_regions.push(crate::overlay::VisionRegion {
                                                     left: preset.x,
                                                     top: preset.y,
                                                     width: preset.width.max(10),
                                                     height: preset.height.max(10),
                                                     is_circle: false,
                                                     angle_offset_deg: None,
                                                     angle_span_deg: None,
                                                 });
                                             }
                                         }
                                     }
                                     MacroAction::ApplyWindowPreset => {
                                         has_hover_support = true;
                                         if let Some(preset) = step.key.trim().parse::<u32>().ok().and_then(|pid| self.state.window_presets.iter().find(|p| p.id == pid)) {
                                             let (screen_width, screen_height) = {
                                                 let bounds = crate::window_list::virtual_screen_bounds();
                                                 (bounds.2, bounds.3)
                                             };
                                             let width = preset.width;
                                             let height = preset.height;
                                             let (wx, wy) = match preset.anchor {
                                                 crate::model::WindowAnchor::Manual => (preset.x, preset.y),
                                                 crate::model::WindowAnchor::Center => ((screen_width - width) / 2, (screen_height - height) / 2),
                                                 crate::model::WindowAnchor::TopLeft => (0, 0),
                                                 crate::model::WindowAnchor::Top => (((screen_width - width) / 2), 0),
                                                 crate::model::WindowAnchor::TopRight => ((screen_width - width), 0),
                                                 crate::model::WindowAnchor::Left => (0, ((screen_height - height) / 2)),
                                                 crate::model::WindowAnchor::Right => ((screen_width - width), ((screen_height - height) / 2)),
                                                 crate::model::WindowAnchor::BottomLeft => (0, (screen_height - height)),
                                                 crate::model::WindowAnchor::Bottom => (((screen_width - width) / 2), (screen_height - height)),
                                                 crate::model::WindowAnchor::BottomRight => ((screen_width - width), (screen_height - height)),
                                             };
                                             hover_regions.push(crate::overlay::VisionRegion {
                                                 left: wx,
                                                 top: wy,
                                                 width: width.max(10),
                                                 height: height.max(10),
                                                 is_circle: false,
                                                 angle_offset_deg: None,
                                                 angle_span_deg: None,
                                             });
                                         }
                                     }
                                     _ => {}
                                 }
                                 if has_hover_support {
                                     if is_row_hovered {
                                         if !hover_regions.is_empty() {
                                             hovered_region = Some((hover_regions, (preset.id, step_index)));
                                         }
                                     }
                                 }
                                if row_response.secondary_clicked() {
                                    remove_step = Some((preset.id, step_index));
                                }
                                step_rects[step_index] = row_response.rect;
                                if drag_payload.is_some()
                                    && next_compact_preview_index == steps_len
                                    && pointer_y.is_some_and(|pointer_y| {
                                        pointer_y < row_response.rect.bottom()
                                    })
                                {
                                    next_compact_preview_index = if step_is_being_dragged {
                                        compact_cursor
                                    } else {
                                        let moving_up = active_current_compact_index
                                            .is_some_and(|current| compact_cursor < current);
                                        if moving_up {
                                            compact_cursor
                                        } else {
                                            compact_cursor + 1
                                        }
                                    };
                                }
                                if !step_is_being_dragged {
                                    compact_cursor += 1;
                                }
                            }
                            {
                                let mut hook_state = crate::overlay::HOOK_STATE.lock();
                                if let Some((regions, source)) = hovered_region {
                                    if !regions.is_empty() {
                                        hook_state.vision_capture_preview_regions = regions;
                                        hook_state.vision_preview_source = Some(source);
                                    }
                                } else {
                                    let matches_this_preset = hook_state.vision_preview_source.map_or(false, |(pid, _)| pid == preset.id);
                                    if matches_this_preset {
                                        hook_state.vision_capture_preview_regions.clear();
                                        hook_state.vision_preview_source = None;
                                    }
                                }
                            }
                            if drag_payload.is_some() {
                                let compact_steps_len =
                                    steps_len.saturating_sub(active_dragged_indices.len());
                                let resolved_compact_index =
                                    next_compact_preview_index.min(compact_steps_len);
                                ui.memory_mut(|mem| {
                                    mem.data
                                        .insert_temp(drag_preview_index_id, resolved_compact_index);
                                });
                                preview_drop_index = expand_drag_drop_index(
                                    resolved_compact_index,
                                    &active_dragged_indices,
                                );
                            }
                            // Dynamic hover highlight for Loop and If blocks (Gợi ý 2)
                            let hover_pos = ui.ctx().pointer_interact_pos();
                            if let Some(pos) = hover_pos {
                                struct BlockRange {
                                    start_idx: usize,
                                    end_idx: usize,
                                }
                                let mut blocks = Vec::new();
                                let mut loop_stack = Vec::new();
                                let mut if_stack = Vec::new();
                                for (idx, s) in preset.steps.iter().enumerate() {
                                    if s.enabled {
                                        if s.action == MacroAction::LoopStart {
                                            loop_stack.push(idx);
                                        } else if s.action == MacroAction::LoopEnd {
                                            if let Some(start_idx) = loop_stack.pop() {
                                                blocks.push(BlockRange {
                                                    start_idx,
                                                    end_idx: idx,
                                                });
                                            }
                                        } else if s.action == MacroAction::IfStart {
                                            if_stack.push(idx);
                                        } else if s.action == MacroAction::IfEnd {
                                            if let Some(start_idx) = if_stack.pop() {
                                                blocks.push(BlockRange {
                                                    start_idx,
                                                    end_idx: idx,
                                                });
                                            }
                                        }
                                    }
                                }
                                let mut active_block: Option<&BlockRange> = None;
                                for block in &blocks {
                                    if block.start_idx < step_rects.len() && block.end_idx < step_rects.len() {
                                        let start_rect = step_rects[block.start_idx];
                                        let end_rect = step_rects[block.end_idx];
                                        if start_rect != Rect::ZERO && end_rect != Rect::ZERO {
                                            let union_rect = start_rect.union(end_rect);
                                            // Kiểm tra xem chuột có nằm trong union_rect bao gồm cả khoảng hở dọc không
                                            if union_rect.contains(pos) {
                                                match active_block {
                                                    None => active_block = Some(block),
                                                    Some(current) => {
                                                        if (block.end_idx - block.start_idx) < (current.end_idx - current.start_idx) {
                                                            active_block = Some(block);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some(block) = active_block {
                                    let start_rect = step_rects[block.start_idx];
                                    let end_rect = step_rects[block.end_idx];
                                    let union_rect = start_rect.union(end_rect).expand(3.0);
                                    ui.painter().rect_stroke(
                                        union_rect,
                                        6.0,
                                        egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 220)),
                                        egui::StrokeKind::Outside,
                                    );
                                }
                            }
                            if let Some(payload) = drag_payload
                                && ui.input(|input| input.pointer.any_released())
                            {
                                move_step_to = Some((
                                    payload.preset_id,
                                    payload.indices.clone(),
                                    preview_drop_index,
                                ));
                            }
                        });
                                    });
                                    ui.add_space(4.0);
                                }
                            });
                            self.vision_manual_color = geometry_manual_color;
                            self.vision_manual_color_hex = geometry_manual_color_hex.clone();
                        }
                        if let Some((preset_id, step_index)) = insert_step_after {
                            if let Some(target_preset) = group
                                .presets
                                .iter_mut()
                                .find(|preset| preset.id == preset_id)
                            {
                                let insert_at = (step_index + 1).min(target_preset.steps.len());
                                target_preset.steps.insert(insert_at, MacroStep::default());
                                live_sync = true;
                                clear_step_selection = Some((group.id, preset_id));
                            }
                        }
                        if let Some((preset_id, dragged_indices, to_index)) = move_step_to {
                            if let Some(target_preset) = group
                                .presets
                                .iter_mut()
                                .find(|preset| preset.id == preset_id)
                            {
                                let mut indices = dragged_indices
                                    .into_iter()
                                    .filter(|index| *index < target_preset.steps.len())
                                    .collect::<Vec<_>>();
                                indices.sort_unstable();
                                indices.dedup();
                                if !indices.is_empty() {
                                    let mut moved_steps = Vec::with_capacity(indices.len());
                                    for index in indices.iter().rev().copied() {
                                        moved_steps.push(target_preset.steps.remove(index));
                                    }
                                    moved_steps.reverse();
                                    let removed_before_target =
                                        indices.iter().filter(|index| **index < to_index).count();
                                    let insert_at = to_index
                                        .saturating_sub(removed_before_target)
                                        .min(target_preset.steps.len());
                                    for (offset, step) in moved_steps.into_iter().enumerate() {
                                        target_preset.steps.insert(insert_at + offset, step);
                                    }
                                    live_sync = true;
                                }
                            }
                        }
                        if let Some((preset_id, step_index)) = remove_step {
                            if let Some(preset) = group
                                .presets
                                .iter_mut()
                                .find(|preset| preset.id == preset_id)
                                && step_index < preset.steps.len()
                            {
                                preset.steps.remove(step_index);
                                live_sync = true;
                                clear_step_selection = Some((group.id, preset_id));
                            }
                        }
                        if let Some(preset_id) = remove_preset {
                            group.presets.retain(|preset| preset.id != preset_id);
                            live_sync = true;
                            clear_step_selection = Some((group.id, preset_id));
                        }
                            });
                        });
                    });
                    if let Some((group_id, preset_id, step_index, name, command, use_powershell)) =
                        pending_custom_preset_save.take()
                        && let Some(saved_id) = self.upsert_custom_preset_from_step_draft_values(
                            name,
                            command,
                            use_powershell,
                        )
                    {
                        live_sync = true;
                        if let Some(step_index) = step_index {
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
                                    if let Some(step) = preset.steps.get_mut(step_index) {
                                        step.key = saved_id.to_string();
                                        step.command_preset_use_powershell = false;
                                    }
                                }
                            }
                        } else {
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
                                    preset.hold_stop_step.key = saved_id.to_string();
                                    preset.hold_stop_step.command_preset_use_powershell = false;
                                }
                            }
                        }
                    }
                    if let Some((group_id, preset_id, step_index, name, command, use_powershell, is_ad_hoc)) =
                        pending_custom_preset_save_and_open_ai.take()
                    {
                        if is_ad_hoc {
                            self.command_ai_step_target = Some((group_id, preset_id, step_index));
                            self.state.command_presets.retain(|preset| preset.id != 999999);
                            let mut temp_preset = CommandPreset::new(999999);
                            temp_preset.name = "Step Custom Command".to_owned();
                            temp_preset.command = command;
                            temp_preset.use_powershell = use_powershell;
                            temp_preset.collapsed = true;
                            self.state.command_presets.push(temp_preset);
                            self.open_command_ai_dialog_for_preset(999999);
                        } else if let Some(saved_id) = self.upsert_custom_preset_from_step_draft_values(
                            name,
                            command,
                            use_powershell,
                        ) {
                            live_sync = true;
                            if let Some(step_index) = step_index {
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
                                        if let Some(step) = preset.steps.get_mut(step_index) {
                                            step.key = saved_id.to_string();
                                            step.command_preset_command = "".to_owned();
                                            step.command_preset_use_powershell = false;
                                        }
                                    }
                                }
                            } else {
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
                                        preset.hold_stop_step.key = saved_id.to_string();
                                        preset.hold_stop_step.command_preset_command = "".to_owned();
                                        preset.hold_stop_step.command_preset_use_powershell = false;
                                    }
                                }
                            }
                            self.open_command_ai_dialog_for_preset(saved_id);
                        }
                    }
                    if let Some(preset_id) = pending_open_ai_preset_id.take() {
                        self.open_command_ai_dialog_for_preset(preset_id);
                    }
                    if let Some((gid, pid, sidx)) = pending_ocr_step_capture {
                        self.begin_image_search_capture(
                            ui.ctx(),
                            crate::ui::VisionCaptureTarget::OcrStepRegion { group_id: gid, preset_id: pid, step_index: sidx },
                            crate::ui::VisionCaptureMode::SearchRegion,
                        );
                    }
                    if let Some((lang_code, display_name)) = pending_ocr_language_settings.take() {
                        self.install_ocr_language_capability(&lang_code, &display_name);
                    }
                    if cancel_active_capture {
                        self.cancel_capture();
                    }
                    if cancel_mouse_move_absolute_capture {
                        self.cancel_mouse_move_absolute_capture(ui.ctx());
                    }
                    if cancel_mouse_path_draw_capture {
                        self.cancel_mouse_path_draw_capture(ui.ctx());
                    }
                    if let Some(path_preset_id) = preview_mouse_path_step_request.take() {
                        self.mouse_path_step_preview_preset_id = path_preset_id;
                        let preview_events = path_preset_id.and_then(|active_id| {
                            self.state
                                .mouse_path_presets
                                .iter()
                                .find(|preset| preset.id == active_id)
                                .map(|preset| preset.events.clone())
                        });
                        let _ = self
                            .overlay_tx
                            .send(OverlayCommand::PreviewMousePath(
                                path_preset_id.map(|active_id| {
                                    (active_id, preview_events.unwrap_or_default(), None)
                                }),
                            ));
                        crate::overlay::wake_command_queue();
                    }
                    if let Some((group_id, preset_id, step_index, selected_id)) =
                        add_mouse_path_preset_request.take()
                    {
                        let _ = selected_id;
                        let path_preset_id = self.add_mouse_path_preset();
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
                                if let Some(step) = preset.steps.get_mut(step_index) {
                                    step.key = path_preset_id.to_string();
                                }
                            }
                        }
                        self.persist_mouse_path_presets();
                        self.persist_macro_presets();
                    }
                    if let Some((group_id, preset_id, selected_id)) =
                        add_hold_stop_mouse_path_preset_request.take()
                    {
                        let _ = selected_id;
                        let path_preset_id = self.add_mouse_path_preset();
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
                                preset.hold_stop_step.key = path_preset_id.to_string();
                            }
                        }
                        self.persist_mouse_path_presets();
                        self.persist_macro_presets();
                    }
                    if let Some(target) = begin_mouse_move_absolute_capture_target {
                        self.begin_mouse_move_absolute_capture(ui.ctx(), target);
                    }
                    if let Some((group_id, preset_id, step_index, is_fill, is_hold_stop)) = pending_geometry_macro_step_color_pick {
                        self.begin_image_search_capture(
                            ui.ctx(),
                            crate::ui::VisionCaptureTarget::MacroStepGeometryColor { group_id, preset_id, step_index, is_fill, is_hold_stop },
                            crate::ui::VisionCaptureMode::ColorSample,
                        );
                    }
                    if let Some((group_id, preset_id, step_index, selected_id)) =
                        begin_mouse_path_draw_capture_request.take()
                    {
                        let mut created_new_path_preset = false;
                        let path_preset_id = if let Some(path_preset_id) = selected_id {
                            path_preset_id
                        } else {
                            self.add_mouse_path_preset();
                            created_new_path_preset = true;
                            self.state
                                .mouse_path_presets
                                .last()
                                .map(|preset| preset.id)
                                .unwrap_or(1)
                        };
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
                                if let Some(step) = preset.steps.get_mut(step_index) {
                                    step.key = path_preset_id.to_string();
                                }
                            }
                        }
                        if created_new_path_preset {
                            self.persist_mouse_path_presets();
                        }
                        self.persist_macro_presets();
                        self.mouse_path_step_preview_preset_id = None;
                        self.mouse_path_draw_capture_preset_id = None;
                        self.active_mouse_record_preset_id = None;
                        let _ = self.overlay_tx.send(OverlayCommand::PreviewMousePath(None));
                        crate::overlay::wake_command_queue();
                        self.begin_mouse_path_draw_capture(ui.ctx(), path_preset_id);
                    }
                    if should_scroll_to_group {
                        let group_scroll_rect_bottom = ui.cursor().min.y;
                        pending_macro_group_scroll_rect = Some(egui::Rect::from_min_max(
                            egui::pos2(viewport.left(), group_scroll_rect_top),
                            egui::pos2(viewport.right(), group_scroll_rect_bottom),
                        ));
                    }
                    if let Some(target) = next_capture_target {
                        let capture_status = match &target {
                            CaptureRequest::MacroPresetHoldStopInput(_, _) => match self.state.ui_language {
                                UiLanguage::Vietnamese => {
                                    "Đang bắt phím. Nhấn 1 phím để thêm.".to_owned()
                                }
                                _ => "Capturing one key. Press a key to add it.".to_owned(),
                            },
                            CaptureRequest::MacroStepInput { .. } => match self.state.ui_language {
                                UiLanguage::Vietnamese => {
                                    "Đang bắt phím. Nhấn 1 phím để thêm.".to_owned()
                                }
                                _ => "Capturing one key. Press a key to add it.".to_owned(),
                            },
                            _ => "Capturing macro input.".to_owned(),
                        };
                        self.begin_capture(target, capture_status);
                    }
                    if let Some((group_id, preset_id)) = copy_selected_steps {
                        self.copy_selected_macro_steps_for_preset(group_id, preset_id);
                    }
                    if let Some((group_id, preset_id)) = paste_steps_at_start {
                        if let Some(selection) = self.paste_macro_steps_at_start(group_id, preset_id) {
                            self.selected_macro_steps.clear();
                            self.selected_macro_steps.extend(
                                selection
                                    .into_iter()
                                    .map(|step_index| (group_id, preset_id, step_index)),
                            );
                            self.last_selected_macro_step = self
                                .selected_macro_steps
                                .iter()
                                .copied()
                                .max_by_key(|(_, _, step_index)| *step_index);
                            live_sync = true;
                        }
                    }
                    if let Some((group_id, preset_id, step_index)) = copy_single_step {
                        if let Some(group) = self.state.macro_groups.iter().find(|g| g.id == group_id) {
                            if let Some(preset) = group.presets.iter().find(|p| p.id == preset_id) {
                                if let Some(step) = preset.steps.get(step_index) {
                                    self.macro_step_clipboard = vec![step.clone()];
                                    self.macro_step_copy_feedback_target =
                                        Some((group_id, preset_id, step_index));
                                    self.macro_step_copy_feedback_until =
                                        Some(Instant::now() + Duration::from_secs(1));
                                    self.status = format!("Copied 1 step.");
                                }
                            }
                        }
                    }
                    if let Some((group_id, preset_id)) = delete_selected_steps {
                        self.remove_selected_macro_steps_for_preset(group_id, preset_id);
                        live_sync = true;
                    }
                    if let Some((group_id, preset_id, step_index)) = paste_step_after
                        && let Some(selection) =
                            self.paste_macro_steps_after(group_id, preset_id, step_index)
                    {
                        let _ = selection;
                        live_sync = true;
                    }
                    if let Some((group_id, preset_id, step_index, ctrl, shift)) =
                        pending_step_selection
                    {
                        if shift {
                            let num_steps = self.state.macro_groups
                                .iter()
                                .find(|g| g.id == group_id)
                                .and_then(|g| g.presets.iter().find(|p| p.id == preset_id))
                                .map(|p| p.steps.len())
                                .unwrap_or(0);
                            if let Some((anchor_group, anchor_preset, anchor_index)) = self.last_selected_macro_step
                                && anchor_group == group_id
                                && anchor_preset == preset_id
                                && anchor_index < num_steps
                                && step_index < num_steps
                            {
                                let start = std::cmp::min(anchor_index, step_index);
                                let end = std::cmp::max(anchor_index, step_index);
                                if !ctrl {
                                    self.clear_macro_step_selection_for_preset(group_id, preset_id);
                                }
                                for i in start..=end {
                                    self.selected_macro_steps.insert((group_id, preset_id, i));
                                }
                            } else {
                                self.clear_macro_step_selection_for_preset(group_id, preset_id);
                                self.selected_macro_steps.insert((group_id, preset_id, step_index));
                                self.last_selected_macro_step = Some((group_id, preset_id, step_index));
                            }
                        } else {
                            let currently_selected = self
                                .selected_macro_steps
                                .contains(&(group_id, preset_id, step_index));
                            let selected_count_in_preset = self
                                .selected_macro_steps
                                .iter()
                                .filter(|(selected_group, selected_preset, _)| {
                                    *selected_group == group_id && *selected_preset == preset_id
                                })
                                .count();
                            self.select_macro_step(
                                group_id,
                                preset_id,
                                step_index,
                                ctrl,
                                currently_selected,
                                selected_count_in_preset,
                            );
                            self.last_selected_macro_step = Some((group_id, preset_id, step_index));
                        }
                    }
                    if !ui.input(|input| input.pointer.primary_down()) {
                        self.macro_drag_select_anchor = None;
                    }
                    if let Some((group_id, preset_id)) = clear_step_selection {
                        self.clear_macro_step_selection_for_preset(group_id, preset_id);
                    }
                    if let Some((preset_id, step_index)) = export_step {
                        let step_opt = self.state.macro_groups.iter().flat_map(|g| &g.presets).find(|p| p.id == preset_id).and_then(|p| p.steps.get(step_index)).cloned();
                        if let Some(step) = step_opt {
                            self.export_macro_step(preset_id, step_index, &step);
                        }
                    }
                    if let Some((group_id, preset_id, insert_after_index)) = import_step_to {
                        self.import_macro_step_from_clipboard(group_id, preset_id, insert_after_index);
                    }
                    if let Some(preset_id) = export_preset {
                        let preset_opt = self.state.macro_groups.iter().flat_map(|g| &g.presets).find(|p| p.id == preset_id).cloned();
                        if let Some(preset) = preset_opt {
                            self.export_macro_preset(preset_id, &preset);
                        }
                    }
                    if let Some((group_id, insert_after_preset_id)) = import_preset_to_group {
                        self.import_macro_preset_from_clipboard(group_id, insert_after_preset_id);
                    }
                    if let Some(group_id) = export_group {
                        let group_opt = self.state.macro_groups.iter().find(|g| g.id == group_id).cloned();
                        if let Some(group) = group_opt {
                            self.export_macro_group(group_id, &group);
                        }
                    }
                    if let Some(group_id) = import_group_after {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("pending_macro_group_share_import_after"), group_id);
                        });
                    }
                    if group_is_collapsed {
                        let rendered_group_height = (ui.cursor().min.y - group_top).max(1.0);
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(macro_group_height_key, rendered_group_height);
                        });
                    } else {
                        ui.ctx().data_mut(|data| {
                            data.remove::<f32>(macro_group_height_key);
                        });
                    }
                }
            }
        }
    }

        if let Some(group_id) = add_preset_to_group {
            self.add_macro_preset_to_group(group_id);
            self.persist();
        }
        if let Some(group_id) = paste_preset_to_group
            && let Some(source_preset) = self.macro_preset_clipboard.clone()
        {
            let copied_preset = self.clone_macro_preset_with_new_id(&source_preset);
            if let Some(group) = self
                .state
                .macro_groups
                .iter_mut()
                .find(|group| group.id == group_id)
            {
                group.presets.push(copied_preset);
                self.persist_macro_presets();
            }
        }
        if ui
            .memory(|mem| mem.data.get_temp::<bool>(egui::Id::new("macro_variable_suggestion_committed")))
            .unwrap_or(false)
        {
            live_sync = true;
        }
        if live_sync {
            self.persist_macro_presets();
        }
        if geom_presets_changed {
            self.sync_geometry_presets();
            self.persist();
        }
        if audio_sense_presets_changed {
            self.sync_audio_sense_presets();
            self.persist();
        }
        if let Some(folder_id) = release_folder_id {
            self.state
                .macro_folders
                .retain(|folder| folder.id != folder_id);
            for group in &mut self.state.macro_groups {
                if group.folder_id == Some(folder_id) {
                    group.folder_id = None;
                }
            }
            self.confirm_release_folder_id = None;
            if self.active_macro_folder_view == Some(folder_id) {
                self.set_active_macro_folder_view(None);
            }
            self.persist_macro_presets();
        }
        if let Some(folder_id) = delete_folder_id {
            let should_confirm = self
                .state
                .macro_groups
                .iter()
                .any(|group| group.folder_id == Some(folder_id))
                && self.confirm_delete_folder_id != Some(folder_id);
            if should_confirm {
                self.confirm_delete_folder_id = Some(folder_id);
            } else {
                self.state
                    .macro_groups
                    .retain(|group| group.folder_id != Some(folder_id));
                self.state
                    .macro_folders
                    .retain(|folder| folder.id != folder_id);
                self.confirm_delete_folder_id = None;
                self.confirm_release_folder_id = None;
                if self.active_macro_folder_view == Some(folder_id) {
                    self.set_active_macro_folder_view(None);
                }
                self.persist_macro_presets();
            }
        }
        if let Some(group_id) = ui.ctx().data_mut(|data| {
            data.remove_temp::<u32>(egui::Id::new("pending_macro_group_share_import_after"))
        }) {
            self.import_macro_group_from_clipboard(None, Some(group_id));
        }
        if let Some(folder_id) = enter_folder_id {
            self.set_active_macro_folder_view(Some(folder_id));
        }
        if let Some(id) = remove_group {
            let should_confirm = self.confirm_delete_macro_group_id != Some(id);
            if should_confirm {
                self.confirm_delete_macro_group_id = Some(id);
            } else {
                self.state.macro_groups.retain(|group| group.id != id);
                self.selected_macro_groups.remove(&id);
                self.macro_group_clipboard
                    .retain(|group_id| *group_id != id);
                self.confirm_delete_macro_group_id = None;
                self.persist_macro_presets();
            }
        }
        if let Some((folder_id, name)) = renamed_folder {
            if let Some(folder) = self
                .state
                .macro_folders
                .iter_mut()
                .find(|folder| folder.id == folder_id)
            {
                folder.name = name;
                self.persist();
            }
        }
        if let Some(folder_id) = toggle_collapsed_folder_id {
            if let Some(folder) = self
                .state
                .macro_folders
                .iter_mut()
                .find(|folder| folder.id == folder_id)
            {
                folder.collapsed = !folder.collapsed;
                self.persist();
            }
        }
        if let Some(folder_id) = add_group_to_folder_id {
            self.add_macro_group_to_folder(folder_id);
            if let Some(folder) = self
                .state
                .macro_folders
                .iter_mut()
                .find(|folder| folder.id == folder_id)
            {
                folder.collapsed = false;
            }
            self.persist();
        }
        if lazy_render_active {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new(Self::tr_lang(
                        language,
                        "Loading more macro groups...",
                        "Dang tai them macro group...",
                    ))
                    .small()
                    .weak(),
                );
            });
            self.macro_panel_render_limit =
                (self.macro_panel_render_limit + 24).min(total_render_items);
            ui.ctx().request_repaint();
        }
        ui.add_space((macro_panel_scroll_height - 50.0).max(0.0));
        if let Some(group_scroll_rect) = pending_macro_group_scroll_rect {
            ui.scroll_to_rect(group_scroll_rect, Some(egui::Align::Center));
            pending_macro_group_scroll_consumed = true;
            ui.ctx().data_mut(|data| {
                data.remove_temp::<u32>(scroll_target_key);
                data.remove_temp::<usize>(scroll_frames_key);
            });
        } else if pending_macro_group_scroll_target.is_some() {
            pending_macro_group_scroll_consumed = true;
        }
        if !pending_macro_group_scroll_consumed {
            if self.pending_macro_group_scroll_target.is_none() {
                self.pending_macro_group_scroll_target = pending_macro_group_scroll_target;
            }
        } else {
            self.pending_macro_group_scroll_target = None;
        }
        });
    }

    pub(crate) fn collect_all_macro_referenced_variables(&self) -> Vec<String> {
        let mut vars = std::collections::HashSet::new();
        for group in &self.state.macro_groups {
            for preset in &group.presets {
                for step in &preset.steps {
                    Self::collect_vars_from_step(step, &mut vars);
                }
                if preset.hold_stop_step_enabled {
                    Self::collect_vars_from_step(&preset.hold_stop_step, &mut vars);
                }
            }
        }
        let mut list: Vec<String> = vars.into_iter().collect();
        list.sort();
        list
    }

    pub(crate) fn collect_vars_from_step(
        step: &MacroStep,
        vars: &mut std::collections::HashSet<String>,
    ) {
        if step.action == MacroAction::SetVariable {
            let name = step.if_variable_name.trim();
            if !name.is_empty() {
                vars.insert(name.to_string());
            }
            if matches!(
                step.set_variable_source,
                crate::model::SetVariableSource::Expression
            ) {
                Self::extract_vars_from_expression(&step.key, vars);
            }
        }
        if step.action == MacroAction::IfStart
            && matches!(step.if_condition_type, IfConditionType::Variable)
        {
            Self::extract_vars_from_expression(&step.if_variable_name, vars);
            Self::extract_vars_from_expression(&step.key, vars);
        }
        for output_var in [
            step.vision_found_var.trim(),
            step.vision_pos_var_x.trim(),
            step.vision_pos_var_y.trim(),
            step.ocr_success_var.trim(),
            step.ocr_pos_var_x.trim(),
            step.ocr_pos_var_y.trim(),
            step.ocr_numeric_var.trim(),
            step.ocr_text_var.trim(),
        ] {
            if !output_var.is_empty() {
                vars.insert(output_var.to_string());
            }
        }
        for cond in &step.extra_conditions {
            let name = cond.variable_name.trim();
            if !name.is_empty() {
                if matches!(cond.condition_type, IfConditionType::Variable) {
                    Self::extract_vars_from_expression(name, vars);
                } else {
                    vars.insert(name.to_string());
                }
            }
            Self::extract_vars_from_expression(&cond.expression, vars);
        }
        Self::extract_braced_vars(&step.delay_expr, vars);
        Self::extract_braced_vars(&step.x_expr, vars);
        Self::extract_braced_vars(&step.y_expr, vars);
        Self::extract_braced_vars(&step.mouse_speed_expr, vars);
        Self::extract_braced_vars(&step.text_override, vars);
        Self::extract_braced_vars(&step.command_preset_command, vars);
        if matches!(step.action, MacroAction::DrawGeometry) {
            for expr in [
                &step.geometry_spec.x1_expr,
                &step.geometry_spec.y1_expr,
                &step.geometry_spec.x2_expr,
                &step.geometry_spec.y2_expr,
                &step.geometry_spec.x3_expr,
                &step.geometry_spec.y3_expr,
                &step.geometry_spec.x4_expr,
                &step.geometry_spec.y4_expr,
                &step.geometry_spec.width_expr,
                &step.geometry_spec.height_expr,
                &step.geometry_spec.radius_expr,
                &step.geometry_spec.radius_x_expr,
                &step.geometry_spec.radius_y_expr,
                &step.geometry_spec.start_angle_expr,
                &step.geometry_spec.end_angle_expr,
                &step.geometry_spec.arrow_head_size_expr,
                &step.geometry_spec.font_size_expr,
                &step.geometry_spec.thickness_expr,
                &step.geometry_spec.opacity_expr,
                &step.geometry_spec.fill_opacity_expr,
                &step.geometry_spec.points_expr,
                &step.geometry_spec.stroke_color_expr,
                &step.geometry_spec.fill_color_expr,
                &step.geometry_spec.text,
            ] {
                Self::extract_braced_vars(expr, vars);
            }
        }
    }

    fn is_builtin_expression_identifier(token: &str) -> bool {
        matches!(
            token.trim().to_ascii_lowercase().as_str(),
            "abs"
                | "min"
                | "max"
                | "random"
                | "choice"
                | "atan"
                | "atan2"
                | "sin"
                | "cos"
                | "sqrt"
                | "pow"
                | "ceil"
                | "floor"
                | "degrees"
                | "radians"
                | "factorial"
                | "gcd"
                | "lcm"
                | "isqrt"
                | "comb"
                | "perm"
                | "pi"
                | "tonumber"
        )
    }

    fn extract_braced_vars(text: &str, vars: &mut std::collections::HashSet<String>) {
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                let mut name = String::new();
                let mut found = false;
                while let Some(&next_c) = chars.peek() {
                    if next_c == '}' {
                        chars.next();
                        found = true;
                        break;
                    } else if next_c == '{' {
                        break;
                    } else {
                        name.push(chars.next().unwrap());
                    }
                }
                if found {
                    Self::extract_vars_from_expression(&name, vars);
                }
            }
        }
    }

    fn extract_vars_from_expression(expr: &str, vars: &mut std::collections::HashSet<String>) {
        let mut current_var = String::new();
        for c in expr.chars() {
            if c.is_alphanumeric() || c == '_' {
                current_var.push(c);
            } else {
                let trimmed = current_var.trim();
                if !trimmed.is_empty()
                    && !trimmed.chars().next().unwrap().is_ascii_digit()
                    && !Self::is_builtin_expression_identifier(trimmed)
                {
                    vars.insert(trimmed.to_string());
                }
                current_var.clear();
            }
        }
        let trimmed = current_var.trim();
        if !trimmed.is_empty()
            && !trimmed.chars().next().unwrap().is_ascii_digit()
            && !Self::is_builtin_expression_identifier(trimmed)
        {
            vars.insert(trimmed.to_string());
        }
    }

    fn render_mouse_move_direction_controls(
        ui: &mut egui::Ui,
        step: &mut MacroStep,
        language: UiLanguage,
        live_sync: &mut bool,
        show_unlock_on_exit: bool,
        warning_tooltip_id: &'static str,
    ) {
        ui.horizontal(|ui| {
            for (value, label, tip_en, tip_vi) in [
                (
                    &mut step.lock_mouse_left,
                    "L",
                    "Lock movement to the left.",
                    "Khoa di chuot sang trai.",
                ),
                (
                    &mut step.lock_mouse_right,
                    "R",
                    "Lock movement to the right.",
                    "Khoa di chuot sang phai.",
                ),
                (
                    &mut step.lock_mouse_middle,
                    "U",
                    "Lock movement upward.",
                    "Khoa di chuot len tren.",
                ),
                (
                    &mut step.lock_mouse_scroll,
                    "D",
                    "Lock movement downward.",
                    "Khoa di chuot xuong duoi.",
                ),
            ] {
                let response =
                    ui.checkbox(value, label)
                        .on_hover_text(Self::tr_lang(language, tip_en, tip_vi));
                if response.changed() {
                    *live_sync = true;
                }
            }
            if show_unlock_on_exit {
                ui.add_space(6.0);
                let unlock_resp = ui.checkbox(
                    &mut step.unlock_on_exit,
                    Self::tr_lang(language, "Unlock when macro ends", ""),
                );
                if unlock_resp.changed() {
                    *live_sync = true;
                }
                if !step.unlock_on_exit {
                    let warn_color = Color32::from_rgb(255, 90, 0);
                    let response = ui.add(
                        egui::Label::new(
                            Self::material_icon_text(0xe002, 14.0).color(warn_color),
                        )
                        .sense(egui::Sense::hover()),
                    );
                    if response.contains_pointer() {
                        egui::show_tooltip_at_pointer(
                            ui.ctx(),
                            ui.layer_id(),
                            response.id.with(warning_tooltip_id),
                            |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(Self::material_icon_text(0xe002, 14.0).color(warn_color));
                                    ui.label(
                                        RichText::new(Self::tr_lang(
                                            language,
                                            "STEP WARNING",
                                            "CANH BAO BUOC",
                                        ))
                                        .strong()
                                        .color(warn_color),
                                    );
                                });
                                ui.label(Self::tr_lang(
                                    language,
                                    "Warning: Keeping mouse movement locked after the macro ends can make the cursor feel stuck until you unlock it manually.",
                                    "Canh bao: Neu giu khoa di chuot sau khi macro ket thuc, con tro co the bi ket cho toi khi ban tu mo khoa.",
                                ));
                            },
                        );
                    }
                }
            }
        });
    }

    pub(crate) fn render_variable_inspector(&mut self, ui: &mut egui::Ui) {
        let language = self.state.ui_language;
        ui.vertical(|ui| {
            ui.add_space(4.0);
            Self::render_expression_help_box(ui, language);
            ui.add_space(6.0);

            let mut all_vars_set = std::collections::HashSet::new();
            for v in self.collect_all_macro_referenced_variables() {
                all_vars_set.insert(v);
            }
            {
                let vars = crate::overlay::RUNTIME_VARIABLES.lock();
                for k in vars.keys() {
                    if !self.state.global_constants.iter().any(|(n, _)| n == k) {
                        all_vars_set.insert(k.clone());
                    }
                }
            }
            let mut vars_list: Vec<String> = all_vars_set.into_iter().collect();
            vars_list.sort();

            ui.columns(2, |columns| {
                columns[0].vertical(|ui| {
                    let id_const_name = ui.id().with("new_const_name");
                    let id_const_val = ui.id().with("new_const_val");
                    let mut name_buf = ui.memory(|mem| {
                        mem.data
                            .get_temp::<String>(id_const_name)
                            .unwrap_or_default()
                    });
                    let mut val_buf = ui.memory(|mem| {
                        mem.data
                            .get_temp::<String>(id_const_val)
                            .unwrap_or_default()
                    });
                    let is_dark_theme = self.state.ui_theme == UiThemeMode::Dark;
                    let hint_color = if is_dark_theme {
                        Color32::from_rgba_premultiplied(140, 140, 140, 150)
                    } else {
                        Color32::from_rgba_premultiplied(100, 100, 100, 150)
                    };
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [100.0, 20.0],
                            egui::TextEdit::singleline(&mut name_buf).hint_text(
                                RichText::new(Self::tr_lang(language, "CONST_NAME", "CONST_NAME"))
                                    .color(hint_color)
                                    .weak(),
                            ),
                        );
                        ui.label("=");
                        ui.add_sized(
                            [70.0, 20.0],
                            egui::TextEdit::singleline(&mut val_buf).hint_text(
                                RichText::new(Self::tr_lang(language, "Value", "Value"))
                                    .color(hint_color)
                                    .weak(),
                            ),
                        );
                        if ui.button(Self::tr_lang(language, "Add", "Add")).clicked() {
                            let name_trimmed = name_buf.trim().to_uppercase();
                            if !name_trimmed.is_empty() {
                                let parsed_val = val_buf.trim().parse::<i32>().unwrap_or(0);
                                if !self
                                    .state
                                    .global_constants
                                    .iter()
                                    .any(|(n, _)| n == &name_trimmed)
                                {
                                    self.state
                                        .global_constants
                                        .push((name_trimmed.clone(), parsed_val));
                                    let mut vars = crate::overlay::RUNTIME_VARIABLES.lock();
                                    vars.insert(name_trimmed, parsed_val as f64);
                                    name_buf.clear();
                                    val_buf.clear();
                                    self.persist();
                                }
                            }
                        }
                    });
                    ui.memory_mut(|mem| {
                        mem.data.insert_temp(id_const_name, name_buf);
                        mem.data.insert_temp(id_const_val, val_buf);
                    });

                    ui.add_space(6.0);
                    if !self.state.global_constants.is_empty() {
                        egui::ScrollArea::vertical()
                            .id_salt("global_constants_scroll")
                            .max_height(160.0)
                            .show(ui, |ui| {
                                egui::Grid::new("global_constants_grid")
                                    .num_columns(3)
                                    .spacing([8.0, 6.0])
                                    .striped(true)
                                    .show(ui, |ui| {
                                        let mut to_remove_idx = None;
                                        let mut to_update = None;
                                        for (idx, (name, val)) in
                                            self.state.global_constants.iter().enumerate()
                                        {
                                            ui.label(
                                                RichText::new(name)
                                                    .monospace()
                                                    .color(Color32::from_rgb(0, 180, 216)),
                                            );
                                            let id_editing = ui.id().with(("var-edit", name));
                                            let mut val_str = ui
                                                .memory(|mem| mem.data.get_temp::<String>(id_editing))
                                                .unwrap_or_else(|| val.to_string());
                                            let response = ui.add(
                                                egui::TextEdit::singleline(&mut val_str)
                                                    .desired_width(70.0)
                                                    .font(egui::TextStyle::Monospace),
                                            );
                                            if response.changed() {
                                                ui.memory_mut(|mem| {
                                                    mem.data.insert_temp(id_editing, val_str.clone())
                                                });
                                            }
                                            if response.lost_focus() || response.clicked_elsewhere() {
                                                if let Ok(new_val) = val_str.trim().parse::<i32>() {
                                                    to_update = Some((name.clone(), new_val));
                                                }
                                                ui.memory_mut(|mem| {
                                                    mem.data.remove_temp::<String>(id_editing)
                                                });
                                            }
                                            if ui
                                                .button(Self::material_icon_text(0xe872, 14.0))
                                                .on_hover_text(Self::tr_lang(language, "Delete", "Delete"))
                                                .clicked()
                                            {
                                                to_remove_idx = Some(idx);
                                            }
                                            ui.end_row();
                                        }
                                        if let Some(idx) = to_remove_idx {
                                            let (removed_name, _) = self.state.global_constants.remove(idx);
                                            let mut vars = crate::overlay::RUNTIME_VARIABLES.lock();
                                            vars.remove(&removed_name);
                                            self.persist();
                                        } else if let Some((name_to_up, new_val)) = to_update {
                                            if let Some(pos) = self
                                                .state
                                                .global_constants
                                                .iter()
                                                .position(|(n, _)| n == &name_to_up)
                                            {
                                                self.state.global_constants[pos].1 = new_val;
                                                let mut vars = crate::overlay::RUNTIME_VARIABLES.lock();
                                                vars.insert(name_to_up, new_val as f64);
                                                self.persist();
                                            }
                                        }
                                    });
                            });
                    }
                });

                columns[1].vertical(|ui| {
                    ui.set_row_height(24.0);
                    let id_name = ui.id().with("new_dyn_var_name");
                    let id_val = ui.id().with("new_dyn_var_val");
                    let mut name_buf =
                        ui.memory(|mem| mem.data.get_temp::<String>(id_name).unwrap_or_default());
                    let mut val_buf =
                        ui.memory(|mem| mem.data.get_temp::<String>(id_val).unwrap_or_default());
                    let is_dark_theme = self.state.ui_theme == UiThemeMode::Dark;
                    let hint_color = if is_dark_theme {
                        Color32::from_rgba_premultiplied(140, 140, 140, 150)
                    } else {
                        Color32::from_rgba_premultiplied(100, 100, 100, 150)
                    };
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [100.0, 20.0],
                            egui::TextEdit::singleline(&mut name_buf).hint_text(
                                RichText::new(Self::tr_lang(language, "Var Name", "Var Name"))
                                    .color(hint_color)
                                    .weak(),
                            ),
                        );
                        ui.label("=");
                        ui.add_sized(
                            [70.0, 20.0],
                            egui::TextEdit::singleline(&mut val_buf).hint_text(
                                RichText::new(Self::tr_lang(language, "Value", "Value"))
                                    .color(hint_color)
                                    .weak(),
                            ),
                        );
                        if ui.button(Self::tr_lang(language, "Set", "Set")).clicked() {
                            let name_trimmed = name_buf.trim().to_string();
                            if !name_trimmed.is_empty() {
                                let parsed_val = val_buf.trim().parse::<f64>().unwrap_or(0.0);
                                let mut vars = crate::overlay::RUNTIME_VARIABLES.lock();
                                vars.insert(name_trimmed, parsed_val);
                                name_buf.clear();
                                val_buf.clear();
                            }
                        }
                    });
                    ui.memory_mut(|mem| {
                        mem.data.insert_temp(id_name, name_buf);
                        mem.data.insert_temp(id_val, val_buf);
                    });

                    ui.add_space(6.0);
                    if !vars_list.is_empty() {
                        egui::ScrollArea::vertical()
                            .id_salt("macro_vars_scroll")
                            .max_height(160.0)
                            .show(ui, |ui| {
                                egui::Grid::new("macro_vars_grid")
                                    .num_columns(2)
                                    .spacing([8.0, 6.0])
                                    .striped(true)
                                    .show(ui, |ui| {
                                        for name in &vars_list {
                                            ui.label(
                                                RichText::new(name)
                                                    .monospace()
                                                    .color(Color32::from_rgb(243, 156, 18)),
                                            );
                                            let runtime_val = {
                                                let vars = crate::overlay::RUNTIME_VARIABLES.lock();
                                                vars.get(name).copied().unwrap_or(0.0)
                                            };
                                            ui.add_sized(
                                                [70.0, 20.0],
                                                egui::Label::new(
                                                    RichText::new(runtime_val.to_string()).monospace(),
                                                ),
                                            );
                                            ui.end_row();
                                        }
                                    });
                            });
                    }
                });
            });
        });
    }

    fn adjust_expression_by_delta(expr: &str, delta: i32) -> String {
        if delta == 0 {
            return expr.to_string();
        }
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return delta.max(0).to_string();
        }
        if let Ok(val) = trimmed.parse::<i32>() {
            return (val + delta).max(0).to_string();
        }
        if let Some(pos) = trimmed.rfind(|c| c == '+' || c == '-') {
            let (left, right) = trimmed.split_at(pos);
            let op = &right[0..1];
            let num_part = right[1..].trim();
            if let Ok(num) = num_part.parse::<i32>() {
                let signed_num = if op == "-" { -num } else { num };
                let new_num = signed_num + delta;
                let left_trimmed = left.trim_end();
                if new_num == 0 {
                    return left_trimmed.to_string();
                } else if new_num > 0 {
                    return format!("{} + {}", left_trimmed, new_num);
                } else {
                    return format!("{} - {}", left_trimmed, -new_num);
                }
            }
        }
        if delta > 0 {
            format!("{} + {}", trimmed, delta)
        } else {
            format!("{} - {}", trimmed, -delta)
        }
    }

    fn ensure_coordinate_exprs(step: &mut MacroStep) {
        if step.x_expr.trim().is_empty() {
            step.x_expr = step.x.to_string();
        }
        if step.y_expr.trim().is_empty() {
            step.y_expr = step.y.to_string();
        }
    }

    fn sync_coordinate_expr_to_value(expr: &str, value: &mut i32) {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            *value = 0;
            return;
        }
        if let Ok(parsed) = trimmed.parse::<i32>() {
            *value = parsed;
            return;
        }
        let interpolated = crate::overlay::interpolate_variables(trimmed);
        *value = crate::overlay::evaluate_math_expression(&interpolated);
    }

    fn builtin_variable_suggestions() -> &'static [&'static str] {
        &["System", "Screen", "Mouse", "Window", "Volume", "Clipboard"]
    }

    fn builtin_expression_function_suggestions() -> &'static [&'static str] {
        &[
            "abs()",
            "min()",
            "max()",
            "random()",
            "choice()",
            "atan()",
            "atan2()",
            "sin()",
            "cos()",
            "sqrt()",
            "pow()",
            "ceil()",
            "floor()",
            "degrees()",
            "radians()",
            "factorial()",
            "gcd()",
            "lcm()",
            "isqrt()",
            "comb()",
            "perm()",
            "pi",
        ]
    }

    fn object_property_suggestions(base: &str) -> Option<&'static [&'static str]> {
        match base.to_ascii_lowercase().as_str() {
            "system" => Some(&[
                "date",
                "time",
                "year",
                "month",
                "day",
                "hour",
                "minute",
                "second",
                "millisecond",
            ]),
            s if s.starts_with("timer") => Some(&[
                "hour",
                "minute",
                "second",
                "millisecond",
                "ms",
                "raw",
                "total_sec",
            ]),
            "screen" => Some(&["width", "height"]),
            "mouse" => Some(&["x", "y", "sensitivity"]),
            "volume" => Some(&["level"]),
            "window" => Some(&["title", "width", "height"]),
            "clipboard" => Some(&["text"]),
            _ => Some(&["toNumber"]),
        }
    }

    fn variable_base_exists(base: &str, suggestion_names: &[String]) -> bool {
        let normalized = base.trim().replace(' ', "").to_lowercase();
        if normalized.is_empty() {
            return false;
        }
        if Self::builtin_variable_suggestions()
            .iter()
            .any(|name| name.replace(' ', "").to_lowercase() == normalized)
        {
            return true;
        }
        suggestion_names
            .iter()
            .any(|name| name.replace(' ', "").to_lowercase() == normalized)
    }

    fn timer_ref_index(ref_name: &str) -> Option<usize> {
        let normalized = ref_name.trim().replace(' ', "").to_lowercase();
        let idx_str = normalized.strip_prefix("timer")?;
        let idx = idx_str.parse::<usize>().ok()?;
        idx.checked_sub(1)
    }

    fn timer_suggestion_label(suggestion: &str, timer_names: &[String]) -> String {
        if let Some((base, prop)) = suggestion.split_once('.') {
            if let Some(idx) = Self::timer_ref_index(base)
                && let Some(timer_name) = timer_names.get(idx)
            {
                return format!("{}.{} ({})", base, prop, timer_name);
            }
        } else if let Some(idx) = Self::timer_ref_index(suggestion)
            && let Some(timer_name) = timer_names.get(idx)
        {
            return format!("{} ({})", suggestion, timer_name);
        }
        suggestion.to_string()
    }

    fn expression_suggestion_label(suggestion: &str, timer_names: &[String]) -> String {
        match suggestion {
            "abs()" => "abs(a)".to_string(),
            "min()" => "min(a, b)".to_string(),
            "max()" => "max(a, b)".to_string(),
            "random()" => "random(min, max)".to_string(),
            "choice()" => "choice(val1, val2, ...)".to_string(),
            "atan()" => "atan(a)".to_string(),
            "atan2()" => "atan2(y, x)".to_string(),
            "sin()" => "sin(angleDeg) * 1000".to_string(),
            "cos()" => "cos(angleDeg) * 1000".to_string(),
            "sqrt()" => "sqrt(a)".to_string(),
            "pow()" => "pow(a, b)".to_string(),
            "ceil()" => "ceil(a)".to_string(),
            "floor()" => "floor(a)".to_string(),
            "degrees()" => "degrees(rad)".to_string(),
            "radians()" => "radians(deg)".to_string(),
            "factorial()" => "factorial(n)".to_string(),
            "gcd()" => "gcd(a, b, ...)".to_string(),
            "lcm()" => "lcm(a, b, ...)".to_string(),
            "isqrt()" => "isqrt(n)".to_string(),
            "comb()" => "comb(n, k)".to_string(),
            "perm()" => "perm(n, k)".to_string(),
            "pi" => "pi".to_string(),
            _ => Self::timer_suggestion_label(suggestion, timer_names),
        }
    }

    fn geometry_macro_actions() -> &'static [MacroAction] {
        &[
            MacroAction::DrawGeometry,
            MacroAction::ShowGeometryPreset,
            MacroAction::HideGeometryPreset,
        ]
    }

    fn macro_action_is_geometry(action: MacroAction) -> bool {
        Self::geometry_macro_actions().contains(&action)
    }

    fn audio_sense_macro_actions() -> &'static [MacroAction] {
        &[
            MacroAction::StartAudioSensePreset,
            MacroAction::StopAudioSense,
        ]
    }

    fn macro_action_is_audio_sense(action: MacroAction) -> bool {
        Self::audio_sense_macro_actions().contains(&action)
    }

    fn render_geometry_action_group_option(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        current: &mut MacroAction,
        live_sync: &mut bool,
        action_hover_id: egui::Id,
    ) {
        let selected = Self::macro_action_is_geometry(*current);
        let owner_id = ui.make_persistent_id("macro-action-submenu-owner");
        let popup_id = ui.make_persistent_id((id_source, "geometry-submenu-popup"));
        let active_owner = ui
            .ctx()
            .data(|data| data.get_temp::<MacroActionSubmenuKind>(owner_id));
        let top_level_hovered = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(action_hover_id))
            .unwrap_or(false);
        let mut open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(popup_id))
            .unwrap_or(false);
        if active_owner.is_some_and(|kind| kind != MacroActionSubmenuKind::Geometry) {
            open = false;
        }
        if top_level_hovered {
            open = false;
            ui.ctx()
                .data_mut(|data| data.insert_temp(owner_id, None::<MacroActionSubmenuKind>));
            ui.ctx().request_repaint();
        }
        let inner = ui.allocate_ui_with_layout(
            vec2(58.0, 42.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let response = ui.add_sized(
                    [34.0, 24.0],
                    Button::new(Self::material_icon_text(0xe8f4, 18.0)).selected(selected),
                );
                if response.hovered() || response.clicked() {
                    Self::clear_macro_action_submenus(ui, id_source);
                    open = true;
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(owner_id, MacroActionSubmenuKind::Geometry)
                    });
                }
                let popup_rect_id = ui.make_persistent_id((id_source, "geometry-submenu-rect"));
                let popup_response = egui::Popup::from_response(&response)
                    .id(popup_id)
                    .open_bool(&mut open)
                    .align(egui::RectAlign::BOTTOM_START)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .width(220.0)
                    .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                    .show(|ui| {
                        let rect = ui.max_rect();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(popup_rect_id, rect));
                        egui::Grid::new((id_source, "geometry-action-grid"))
                            .num_columns(2)
                            .spacing([6.0, 6.0])
                            .show(ui, |ui| {
                                for action in Self::geometry_macro_actions().iter().copied() {
                                    Self::render_macro_action_option(
                                        ui,
                                        language,
                                        current,
                                        action,
                                        live_sync,
                                        action_hover_id,
                                        true,
                                    );
                                }
                            });
                    });
                let popup_rect: Option<egui::Rect> =
                    ui.ctx().data(|data| data.get_temp(popup_rect_id));
                if open {
                    if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
                        let mut keep_open_rect = response.rect.expand(10.0);
                        if let Some(rect) = popup_rect {
                            keep_open_rect = keep_open_rect.union(rect.expand(10.0));
                            if rect.contains(pointer_pos) {
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(owner_id, MacroActionSubmenuKind::Geometry)
                                });
                            }
                        }
                        if !keep_open_rect.contains(pointer_pos) {
                            open = false;
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                            });
                            ui.ctx().request_repaint();
                        }
                    } else {
                        open = false;
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                        });
                        ui.ctx().request_repaint();
                    }
                }
                ui.ctx().data_mut(|data| data.insert_temp(popup_id, open));
                let label_color = if selected {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                };
                ui.label(
                    RichText::new(Self::tr_lang(language, "Geometry", "Hình học"))
                        .size(9.0)
                        .color(label_color),
                );
                if let Some(popup) = popup_response {
                    popup.response
                } else {
                    response
                }
            },
        );
        let response = inner.inner;
        if !open {
            Self::show_instant_hover_tooltip(
                ui,
                &response,
                Self::tr_lang(
                    language,
                    "Geometry\nOpen draw, show, hide, and clear geometry actions.",
                    "Hình học\nMở các hành động vẽ, hiện, ẩn và xóa hình học.",
                ),
            );
        }
    }

    fn render_audio_sense_action_group_option(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash + Copy,
        current: &mut MacroAction,
        live_sync: &mut bool,
        action_hover_id: egui::Id,
    ) {
        let selected = Self::macro_action_is_audio_sense(*current);
        let owner_id = ui.make_persistent_id("macro-action-submenu-owner");
        let popup_id = ui.make_persistent_id((id_source, "audiosense-submenu-popup"));
        let active_owner = ui
            .ctx()
            .data(|data| data.get_temp::<MacroActionSubmenuKind>(owner_id));
        let top_level_hovered = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(action_hover_id))
            .unwrap_or(false);
        let mut open = ui
            .ctx()
            .data(|data| data.get_temp::<bool>(popup_id))
            .unwrap_or(false);
        if active_owner.is_some_and(|kind| kind != MacroActionSubmenuKind::AudioSense) {
            open = false;
        }
        if top_level_hovered {
            open = false;
            ui.ctx()
                .data_mut(|data| data.insert_temp(owner_id, None::<MacroActionSubmenuKind>));
            ui.ctx().request_repaint();
        }
        let inner = ui.allocate_ui_with_layout(
            vec2(58.0, 42.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let response = ui.add_sized(
                    [34.0, 24.0],
                    Button::new(Self::material_icon_text(0xe050, 18.0)).selected(selected),
                );
                if response.hovered() || response.clicked() {
                    Self::clear_macro_action_submenus(ui, id_source);
                    open = true;
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(owner_id, MacroActionSubmenuKind::AudioSense)
                    });
                }
                let popup_rect_id = ui.make_persistent_id((id_source, "audiosense-submenu-rect"));
                let popup_response = egui::Popup::from_response(&response)
                    .id(popup_id)
                    .open_bool(&mut open)
                    .align(egui::RectAlign::BOTTOM_START)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .width(220.0)
                    .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                    .show(|ui| {
                        let rect = ui.max_rect();
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(popup_rect_id, rect));
                        egui::Grid::new((id_source, "audiosense-action-grid"))
                            .num_columns(3)
                            .spacing([6.0, 6.0])
                            .show(ui, |ui| {
                                for action in Self::audio_sense_macro_actions().iter().copied() {
                                    Self::render_macro_action_option(
                                        ui,
                                        language,
                                        current,
                                        action,
                                        live_sync,
                                        action_hover_id,
                                        true,
                                    );
                                }
                            });
                    });
                let popup_rect: Option<egui::Rect> =
                    ui.ctx().data(|data| data.get_temp(popup_rect_id));
                if open {
                    if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
                        let mut keep_open_rect = response.rect.expand(10.0);
                        if let Some(rect) = popup_rect {
                            keep_open_rect = keep_open_rect.union(rect.expand(10.0));
                            if rect.contains(pointer_pos) {
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(owner_id, MacroActionSubmenuKind::AudioSense)
                                });
                            }
                        }
                        if !keep_open_rect.contains(pointer_pos) {
                            open = false;
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                            });
                            ui.ctx().request_repaint();
                        }
                    } else {
                        open = false;
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(owner_id, None::<MacroActionSubmenuKind>)
                        });
                        ui.ctx().request_repaint();
                    }
                }
                ui.ctx().data_mut(|data| data.insert_temp(popup_id, open));
                let label_color = if selected {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                };
                ui.label(
                    RichText::new(Self::tr_lang(language, "Audio", "Am thanh"))
                        .size(9.0)
                        .color(label_color),
                );
                if let Some(popup) = popup_response {
                    popup.response
                } else {
                    response
                }
            },
        );
        let response = inner.inner;
        if !open {
            Self::show_instant_hover_tooltip(
                ui,
                &response,
                Self::tr_lang(
                    language,
                    "AudioSense\nOpen pitch audio actions.",
                    "AudioSense\nMo cac hanh dong cao do va huong am.",
                ),
            );
        }
    }

    fn render_audio_sense_preset_selector(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash,
        preset_options: &[(u32, String)],
        preset_id: &mut Option<u32>,
        live_sync: &mut bool,
    ) {
        let selected_label = preset_id
            .and_then(|id| {
                preset_options
                    .iter()
                    .find(|preset| preset.0 == id)
                    .map(|preset| preset.1.clone())
            })
            .unwrap_or_else(|| {
                Self::tr_lang(language, "Select AudioSense", "Chon AudioSense").to_owned()
            });
        ComboBox::from_id_salt(id_source)
            .width(146.0)
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                for (preset_entry_id, preset_entry_name) in preset_options {
                    if ui
                        .selectable_label(*preset_id == Some(*preset_entry_id), preset_entry_name)
                        .clicked()
                    {
                        *preset_id = Some(*preset_entry_id);
                        *live_sync = true;
                    }
                }
            });
    }

    fn render_audio_sense_var_box(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id: egui::Id,
        text: &mut String,
        timer_names: &[String],
        normal_width: f32,
        hint: &str,
    ) -> bool {
        let response = Self::render_variable_text_edit(
            ui,
            text,
            id,
            normal_width,
            132.0,
            20.0,
            20.0,
            hint,
            false,
        );
        let changed = response.changed();
        Self::render_variable_suggestions(ui, &response, text, timer_names, language);
        changed
    }

    fn render_geometry_preset_selector(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_source: impl std::hash::Hash,
        preset_options: &[(u32, String)],
        preset_id: &mut Option<u32>,
        live_sync: &mut bool,
    ) {
        let selected_label = preset_id
            .and_then(|id| {
                preset_options
                    .iter()
                    .find(|preset| preset.0 == id)
                    .map(|preset| preset.1.clone())
            })
            .unwrap_or_else(|| {
                Self::tr_lang(language, "Select geometry", "Chon geometry").to_owned()
            });
        ComboBox::from_id_salt(id_source)
            .width(180.0)
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                for (preset_entry_id, preset_entry_name) in preset_options {
                    if ui
                        .selectable_label(*preset_id == Some(*preset_entry_id), preset_entry_name)
                        .clicked()
                    {
                        *preset_id = Some(*preset_entry_id);
                        *live_sync = true;
                    }
                }
            });
    }

    fn render_overlay_eye_button(
        ui: &mut egui::Ui,
        language: UiLanguage,
        step: &MacroStep,
        group_id: u32,
        macro_preset_id: u32,
        step_index: usize,
        is_hold_stop: bool,
        hud_presets: &[HudPreset],
        preview_target: &mut Option<(u32, u32, usize, bool)>,
        overlay_tx: &crossbeam_channel::Sender<crate::overlay::OverlayCommand>,
        button_size: [f32; 2],
        icon_size: f32,
    ) {
        let is_active = match step.action {
            MacroAction::DrawGeometry => {
                let preview_active = *preview_target
                    == Some((group_id, macro_preset_id, step_index, is_hold_stop));
                let running_active =
                    crate::overlay::is_geometry_active(macro_preset_id, step_index);
                preview_active || running_active
            }
            MacroAction::EnableCrosshairProfile => crate::overlay::is_crosshair_active(&step.key),
            MacroAction::EnablePinPreset => crate::overlay::is_pin_active(&step.key),
            MacroAction::PlayVideoPreset => crate::overlay::is_video_active(&step.key),
            MacroAction::ShowHud => crate::overlay::is_hud_active(&step.key),
            _ => false,
        };
        let icon = if is_active { 0xe8f5 } else { 0xe8f4 }; // 0xe8f5: open eye, 0xe8f4: closed eye
        let preview_btn = egui::Button::new(Self::material_icon_text(icon, icon_size));
        let tooltip = if is_active {
            Self::tr_lang(
                language,
                "Stop active overlay/preview",
                "Dừng hiển thị/preview",
            )
        } else {
            Self::tr_lang(language, "Preview/Show overlay", "Xem trước/Hiện overlay")
        };
        let response = ui.add_sized(button_size, preview_btn);
        // Immediate tooltip
        if response.hovered() {
            egui::show_tooltip_text(ui.ctx(), ui.layer_id(), response.id, tooltip);
        }
        if response.clicked() {
            if is_active {
                match step.action {
                    MacroAction::DrawGeometry => {
                        let preview_active = *preview_target
                            == Some((group_id, macro_preset_id, step_index, is_hold_stop));
                        if preview_active {
                            *preview_target = None;
                            let _ = overlay_tx
                                .send(crate::overlay::OverlayCommand::PreviewGeometrySpec(None));
                        }
                        crate::overlay::stop_geometry(macro_preset_id, step_index);
                    }
                    MacroAction::EnableCrosshairProfile => {
                        crate::overlay::disable_crosshair_profile(&step.key);
                    }
                    MacroAction::EnablePinPreset => {
                        crate::overlay::disable_pin_preset(&step.key);
                    }
                    MacroAction::PlayVideoPreset => {
                        let _ = overlay_tx.send(crate::overlay::OverlayCommand::StopVideoPlayback);
                    }
                    MacroAction::ShowHud => {
                        crate::overlay::hide_hud_now();
                    }
                    _ => {}
                }
            } else {
                match step.action {
                    MacroAction::DrawGeometry => {
                        *preview_target =
                            Some((group_id, macro_preset_id, step_index, is_hold_stop));
                        let _ =
                            overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometrySpec(
                                Some(step.geometry_spec.clone()),
                            ));
                    }
                    MacroAction::EnableCrosshairProfile => {
                        let _ = crate::overlay::enable_crosshair_profile(&step.key);
                    }
                    MacroAction::EnablePinPreset => {
                        let _ = crate::overlay::enable_pin_preset(&step.key);
                    }
                    MacroAction::PlayVideoPreset => {
                        if let Ok(preset_id) = step.key.trim().parse::<u32>() {
                            let _ = overlay_tx
                                .send(crate::overlay::OverlayCommand::PlayVideoPreset(preset_id));
                        }
                    }
                    MacroAction::ShowHud => {
                        if let Ok(preset_id) = step.key.trim().parse::<u32>() {
                            if let Some(hud) = hud_presets.iter().find(|h| h.id == preset_id) {
                                let mut hud_clone = hud.clone();
                                if !step.text_override.is_empty() {
                                    hud_clone.text = step.text_override.clone();
                                }
                                let _ = overlay_tx.send(
                                    crate::overlay::OverlayCommand::PreviewHudPreset(vec![
                                        hud_clone,
                                    ]),
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn render_geometry_macro_step_editor(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_prefix: impl std::hash::Hash + Copy,
        group_id: u32,
        macro_preset_id: u32,
        step_index: usize,
        is_hold_stop: bool,
        preset_options: &[(u32, String)],
        vision_manual_color: &mut RgbaColor,
        vision_manual_color_hex: &mut String,
        request_screen_color_pick: &mut bool,
        begin_mouse_move_absolute_capture_target: &mut Option<MouseMoveAbsoluteCaptureTarget>,
        pending_macro_step_color_pick: &mut Option<(u32, u32, usize, bool, bool)>,
        draw_geometry_step_preview_target: &mut Option<(u32, u32, usize, bool)>,
        overlay_tx: &crossbeam_channel::Sender<crate::overlay::OverlayCommand>,
        step: &mut MacroStep,
        live_sync: &mut bool,
        vietnamese_input_enabled: bool,
        vietnamese_input_mode: VietnameseInputMode,
        timer_names: &[String],
        geometry_presets: &mut Vec<crate::model::GeometryPreset>,
        geometry_presets_changed: &mut bool,
    ) {
        let mut pending_screen_color_target = None;
        ui.scope(|ui| {
            ui.vertical(|ui| match step.action {
                MacroAction::DrawGeometry => {
                    step.geometry_spec.visible = true;
                    let current_preview_target = (group_id, macro_preset_id, step_index, is_hold_stop);
                    let mut geometry_preview_dirty = false;
                    ui.horizontal(|ui| {
                        ComboBox::from_id_salt((id_prefix, "geometry-shape"))
                            .width(90.0)
                            .selected_text(Self::geometry_shape_label(step.geometry_spec.shape, language))
                            .show_ui(ui, |ui| {
                                for shape in Self::geometry_shapes() {
                                    let changed = ui
                                        .selectable_value(
                                            &mut step.geometry_spec.shape,
                                            shape,
                                            Self::geometry_shape_label(shape, language),
                                         )
                                        .changed();
                                    if changed {
                                        step.geometry_spec.apply_shape_defaults();
                                    }
                                    *live_sync |= changed;
                                    geometry_preview_dirty |= changed;
                                }
                            });
                        ui.add_space(6.0);
                        let mut is_permanent = step.duration_expr.trim() == "0" || step.duration_expr.trim().is_empty();
                        let cb_response = ui.checkbox(&mut is_permanent, Self::tr_lang(language, "Permanent", "Vĩnh viễn"));
                        if cb_response.changed() {
                            if is_permanent {
                                step.duration_expr = "0".to_string();
                            } else {
                                step.duration_expr = "1500".to_string();
                            }
                            *live_sync = true;
                        }
                        if !is_permanent {
                            ui.add_space(2.0);
                            let duration_id = ui.make_persistent_id((group_id, macro_preset_id, step_index, is_hold_stop, "geometry-duration"));
                            let response = Self::render_variable_text_edit(
                                ui,
                                &mut step.duration_expr,
                                duration_id,
                                56.0,
                                120.0,
                                18.0,
                                18.0,
                                "0",
                                false,
                            );
                            ui.weak("ms");
                            Self::apply_vietnamese_input_if_changed(
                                &response,
                                vietnamese_input_enabled,
                                vietnamese_input_mode,
                                &mut step.duration_expr,
                            );
                            *live_sync |= response.changed();
                            Self::render_variable_suggestions(
                                ui,
                                &response,
                                &mut step.duration_expr,
                                timer_names,
                                language,
                            );
                            let tooltip_text = Self::tr_lang(
                                language,
                                "Display duration (0 = show until macro/overlay ends, supports variables/math)",
                                "Thời gian hiển thị (0 = hiện đến khi dừng macro/overlay, hỗ trợ biến/phép tính)",
                            );
                            if response.hovered() {
                                egui::show_tooltip_text(ui.ctx(), ui.layer_id(), response.id, tooltip_text);
                            }
                        }
                        ui.add_space(6.0);
                        let collapse_icon = if step.geometry_collapsed { 0xe5cc } else { 0xe5cf };
                        let collapse_btn = Button::new(Self::material_icon_text(collapse_icon, 12.0));
                        if ui
                            .add_sized([18.0, 18.0], collapse_btn)
                            .on_hover_text(if step.geometry_collapsed { "Expand" } else { "Collapse" })
                            .clicked()
                        {
                            step.geometry_collapsed = !step.geometry_collapsed;
                            *live_sync = true;
                            if step.geometry_collapsed {
                                let current_target = (group_id, macro_preset_id, step_index, is_hold_stop);
                                if *draw_geometry_step_preview_target == Some(current_target) {
                                    *draw_geometry_step_preview_target = None;
                                    let _ = overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometrySpec(None));
                                }
                            }
                        }
                        Self::render_overlay_eye_button(
                            ui,
                            language,
                            step,
                            group_id,
                            macro_preset_id,
                            step_index,
                            is_hold_stop,
                            &[],
                            draw_geometry_step_preview_target,
                            overlay_tx,
                            [18.0, 18.0],
                            12.0,
                        );
                    });
                    if !step.geometry_collapsed {
                        ui.add_space(4.0);
                        let geometry_editor_changed = Self::render_geometry_spec_editor(
                            ui,
                            language,
                            macro_preset_id,
                            step_index as u32,
                            true,
                            &mut step.geometry_spec,
                            vision_manual_color,
                            vision_manual_color_hex,
                            request_screen_color_pick,
                            &mut pending_screen_color_target,
                            begin_mouse_move_absolute_capture_target,
                            vietnamese_input_enabled,
                            vietnamese_input_mode,
                            Some(group_id),
                        );
                        *live_sync |= geometry_editor_changed;
                        geometry_preview_dirty |= geometry_editor_changed;
                        if geometry_preview_dirty
                            && *draw_geometry_step_preview_target == Some(current_preview_target)
                        {
                            let _ = overlay_tx.send(crate::overlay::OverlayCommand::PreviewGeometrySpec(
                                Some(step.geometry_spec.clone()),
                            ));
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let save_preset_btn_text = Self::tr_lang(language, "Save as Preset", "Lưu làm preset");
                            ui.menu_button(save_preset_btn_text, |ui| {
                                if ui.button(Self::tr_lang(language, "+ New preset", "+ Preset mới")).clicked() {
                                    let next_preset_id = geometry_presets.iter().map(|p| p.id).max().unwrap_or(0) + 1;
                                    let mut new_preset = crate::model::GeometryPreset::new(next_preset_id);
                                    new_preset.name = format!("Preset {}", next_preset_id);
                                    new_preset.objects.clear();
                                    let object_id = 1;
                                    new_preset.objects.push(crate::model::GeometryObject {
                                        id: object_id,
                                        name: format!("{:?} {}", step.geometry_spec.shape, object_id),
                                        enabled: true,
                                        spec: step.geometry_spec.clone(),
                                    });
                                    geometry_presets.push(new_preset);
                                    *geometry_presets_changed = true;
                                    ui.close_menu();
                                }
                                for preset in geometry_presets.iter_mut() {
                                    if ui.button(&preset.name).clicked() {
                                        let object_id = preset.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
                                        preset.objects.push(crate::model::GeometryObject {
                                            id: object_id,
                                            name: format!("{:?} {}", step.geometry_spec.shape, object_id),
                                            enabled: true,
                                            spec: step.geometry_spec.clone(),
                                        });
                                        *geometry_presets_changed = true;
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                    }
                    if *request_screen_color_pick {
                        if let Some((_, _, is_fill)) = pending_screen_color_target {
                            *pending_macro_step_color_pick = Some((group_id, macro_preset_id, step_index, is_fill, is_hold_stop));
                        }
                    }
                }
                MacroAction::ShowGeometryPreset => {
                    ui.horizontal(|ui| {
                        ui.label(Self::tr_lang(language, "Preset", "Preset"));
                        Self::render_geometry_preset_selector(
                            ui,
                            language,
                            (id_prefix, "geometry-preset"),
                            preset_options,
                            &mut step.geometry_preset_id,
                            live_sync,
                        );
                    });
                }
                MacroAction::HideGeometryPreset => {
                    ui.horizontal(|ui| {
                        let mut clear_all = step.geometry_preset_id.is_none();
                        let changed = ui.checkbox(&mut clear_all, Self::tr_lang(language, "Clear all", "Xóa hết")).changed();
                        if changed {
                            if clear_all {
                                step.geometry_preset_id = None;
                            } else {
                                step.geometry_preset_id = preset_options.first().map(|o| o.0).or(Some(1));
                            }
                            *live_sync = true;
                        }
                        if !clear_all {
                            ui.label(Self::tr_lang(language, "Preset", "Preset"));
                            Self::render_geometry_preset_selector(
                                ui,
                                language,
                                (id_prefix, "geometry-preset"),
                                preset_options,
                                &mut step.geometry_preset_id,
                                live_sync,
                            );
                        }
                    });
                }
                _ => {}
            });
        });
    }


    fn render_audio_sense_monitor_settings_inline(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_prefix: impl std::hash::Hash + Copy,
        audio_sense_devices: &[String],
        settings: &mut AudioSenseMonitorSettings,
        live_sync: &mut bool,
    ) {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(Self::tr_lang(language, "Source", "Nguon"));
            ComboBox::from_id_salt((id_prefix, "source"))
                .width(108.0)
                .selected_text(match settings.source {
                    AudioSenseSource::System => Self::tr_lang(language, "System", "He thong"),
                    AudioSenseSource::Microphone => Self::tr_lang(language, "Microphone", "Micro"),
                })
                .show_ui(ui, |ui| {
                    *live_sync |= ui
                        .selectable_value(
                            &mut settings.source,
                            AudioSenseSource::System,
                            Self::tr_lang(language, "System", "He thong"),
                        )
                        .changed();
                    *live_sync |= ui
                        .selectable_value(
                            &mut settings.source,
                            AudioSenseSource::Microphone,
                            Self::tr_lang(language, "Microphone", "Micro"),
                        )
                        .changed();
                });
            if settings.source == AudioSenseSource::Microphone {
                ComboBox::from_id_salt((id_prefix, "device"))
                    .width(176.0)
                    .selected_text(settings.input_device_name.clone().unwrap_or_else(|| {
                        Self::tr_lang(language, "Default microphone", "Micro mac dinh").to_owned()
                    }))
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                settings.input_device_name.is_none(),
                                Self::tr_lang(language, "Default microphone", "Micro mac dinh"),
                            )
                            .clicked()
                        {
                            settings.input_device_name = None;
                            *live_sync = true;
                        }
                        for device in audio_sense_devices {
                            if ui
                                .selectable_label(
                                    settings.input_device_name.as_deref() == Some(device.as_str()),
                                    device,
                                )
                                .clicked()
                            {
                                settings.input_device_name = Some(device.clone());
                                *live_sync = true;
                            }
                        }
                    });
            }
            ui.label("Hz");
            *live_sync |= ui
                .add(
                    DragValue::new(&mut settings.updates_per_second)
                        .range(1..=60)
                        .speed(0.2),
                )
                .changed();
        });
    }

    fn render_audio_sense_pitch_custom_step(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_prefix: impl std::hash::Hash + Copy,
        audio_sense_devices: &[String],
        timer_names: &[String],
        step: &mut MacroStep,
        live_sync: &mut bool,
        audio_sense_presets: &mut Vec<AudioSensePreset>,
        audio_sense_presets_changed: &mut bool,
    ) {
        step.audio_sense_spec.kind = AudioSensePresetKind::Pitch;
        ui.vertical(|ui| {
            Self::render_audio_sense_monitor_settings_inline(
                ui,
                language,
                (id_prefix, "pitch-monitor"),
                audio_sense_devices,
                &mut step.audio_sense_spec.pitch.monitor,
                live_sync,
            );
            ui.add_space(4.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                *live_sync |= ui
                    .checkbox(
                        &mut step.audio_sense_spec.pitch.show_sharps,
                        Self::tr_lang(language, "Use sharps", "Dung dau thang"),
                    )
                    .changed();
            });
            ui.add_space(4.0);
            egui::Grid::new(ui.id().with((id_prefix, "pitch-vars-grid")))
                .num_columns(4)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    ui.label(Self::tr_lang(language, "Note", "Note"));
                    *live_sync |= Self::render_audio_sense_var_box(
                        ui,
                        language,
                        ui.id().with((id_prefix, "pitch-note-var")),
                        &mut step.audio_sense_spec.pitch.output_note_var,
                        timer_names,
                        78.0,
                        "note_var",
                    );
                    ui.label(Self::tr_lang(language, "Level", "Muc"));
                    *live_sync |= Self::render_audio_sense_var_box(
                        ui,
                        language,
                        ui.id().with((id_prefix, "pitch-level-var")),
                        &mut step.audio_sense_spec.pitch.output_level_var,
                        timer_names,
                        78.0,
                        "level_var",
                    );
                    ui.end_row();
                    ui.label(Self::tr_lang(language, "Min Conf", "Tin cậy tối thiểu"));
                    *live_sync |= ui
                        .add(
                            DragValue::new(&mut step.audio_sense_spec.pitch.min_confidence)
                                .range(0..=1000)
                                .speed(5.0),
                        )
                        .changed();
                    ui.label(Self::tr_lang(language, "Min Level", "Mức tối thiểu"));
                    *live_sync |= ui
                        .add(
                            DragValue::new(&mut step.audio_sense_spec.pitch.min_level)
                                .range(0..=1000)
                                .speed(5.0),
                        )
                        .changed();
                    ui.end_row();
                });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button(Self::tr_lang(
                        language,
                        "Add as preset",
                        "Them thanh preset",
                    ))
                    .clicked()
                {
                    let next_id = audio_sense_presets
                        .iter()
                        .map(|preset| preset.id)
                        .max()
                        .unwrap_or(0)
                        + 1;
                    let mut preset = AudioSensePreset::new_pitch(next_id);
                    preset.name = format!("Pitch Detect {}", next_id);
                    preset.pitch = step.audio_sense_spec.pitch.clone();
                    audio_sense_presets.push(preset);
                    *audio_sense_presets_changed = true;
                }
            });
        });
    }

    fn render_audio_sense_step_editor(
        ui: &mut egui::Ui,
        language: UiLanguage,
        id_prefix: impl std::hash::Hash + Copy,
        group_id: u32,
        macro_preset_id: u32,
        step_index: usize,
        is_hold_stop: bool,
        preset_options: &[(u32, String)],
        audio_sense_presets: &mut Vec<AudioSensePreset>,
        audio_sense_devices: &[String],
        timer_names: &[String],
        step: &mut MacroStep,
        live_sync: &mut bool,
        audio_sense_presets_changed: &mut bool,
    ) {
        let mut cleared_legacy_defaults = false;
        for var in [
            &mut step.audio_sense_spec.pitch.output_note_var,
            &mut step.audio_sense_spec.pitch.output_level_var,
        ] {
            if matches!(
                var.as_str(),
                "pitch_note" | "pitch_confidence" | "audio_level"
            ) {
                var.clear();
                cleared_legacy_defaults = true;
            }
        }
        if cleared_legacy_defaults {
            *live_sync = true;
        }
        ui.scope(|ui| {
            match step.action {
                MacroAction::StartAudioSensePreset => {
                    let mut start_mode = if step.audio_sense_preset_id.is_some() {
                        0_u8
                    } else {
                        1_u8
                    };
                    ui.vertical(|ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            egui::ComboBox::from_id_salt(ui.id().with((id_prefix, "audio-start-mode")))
                                .selected_text(match start_mode {
                                    0 => Self::tr_lang(language, "Preset", "Preset"),
                                    _ => Self::tr_lang(language, "Pitch", "Pitch"),
                                })
                                .width(90.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut start_mode,
                                        0,
                                        Self::tr_lang(language, "Preset", "Preset"),
                                    );
                                    ui.selectable_value(
                                        &mut start_mode,
                                        1,
                                        Self::tr_lang(language, "Pitch", "Pitch"),
                                    );
                                });
                            match start_mode {
                                0 => {
                                    if step.audio_sense_preset_id.is_none() {
                                        step.audio_sense_preset_id = preset_options.first().map(|(id, _)| *id);
                                        *live_sync = true;
                                    }
                                    Self::render_audio_sense_preset_selector(
                                        ui,
                                        language,
                                        (id_prefix, "audio-preset"),
                                        preset_options,
                                        &mut step.audio_sense_preset_id,
                                        live_sync,
                                    );
                                    ui.label(Self::tr_lang(language, "Note", "Note"));
                                    *live_sync |= Self::render_audio_sense_var_box(
                                        ui,
                                        language,
                                        ui.id().with((id_prefix, "preset-note-var")),
                                        &mut step.audio_sense_spec.pitch.output_note_var,
                                        timer_names,
                                        64.0,
                                        "note_var",
                                    );
                                    ui.label(Self::tr_lang(language, "Level", "Muc"));
                                    *live_sync |= Self::render_audio_sense_var_box(
                                        ui,
                                        language,
                                        ui.id().with((id_prefix, "preset-level-var")),
                                        &mut step.audio_sense_spec.pitch.output_level_var,
                                        timer_names,
                                        62.0,
                                        "level_var",
                                    );
                                }
                                _ => {
                                    if step.audio_sense_preset_id.is_some() || step.audio_sense_spec.kind != AudioSensePresetKind::Pitch {
                                        step.audio_sense_preset_id = None;
                                        step.audio_sense_spec.kind = AudioSensePresetKind::Pitch;
                                        *live_sync = true;
                                    }
                                    ui.add_space(6.0);
                                    *live_sync |= ui
                                        .checkbox(
                                            &mut step.audio_sense_spec.pitch.monitor.permanent,
                                            Self::tr_lang(language, "Permanent", "Vĩnh viễn"),
                                        )
                                        .changed();
                                    if !step.audio_sense_spec.pitch.monitor.permanent {
                                        ui.add_space(2.0);
                                        ui.label("ms");
                                        *live_sync |= ui
                                            .add(
                                                DragValue::new(&mut step.audio_sense_spec.pitch.monitor.duration_ms)
                                                    .range(100..=60_000)
                                                    .speed(10.0),
                                            )
                                            .changed();
                                    }
                                    ui.add_space(6.0);
                                    let collapse_icon = if step.audio_sense_collapsed { 0xe5cc } else { 0xe5cf };
                                    let collapse_btn = Button::new(Self::material_icon_text(collapse_icon, 12.0));
                                    if ui
                                        .add_sized([18.0, 18.0], collapse_btn)
                                        .on_hover_text(if step.audio_sense_collapsed { "Expand" } else { "Collapse" })
                                        .clicked()
                                    {
                                        step.audio_sense_collapsed = !step.audio_sense_collapsed;
                                        *live_sync = true;
                                    }
                                    ui.add_space(2.0);
                                    let is_active = crate::overlay::is_audio_sense_active(
                                        step.audio_sense_preset_id,
                                        macro_preset_id,
                                        step_index,
                                        is_hold_stop,
                                    );
                                    let icon = if is_active { 0xe8f5 } else { 0xe8f4 };
                                    let preview_btn = Button::new(Self::material_icon_text(icon, 12.0));
                                    let preview_response = ui.add_sized([18.0, 18.0], preview_btn);
                                    let tooltip = if is_active {
                                        Self::tr_lang(language, "Stop preview", "Dừng xem trước")
                                    } else {
                                        Self::tr_lang(language, "Preview sound capture", "Xem trước âm thanh")
                                    };
                                    if preview_response.hovered() {
                                        egui::show_tooltip_text(ui.ctx(), ui.layer_id(), preview_response.id, tooltip);
                                    }
                                    if preview_response.clicked() {
                                        if is_active {
                                            crate::overlay::stop_audio_sense(
                                                step.audio_sense_preset_id,
                                                macro_preset_id,
                                                step_index,
                                                is_hold_stop,
                                            );
                                        } else {
                                            crate::overlay::start_audio_sense_preview(
                                                step,
                                                macro_preset_id,
                                                step_index,
                                                is_hold_stop,
                                            );
                                        }
                                        *live_sync = true;
                                    }
                                    if is_active {
                                        ui.ctx().request_repaint();
                                        if let Some(snapshot) = crate::overlay::get_audio_sense_snapshot(
                                            step.audio_sense_preset_id,
                                            macro_preset_id,
                                            step_index,
                                            is_hold_stop,
                                        ) {
                                            ui.add_space(4.0);
                                            let text = format!(
                                                "Note: {} | Conf: {:.2} | Level: {:.2}",
                                                snapshot.note,
                                                snapshot.confidence,
                                                snapshot.level
                                            );
                                            ui.label(
                                                egui::RichText::new(text)
                                                    .size(11.0)
                                                    .color(ui.visuals().weak_text_color())
                                            );
                                        }
                                    }
                                }
                            }
                        });
                        match start_mode {
                            1 if !step.audio_sense_collapsed => {
                                ui.add_space(4.0);
                                Self::render_audio_sense_pitch_custom_step(
                                    ui,
                                    language,
                                    id_prefix,
                                    audio_sense_devices,
                                    timer_names,
                                    step,
                                    live_sync,
                                    audio_sense_presets,
                                    audio_sense_presets_changed,
                                );
                            }
                            _ => {}
                        }
                    });
                }
                MacroAction::StopAudioSense => {
                    ui.horizontal(|ui| {
                        *live_sync |= ui
                            .checkbox(
                                &mut step.audio_sense_stop_all,
                                Self::tr_lang(language, "Stop all", "Dung het"),
                            )
                            .changed();
                        if !step.audio_sense_stop_all {
                            if step.audio_sense_preset_id.is_none() {
                                step.audio_sense_preset_id = preset_options.first().map(|(id, _)| *id);
                                *live_sync = true;
                            }
                            Self::render_audio_sense_preset_selector(
                                ui,
                                language,
                                (id_prefix, "audio-preset"),
                                preset_options,
                                &mut step.audio_sense_preset_id,
                                live_sync,
                            );
                        }
                    });
                }
                _ => {}
            }
        });
    }

    fn variable_value_kind(token: &str) -> VariableValueKind {
        let trimmed = token.trim().trim_matches(|c| c == '{' || c == '}');
        if trimmed.is_empty() {
            return VariableValueKind::Neutral;
        }
        if let Some((base, prop)) = trimmed.split_once('.') {
            let base = base.trim().replace(' ', "").to_ascii_lowercase();
            let prop = prop.trim().to_ascii_lowercase();
            if base.is_empty() || prop.is_empty() {
                return VariableValueKind::Neutral;
            }
            return match base.as_str() {
                "system" => match prop.as_str() {
                    "date" | "time" => VariableValueKind::Text,
                    "year" | "month" | "day" | "hour" | "minute" | "second" | "millisecond"
                    | "ms" => VariableValueKind::Number,
                    _ => VariableValueKind::Neutral,
                },
                "screen" => match prop.as_str() {
                    "width" | "height" | "w" | "h" => VariableValueKind::Number,
                    _ => VariableValueKind::Neutral,
                },
                "mouse" => match prop.as_str() {
                    "x" | "y" | "sensitivity" => VariableValueKind::Number,
                    _ => VariableValueKind::Neutral,
                },
                "window" => match prop.as_str() {
                    "title" => VariableValueKind::Text,
                    "width" | "height" | "w" | "h" => VariableValueKind::Number,
                    _ => VariableValueKind::Neutral,
                },
                "volume" => match prop.as_str() {
                    "level" | "percent" | "value" => VariableValueKind::Number,
                    _ => VariableValueKind::Neutral,
                },
                "clipboard" => match prop.as_str() {
                    "text" => VariableValueKind::Text,
                    _ => VariableValueKind::Neutral,
                },
                s if s.starts_with("timer") => match prop.as_str() {
                    "hour" | "minute" | "second" | "millisecond" | "ms" | "raw" | "total_sec" => {
                        VariableValueKind::Number
                    }
                    _ => VariableValueKind::Neutral,
                },
                _ => VariableValueKind::Neutral,
            };
        }
        if Self::builtin_variable_suggestions()
            .iter()
            .any(|name| name.eq_ignore_ascii_case(trimmed))
            || Self::timer_ref_index(trimmed).is_some()
        {
            return VariableValueKind::Neutral;
        }
        VariableValueKind::Number
    }

    fn variable_value_color(kind: VariableValueKind, default_color: Color32) -> Color32 {
        match kind {
            VariableValueKind::Text => Color32::from_rgb(255, 185, 92),
            VariableValueKind::Number => Color32::from_rgb(86, 198, 255),
            VariableValueKind::Neutral => default_color,
        }
    }

    fn interpolation_highlight_job(
        ui: &egui::Ui,
        text: &str,
        wrap_width: f32,
        text_style: egui::TextStyle,
    ) -> egui::text::LayoutJob {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = wrap_width;
        let font_id = text_style.resolve(ui.style());
        let default_color = ui.visuals().text_color();
        let mut segment_start = 0;
        while let Some(open_rel) = text[segment_start..].find('{') {
            let open_idx = segment_start + open_rel;
            if segment_start < open_idx {
                job.append(
                    &text[segment_start..open_idx],
                    0.0,
                    egui::text::TextFormat::simple(font_id.clone(), default_color),
                );
            }
            let close_search_start = open_idx + 1;
            if let Some(close_rel) = text[close_search_start..].find('}') {
                let close_idx = close_search_start + close_rel;
                let token = &text[open_idx + 1..close_idx];
                let color = if token.trim().is_empty() {
                    default_color
                } else {
                    Self::variable_value_color(Self::variable_value_kind(token), default_color)
                };
                job.append(
                    &text[open_idx..close_idx + 1],
                    0.0,
                    egui::text::TextFormat::simple(font_id.clone(), color),
                );
                segment_start = close_idx + 1;
            } else {
                job.append(
                    &text[open_idx..],
                    0.0,
                    egui::text::TextFormat::simple(font_id.clone(), default_color),
                );
                segment_start = text.len();
                break;
            }
        }
        if segment_start < text.len() {
            job.append(
                &text[segment_start..],
                0.0,
                egui::text::TextFormat::simple(font_id, default_color),
            );
        }
        job
    }

    fn highlight_job_wrap_width(has_focus: bool, multiline_on_focus: bool, wrap_width: f32) -> f32 {
        if has_focus && multiline_on_focus {
            wrap_width
        } else {
            f32::INFINITY
        }
    }

    fn apply_variable_suggestion(
        ui: &mut egui::Ui,
        response: &egui::Response,
        text: &mut String,
        prefix: &str,
        chosen: &str,
        wrap_open: bool,
        after_cursor: &str,
    ) {
        let suffix = if wrap_open && after_cursor.starts_with('}') {
            &after_cursor['}'.len_utf8()..]
        } else {
            after_cursor
        };
        let closing = if wrap_open { "}" } else { "" };
        *text = format!("{}{}{}{}", prefix, chosen, closing, suffix);
        let mut response = response.clone();
        response.mark_changed();
        response.request_focus();
        let prefix_chars = prefix.chars().count();
        let chosen_chars = chosen.chars().count();
        let cursor_char_index = if chosen.ends_with("()") {
            prefix_chars + chosen_chars.saturating_sub(1)
        } else {
            prefix_chars + chosen_chars
        };
        if let Some(mut state) =
            egui::widgets::text_edit::TextEditState::load(ui.ctx(), response.id)
        {
            let cursor_pos = egui::text::CCursor::new(cursor_char_index);
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(cursor_pos, cursor_pos)));
            state.store(ui.ctx(), response.id);
        }
    }

    pub(crate) fn render_variable_suggestions(
        ui: &mut egui::Ui,
        response: &egui::Response,
        text: &mut String,
        timer_names: &[String],
        _language: UiLanguage,
    ) {
        Self::render_variable_suggestions_inner(ui, response, text, timer_names, false);
    }

    fn render_variable_suggestions_braced(
        ui: &mut egui::Ui,
        response: &egui::Response,
        text: &mut String,
        timer_names: &[String],
        _language: UiLanguage,
    ) {
        Self::render_variable_suggestions_inner(ui, response, text, timer_names, true);
    }

    fn render_variable_suggestions_inner(
        ui: &mut egui::Ui,
        response: &egui::Response,
        text: &mut String,
        timer_names: &[String],
        require_wrap_open: bool,
    ) {
        let temp_timer_names = ui.memory(|mem| {
            mem.data
                .get_temp::<Vec<String>>(egui::Id::new("macro_timer_suggestion_names"))
        });
        let timer_names = temp_timer_names.as_deref().unwrap_or(timer_names);
        let suggestion_names = ui
            .memory(|mem| {
                mem.data
                    .get_temp::<Vec<String>>(egui::Id::new("macro_variable_suggestion_names"))
            })
            .unwrap_or_default();
        let cursor_index =
            match egui::widgets::text_edit::TextEditState::load(ui.ctx(), response.id).and_then(
                |state| {
                    state
                        .cursor
                        .char_range()
                        .and_then(|range| range.single().map(|c| c.index))
                },
            ) {
                Some(index) => index,
                None => return,
            };
        let cursor_byte = text
            .char_indices()
            .nth(cursor_index)
            .map(|(byte, _)| byte)
            .unwrap_or(text.len());
        let before_cursor = &text[..cursor_byte];
        let after_cursor = text[cursor_byte..].to_string();
        let mut last_word_start = 0;
        for (i, c) in before_cursor.char_indices() {
            if c.is_whitespace()
                || c == '+'
                || c == '-'
                || c == '*'
                || c == '/'
                || c == '('
                || c == ')'
                || c == ','
                || c == '{'
                || c == '}'
            {
                last_word_start = i + c.len_utf8();
            }
        }
        let prefix = before_cursor[..last_word_start].to_string();
        let last_word_trimmed = before_cursor[last_word_start..].trim().to_string();
        let wrap_open = prefix.ends_with('{');
        if last_word_trimmed.is_empty() {
            return;
        }
        if require_wrap_open && !wrap_open {
            return;
        }
        let mut suggestions = Vec::new();
        if last_word_trimmed.contains('.') {
            let parts: Vec<&str> = last_word_trimmed.split('.').collect();
            let base = parts[0].trim();
            let prop_part = parts[1].to_lowercase();
            let timer_exists = Self::timer_ref_index(base).is_some()
                || timer_names.iter().any(|name| {
                    name.replace(' ', "").to_lowercase() == base.replace(' ', "").to_lowercase()
                });
            if !timer_exists && !Self::variable_base_exists(base, &suggestion_names) {
                return;
            }
            let props: Vec<&str> = if timer_exists {
                vec![
                    "hour",
                    "minute",
                    "second",
                    "millisecond",
                    "ms",
                    "raw",
                    "total_sec",
                ]
            } else {
                Self::object_property_suggestions(base)
                    .map_or_else(Vec::new, |props| props.to_vec())
            };
            for prop in props {
                let full_prop = format!("{}.{}", parts[0], prop);
                if prop.starts_with(&prop_part)
                    && full_prop.to_lowercase() != last_word_trimmed.to_lowercase()
                {
                    suggestions.push(full_prop);
                }
            }
        } else {
            for builtin in Self::builtin_variable_suggestions() {
                if builtin
                    .to_ascii_lowercase()
                    .starts_with(&last_word_trimmed.to_ascii_lowercase())
                    && !builtin.eq_ignore_ascii_case(&last_word_trimmed)
                {
                    suggestions.push((*builtin).to_string());
                }
            }
            for name in &suggestion_names {
                let name_no_space = name.replace(" ", "");
                if name_no_space
                    .to_lowercase()
                    .starts_with(&last_word_trimmed.to_lowercase())
                    && name_no_space.to_lowercase() != last_word_trimmed.to_lowercase()
                {
                    suggestions.push(name_no_space);
                }
            }
            for func in Self::builtin_expression_function_suggestions() {
                if func
                    .to_ascii_lowercase()
                    .starts_with(&last_word_trimmed.to_ascii_lowercase())
                    && !func.eq_ignore_ascii_case(&last_word_trimmed)
                {
                    suggestions.push((*func).to_string());
                }
            }
        }
        if suggestions.is_empty() {
            return;
        }
        let popup_open_key = response.id.with("popup_open");
        let mut popup_open = ui
            .memory(|mem| mem.data.get_temp::<bool>(popup_open_key))
            .unwrap_or(false);
        if response.changed() {
            popup_open = true;
        }
        if response.clicked() {
            popup_open = false;
        }
        if !response.has_focus() {
            popup_open = false;
        }
        if !popup_open {
            ui.memory_mut(|mem| mem.data.insert_temp(popup_open_key, false));
            return;
        }
        let mut selected_index = ui
            .memory(|mem| mem.data.get_temp::<usize>(response.id))
            .unwrap_or(0);
        let mut confirm_selected = false;
        let mut selection_changed = false;
        let sug_count = suggestions.len();
        if selected_index >= sug_count {
            selected_index = 0;
        }
        if response.has_focus() {
            let enter_pressed = ui
                .memory(|mem| mem.data.get_temp::<bool>(egui::Id::new("enter_pressed")))
                .unwrap_or(false);
            if enter_pressed {
                confirm_selected = true;
            }
            let arrow_up_pressed = ui
                .memory(|mem| mem.data.get_temp::<bool>(egui::Id::new("arrow_up_pressed")))
                .unwrap_or(false);
            let arrow_down_pressed = ui
                .memory(|mem| {
                    mem.data
                        .get_temp::<bool>(egui::Id::new("arrow_down_pressed"))
                })
                .unwrap_or(false);
            if arrow_down_pressed {
                selected_index = (selected_index + 1) % sug_count;
                selection_changed = true;
            }
            if arrow_up_pressed {
                selected_index = if selected_index == 0 {
                    sug_count - 1
                } else {
                    selected_index - 1
                };
                selection_changed = true;
            }
            ui.memory_mut(|mem| mem.data.insert_temp(response.id, selected_index));
        }
        if confirm_selected {
            let chosen = &suggestions[selected_index];
            Self::apply_variable_suggestion(
                ui,
                response,
                text,
                &prefix,
                chosen,
                wrap_open,
                &after_cursor,
            );
            ui.memory_mut(|mem| {
                mem.data
                    .insert_temp(egui::Id::new("macro_variable_suggestion_committed"), true);
            });
            popup_open = false;
            ui.memory_mut(|mem| {
                mem.data.insert_temp(popup_open_key, popup_open);
                mem.data.insert_temp(egui::Id::new("enter_pressed"), false);
            });
            return;
        }
        let popup_id = response.id.with("sug_popup");
        let popup_position = response.rect.left_bottom();
        let mut clicked_choice: Option<String> = None;
        let popup_max_height =
            (ui.ctx().content_rect().bottom() - popup_position.y - 8.0).max(120.0);
        let area_res = egui::Area::new(popup_id)
            .order(egui::Order::Foreground)
            .fixed_pos(popup_position)
            .show(ui.ctx(), |ui| {
                let frame_res = egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(200.0);
                    egui::ScrollArea::vertical()
                        .max_height(popup_max_height)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                for (idx, sug) in suggestions.iter().enumerate() {
                                    let is_selected = idx == selected_index;
                                    let label = Self::expression_suggestion_label(sug, timer_names);
                                    let color = match Self::variable_value_kind(sug) {
                                        VariableValueKind::Text => Color32::from_rgb(255, 185, 92),
                                        VariableValueKind::Number => {
                                            Color32::from_rgb(86, 198, 255)
                                        }
                                        VariableValueKind::Neutral => ui.visuals().text_color(),
                                    };
                                    let mut resp = ui.selectable_label(
                                        is_selected,
                                        RichText::new(label).color(color),
                                    );
                                    if is_selected && selection_changed {
                                        resp.scroll_to_me(None);
                                    }
                                    if resp.clicked() {
                                        clicked_choice = Some(sug.clone());
                                    }
                                }
                            });
                        });
                });
                let rect = frame_res.response.rect;
                ui.memory_mut(|mem| mem.data.insert_temp(response.id.with("popup_rect"), rect));
            });
        if let Some(chosen) = clicked_choice {
            Self::apply_variable_suggestion(
                ui,
                response,
                text,
                &prefix,
                &chosen,
                wrap_open,
                &after_cursor,
            );
            ui.memory_mut(|mem| {
                mem.data
                    .insert_temp(egui::Id::new("macro_variable_suggestion_committed"), true);
            });
            popup_open = false;
            ui.memory_mut(|mem| {
                mem.data.insert_temp(popup_open_key, popup_open);
                mem.data.insert_temp(egui::Id::new("enter_pressed"), false);
            });
            return;
        }
        ui.memory_mut(|mem| {
            mem.data.insert_temp(popup_open_key, popup_open);
            mem.data.insert_temp(egui::Id::new("any_popup_open"), true);
        });
    }

    fn render_variable_suggestions_raw(
        ui: &mut egui::Ui,
        response: &egui::Response,
        text: &mut String,
        timer_names: &[String],
        _language: UiLanguage,
    ) {
        let suggestion_names = ui
            .memory(|mem| {
                mem.data.get_temp::<Vec<String>>(egui::Id::new(
                    "macro_variable_writable_suggestion_names",
                ))
            })
            .unwrap_or_default();
        let cursor_index =
            match egui::widgets::text_edit::TextEditState::load(ui.ctx(), response.id).and_then(
                |state| {
                    state
                        .cursor
                        .char_range()
                        .and_then(|range| range.single().map(|c| c.index))
                },
            ) {
                Some(index) => index,
                None => return,
            };
        let cursor_byte = text
            .char_indices()
            .nth(cursor_index)
            .map(|(byte, _)| byte)
            .unwrap_or(text.len());
        let before_cursor = &text[..cursor_byte];
        let after_cursor = text[cursor_byte..].to_string();
        let mut last_word_start = 0;
        for (i, c) in before_cursor.char_indices() {
            if c.is_whitespace()
                || c == '+'
                || c == '-'
                || c == '*'
                || c == '/'
                || c == '('
                || c == ')'
                || c == ','
                || c == '{'
                || c == '}'
            {
                last_word_start = i + c.len_utf8();
            }
        }
        let prefix = before_cursor[..last_word_start].to_string();
        let last_word = before_cursor[last_word_start..].trim().to_string();
        let wrap_open = prefix.ends_with('{');
        if last_word.is_empty() {
            return;
        }
        let mut suggestions = Vec::new();
        if last_word.contains('.') {
            let parts: Vec<&str> = last_word.split('.').collect();
            let prop_part = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
            if Self::timer_ref_index(parts[0]).is_some() {
                for prop in [
                    "hour",
                    "minute",
                    "second",
                    "millisecond",
                    "ms",
                    "raw",
                    "total_sec",
                ] {
                    let full_prop = format!("{}.{}", parts[0], prop);
                    if prop.starts_with(&prop_part)
                        && full_prop.to_lowercase() != last_word.to_lowercase()
                    {
                        suggestions.push(full_prop);
                    }
                }
            }
        } else {
            for name in &suggestion_names {
                let name_no_space = name.replace(" ", "");
                if name_no_space
                    .to_lowercase()
                    .starts_with(&last_word.to_lowercase())
                    && name_no_space.to_lowercase() != last_word.to_lowercase()
                {
                    suggestions.push(name_no_space);
                }
            }
            for func in Self::builtin_expression_function_suggestions() {
                if func
                    .to_ascii_lowercase()
                    .starts_with(&last_word.to_ascii_lowercase())
                    && !func.eq_ignore_ascii_case(&last_word)
                {
                    suggestions.push((*func).to_string());
                }
            }
        }
        if suggestions.is_empty() {
            return;
        }
        let popup_open_key = response.id.with("popup_open_raw");
        let mut popup_open = ui
            .memory(|mem| mem.data.get_temp::<bool>(popup_open_key))
            .unwrap_or(false);
        if response.changed() {
            popup_open = true;
        }
        if response.clicked() {
            popup_open = false;
        }
        if !response.has_focus() {
            popup_open = false;
        }
        if !popup_open {
            ui.memory_mut(|mem| mem.data.insert_temp(popup_open_key, false));
            return;
        }
        let mut selected_index = ui
            .memory(|mem| mem.data.get_temp::<usize>(response.id))
            .unwrap_or(0);
        let mut confirm_selected = false;
        let mut selection_changed = false;
        let sug_count = suggestions.len();
        if selected_index >= sug_count {
            selected_index = 0;
        }
        if response.has_focus() {
            let enter_pressed = ui
                .memory(|mem| mem.data.get_temp::<bool>(egui::Id::new("enter_pressed")))
                .unwrap_or(false);
            if enter_pressed {
                confirm_selected = true;
            }
            let arrow_up_pressed = ui
                .memory(|mem| mem.data.get_temp::<bool>(egui::Id::new("arrow_up_pressed")))
                .unwrap_or(false);
            let arrow_down_pressed = ui
                .memory(|mem| {
                    mem.data
                        .get_temp::<bool>(egui::Id::new("arrow_down_pressed"))
                })
                .unwrap_or(false);
            if arrow_down_pressed {
                selected_index = (selected_index + 1) % sug_count;
                selection_changed = true;
            }
            if arrow_up_pressed {
                selected_index = if selected_index == 0 {
                    sug_count - 1
                } else {
                    selected_index - 1
                };
                selection_changed = true;
            }
            ui.memory_mut(|mem| mem.data.insert_temp(response.id, selected_index));
        }
        if confirm_selected {
            let chosen = &suggestions[selected_index];
            Self::apply_variable_suggestion(
                ui,
                response,
                text,
                &prefix,
                chosen,
                wrap_open,
                &after_cursor,
            );
            popup_open = false;
            ui.memory_mut(|mem| {
                mem.data.insert_temp(popup_open_key, popup_open);
                mem.data.insert_temp(egui::Id::new("enter_pressed"), false);
            });
            return;
        }
        let popup_id = response.id.with("sug_popup_raw");
        let popup_position = response.rect.left_bottom();
        let mut clicked_choice: Option<String> = None;
        let popup_max_height =
            (ui.ctx().content_rect().bottom() - popup_position.y - 8.0).max(120.0);
        let area_res = egui::Area::new(popup_id)
            .order(egui::Order::Foreground)
            .fixed_pos(popup_position)
            .show(ui.ctx(), |ui| {
                let frame_res = egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(200.0);
                    egui::ScrollArea::vertical()
                        .max_height(popup_max_height)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                for (idx, sug) in suggestions.iter().enumerate() {
                                    let is_selected = idx == selected_index;
                                    let label = Self::expression_suggestion_label(sug, timer_names);
                                    let color = match Self::variable_value_kind(sug) {
                                        VariableValueKind::Text => Color32::from_rgb(255, 185, 92),
                                        VariableValueKind::Number => {
                                            Color32::from_rgb(86, 198, 255)
                                        }
                                        VariableValueKind::Neutral => ui.visuals().text_color(),
                                    };
                                    let mut resp = ui.selectable_label(
                                        is_selected,
                                        RichText::new(label).color(color),
                                    );
                                    if is_selected && selection_changed {
                                        resp.scroll_to_me(None);
                                    }
                                    if resp.clicked() {
                                        clicked_choice = Some(sug.clone());
                                    }
                                }
                            });
                        });
                });
                let rect = frame_res.response.rect;
                ui.memory_mut(|mem| {
                    mem.data
                        .insert_temp(response.id.with("popup_rect_raw"), rect)
                });
            });
        if let Some(chosen) = clicked_choice {
            Self::apply_variable_suggestion(
                ui,
                response,
                text,
                &prefix,
                &chosen,
                wrap_open,
                &after_cursor,
            );
            popup_open = false;
            ui.memory_mut(|mem| {
                mem.data.insert_temp(popup_open_key, popup_open);
                mem.data.insert_temp(egui::Id::new("enter_pressed"), false);
            });
            return;
        }
        ui.memory_mut(|mem| {
            mem.data.insert_temp(popup_open_key, popup_open);
            mem.data.insert_temp(egui::Id::new("any_popup_open"), true);
        });
    }

    fn render_expandable_text_edit_impl(
        ui: &mut egui::Ui,
        text: &mut String,
        id: egui::Id,
        normal_width: f32,
        expanded_width: f32,
        normal_height: f32,
        expanded_height: f32,
        hint: &str,
        multiline_on_focus: bool,
        highlight_mode: TextHighlightMode,
    ) -> egui::Response {
        let focus_key = id.with("expand-focus");
        let has_focus = ui
            .memory(|mem| mem.data.get_temp::<bool>(focus_key))
            .unwrap_or(false);
        let target_width = if has_focus {
            expanded_width
        } else {
            normal_width
        };
        // Calculate dynamic height based on text content when focused
        let normal_row_height = normal_height;
        let expanded_row_height = expanded_height;
        let target_height = if has_focus {
            if !multiline_on_focus {
                expanded_row_height
            } else {
                let chars_per_line = ((expanded_width / 7.2) as usize).max(10);
                let mut estimated_rows = 0;
                for line in text.split('\n') {
                    let line_len = line.chars().count();
                    estimated_rows += 1 + line_len / chars_per_line;
                }
                let rows = estimated_rows.clamp(1, 12);
                if rows > 1 {
                    (rows as f32 * 20.0 + 6.0).max(expanded_row_height)
                } else {
                    expanded_row_height
                }
            }
        } else {
            normal_row_height
        };
        let animated_width = ui
            .ctx()
            .animate_value_with_time(id.with("w"), target_width, 0.20);
        let animated_height = ui
            .ctx()
            .animate_value_with_time(id.with("h"), target_height, 0.20);
        let is_multiline = has_focus
            && (multiline_on_focus
                || (text.chars().count() > (expanded_width / 7.2) as usize)
                || text.contains('\n'));
        let text_edit = if is_multiline {
            let chars_per_line = ((expanded_width / 7.2) as usize).max(10);
            let mut estimated_rows = 0;
            for line in text.split('\n') {
                let line_len = line.chars().count();
                estimated_rows += 1 + line_len / chars_per_line;
            }
            let rows = estimated_rows.clamp(1, 12);
            egui::TextEdit::multiline(text)
                .hint_text(hint)
                .desired_rows(rows)
                .font(egui::TextStyle::Body)
                .id(id)
        } else {
            egui::TextEdit::singleline(text)
                .hint_text(hint)
                .font(egui::TextStyle::Body)
                .id(id)
        };
        // Temporarily clear override_text_color so hint/placeholder text is properly dimmed.
        // Preset cards set override_text_color for their content, which bleeds into TextEdit
        // and makes hint text appear at full brightness instead of the dimmed weak_text_color.
        let prev_override = ui.visuals().override_text_color;
        ui.visuals_mut().override_text_color = None;
        let current_interact_height = ui.spacing().interact_size.y;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(animated_width, current_interact_height),
            egui::Sense::hover(),
        );
        let top_y = if animated_height > normal_height + 0.1 {
            rect.min.y
        } else {
            rect.min.y + (rect.height() - animated_height) / 2.0
        };
        let textbox_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, top_y),
            egui::vec2(animated_width, animated_height),
        );
        let prev_interact_y = ui.spacing().interact_size.y;
        let prev_padding = ui.spacing().button_padding;
        ui.spacing_mut().interact_size.y = animated_height;
        let font_size = 16.0;
        ui.spacing_mut().button_padding.y = ((animated_height - font_size) / 2.0 - 0.5).max(0.0);
        let response = match highlight_mode {
            TextHighlightMode::None => ui.put(textbox_rect, text_edit),
            TextHighlightMode::VariableTokens => {
                let mut layouter = |ui: &egui::Ui, string: &dyn TextBuffer, wrap_width: f32| {
                    let effective_wrap_width =
                        Self::highlight_job_wrap_width(has_focus, multiline_on_focus, wrap_width);
                    let text_style = egui::TextStyle::Body;
                    let job = Self::interpolation_highlight_job(
                        ui,
                        string.as_str(),
                        effective_wrap_width,
                        text_style,
                    );
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
                ui.put(textbox_rect, text_edit.layouter(&mut layouter))
            }
            TextHighlightMode::Interpolations => {
                let mut layouter = |ui: &egui::Ui, string: &dyn TextBuffer, wrap_width: f32| {
                    let effective_wrap_width =
                        Self::highlight_job_wrap_width(has_focus, multiline_on_focus, wrap_width);
                    let text_style = egui::TextStyle::Body;
                    let job = Self::interpolation_highlight_job(
                        ui,
                        string.as_str(),
                        effective_wrap_width,
                        text_style,
                    );
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
                ui.put(textbox_rect, text_edit.layouter(&mut layouter))
            }
        };
        ui.spacing_mut().interact_size.y = prev_interact_y;
        ui.spacing_mut().button_padding = prev_padding;
        ui.visuals_mut().override_text_color = prev_override;
        let now_focused = response.has_focus();
        if now_focused != has_focus {
            ui.memory_mut(|mem| mem.data.insert_temp(focus_key, now_focused));
        }
        response
    }

    fn render_expandable_text_edit(
        ui: &mut egui::Ui,
        text: &mut String,
        id: egui::Id,
        normal_width: f32,
        expanded_width: f32,
        normal_height: f32,
        expanded_height: f32,
        hint: &str,
        multiline_on_focus: bool,
    ) -> egui::Response {
        Self::render_expandable_text_edit_impl(
            ui,
            text,
            id,
            normal_width,
            expanded_width,
            normal_height,
            expanded_height,
            hint,
            multiline_on_focus,
            TextHighlightMode::None,
        )
    }

    pub(crate) fn render_variable_text_edit(
        ui: &mut egui::Ui,
        text: &mut String,
        id: egui::Id,
        normal_width: f32,
        expanded_width: f32,
        normal_height: f32,
        expanded_height: f32,
        hint: &str,
        multiline_on_focus: bool,
    ) -> egui::Response {
        Self::render_expandable_text_edit_impl(
            ui,
            text,
            id,
            normal_width,
            expanded_width,
            normal_height,
            expanded_height,
            hint,
            multiline_on_focus,
            TextHighlightMode::VariableTokens,
        )
    }

    pub(crate) fn render_interpolated_text_edit(
        ui: &mut egui::Ui,
        text: &mut String,
        id: egui::Id,
        normal_width: f32,
        expanded_width: f32,
        normal_height: f32,
        expanded_height: f32,
        hint: &str,
        multiline_on_focus: bool,
    ) -> egui::Response {
        Self::render_expandable_text_edit_impl(
            ui,
            text,
            id,
            normal_width,
            expanded_width,
            normal_height,
            expanded_height,
            hint,
            multiline_on_focus,
            TextHighlightMode::Interpolations,
        )
    }

    pub(crate) fn render_plain_text_edit(
        ui: &mut egui::Ui,
        text: &mut String,
        id: egui::Id,
        normal_width: f32,
        expanded_width: f32,
        normal_height: f32,
        expanded_height: f32,
        hint: &str,
        multiline_on_focus: bool,
    ) -> egui::Response {
        Self::render_expandable_text_edit_impl(
            ui,
            text,
            id,
            normal_width,
            expanded_width,
            normal_height,
            expanded_height,
            hint,
            multiline_on_focus,
            TextHighlightMode::None,
        )
    }

    fn render_compact_plain_text_edit_at_rect(
        ui: &mut egui::Ui,
        rect: Rect,
        text: &mut String,
        id: egui::Id,
        hint: &str,
        highlight_mode: TextHighlightMode,
    ) -> egui::Response {
        let prev_override = ui.visuals().override_text_color;
        ui.visuals_mut().override_text_color = None;
        let prev_interact_y = ui.spacing().interact_size.y;
        let prev_padding = ui.spacing().button_padding;
        let height = rect.height();
        ui.spacing_mut().interact_size.y = height;
        let font_size = 15.5; // Monospace font size
        ui.spacing_mut().button_padding.y = ((height - font_size) / 2.0 - 0.5).max(0.0);
        let text_edit = egui::TextEdit::singleline(text)
            .font(egui::TextStyle::Monospace)
            .hint_text(hint)
            .id(id);
        let response = match highlight_mode {
            TextHighlightMode::None => ui.put(rect, text_edit),
            TextHighlightMode::VariableTokens => {
                let mut layouter = |ui: &egui::Ui, string: &dyn TextBuffer, wrap_width: f32| {
                    let effective_wrap_width =
                        Self::highlight_job_wrap_width(false, false, wrap_width);
                    let job = Self::interpolation_highlight_job(
                        ui,
                        string.as_str(),
                        effective_wrap_width,
                        egui::TextStyle::Monospace,
                    );
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
                ui.put(rect, text_edit.layouter(&mut layouter))
            }
            TextHighlightMode::Interpolations => {
                let mut layouter = |ui: &egui::Ui, string: &dyn TextBuffer, wrap_width: f32| {
                    let effective_wrap_width =
                        Self::highlight_job_wrap_width(false, false, wrap_width);
                    let job = Self::interpolation_highlight_job(
                        ui,
                        string.as_str(),
                        effective_wrap_width,
                        egui::TextStyle::Monospace,
                    );
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
                ui.put(rect, text_edit.layouter(&mut layouter))
            }
        };
        ui.spacing_mut().interact_size.y = prev_interact_y;
        ui.spacing_mut().button_padding = prev_padding;
        ui.visuals_mut().override_text_color = prev_override;
        response
    }

    fn render_coordinate_compact_pair(
        ui: &mut egui::Ui,
        x_text: &mut String,
        x_id: egui::Id,
        x_hint: &str,
        y_text: &mut String,
        y_id: egui::Id,
        y_hint: &str,
        width: f32,
        height: f32,
    ) -> (egui::Response, egui::Response) {
        let label_width = 12.0;
        let inner_gap = 4.0;
        let pair_gap = 4.0;
        let pair_width = (label_width + inner_gap + width) * 2.0 + pair_gap;
        let (pair_rect, _) = ui.allocate_exact_size(vec2(pair_width, height), egui::Sense::hover());
        let target_height = 21.0;
        let y_offset = (height - target_height) / 2.0;
        let x_label_rect = Rect::from_min_size(
            pos2(pair_rect.min.x, pair_rect.min.y + y_offset),
            vec2(label_width, target_height),
        );
        let x_input_rect = Rect::from_min_size(
            pos2(
                pair_rect.min.x + label_width + inner_gap,
                pair_rect.min.y + y_offset,
            ),
            vec2(width, target_height),
        );
        let y_label_x = pair_rect.min.x + label_width + inner_gap + width + pair_gap;
        let y_label_rect = Rect::from_min_size(
            pos2(y_label_x, pair_rect.min.y + y_offset),
            vec2(label_width, target_height),
        );
        let y_input_rect = Rect::from_min_size(
            pos2(
                y_label_x + label_width + inner_gap,
                pair_rect.min.y + y_offset,
            ),
            vec2(width, target_height),
        );
        ui.put(
            x_label_rect,
            egui::Label::new(
                egui::RichText::new("X")
                    .strong()
                    .color(Color32::from_rgb(86, 198, 255)),
            ),
        );
        let x_response = Self::render_compact_plain_text_edit_at_rect(
            ui,
            x_input_rect,
            x_text,
            x_id,
            x_hint,
            TextHighlightMode::Interpolations,
        );
        ui.put(
            y_label_rect,
            egui::Label::new(
                egui::RichText::new("Y")
                    .strong()
                    .color(Color32::from_rgb(255, 185, 92)),
            ),
        );
        let y_response = Self::render_compact_plain_text_edit_at_rect(
            ui,
            y_input_rect,
            y_text,
            y_id,
            y_hint,
            TextHighlightMode::Interpolations,
        );
        (x_response, y_response)
    }

    fn render_expandable_command_text_edit(
        ui: &mut egui::Ui,
        text: &mut String,
        id: egui::Id,
        hint: &str,
    ) -> egui::Response {
        let focus_key = id.with("expand-focus");
        let has_focus = ui
            .memory(|mem| mem.data.get_temp::<bool>(focus_key))
            .unwrap_or(false);
        let target_height = if has_focus { 160.0 } else { 72.0 };
        let animated_height = ui
            .ctx()
            .animate_value_with_time(id.with("h"), target_height, 0.20);
        let response = ui.add_sized(
            [300.0, animated_height],
            egui::TextEdit::multiline(text)
                .hint_text(hint)
                .desired_rows(if has_focus { 7 } else { 3 })
                .id(id),
        );
        let now_focused = response.has_focus();
        if now_focused != has_focus {
            ui.memory_mut(|mem| mem.data.insert_temp(focus_key, now_focused));
        }
        response
    }
}
