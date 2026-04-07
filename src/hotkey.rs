use eframe::egui::{Key, Modifiers};
use std::collections::HashSet;

use crate::model::HotkeyBinding;

#[allow(dead_code)]
pub fn capture_from_egui(key: Key, modifiers: Modifiers) -> Option<HotkeyBinding> {
    let key_name = key_to_name(key)?;
    let mut combo_keys = Vec::new();
    if modifiers.ctrl || modifiers.command {
        combo_keys.push("Ctrl".to_owned());
    }
    if modifiers.alt {
        combo_keys.push("Alt".to_owned());
    }
    if modifiers.shift {
        combo_keys.push("Shift".to_owned());
    }
    if modifiers.mac_cmd {
        combo_keys.push("Win".to_owned());
    }
    combo_keys.push(key_name.to_owned());
    binding_from_keys(combo_keys)
}

pub fn format_binding(binding: Option<&HotkeyBinding>) -> String {
    let Some(binding) = binding else {
        return "Not set".to_owned();
    };

    let parts = binding_key_names(binding);

    if parts.is_empty() {
        "Not set".to_owned()
    } else {
        parts.join("+")
    }
}

pub fn format_key_list(spec: &str) -> String {
    let keys = split_key_list(spec);
    if keys.is_empty() {
        "Not set".to_owned()
    } else {
        keys.join(", ")
    }
}

pub fn is_modifier_key_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "ctrl" | "control" | "alt" | "shift" | "win" | "meta"
    )
}

pub fn split_key_list(spec: &str) -> Vec<String> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    trimmed
        .split(|ch: char| matches!(ch, ',' | ';' | '+' | ' ' | '\t' | '\n'))
        .filter_map(|part| {
            let key = part.trim();
            (!key.is_empty()).then(|| normalize_key_name(key))
        })
        .collect()
}

pub fn key_list_contains(spec: &str, key_name: &str) -> bool {
    split_key_list(spec)
        .iter()
        .any(|item| item.eq_ignore_ascii_case(key_name))
}

pub fn parse_binding(spec: &str) -> Option<HotkeyBinding> {
    let mut combo_keys = split_key_list(spec);
    if combo_keys.is_empty() {
        return None;
    }
    binding_from_keys(std::mem::take(&mut combo_keys))
}

pub fn split_binding_list(spec: &str) -> Vec<String> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    trimmed
        .split(|ch: char| matches!(ch, ',' | ';' | '|' | '\n' | '\r'))
        .filter_map(|part| {
            let binding = part.trim();
            (!binding.is_empty()).then(|| binding.to_owned())
        })
        .collect()
}

pub fn parse_binding_list(spec: &str) -> Vec<HotkeyBinding> {
    split_binding_list(spec)
        .into_iter()
        .filter_map(|entry| parse_binding(&entry))
        .collect()
}

pub fn format_binding_list(bindings: &[HotkeyBinding]) -> String {
    let entries = bindings
        .iter()
        .map(|binding| format_binding(Some(binding)))
        .filter(|label| label != "Not set")
        .collect::<Vec<_>>();
    if entries.is_empty() {
        "Not set".to_owned()
    } else {
        entries.join(", ")
    }
}

pub fn binding_list_matches(spec: &str, observed: &HotkeyBinding) -> bool {
    parse_binding_list(spec)
        .iter()
        .any(|binding| binding_matches(binding, observed))
}

pub fn append_binding_to_list(spec: &mut String, binding: &HotkeyBinding) -> bool {
    let normalized = format_binding(Some(binding));
    if normalized == "Not set" {
        return false;
    }
    let entries = split_binding_list(spec);
    if entries
        .iter()
        .filter_map(|entry| parse_binding(entry))
        .any(|existing| binding_matches(&existing, binding))
    {
        return false;
    }

    if spec.trim().is_empty() {
        *spec = normalized;
    } else {
        spec.push_str(", ");
        spec.push_str(&normalized);
    }
    true
}

pub fn pop_binding_list_entry(spec: &mut String) -> bool {
    let mut entries = split_binding_list(spec);
    let Some(_) = entries.pop() else {
        return false;
    };
    *spec = entries.join(", ");
    true
}

#[allow(dead_code)]
pub fn is_mouse_key_name(name: &str) -> bool {
    matches!(
        name,
        "MouseLeft"
            | "MouseRight"
            | "MouseMiddle"
            | "MouseX1"
            | "MouseX2"
            | "MouseWheelUp"
            | "MouseWheelDown"
    )
}

pub fn binding_key_names(binding: &HotkeyBinding) -> Vec<String> {
    let keys = if binding.combo_keys.is_empty() {
        let mut legacy = Vec::new();
        if binding.ctrl {
            legacy.push("Ctrl".to_owned());
        }
        if binding.alt {
            legacy.push("Alt".to_owned());
        }
        if binding.shift {
            legacy.push("Shift".to_owned());
        }
        if binding.win {
            legacy.push("Win".to_owned());
        }
        if !binding.key.trim().is_empty() {
            legacy.push(binding.key.trim().to_owned());
        }
        legacy
    } else {
        binding.combo_keys.clone()
    };

    let mut seen = HashSet::new();
    keys.into_iter()
        .filter_map(|key| {
            let normalized = normalize_key_name(&key);
            let lower = normalized.trim().to_ascii_lowercase();
            if lower.is_empty() || !seen.insert(lower) {
                None
            } else {
                Some(normalized)
            }
        })
        .collect()
}

pub fn binding_matches(expected: &HotkeyBinding, observed: &HotkeyBinding) -> bool {
    let expected_keys = binding_key_signature(expected);
    let observed_keys = binding_key_signature(observed);
    !expected_keys.is_empty() && expected_keys == observed_keys
}

fn binding_key_signature(binding: &HotkeyBinding) -> Vec<String> {
    let mut keys = binding_key_names(binding)
        .into_iter()
        .map(|key| key.to_ascii_lowercase())
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn binding_from_keys(mut combo_keys: Vec<String>) -> Option<HotkeyBinding> {
    combo_keys.retain(|key| !key.trim().is_empty());
    if combo_keys.is_empty() {
        return None;
    }
    let mut seen = HashSet::new();
    combo_keys.retain(|key| seen.insert(key.to_ascii_lowercase()));
    let key = combo_keys
        .iter()
        .rev()
        .find(|key| !is_modifier_key_name(key))
        .cloned()
        .or_else(|| combo_keys.last().cloned())
        .unwrap_or_default();
    Some(HotkeyBinding {
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
    })
}

#[allow(dead_code)]
fn key_to_name(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::ArrowDown => "Down",
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        Key::ArrowUp => "Up",
        Key::Escape => "Escape",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Enter => "Enter",
        Key::Space => "Space",
        Key::Insert => "Insert",
        Key::Delete => "Delete",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::F13 => "F13",
        Key::F14 => "F14",
        Key::F15 => "F15",
        Key::F16 => "F16",
        Key::F17 => "F17",
        Key::F18 => "F18",
        Key::F19 => "F19",
        Key::F20 => "F20",
        Key::F21 => "F21",
        Key::F22 => "F22",
        Key::F23 => "F23",
        Key::F24 => "F24",
        _ => return None,
    })
}

fn normalize_key_name(key: &str) -> String {
    let trimmed = key.trim();
    if let Some(vk) = key_name_to_vk(trimmed)
        && let Some(name) = vk_to_key_name(vk)
    {
        return name.to_owned();
    }
    trimmed.to_owned()
}

#[cfg(windows)]
#[allow(dead_code)]
pub fn to_windows_registration(
    binding: &HotkeyBinding,
) -> Option<(
    windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS,
    u32,
)> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
    };

    let keys = binding_key_names(binding);
    let mut modifiers = HOT_KEY_MODIFIERS(0);
    let mut vk = None;
    for key in keys {
        match key.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" | "meta" => modifiers |= MOD_WIN,
            _ => {
                if is_mouse_key_name(&key) {
                    return None;
                }
                if vk.is_some() {
                    return None;
                }
                vk = key_name_to_vk(&key);
            }
        }
    }

    vk.map(|vk| (modifiers, vk))
}

#[cfg(windows)]
pub fn key_name_to_vk(name: &str) -> Option<u32> {
    Some(match name.to_ascii_uppercase().as_str() {
        "MOUSELEFT" | "LEFTBUTTON" | "LBUTTON" | "MOUSE LEFT" => 0x01,
        "MOUSERIGHT" | "RIGHTBUTTON" | "RBUTTON" | "MOUSE RIGHT" => 0x02,
        "MOUSEMIDDLE" | "MIDDLEBUTTON" | "MBUTTON" | "MOUSE MIDDLE" => 0x04,
        "MOUSEX1" | "XBUTTON1" | "MOUSE X1" => 0x05,
        "MOUSEX2" | "XBUTTON2" | "MOUSE X2" => 0x06,
        "CTRL" | "CONTROL" => 0x11,
        "ALT" => 0x12,
        "SHIFT" => 0x10,
        "WIN" | "META" => 0x5B,
        "A" => b'A' as u32,
        "B" => b'B' as u32,
        "C" => b'C' as u32,
        "D" => b'D' as u32,
        "E" => b'E' as u32,
        "F" => b'F' as u32,
        "G" => b'G' as u32,
        "H" => b'H' as u32,
        "I" => b'I' as u32,
        "J" => b'J' as u32,
        "K" => b'K' as u32,
        "L" => b'L' as u32,
        "M" => b'M' as u32,
        "N" => b'N' as u32,
        "O" => b'O' as u32,
        "P" => b'P' as u32,
        "Q" => b'Q' as u32,
        "R" => b'R' as u32,
        "S" => b'S' as u32,
        "T" => b'T' as u32,
        "U" => b'U' as u32,
        "V" => b'V' as u32,
        "W" => b'W' as u32,
        "X" => b'X' as u32,
        "Y" => b'Y' as u32,
        "Z" => b'Z' as u32,
        "0" => b'0' as u32,
        "1" => b'1' as u32,
        "2" => b'2' as u32,
        "3" => b'3' as u32,
        "4" => b'4' as u32,
        "5" => b'5' as u32,
        "6" => b'6' as u32,
        "7" => b'7' as u32,
        "8" => b'8' as u32,
        "9" => b'9' as u32,
        "SPACE" => 0x20,
        "BACKSPACE" => 0x08,
        "ENTER" => 0x0D,
        "TAB" => 0x09,
        "ESCAPE" => 0x1B,
        "INSERT" => 0x2D,
        "DELETE" => 0x2E,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" => 0x21,
        "PAGEDOWN" => 0x22,
        "LEFT" => 0x25,
        "UP" => 0x26,
        "RIGHT" => 0x27,
        "DOWN" => 0x28,
        "CAPSLOCK" => 0x14,
        "NUMLOCK" => 0x90,
        "SCROLLLOCK" => 0x91,
        "PRINTSCREEN" => 0x2C,
        "PAUSE" => 0x13,
        "APPS" | "MENU" => 0x5D,
        "NUMPAD0" => 0x60,
        "NUMPAD1" => 0x61,
        "NUMPAD2" => 0x62,
        "NUMPAD3" => 0x63,
        "NUMPAD4" => 0x64,
        "NUMPAD5" => 0x65,
        "NUMPAD6" => 0x66,
        "NUMPAD7" => 0x67,
        "NUMPAD8" => 0x68,
        "NUMPAD9" => 0x69,
        "NUMPADMULTIPLY" => 0x6A,
        "NUMPADADD" => 0x6B,
        "NUMPADSUBTRACT" => 0x6D,
        "NUMPADDECIMAL" => 0x6E,
        "NUMPADDIVIDE" => 0x6F,
        ";" => 0xBA,
        "=" => 0xBB,
        "," => 0xBC,
        "-" => 0xBD,
        "." => 0xBE,
        "/" => 0xBF,
        "`" => 0xC0,
        "[" => 0xDB,
        "\\" => 0xDC,
        "]" => 0xDD,
        "'" => 0xDE,
        value if value.starts_with('F') => {
            let number = value.trim_start_matches('F').parse::<u32>().ok()?;
            if !(1..=24).contains(&number) {
                return None;
            }
            0x70 + (number - 1)
        }
        _ => return None,
    })
}

#[cfg(windows)]
pub fn vk_to_key_name(vk: u32) -> Option<&'static str> {
    Some(match vk {
        0x01 => "MouseLeft",
        0x02 => "MouseRight",
        0x04 => "MouseMiddle",
        0x05 => "MouseX1",
        0x06 => "MouseX2",
        0x10 | 0xA0 | 0xA1 => "Shift",
        0x11 | 0xA2 | 0xA3 => "Ctrl",
        0x12 | 0xA4 | 0xA5 => "Alt",
        0x5B | 0x5C => "Win",
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x13 => "Pause",
        0x14 => "CapsLock",
        0x1B => "Escape",
        0x20 => "Space",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2C => "PrintScreen",
        0x2D => "Insert",
        0x2E => "Delete",
        0x5D => "Apps",
        0x90 => "NumLock",
        0x91 => "ScrollLock",
        0x30..=0x39 => match vk as u8 as char {
            '0' => "0",
            '1' => "1",
            '2' => "2",
            '3' => "3",
            '4' => "4",
            '5' => "5",
            '6' => "6",
            '7' => "7",
            '8' => "8",
            '9' => "9",
            _ => return None,
        },
        0x41..=0x5A => match vk as u8 as char {
            'A' => "A",
            'B' => "B",
            'C' => "C",
            'D' => "D",
            'E' => "E",
            'F' => "F",
            'G' => "G",
            'H' => "H",
            'I' => "I",
            'J' => "J",
            'K' => "K",
            'L' => "L",
            'M' => "M",
            'N' => "N",
            'O' => "O",
            'P' => "P",
            'Q' => "Q",
            'R' => "R",
            'S' => "S",
            'T' => "T",
            'U' => "U",
            'V' => "V",
            'W' => "W",
            'X' => "X",
            'Y' => "Y",
            'Z' => "Z",
            _ => return None,
        },
        0x60 => "Numpad0",
        0x61 => "Numpad1",
        0x62 => "Numpad2",
        0x63 => "Numpad3",
        0x64 => "Numpad4",
        0x65 => "Numpad5",
        0x66 => "Numpad6",
        0x67 => "Numpad7",
        0x68 => "Numpad8",
        0x69 => "Numpad9",
        0x6A => "NumpadMultiply",
        0x6B => "NumpadAdd",
        0x6D => "NumpadSubtract",
        0x6E => "NumpadDecimal",
        0x6F => "NumpadDivide",
        0x70..=0x87 => match vk - 0x70 + 1 {
            1 => "F1",
            2 => "F2",
            3 => "F3",
            4 => "F4",
            5 => "F5",
            6 => "F6",
            7 => "F7",
            8 => "F8",
            9 => "F9",
            10 => "F10",
            11 => "F11",
            12 => "F12",
            13 => "F13",
            14 => "F14",
            15 => "F15",
            16 => "F16",
            17 => "F17",
            18 => "F18",
            19 => "F19",
            20 => "F20",
            21 => "F21",
            22 => "F22",
            23 => "F23",
            24 => "F24",
            _ => return None,
        },
        0xBA => ";",
        0xBB => "=",
        0xBC => ",",
        0xBD => "-",
        0xBE => ".",
        0xBF => "/",
        0xC0 => "`",
        0xDB => "[",
        0xDC => "\\",
        0xDD => "]",
        0xDE => "'",
        _ => return None,
    })
}
