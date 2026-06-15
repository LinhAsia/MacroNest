use std::sync::atomic::Ordering;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetSystemMetrics, GetWindowRect, SM_CXSCREEN, SM_CYSCREEN, GetCursorPos};
use windows::Win32::Foundation::{RECT, POINT};

use crate::model::TimerPreset;
use super::{
    HOOK_STATE, RANDOM_STATE, RUNTIME_VARIABLES, TEXT_VARIABLES,
    ActiveTimerState,
    current_mouse_speed, current_system_volume_percent, window_title,
    wake_command_queue, request_ui_repaint,
};

pub fn interpolate_variables(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut var_name = String::new();
            let mut found_close = false;
            while let Some(&next_c) = chars.peek() {
                if next_c == '}' {
                    chars.next();
                    found_close = true;
                    break;
                } else {
                    var_name.push(chars.next().unwrap());
                }
            }

            if found_close {
                let var_trimmed = var_name.trim();
                if let Some(text_val) = resolve_text_variable_value(var_trimmed) {
                    result.push_str(&text_val);
                } else {
                    let val = evaluate_math_expression_f64(var_trimmed);
                    if val.fract() == 0.0 {
                        result.push_str(&(val as i64).to_string());
                    } else {
                        result.push_str(&val.to_string());
                    }
                }
            } else {
                result.push('{');
                result.push_str(&var_name);
            }
        } else {
            result.push(c);
        }
    }

    result
}

pub(crate) fn evaluate_interpolated_math_expression(expr: &str) -> i32 {
    let interpolated = interpolate_variables(expr.trim());
    evaluate_math_expression(&interpolated)
}

pub(crate) fn evaluate_math_expression(expr: &str) -> i32 {
    clamp_f64_to_i32(evaluate_math_expression_f64(expr))
}

pub(crate) fn clamp_f64_to_i32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }

    value.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

pub(crate) fn evaluate_math_expression_f64(expr: &str) -> f64 {
    let mut expr_str = expr.trim().to_string();
    if expr_str.is_empty() {
        return 0.0;
    }

    while let Some(open_idx) = expr_str.rfind('(') {
        let mut func_name = String::new();
        let mut func_start_idx = open_idx;
        while func_start_idx > 0 {
            let prev_char = expr_str.chars().nth(func_start_idx - 1).unwrap_or('\0');
            if prev_char.is_ascii_alphanumeric() {
                func_name.insert(0, prev_char);
                func_start_idx -= 1;
            } else {
                break;
            }
        }

        if let Some(close_offset) = expr_str[open_idx..].find(')') {
            let close_idx = open_idx + close_offset;
            let sub_expr = &expr_str[open_idx + 1..close_idx];
            if !func_name.is_empty() {
                let args: Vec<&str> = sub_expr.split(',').map(|s| s.trim()).collect();
                let resolved_args: Vec<f64> = args
                    .into_iter()
                    .map(evaluate_math_expression_f64)
                    .collect();
                let result_val = match func_name.to_ascii_lowercase().as_str() {
                    "random" => {
                        let min_val = clamp_f64_to_i32(resolved_args.first().copied().unwrap_or(0.0));
                        let max_val = clamp_f64_to_i32(resolved_args.get(1).copied().unwrap_or(min_val as f64));
                        get_pseudo_random(min_val, max_val) as f64
                    }
                    "min" => resolved_args.first().copied().unwrap_or(0.0).min(resolved_args.get(1).copied().unwrap_or(0.0)),
                    "max" => resolved_args.first().copied().unwrap_or(0.0).max(resolved_args.get(1).copied().unwrap_or(0.0)),
                    "abs" => resolved_args.first().copied().unwrap_or(0.0).abs(),
                    "atan" => resolved_args.first().copied().unwrap_or(0.0).atan(),
                    "atan2" => {
                        let y = resolved_args.first().copied().unwrap_or(0.0);
                        let x = resolved_args.get(1).copied().unwrap_or(0.0);
                        y.atan2(x)
                    }
                    "sin" => resolved_args.first().copied().unwrap_or(0.0).sin(),
                    "cos" => resolved_args.first().copied().unwrap_or(0.0).cos(),
                    "tan" => resolved_args.first().copied().unwrap_or(0.0).tan(),
                    "asin" => resolved_args.first().copied().unwrap_or(0.0).asin(),
                    "acos" => resolved_args.first().copied().unwrap_or(0.0).acos(),
                    "sinh" => resolved_args.first().copied().unwrap_or(0.0).sinh(),
                    "cosh" => resolved_args.first().copied().unwrap_or(0.0).cosh(),
                    "tanh" => resolved_args.first().copied().unwrap_or(0.0).tanh(),
                    "sqrt" => {
                        let value = resolved_args.first().copied().unwrap_or(0.0);
                        if value < 0.0 { 0.0 } else { value.sqrt() }
                    }
                    "pow" => {
                        let base = resolved_args.first().copied().unwrap_or(0.0);
                        let exponent = resolved_args.get(1).copied().unwrap_or(1.0);
                        let value = base.powf(exponent);
                        if value.is_finite() { value } else { 0.0 }
                    }
                    "round" => {
                        let value = resolved_args.first().copied().unwrap_or(0.0);
                        let digits = clamp_f64_to_i32(resolved_args.get(1).copied().unwrap_or(0.0))
                            .clamp(0, 9);
                        let factor = 10_f64.powi(digits);
                        if value.is_finite() {
                            (value * factor).round() / factor
                        } else {
                            0.0
                        }
                    }
                    "ceil" => resolved_args.first().copied().unwrap_or(0.0).ceil(),
                    "floor" => resolved_args.first().copied().unwrap_or(0.0).floor(),
                    "degrees" => resolved_args.first().copied().unwrap_or(0.0).to_degrees(),
                    "radians" => resolved_args.first().copied().unwrap_or(0.0).to_radians(),
                    "factorial" => {
                        let value = clamp_f64_to_i32(resolved_args.first().copied().unwrap_or(0.0));
                        if value < 0 { 0.0 } else { factorial_u128(value as u64).min(i32::MAX as u128) as f64 }
                    }
                    "gcd" => {
                        let mut result = 0i64;
                        for arg in resolved_args {
                            result = gcd_i64(result, clamp_f64_to_i32(arg) as i64);
                        }
                        result as f64
                    }
                    "lcm" => {
                        let mut iter = resolved_args.into_iter();
                        if let Some(first) = iter.next() {
                            let mut result = clamp_f64_to_i32(first) as i64;
                            for arg in iter {
                                result = lcm_i64(result, clamp_f64_to_i32(arg) as i64);
                            }
                            result.min(i32::MAX as i64) as f64
                        } else {
                            0.0
                        }
                    }
                    "isqrt" => {
                        let value = resolved_args.first().copied().unwrap_or(0.0);
                        if value < 0.0 { 0.0 } else { value.sqrt().floor() }
                    }
                    "comb" => {
                        let n = clamp_f64_to_i32(resolved_args.first().copied().unwrap_or(0.0));
                        let k = clamp_f64_to_i32(resolved_args.get(1).copied().unwrap_or(0.0));
                        if n < 0 || k < 0 { 0.0 } else { combination_u128(n as u64, k as u64).min(i32::MAX as u128) as f64 }
                    }
                    "perm" => {
                        let n = clamp_f64_to_i32(resolved_args.first().copied().unwrap_or(0.0));
                        let k = clamp_f64_to_i32(resolved_args.get(1).copied().unwrap_or(0.0));
                        if n < 0 || k < 0 { 0.0 } else { permutation_u128(n as u64, k as u64).min(i32::MAX as u128) as f64 }
                    }
                    "choice" => {
                        if resolved_args.is_empty() {
                            0.0
                        } else {
                            let idx = get_pseudo_random(0, (resolved_args.len() - 1) as i32) as usize;
                            resolved_args.get(idx).copied().unwrap_or(0.0)
                        }
                    }
                    _ => 0.0,
                };
                expr_str.replace_range(func_start_idx..=close_idx, &result_val.to_string());
            } else {
                let sub_value = evaluate_math_expression_f64(sub_expr);
                expr_str.replace_range(open_idx..=close_idx, &sub_value.to_string());
            }
        } else {
            expr_str.remove(open_idx);
        }
    }

    let expr = expr_str.trim();
    if expr.is_empty() {
        return 0.0;
    }

    if let Ok(val) = expr.parse::<f64>() {
        return val;
    }

    let mut tokens = Vec::new();
    let mut current_token = String::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut idx = 0;
    while idx < chars.len() {
        let c = chars[idx];
        if c.is_whitespace() {
            if !current_token.is_empty() {
                tokens.push(current_token.clone());
                current_token.clear();
            }
        } else if c == '+' || c == '*' || c == '/' {
            if !current_token.is_empty() {
                tokens.push(current_token.clone());
                current_token.clear();
            }
            tokens.push(c.to_string());
        } else if c == '-' {
            let is_unary = current_token.is_empty()
                && (tokens.is_empty()
                    || matches!(
                        tokens.last().map(|s| s.as_str()),
                        Some("+") | Some("-") | Some("*") | Some("/")
                    ));
            if is_unary {
                current_token.push(c);
            } else {
                if !current_token.is_empty() {
                    tokens.push(current_token.clone());
                    current_token.clear();
                }
                tokens.push(c.to_string());
            }
        } else {
            current_token.push(c);
        }
        idx += 1;
    }

    if !current_token.is_empty() {
        tokens.push(current_token);
    }

    if tokens.is_empty() {
        return 0.0;
    }

    let get_value = |token: &str| -> f64 {
        let normalized = token.trim();
        if normalized.eq_ignore_ascii_case("pi") {
            std::f64::consts::PI
        } else if let Ok(num) = normalized.parse::<f64>() {
            num
        } else if let Some(obj_val) = get_object_property_value(normalized) {
            obj_val as f64
        } else {
            *RUNTIME_VARIABLES.lock().get(normalized).unwrap_or(&0.0)
        }
    };
    let mut values = Vec::new();
    let mut operators = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        if token == "+" || token == "-" || token == "*" || token == "/" {
            operators.push(token.as_str());
        } else {
            values.push(get_value(token));
        }
        i += 1;
    }

    if values.is_empty() {
        return 0.0;
    }

    let mut val_stack = Vec::new();
    let mut op_stack = Vec::new();
    val_stack.push(values[0]);
    let mut val_idx = 1;
    for op in operators {
        let next_val = if val_idx < values.len() { values[val_idx] } else { 0.0 };
        val_idx += 1;
        if op == "*" {
            if let Some(prev) = val_stack.pop() {
                val_stack.push(prev * next_val);
            } else {
                val_stack.push(0.0);
            }
        } else if op == "/" {
            if let Some(prev) = val_stack.pop() {
                let divisor = if next_val == 0.0 { 1.0 } else { next_val };
                val_stack.push(prev / divisor);
            } else {
                val_stack.push(0.0);
            }
        } else {
            op_stack.push(op);
            val_stack.push(next_val);
        }
    }

    let mut result = val_stack[0];
    for (idx, op) in op_stack.into_iter().enumerate() {
        let next_val = if idx + 1 < val_stack.len() { val_stack[idx + 1] } else { 0.0 };
        if op == "+" {
            result += next_val;
        } else if op == "-" {
            result -= next_val;
        }
    }

    result
}

pub(crate) fn resolve_text_variable_value(token: &str) -> Option<String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(text) = get_object_property_text_value(trimmed) {
        return Some(text);
    }

    if let Some(value) = get_object_property_value(trimmed) {
        return Some(value.to_string());
    }

    {
        let text_vars = TEXT_VARIABLES.lock();
        if let Some(val) = text_vars.get(trimmed) {
            return Some(val.clone());
        }
    }

    let vars = RUNTIME_VARIABLES.lock();
    vars.get(trimmed).map(|v| v.to_string())
}

pub(crate) fn set_text_variable_value(target_var: &str, value: &str) {
    let target_trimmed = target_var.trim();
    if target_trimmed.is_empty() {
        return;
    }

    RUNTIME_VARIABLES.lock().remove(target_trimmed);
    let mut vars = TEXT_VARIABLES.lock();
    vars.insert(target_trimmed.to_string(), value.to_string());
}

pub(crate) fn set_variable_value(target_var: &str, value: f64) {
    let target_trimmed = target_var.trim();
    if target_trimmed.is_empty() {
        return;
    }

    TEXT_VARIABLES.lock().remove(target_trimmed);

    if target_trimmed.contains('.') {
        let parts: Vec<&str> = target_trimmed.split('.').collect();
        if parts.len() == 2 {
            let obj_name = parts[0].trim().to_lowercase();
            let prop_name = parts[1].trim().to_lowercase();
            let mut hook_state = HOOK_STATE.lock();
            let timer_preset = resolve_timer_preset_ref(&hook_state, &obj_name);
            if let Some(timer) = timer_preset {
                let state = hook_state.active_timers.entry(timer.id).or_insert_with(|| {
                    ActiveTimerState {
                        running: false,
                        start_time: None,
                        elapsed_ms: 0,
                        on_complete_macro_preset_id: None,
                    }
                });
                let current_elapsed = state.get_elapsed_ms();
                let current_ms = if timer.is_countdown {
                    let total_ms = (timer.duration_secs as u64) * 1000;
                    if total_ms > current_elapsed {
                        total_ms - current_elapsed
                    } else {
                        0
                    }
                } else {
                    current_elapsed
                };
                let mut hour = (current_ms / 3600000) as i32;
                let mut minute = ((current_ms % 3600000) / 60000) as i32;
                let mut second = ((current_ms % 60000) / 1000) as i32;
                let mut millisecond = (current_ms % 1000) as i32;
                let value_i = clamp_f64_to_i32(value);
                match prop_name.as_str() {
                    "hour" | "h" => hour = value_i.max(0),
                    "minute" | "m" => minute = value_i.clamp(0, 59),
                    "second" | "s" => second = value_i.clamp(0, 59),
                    "millisecond" | "ms" => millisecond = value_i.clamp(0, 999),
                    "raw" | "total_ms" => {
                        let new_ms = value_i.max(0) as u64;
                        hour = (new_ms / 3600000) as i32;
                        minute = ((new_ms % 3600000) / 60000) as i32;
                        second = ((new_ms % 60000) / 1000) as i32;
                        millisecond = (new_ms % 1000) as i32;
                    }
                    "total_sec" => {
                        let new_ms = (value_i.max(0) as u64) * 1000;
                        hour = (new_ms / 3600000) as i32;
                        minute = ((new_ms % 3600000) / 60000) as i32;
                        second = ((new_ms % 60000) / 1000) as i32;
                        millisecond = 0;
                    }
                    _ => {}
                }

                let new_ms = (hour as u64) * 3600000
                    + (minute as u64) * 60000
                    + (second as u64) * 1000
                    + (millisecond as u64);
                if timer.is_countdown {
                    let total_ms = (timer.duration_secs as u64) * 1000;
                    let safe_new_ms = new_ms.min(total_ms);
                    let new_elapsed = total_ms - safe_new_ms;
                    if state.running {
                        let elapsed_since_start = state
                            .start_time
                            .map(|t| t.elapsed().as_millis() as u64)
                            .unwrap_or(0);
                        if new_elapsed >= elapsed_since_start {
                            state.elapsed_ms = new_elapsed - elapsed_since_start;
                        } else {
                            state.elapsed_ms = 0;
                            state.start_time = Some(std::time::Instant::now());
                        }
                    } else {
                        state.elapsed_ms = new_elapsed;
                    }
                } else {
                    if state.running {
                        let elapsed_since_start = state
                            .start_time
                            .map(|t| t.elapsed().as_millis() as u64)
                            .unwrap_or(0);
                        if new_ms >= elapsed_since_start {
                            state.elapsed_ms = new_ms - elapsed_since_start;
                        } else {
                            state.elapsed_ms = 0;
                            state.start_time = Some(std::time::Instant::now());
                        }
                    } else {
                        state.elapsed_ms = new_ms;
                    }
                }

                drop(hook_state);
                wake_command_queue();
                request_ui_repaint();
                return;
            }
        }
    }

    let mut vars = RUNTIME_VARIABLES.lock();
    vars.insert(target_trimmed.to_string(), value);
}

pub(crate) fn smart_set_variable_from_expression(target_var: &str, expr_raw: &str) {
    let target_trimmed = target_var.trim();
    if target_trimmed.is_empty() {
        return;
    }

    let expr_trimmed = expr_raw.trim().to_string();
    if let Some(chosen) = resolve_choice_expression_value(&expr_trimmed) {
        if let Ok(val) = chosen.parse::<f64>() {
            set_variable_value(target_trimmed, val);
            TEXT_VARIABLES.lock().remove(target_trimmed);
        } else {
            set_text_variable_value(target_trimmed, &chosen);
            RUNTIME_VARIABLES.lock().remove(target_trimmed);
        }
        return;
    }
    if !expr_trimmed.contains('{') {
        if let Ok(val) = expr_trimmed.parse::<f64>() {
            set_variable_value(target_trimmed, val);
            TEXT_VARIABLES.lock().remove(target_trimmed);
        } else if looks_like_math_expression_text(&expr_trimmed) {
            let val = evaluate_math_expression_f64(&expr_trimmed);
            set_variable_value(target_trimmed, val);
            TEXT_VARIABLES.lock().remove(target_trimmed);
        } else {
            set_text_variable_value(target_trimmed, &expr_trimmed);
            RUNTIME_VARIABLES.lock().remove(target_trimmed);
        }
    } else {
        let interpolated = interpolate_variables(&expr_trimmed);
        if let Ok(val) = interpolated.parse::<f64>() {
            set_variable_value(target_trimmed, val);
            TEXT_VARIABLES.lock().remove(target_trimmed);
        } else {
            if looks_like_math_expression_text(&interpolated) {
                let val = evaluate_math_expression_f64(&interpolated);
                set_variable_value(target_trimmed, val);
                TEXT_VARIABLES.lock().remove(target_trimmed);
            } else {
                set_text_variable_value(target_trimmed, &interpolated);
                RUNTIME_VARIABLES.lock().remove(target_trimmed);
            }
        }
    }
}

fn looks_like_math_expression_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.chars().any(|c| "+-*/()".contains(c))
        || lower == "pi"
        || lower.contains("random(")
        || lower.contains("choice(")
        || lower.contains("min(")
        || lower.contains("max(")
        || lower.contains("abs(")
        || lower.contains("atan(")
        || lower.contains("atan2(")
        || lower.contains("sin(")
        || lower.contains("cos(")
        || lower.contains("tan(")
        || lower.contains("asin(")
        || lower.contains("acos(")
        || lower.contains("sinh(")
        || lower.contains("cosh(")
        || lower.contains("tanh(")
        || lower.contains("sqrt(")
        || lower.contains("pow(")
        || lower.contains("round(")
        || lower.contains("ceil(")
        || lower.contains("floor(")
        || lower.contains("degrees(")
        || lower.contains("radians(")
        || lower.contains("factorial(")
        || lower.contains("gcd(")
        || lower.contains("lcm(")
        || lower.contains("isqrt(")
        || lower.contains("comb(")
        || lower.contains("perm(")
        || lower.contains(".tonumber")
}

pub(crate) fn resolve_choice_expression_value(expr: &str) -> Option<String> {
    let trimmed = expr.trim();
    let inner = trimmed
        .strip_prefix("choice(")?
        .strip_suffix(')')?
        .trim();
    let args = split_expression_arguments(inner);
    if args.is_empty() {
        return None;
    }

    let idx = get_pseudo_random(0, (args.len() - 1) as i32) as usize;
    let chosen = args.get(idx)?.trim();
    if chosen.is_empty() {
        return None;
    }

    let interpolated = interpolate_variables(chosen);
    let resolved = interpolated.trim();
    if resolved.is_empty() {
        return Some(String::new());
    }

    if looks_like_math_expression_text(resolved) {
        Some(evaluate_math_expression_f64(resolved).to_string())
    } else {
        Some(resolved.to_string())
    }
}

fn split_expression_arguments(expr: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut paren_depth = 0i32;
    let mut brace_depth = 0i32;

    for ch in expr.chars() {
        match ch {
            ',' if paren_depth == 0 && brace_depth == 0 => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    args.push(trimmed.to_string());
                }
                current.clear();
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                paren_depth = (paren_depth - 1).max(0);
                current.push(ch);
            }
            '{' => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' => {
                brace_depth = (brace_depth - 1).max(0);
                current.push(ch);
            }
            _ => current.push(ch),
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        args.push(trimmed.to_string());
    }

    args
}

fn get_pseudo_random(min: i32, max: i32) -> i32 {
    if min >= max {
        return min;
    }

    let range = (i64::from(max) - i64::from(min) + 1).max(1) as u64;
    let mut state = RANDOM_STATE.load(Ordering::Relaxed);
    loop {
        let mut next = state;
        next ^= next >> 12;
        next ^= next << 25;
        next ^= next >> 27;
        next = next.wrapping_mul(2685821657736338717);
        match RANDOM_STATE.compare_exchange_weak(
            state,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return min + (next % range) as i32,
            Err(observed) => state = observed,
        }
    }
}

fn get_object_property_value(token: &str) -> Option<i32> {
    if !token.contains('.') {
        return None;
    }

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return None;
    }

    let obj_name_raw = parts[0].trim();
    let obj_name = obj_name_raw.to_lowercase();
    let prop_name = parts[1].trim().to_lowercase();
    if obj_name == "screen" {
        return match prop_name.as_str() {
            "width" | "w" => Some(unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(0)),
            "height" | "h" => Some(unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(0)),
            _ => None,
        };
    }

    if obj_name == "mouse" {
        let mut point = POINT::default();
        unsafe {
            if GetCursorPos(&mut point).is_err() {
                return Some(0);
            }
        }

        return match prop_name.as_str() {
            "x" => Some(point.x),
            "y" => Some(point.y),
            "sensitivity" => current_mouse_speed().ok().map(|speed| speed as i32),
            _ => None,
        };
    }

    if obj_name == "volume" {
        return match prop_name.as_str() {
            "level" | "percent" | "value" => current_system_volume_percent(),
            _ => None,
        };
    }

    if obj_name == "system" {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        return match prop_name.as_str() {
            "year" => Some(now.year() as i32),
            "month" => Some(now.month() as i32),
            "day" => Some(now.day() as i32),
            "hour" => Some(now.hour() as i32),
            "minute" => Some(now.minute() as i32),
            "second" => Some(now.second() as i32),
            "millisecond" | "ms" => Some((now.nanosecond() / 1_000_000) as i32),
            _ => None,
        };
    }

    if obj_name == "window" {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return Some(0);
        }

        let mut rect = RECT::default();
        unsafe {
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return Some(0);
            }
        }

        return match prop_name.as_str() {
            "x" | "left" => Some(rect.left),
            "y" | "top" => Some(rect.top),
            "right" => Some(rect.right),
            "bottom" => Some(rect.bottom),
            "width" | "w" => Some((rect.right - rect.left).max(0)),
            "height" | "h" => Some((rect.bottom - rect.top).max(0)),
            "centerx" | "cx" => Some(rect.left + ((rect.right - rect.left) / 2)),
            "centery" | "cy" => Some(rect.top + ((rect.bottom - rect.top) / 2)),
            _ => None,
        };
    }

    let hook_state = HOOK_STATE.lock();
    let timer_preset = resolve_timer_preset_ref(&hook_state, &obj_name);
    if let Some(timer) = timer_preset {
        let ms = if let Some(state) = hook_state.active_timers.get(&timer.id) {
            let elapsed = state.get_elapsed_ms();
            if timer.is_countdown {
                let total_ms = (timer.duration_secs as u64) * 1000;
                if total_ms > elapsed {
                    total_ms - elapsed
                } else {
                    0
                }
            } else {
                elapsed
            }
        } else {
            if timer.is_countdown {
                (timer.duration_secs as u64) * 1000
            } else {
                0
            }
        };
        let val = match prop_name.as_str() {
            "hour" | "h" => (ms / 3600000) as i32,
            "minute" | "m" => ((ms % 3600000) / 60000) as i32,
            "second" | "s" => ((ms % 60000) / 1000) as i32,
            "millisecond" | "ms" => (ms % 1000) as i32,
            "raw" | "total_ms" => ms as i32,
            "total_sec" => (ms / 1000) as i32,
            _ => 0,
        };
        return Some(val);
    }

    if prop_name == "tonumber" {
        let mut found_str = None;
        let mut is_text_var = false;
        {
            let text_vars = TEXT_VARIABLES.lock();
            if let Some(val) = text_vars.get(obj_name_raw) {
                found_str = Some(val.clone());
                is_text_var = true;
            }
        }

        if found_str.is_none() {
            let vars = RUNTIME_VARIABLES.lock();
            if let Some(val) = vars.get(obj_name_raw) {
                found_str = Some(val.to_string());
            }
        }

        if let Some(s) = found_str {
            let digit_str: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
            let parsed_val = digit_str.parse::<i32>().unwrap_or(0);
            if is_text_var {
                let mut text_vars = TEXT_VARIABLES.lock();
                text_vars.remove(obj_name_raw);
            }

            let mut vars = RUNTIME_VARIABLES.lock();
            vars.insert(obj_name_raw.to_string(), parsed_val as f64);
            return Some(parsed_val);
        }
    }

    None
}

fn get_object_property_text_value(token: &str) -> Option<String> {
    if !token.contains('.') {
        return None;
    }

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return None;
    }

    let obj_name = parts[0].trim().to_lowercase();
    let prop_name = parts[1].trim().to_lowercase();
    if obj_name == "system" {
        let now = chrono::Local::now();
        return match prop_name.as_str() {
            "date" => Some(now.format("%Y-%m-%d").to_string()),
            "time" => Some(now.format("%H:%M:%S").to_string()),
            _ => None,
        };
    }

    if obj_name == "window" && prop_name == "title" {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return Some(String::new());
        }

        return unsafe { window_title(hwnd) }.or_else(|| Some(String::new()));
    }

    if obj_name == "clipboard" && prop_name == "text" {
        let text = arboard::Clipboard::new()
            .ok()
            .and_then(|mut clipboard| clipboard.get_text().ok())
            .unwrap_or_default();
        return Some(text);
    }

    None
}

fn resolve_timer_preset_ref(hook_state: &super::HookState, obj_name: &str) -> Option<TimerPreset> {
    let normalized = obj_name.trim().replace(' ', "").to_lowercase();
    if let Some(idx_str) = normalized.strip_prefix("timer")
        && let Ok(idx) = idx_str.parse::<usize>()
        && idx > 0
        && let Some(timer) = hook_state.timer_presets.get(idx - 1)
    {
        return Some(timer.clone());
    }

    hook_state
        .timer_presets
        .iter()
        .find(|t| t.name.replace(" ", "").to_lowercase() == normalized)
        .cloned()
}

fn gcd_i64(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let temp = b;
        b = a % b;
        a = temp;
    }

    a
}

fn lcm_i64(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        return 0;
    }

    let gcd = gcd_i64(a, b);
    let reduced = a / gcd;
    reduced.checked_mul(b).unwrap_or(i64::MAX).abs()
}

fn factorial_u128(n: u64) -> u128 {
    let mut result = 1u128;
    for value in 2..=n {
        result = result.saturating_mul(value as u128);
    }
    result
}

fn permutation_u128(n: u64, k: u64) -> u128 {
    if k > n {
        return 0;
    }

    let mut result = 1u128;
    for value in (n - k + 1)..=n {
        result = result.saturating_mul(value as u128);
    }
    result
}

fn combination_u128(n: u64, k: u64) -> u128 {
    if k > n {
        return 0;
    }

    let choose = k.min(n - k);
    if choose == 0 {
        return 1;
    }

    let mut result = 1u128;
    for i in 0..choose {
        let numerator = (n - i) as u128;
        let denominator = (i + 1) as u128;
        result = result.saturating_mul(numerator) / denominator;
    }
    result
}
