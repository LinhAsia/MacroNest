use std::sync::atomic::Ordering;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowRect, SM_CXSCREEN, SM_CYSCREEN,
};

use super::{
    RANDOM_STATE, RUNTIME_VARIABLES, TEXT_VARIABLES, current_mouse_speed,
    current_system_volume_percent,
};
use crate::window_list::window_title;

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
                let args = split_expression_arguments(sub_expr);
                let func_name_lower = func_name.to_ascii_lowercase();
                let replacement = match func_name_lower.as_str() {
                    "substr" => quote_expression_text(&evaluate_substr_expression(&args)),
                    "charat" => quote_expression_text(&evaluate_char_at_expression(&args)),
                    "concat" => quote_expression_text(&evaluate_concat_expression(&args)),
                    "lower" => quote_expression_text(&evaluate_lower_expression(&args)),
                    "upper" => quote_expression_text(&evaluate_upper_expression(&args)),
                    "trim" => quote_expression_text(&evaluate_trim_expression(&args)),
                    "len" => evaluate_len_expression(&args).to_string(),
                    "contains" => {
                        let left = args
                            .first()
                            .map(|arg| resolve_expression_argument_text(arg))
                            .unwrap_or_default();
                        let right = args
                            .get(1)
                            .map(|arg| resolve_expression_argument_text(arg))
                            .unwrap_or_default();
                        if left.contains(&right) { 1.0 } else { 0.0 }.to_string()
                    }
                    "random" => {
                        let resolved_args: Vec<f64> = args
                            .iter()
                            .map(|arg| evaluate_math_expression_f64(arg))
                            .collect();
                        let min_val =
                            clamp_f64_to_i32(resolved_args.first().copied().unwrap_or(0.0));
                        let max_val = clamp_f64_to_i32(
                            resolved_args.get(1).copied().unwrap_or(min_val as f64),
                        );
                        (get_pseudo_random(min_val, max_val) as f64).to_string()
                    }
                    _ => {
                        let resolved_args: Vec<f64> = args
                            .iter()
                            .map(|arg| evaluate_math_expression_f64(arg))
                            .collect();
                        let result_val = match func_name_lower.as_str() {
                            "clamp" => {
                                let value = resolved_args.first().copied().unwrap_or(0.0);
                                let first_bound = resolved_args.get(1).copied().unwrap_or(0.0);
                                let second_bound =
                                    resolved_args.get(2).copied().unwrap_or(first_bound);
                                let min_bound = first_bound.min(second_bound);
                                let max_bound = first_bound.max(second_bound);
                                value.clamp(min_bound, max_bound)
                            }
                            "between" => {
                                let value = resolved_args.first().copied().unwrap_or(0.0);
                                let first_bound = resolved_args.get(1).copied().unwrap_or(0.0);
                                let second_bound =
                                    resolved_args.get(2).copied().unwrap_or(first_bound);
                                let min_bound = first_bound.min(second_bound);
                                let max_bound = first_bound.max(second_bound);
                                if value >= min_bound && value <= max_bound {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            "mod" => {
                                let dividend = resolved_args.first().copied().unwrap_or(0.0);
                                let divisor = resolved_args.get(1).copied().unwrap_or(0.0);
                                if divisor == 0.0 {
                                    0.0
                                } else {
                                    dividend % divisor
                                }
                            }
                            "div" => {
                                let dividend = resolved_args.first().copied().unwrap_or(0.0);
                                let divisor = resolved_args.get(1).copied().unwrap_or(0.0);
                                if divisor == 0.0 {
                                    0.0
                                } else {
                                    (dividend / divisor).trunc()
                                }
                            }
                            "min" => resolved_args
                                .first()
                                .copied()
                                .unwrap_or(0.0)
                                .min(resolved_args.get(1).copied().unwrap_or(0.0)),
                            "max" => resolved_args
                                .first()
                                .copied()
                                .unwrap_or(0.0)
                                .max(resolved_args.get(1).copied().unwrap_or(0.0)),
                            "abs" => resolved_args.first().copied().unwrap_or(0.0).abs(),
                            "atan" => resolved_args.first().copied().unwrap_or(0.0).atan(),
                            "ln" | "log" => {
                                let value = resolved_args.first().copied().unwrap_or(0.0);
                                if value > 0.0 { value.ln() } else { 0.0 }
                            }
                            "log10" => {
                                let value = resolved_args.first().copied().unwrap_or(0.0);
                                if value > 0.0 { value.log10() } else { 0.0 }
                            }
                            "exp" => {
                                let value = resolved_args.first().copied().unwrap_or(0.0);
                                let exp = value.exp();
                                if exp.is_finite() { exp } else { 0.0 }
                            }
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
                                let digits =
                                    clamp_f64_to_i32(resolved_args.get(1).copied().unwrap_or(0.0))
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
                                let value =
                                    clamp_f64_to_i32(resolved_args.first().copied().unwrap_or(0.0));
                                if value < 0 {
                                    0.0
                                } else {
                                    factorial_u128(value as u64).min(i32::MAX as u128) as f64
                                }
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
                                if value < 0.0 {
                                    0.0
                                } else {
                                    value.sqrt().floor()
                                }
                            }
                            "comb" => {
                                let n =
                                    clamp_f64_to_i32(resolved_args.first().copied().unwrap_or(0.0));
                                let k =
                                    clamp_f64_to_i32(resolved_args.get(1).copied().unwrap_or(0.0));
                                if n < 0 || k < 0 {
                                    0.0
                                } else {
                                    combination_u128(n as u64, k as u64).min(i32::MAX as u128)
                                        as f64
                                }
                            }
                            "perm" => {
                                let n =
                                    clamp_f64_to_i32(resolved_args.first().copied().unwrap_or(0.0));
                                let k =
                                    clamp_f64_to_i32(resolved_args.get(1).copied().unwrap_or(0.0));
                                if n < 0 || k < 0 {
                                    0.0
                                } else {
                                    permutation_u128(n as u64, k as u64).min(i32::MAX as u128)
                                        as f64
                                }
                            }
                            "choice" => {
                                if resolved_args.is_empty() {
                                    0.0
                                } else {
                                    let idx = get_pseudo_random(0, (resolved_args.len() - 1) as i32)
                                        as usize;
                                    resolved_args.get(idx).copied().unwrap_or(0.0)
                                }
                            }
                            _ => 0.0,
                        };
                        result_val.to_string()
                    }
                };
                expr_str.replace_range(func_start_idx..=close_idx, &replacement);
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

    if let Some((left, operator, right)) = split_top_level_comparison(expr) {
        return evaluate_comparison_expression(left, operator, right);
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
        } else if c == '+' || c == '*' || c == '/' || c == '^' {
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
                        Some("+") | Some("-") | Some("*") | Some("/") | Some("^")
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
        } else if normalized.eq_ignore_ascii_case("e") {
            std::f64::consts::E
        } else if let Ok(num) = normalized.parse::<f64>() {
            num
        } else if let Some(obj_val) = get_object_property_value(normalized) {
            obj_val as f64
        } else {
            let variable_name = resolve_variable_name(normalized);
            *RUNTIME_VARIABLES.lock().get(&variable_name).unwrap_or(&0.0)
        }
    };
    let mut values = Vec::new();
    let mut operators = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        if token == "+" || token == "-" || token == "*" || token == "/" || token == "^" {
            while operators
                .last()
                .copied()
                .is_some_and(|stack_op| should_apply_operator_before(stack_op, token))
            {
                if let Some(stack_op) = operators.pop() {
                    apply_numeric_operator(&mut values, stack_op);
                }
            }
            operators.push(token.as_str());
        } else {
            values.push(get_value(token));
        }
        i += 1;
    }

    if values.is_empty() {
        return 0.0;
    }

    while let Some(op) = operators.pop() {
        apply_numeric_operator(&mut values, op);
    }

    values.pop().unwrap_or(0.0)
}

pub(crate) fn resolve_text_variable_value(token: &str) -> Option<String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(value) = evaluate_text_function_expression(trimmed) {
        return Some(value);
    }

    if let Some(text) = get_object_property_text_value(trimmed) {
        return Some(text);
    }

    if let Some(value) = get_object_property_value(trimmed) {
        return Some(value.to_string());
    }

    let variable_name = resolve_variable_name(trimmed);
    {
        let text_vars = TEXT_VARIABLES.lock();
        if let Some(val) = text_vars.get(&variable_name) {
            return Some(val.clone());
        }
    }

    let vars = RUNTIME_VARIABLES.lock();
    vars.get(&variable_name).map(|v| v.to_string())
}

pub(crate) fn resolve_variable_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.contains('{') {
        interpolate_variables(trimmed)
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn set_text_variable_value(target_var: &str, value: &str) {
    let target_name = resolve_variable_name(target_var);
    if target_name.is_empty() {
        return;
    }

    RUNTIME_VARIABLES.lock().remove(&target_name);
    let mut vars = TEXT_VARIABLES.lock();
    vars.insert(target_name, value.to_string());
}

pub(crate) fn set_variable_value(target_var: &str, value: f64) {
    let target_name = resolve_variable_name(target_var);
    if target_name.is_empty() {
        return;
    }

    TEXT_VARIABLES.lock().remove(&target_name);

    let mut vars = RUNTIME_VARIABLES.lock();
    vars.insert(target_name, value);
}

pub(crate) fn smart_set_variable_from_expression(target_var: &str, expr_raw: &str) {
    let target_trimmed = target_var.trim();
    if target_trimmed.is_empty() {
        return;
    }

    let expr_trimmed = expr_raw.trim().to_string();
    if let Some(literal) = expr_trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            expr_trimmed
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
    {
        set_text_variable_value(target_trimmed, literal);
        return;
    }
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
        } else if let Some(text_val) = evaluate_text_function_expression(&expr_trimmed) {
            set_text_variable_value(target_trimmed, &text_val);
            RUNTIME_VARIABLES.lock().remove(target_trimmed);
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
            if let Some(text_val) = evaluate_text_function_expression(&interpolated) {
                set_text_variable_value(target_trimmed, &text_val);
                RUNTIME_VARIABLES.lock().remove(target_trimmed);
            } else if looks_like_math_expression_text(&interpolated) {
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

pub(crate) fn looks_like_math_expression_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.chars().any(|c| "+-*/^()".contains(c))
        || text.contains("==")
        || text.contains("!=")
        || text.contains(">=")
        || text.contains("<=")
        || text.contains('>')
        || text.contains('<')
        || lower == "pi"
        || lower == "e"
        || lower.contains("len(")
        || lower.contains("substr(")
        || lower.contains("charat(")
        || lower.contains("contains(")
        || lower.contains("concat(")
        || lower.contains("lower(")
        || lower.contains("upper(")
        || lower.contains("trim(")
        || lower.contains("mod(")
        || lower.contains("div(")
        || lower.contains("clamp(")
        || lower.contains("between(")
        || lower.contains("random(")
        || lower.contains("choice(")
        || lower.contains("min(")
        || lower.contains("max(")
        || lower.contains("abs(")
        || lower.contains("atan(")
        || lower.contains("atan2(")
        || lower.contains("ln(")
        || lower.contains("log(")
        || lower.contains("log10(")
        || lower.contains("exp(")
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
    let inner = trimmed.strip_prefix("choice(")?.strip_suffix(')')?.trim();
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

    if let Some(text_val) = evaluate_text_function_expression(resolved) {
        Some(text_val)
    } else if looks_like_math_expression_text(resolved) {
        Some(evaluate_math_expression_f64(resolved).to_string())
    } else {
        Some(resolved.to_string())
    }
}

pub(crate) fn resolve_expression_argument_text(arg: &str) -> String {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Some(unquoted) = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
    {
        return unquoted.to_string();
    }

    if let Some(value) = resolve_text_variable_value(trimmed) {
        return value;
    }

    if let Some(value) = get_object_property_value(trimmed) {
        return value.to_string();
    }

    if trimmed.parse::<f64>().is_ok() || looks_like_math_expression_text(trimmed) {
        let value = evaluate_math_expression_f64(trimmed);
        if value.fract() == 0.0 {
            return (value as i64).to_string();
        }
        return value.to_string();
    }

    interpolate_variables(trimmed)
}

fn evaluate_text_function_expression(expr: &str) -> Option<String> {
    let (func_name, args) = parse_expression_function_call(expr)?;
    match func_name.to_ascii_lowercase().as_str() {
        "substr" => Some(evaluate_substr_expression(&args)),
        "charat" => Some(evaluate_char_at_expression(&args)),
        "concat" => Some(evaluate_concat_expression(&args)),
        "lower" => Some(evaluate_lower_expression(&args)),
        "upper" => Some(evaluate_upper_expression(&args)),
        "trim" => Some(evaluate_trim_expression(&args)),
        _ => None,
    }
}

fn parse_expression_function_call(expr: &str) -> Option<(String, Vec<String>)> {
    let trimmed = expr.trim();
    let open_idx = trimmed.find('(')?;
    let func_name = trimmed[..open_idx].trim();
    let inner = trimmed[open_idx + 1..].strip_suffix(')')?;
    if func_name.is_empty()
        || !func_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some((func_name.to_string(), split_expression_arguments(inner)))
}

fn evaluate_substr_expression(args: &[String]) -> String {
    let source = args
        .first()
        .map(|arg| resolve_expression_argument_text(arg))
        .unwrap_or_default();
    let start = args
        .get(1)
        .map(|arg| clamp_f64_to_i32(evaluate_math_expression_f64(arg)).max(0) as usize)
        .unwrap_or(0);
    let len = args
        .get(2)
        .map(|arg| clamp_f64_to_i32(evaluate_math_expression_f64(arg)).max(0) as usize);

    match len {
        Some(len) => source.chars().skip(start).take(len).collect(),
        None => source.chars().skip(start).collect(),
    }
}

fn evaluate_char_at_expression(args: &[String]) -> String {
    let source = args
        .first()
        .map(|arg| resolve_expression_argument_text(arg))
        .unwrap_or_default();
    let index = args
        .get(1)
        .map(|arg| clamp_f64_to_i32(evaluate_math_expression_f64(arg)))
        .unwrap_or(-1);
    if index < 0 {
        return String::new();
    }
    source.chars().nth(index as usize).map(String::from).unwrap_or_default()
}

fn evaluate_concat_expression(args: &[String]) -> String {
    args.iter()
        .map(|arg| resolve_expression_argument_text(arg))
        .collect::<Vec<_>>()
        .join("")
}

fn evaluate_lower_expression(args: &[String]) -> String {
    args.first()
        .map(|arg| resolve_expression_argument_text(arg).to_lowercase())
        .unwrap_or_default()
}

fn evaluate_upper_expression(args: &[String]) -> String {
    args.first()
        .map(|arg| resolve_expression_argument_text(arg).to_uppercase())
        .unwrap_or_default()
}

fn evaluate_trim_expression(args: &[String]) -> String {
    args.first()
        .map(|arg| resolve_expression_argument_text(arg).trim().to_string())
        .unwrap_or_default()
}

fn evaluate_len_expression(args: &[String]) -> usize {
    args.first()
        .map(|arg| resolve_expression_argument_text(arg).chars().count())
        .unwrap_or(0)
}

fn quote_expression_text(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[derive(Debug, Clone, PartialEq)]
enum ComparisonOperand {
    Number(f64),
    Text(String),
}

fn split_top_level_comparison<'a>(expr: &'a str) -> Option<(&'a str, &'static str, &'a str)> {
    let chars: Vec<(usize, char)> = expr.char_indices().collect();
    let mut paren_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut quote_char: Option<char> = None;
    let mut idx = 0usize;

    while idx < chars.len() {
        let (byte_idx, ch) = chars[idx];
        if let Some(active_quote) = quote_char {
            if ch == active_quote {
                quote_char = None;
            }
            idx += 1;
            continue;
        }

        match ch {
            '"' | '\'' => {
                quote_char = Some(ch);
                idx += 1;
                continue;
            }
            '(' => paren_depth += 1,
            ')' => paren_depth = (paren_depth - 1).max(0),
            '{' => brace_depth += 1,
            '}' => brace_depth = (brace_depth - 1).max(0),
            _ => {}
        }

        if paren_depth == 0 && brace_depth == 0 {
            if let Some((next_byte_idx, next_ch)) = chars.get(idx + 1).copied() {
                let operator = match (ch, next_ch) {
                    ('=', '=') => Some("=="),
                    ('!', '=') => Some("!="),
                    ('>', '=') => Some(">="),
                    ('<', '=') => Some("<="),
                    _ => None,
                };
                if let Some(operator) = operator {
                    let left = expr[..byte_idx].trim();
                    let right = expr[next_byte_idx + next_ch.len_utf8()..].trim();
                    if !left.is_empty() && !right.is_empty() {
                        return Some((left, operator, right));
                    }
                }
            }

            let operator = match ch {
                '>' => Some(">"),
                '<' => Some("<"),
                _ => None,
            };
            if let Some(operator) = operator {
                let left = expr[..byte_idx].trim();
                let right = expr[byte_idx + ch.len_utf8()..].trim();
                if !left.is_empty() && !right.is_empty() {
                    return Some((left, operator, right));
                }
            }
        }

        idx += 1;
    }

    None
}

fn resolve_comparison_operand(expr: &str) -> ComparisonOperand {
    let trimmed = expr.trim();
    if let Some(unquoted) = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
    {
        return ComparisonOperand::Text(unquoted.to_string());
    }

    if let Some(text_value) = evaluate_text_function_expression(trimmed) {
        return ComparisonOperand::Text(text_value);
    }

    if let Some(text_value) = get_object_property_text_value(trimmed) {
        return ComparisonOperand::Text(text_value);
    }

    let variable_name = resolve_variable_name(trimmed);
    {
        let text_vars = TEXT_VARIABLES.lock();
        if let Some(text_value) = text_vars.get(&variable_name) {
            return ComparisonOperand::Text(text_value.clone());
        }
    }

    if trimmed.parse::<f64>().is_ok() || looks_like_math_expression_text(trimmed) {
        return ComparisonOperand::Number(evaluate_math_expression_f64(trimmed));
    }

    if get_object_property_value(trimmed).is_some() {
        return ComparisonOperand::Number(evaluate_math_expression_f64(trimmed));
    }

    {
        let vars = RUNTIME_VARIABLES.lock();
        if let Some(value) = vars.get(&variable_name) {
            return ComparisonOperand::Number(*value);
        }
    }

    ComparisonOperand::Text(interpolate_variables(trimmed))
}

fn numeric_truth(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

fn nearly_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= 0.000_000_1
}

fn format_expression_number(value: f64) -> String {
    if value.fract() == 0.0 {
        (value as i64).to_string()
    } else {
        value.to_string()
    }
}

fn evaluate_comparison_expression(left: &str, operator: &str, right: &str) -> f64 {
    let left_operand = resolve_comparison_operand(left);
    let right_operand = resolve_comparison_operand(right);
    match (left_operand, right_operand) {
        (ComparisonOperand::Number(left), ComparisonOperand::Number(right)) => match operator {
            "==" => numeric_truth(nearly_equal(left, right)),
            "!=" => numeric_truth(!nearly_equal(left, right)),
            ">" => numeric_truth(left > right),
            "<" => numeric_truth(left < right),
            ">=" => numeric_truth(left > right || nearly_equal(left, right)),
            "<=" => numeric_truth(left < right || nearly_equal(left, right)),
            _ => 0.0,
        },
        (ComparisonOperand::Text(left), ComparisonOperand::Text(right)) => match operator {
            "==" => numeric_truth(left == right),
            "!=" => numeric_truth(left != right),
            ">" => numeric_truth(left > right),
            "<" => numeric_truth(left < right),
            ">=" => numeric_truth(left >= right),
            "<=" => numeric_truth(left <= right),
            _ => 0.0,
        },
        (ComparisonOperand::Number(left), ComparisonOperand::Text(right)) => {
            if let Ok(parsed) = right.parse::<f64>() {
                match operator {
                    "==" => numeric_truth(nearly_equal(left, parsed)),
                    "!=" => numeric_truth(!nearly_equal(left, parsed)),
                    ">" => numeric_truth(left > parsed),
                    "<" => numeric_truth(left < parsed),
                    ">=" => numeric_truth(left > parsed || nearly_equal(left, parsed)),
                    "<=" => numeric_truth(left < parsed || nearly_equal(left, parsed)),
                    _ => 0.0,
                }
            } else {
                let left_text = format_expression_number(left);
                match operator {
                    "==" => numeric_truth(left_text == right),
                    "!=" => numeric_truth(left_text != right),
                    ">" => numeric_truth(left_text > right),
                    "<" => numeric_truth(left_text < right),
                    ">=" => numeric_truth(left_text >= right),
                    "<=" => numeric_truth(left_text <= right),
                    _ => 0.0,
                }
            }
        }
        (ComparisonOperand::Text(left), ComparisonOperand::Number(right)) => {
            if let Ok(parsed) = left.parse::<f64>() {
                match operator {
                    "==" => numeric_truth(nearly_equal(parsed, right)),
                    "!=" => numeric_truth(!nearly_equal(parsed, right)),
                    ">" => numeric_truth(parsed > right),
                    "<" => numeric_truth(parsed < right),
                    ">=" => numeric_truth(parsed > right || nearly_equal(parsed, right)),
                    "<=" => numeric_truth(parsed < right || nearly_equal(parsed, right)),
                    _ => 0.0,
                }
            } else {
                let right_text = format_expression_number(right);
                match operator {
                    "==" => numeric_truth(left == right_text),
                    "!=" => numeric_truth(left != right_text),
                    ">" => numeric_truth(left > right_text),
                    "<" => numeric_truth(left < right_text),
                    ">=" => numeric_truth(left >= right_text),
                    "<=" => numeric_truth(left <= right_text),
                    _ => 0.0,
                }
            }
        }
    }
}

fn operator_precedence(op: &str) -> u8 {
    match op {
        "+" | "-" => 1,
        "*" | "/" => 2,
        "^" => 3,
        _ => 0,
    }
}

fn should_apply_operator_before(stack_op: &str, incoming_op: &str) -> bool {
    let stack_prec = operator_precedence(stack_op);
    let incoming_prec = operator_precedence(incoming_op);
    stack_prec > incoming_prec || (stack_prec == incoming_prec && incoming_op != "^")
}

fn apply_numeric_operator(values: &mut Vec<f64>, op: &str) {
    let right = values.pop().unwrap_or(0.0);
    let left = values.pop().unwrap_or(0.0);
    let result = match op {
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        "/" => {
            if right == 0.0 {
                0.0
            } else {
                left / right
            }
        }
        "^" => {
            let value = left.powf(right);
            if value.is_finite() { value } else { 0.0 }
        }
        _ => 0.0,
    };
    values.push(result);
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
        match RANDOM_STATE.compare_exchange_weak(state, next, Ordering::Relaxed, Ordering::Relaxed)
        {
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

    if prop_name == "tostring" {
        let mut found_str = None;
        let mut is_runtime_var = false;
        let obj_name_raw = parts[0].trim();
        {
            let vars = RUNTIME_VARIABLES.lock();
            if let Some(val) = vars.get(obj_name_raw) {
                let num = *val;
                let s = if num.fract() == 0.0 {
                    (num as i64).to_string()
                } else {
                    num.to_string()
                };
                found_str = Some(s);
                is_runtime_var = true;
            }
        }

        if found_str.is_none() {
            let text_vars = TEXT_VARIABLES.lock();
            if let Some(val) = text_vars.get(obj_name_raw) {
                found_str = Some(val.clone());
            }
        }

        if let Some(s) = found_str {
            let filtered: String = s.chars().filter(|c| !c.is_ascii_digit()).collect();
            if is_runtime_var {
                let mut vars = RUNTIME_VARIABLES.lock();
                vars.remove(obj_name_raw);
            }
            let mut text_vars = TEXT_VARIABLES.lock();
            text_vars.insert(obj_name_raw.to_string(), filtered.clone());
            return Some(filtered);
        }

        return Some(String::new());
    }

    None
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

#[cfg(test)]
mod tests {
    use super::{
        RUNTIME_VARIABLES, TEXT_VARIABLES, evaluate_math_expression, evaluate_math_expression_f64,
        resolve_text_variable_value, smart_set_variable_from_expression,
    };

    #[test]
    fn substr_and_len_support_text_variables() {
        {
            let mut runtime_vars = RUNTIME_VARIABLES.lock();
            runtime_vars.clear();
        }
        {
            let mut text_vars = TEXT_VARIABLES.lock();
            text_vars.clear();
            text_vars.insert("player_name".to_string(), "DungeonBoss".to_string());
        }

        assert_eq!(
            resolve_text_variable_value("substr(player_name, 0, 7)").as_deref(),
            Some("Dungeon")
        );
        assert_eq!(evaluate_math_expression("len(player_name)"), 11);
        assert_eq!(
            evaluate_math_expression("contains(substr(player_name, 7, 4), Boss)"),
            1
        );

        smart_set_variable_from_expression("name_head", "substr(player_name, 0, 4)");
        smart_set_variable_from_expression("name_len", "len(player_name)");

        {
            let text_vars = TEXT_VARIABLES.lock();
            assert_eq!(text_vars.get("name_head").map(String::as_str), Some("Dung"));
        }
        {
            let runtime_vars = RUNTIME_VARIABLES.lock();
            assert_eq!(runtime_vars.get("name_len").copied(), Some(11.0));
        }

        {
            let mut runtime_vars = RUNTIME_VARIABLES.lock();
            runtime_vars.clear();
        }
        {
            let mut text_vars = TEXT_VARIABLES.lock();
            text_vars.clear();
        }
    }

    #[test]
    fn char_at_whitespace_comparison_and_dynamic_variable_names_work() {
        RUNTIME_VARIABLES.lock().clear();
        TEXT_VARIABLES.lock().clear();

        smart_set_variable_from_expression("text", "a b");
        smart_set_variable_from_expression("space", "\" \"");
        smart_set_variable_from_expression("index", "2");
        smart_set_variable_from_expression("item[{index}]", "hello");

        assert_eq!(resolve_text_variable_value("charat(text, 1)").as_deref(), Some(" "));
        assert_eq!(evaluate_math_expression("charat(text, 1) == \" \""), 1);
        assert_eq!(evaluate_math_expression("space == \" \""), 1);
        assert_eq!(resolve_text_variable_value("item[2]").as_deref(), Some("hello"));
        assert_eq!(
            resolve_text_variable_value("item[{index}]").as_deref(),
            Some("hello")
        );

        RUNTIME_VARIABLES.lock().clear();
        TEXT_VARIABLES.lock().clear();
    }

    #[test]
    fn div_mod_and_power_work() {
        assert_eq!(evaluate_math_expression("5^2"), 25);
        assert_eq!(evaluate_math_expression("2^3^2"), 512);
        assert_eq!(evaluate_math_expression("div(5, 2)"), 2);
        assert_eq!(evaluate_math_expression("mod(5, 2)"), 1);
        assert_eq!(evaluate_math_expression("3 + 2^3 * 2"), 19);
    }

    #[test]
    fn e_and_log_functions_work() {
        assert!((evaluate_math_expression_f64("e") - std::f64::consts::E).abs() < 0.000001);
        assert!((evaluate_math_expression_f64("exp(1)") - std::f64::consts::E).abs() < 0.000001);
        assert!((evaluate_math_expression_f64("log(e)") - 1.0).abs() < 0.000001);
        assert!((evaluate_math_expression_f64("ln(e)") - 1.0).abs() < 0.000001);
        assert!((evaluate_math_expression_f64("log10(1000)") - 3.0).abs() < 0.000001);
    }

    #[test]
    fn comparisons_return_numeric_truth_values() {
        assert_eq!(evaluate_math_expression("5 > 2"), 1);
        assert_eq!(evaluate_math_expression("5 < 2"), 0);
        assert_eq!(evaluate_math_expression("5 >= 5"), 1);
        assert_eq!(evaluate_math_expression("5 != 5"), 0);
        assert_eq!(evaluate_math_expression("2 + 3 == 5"), 1);
    }

    #[test]
    fn clamp_between_and_text_helpers_work() {
        {
            let mut runtime_vars = RUNTIME_VARIABLES.lock();
            runtime_vars.clear();
        }
        {
            let mut text_vars = TEXT_VARIABLES.lock();
            text_vars.clear();
            text_vars.insert("player_name".to_string(), "  DungeonBoss  ".to_string());
        }

        assert_eq!(evaluate_math_expression("clamp(120, 0, 100)"), 100);
        assert_eq!(evaluate_math_expression("between(7, 1, 10)"), 1);
        assert_eq!(
            resolve_text_variable_value("concat(trim(player_name), \"-\", upper(\"ok\"))")
                .as_deref(),
            Some("DungeonBoss-OK")
        );
        assert_eq!(
            resolve_text_variable_value("lower(trim(player_name))").as_deref(),
            Some("dungeonboss")
        );

        {
            let mut text_vars = TEXT_VARIABLES.lock();
            text_vars.clear();
        }
    }
}
