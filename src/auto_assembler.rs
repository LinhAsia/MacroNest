use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveHookSession {
    pub pid: u32,
    pub allocated_blocks: Vec<(String, usize, usize)>, // (name, address, size)
    pub original_patches: Vec<(usize, Vec<u8>)>,       // (address, original_bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AsmStatement {
    Alloc {
        name: String,
        size: usize,
        near_address: Option<String>,
    },
    Dealloc {
        name: String,
    },
    LabelDecl(String),
    LabelDef(String),
    TargetAddress(String),
    Bytes(Vec<u8>),
    JmpRel32 {
        target: String,
        is_call: bool,
    },
    CondJmpRel32 {
        target: String,
        cond_byte: u8,
    },
    Nop(usize),
}

#[derive(Debug, Clone, Default)]
struct BlockBuffer {
    target: String,
    base_address: usize,
    items: Vec<BlockItem>,
}

#[derive(Debug, Clone)]
enum BlockItem {
    Raw(Vec<u8>),
    Rel32Jump { target: String, is_call: bool },
    CondRel32Jump { target: String, cond_byte: u8 },
}

pub fn execute_auto_assembler_script(
    pid: u32,
    script: &str,
    is_enable: bool,
) -> Result<ActiveHookSession, String> {
    #[cfg(windows)]
    let _ = crate::memory_debugger::debugger::enable_debug_privilege();

    let lines = extract_script_section(script, is_enable);
    let mut symbols: HashMap<String, usize> = HashMap::new();
    let mut allocated_blocks: Vec<(String, usize, usize)> = Vec::new();
    let mut original_patches: Vec<(usize, Vec<u8>)> = Vec::new();

    // Pass 1: Parse statements
    let mut statements = Vec::new();
    for raw_line in lines {
        let stripped = strip_comments(&raw_line);
        let line = stripped.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(stmt) = parse_statement(line)? {
            statements.push(stmt);
        }
    }

    // Process alloc and dealloc directives first
    for stmt in &statements {
        match stmt {
            AsmStatement::Alloc {
                name,
                size,
                near_address,
            } => {
                let near_addr = if let Some(near_expr) = near_address {
                    resolve_address_expr(pid, near_expr, &symbols).unwrap_or(0)
                } else {
                    0
                };
                let alloc_addr = crate::process_memory::allocate_memory_near(pid, near_addr, *size)
                    .map_err(|e| format!("VirtualAllocEx failed for {name}: {e}"))?;
                symbols.insert(name.to_ascii_lowercase(), alloc_addr);
                allocated_blocks.push((name.clone(), alloc_addr, *size));
            }
            AsmStatement::Dealloc { name } => {
                if let Some(addr) = symbols.get(&name.to_ascii_lowercase()) {
                    let _ = crate::process_memory::free_allocated_memory(pid, *addr);
                }
            }
            _ => {}
        }
    }

    // Group items into target blocks
    let mut blocks: Vec<BlockBuffer> = Vec::new();

    for stmt in statements {
        match stmt {
            AsmStatement::Alloc { .. } | AsmStatement::Dealloc { .. } | AsmStatement::LabelDecl(_) => {
                // Already handled or pre-declared
            }
            AsmStatement::TargetAddress(target_expr) => {
                blocks.push(BlockBuffer {
                    target: target_expr,
                    base_address: 0,
                    items: Vec::new(),
                });
            }
            AsmStatement::LabelDef(label_name) => {
                let label_lower = label_name.to_ascii_lowercase();
                let is_alloc_block = allocated_blocks
                    .iter()
                    .any(|(n, _, _)| n.eq_ignore_ascii_case(&label_name));

                if is_alloc_block && symbols.contains_key(&label_lower) {
                    // This label was an allocated block name (e.g. `newmem:`)
                    let base = *symbols.get(&label_lower).unwrap();
                    blocks.push(BlockBuffer {
                        target: label_name,
                        base_address: base,
                        items: Vec::new(),
                    });
                } else {
                    // Regular sub-label within current block
                    let current_len: usize = blocks
                        .last()
                        .map(|b| {
                            b.items
                                .iter()
                                .map(|it| match it {
                                    BlockItem::Raw(bytes) => bytes.len(),
                                    BlockItem::Rel32Jump { .. } => 5,
                                    BlockItem::CondRel32Jump { .. } => 6,
                                })
                                .sum()
                        })
                        .unwrap_or(0);

                    // Compute tentative address if base is known
                    if let Some(last_block) = blocks.last_mut() {
                        let target_base = if last_block.base_address != 0 {
                            last_block.base_address
                        } else {
                            resolve_address_expr(pid, &last_block.target, &symbols).unwrap_or(0)
                        };
                        last_block.base_address = target_base;
                        symbols.insert(label_lower, target_base + current_len);
                    }
                }
            }
            AsmStatement::Bytes(bytes) => {
                if let Some(b) = blocks.last_mut() {
                    b.items.push(BlockItem::Raw(bytes));
                }
            }
            AsmStatement::Nop(count) => {
                if let Some(b) = blocks.last_mut() {
                    b.items.push(BlockItem::Raw(vec![0x90; count]));
                }
            }
            AsmStatement::JmpRel32 { target, is_call } => {
                if let Some(b) = blocks.last_mut() {
                    b.items.push(BlockItem::Rel32Jump { target, is_call });
                }
            }
            AsmStatement::CondJmpRel32 { target, cond_byte } => {
                if let Some(b) = blocks.last_mut() {
                    b.items.push(BlockItem::CondRel32Jump { target, cond_byte });
                }
            }
        }
    }

    // Resolve base addresses of all blocks
    for block in &mut blocks {
        if block.base_address == 0 {
            block.base_address = resolve_address_expr(pid, &block.target, &symbols)
                .map_err(|e| format!("Could not resolve target '{}': {e}", block.target))?;
        }
    }

    // Second pass: compute label offsets accurately
    let mut block_offsets = Vec::new();
    for block in &blocks {
        let mut cur = block.base_address;
        for item in &block.items {
            match item {
                BlockItem::Raw(bytes) => cur += bytes.len(),
                BlockItem::Rel32Jump { .. } => cur += 5,
                BlockItem::CondRel32Jump { .. } => cur += 6,
            }
        }
        block_offsets.push(cur);
    }

    // Pass 3: Assemble bytes with resolved jump targets
    let mut compiled_writes: Vec<(usize, Vec<u8>)> = Vec::new();
    for block in &blocks {
        let mut block_bytes = Vec::new();
        let mut current_ip = block.base_address;

        for item in &block.items {
            match item {
                BlockItem::Raw(bytes) => {
                    current_ip += bytes.len();
                    block_bytes.extend_from_slice(bytes);
                }
                BlockItem::Rel32Jump { target, is_call } => {
                    let target_addr = resolve_address_expr(pid, target, &symbols)
                        .map_err(|e| format!("Cannot resolve jump target '{target}': {e}"))?;
                    let next_ip = current_ip + 5;
                    let rel32 = (target_addr as i64) - (next_ip as i64);
                    if rel32 < i32::MIN as i64 || rel32 > i32::MAX as i64 {
                        return Err(format!(
                            "Jump to '{target}' is out of 32-bit relative range (delta {rel32})"
                        ));
                    }
                    let opcode = if *is_call { 0xE8 } else { 0xE9 };
                    block_bytes.push(opcode);
                    block_bytes.extend_from_slice(&(rel32 as i32).to_le_bytes());
                    current_ip = next_ip;
                }
                BlockItem::CondRel32Jump { target, cond_byte } => {
                    let target_addr = resolve_address_expr(pid, target, &symbols)
                        .map_err(|e| format!("Cannot resolve jump target '{target}': {e}"))?;
                    let next_ip = current_ip + 6;
                    let rel32 = (target_addr as i64) - (next_ip as i64);
                    if rel32 < i32::MIN as i64 || rel32 > i32::MAX as i64 {
                        return Err(format!(
                            "Conditional jump to '{target}' is out of 32-bit range (delta {rel32})"
                        ));
                    }
                    block_bytes.push(0x0F);
                    block_bytes.push(*cond_byte);
                    block_bytes.extend_from_slice(&(rel32 as i32).to_le_bytes());
                    current_ip = next_ip;
                }
            }
        }

        if !block_bytes.is_empty() {
            compiled_writes.push((block.base_address, block_bytes));
        }
    }

    // Pass 4: Read original bytes and write to target process
    for (addr, bytes) in compiled_writes {
        if let Ok(orig) = crate::process_memory::read_memory_bytes(pid, addr, bytes.len()) {
            original_patches.push((addr, orig));
        }
        crate::process_memory::write_code_bytes(pid, addr, &bytes)
            .map_err(|e| format!("Failed writing {} bytes at 0x{addr:X}: {e}", bytes.len()))?;
    }

    Ok(ActiveHookSession {
        pid,
        allocated_blocks,
        original_patches,
    })
}

pub fn restore_hook_session(pid: u32, session: &ActiveHookSession) {
    // 1. Restore original code bytes at hook sites
    for (addr, bytes) in &session.original_patches {
        let _ = crate::process_memory::write_code_bytes(pid, *addr, bytes);
    }
    // 2. Free all allocated memory blocks
    for (_name, addr, _size) in &session.allocated_blocks {
        let _ = crate::process_memory::free_allocated_memory(pid, *addr);
    }
}

const MNEMONICS: &[&str] = &[
    "movups", "movaps", "movss", "movsd", "mov",
    "cvtsi2sd", "cvtsi2ss", "cvttsd2si", "cvttss2si",
    "jmp", "call", "ret", "nop", "db",
    "xorps", "xorpd", "xor", "push", "pop",
    "sub", "add", "lea", "cmp", "test", "and", "or", "inc", "dec",
    "je", "jz", "jne", "jnz", "ja", "jae", "jb", "jbe", "jg", "jge", "jl", "jle", "js", "jns",
];

const DIRECTIVE_PREFIXES: &[&str] = &[
    "alloc(", "dealloc(", "label(", "registersymbol(", "unregistersymbol(", "assert(",
];

fn get_section_text(script: &str, is_enable: bool) -> &str {
    let upper = script.to_ascii_uppercase();
    let enable_pos = upper.find("[ENABLE]");
    let disable_pos = upper.find("[DISABLE]");

    match (enable_pos, disable_pos) {
        (Some(e), Some(d)) => {
            if is_enable {
                if e < d {
                    &script[e + 8..d]
                } else {
                    &script[e + 8..]
                }
            } else if d < e {
                &script[d + 9..e]
            } else {
                &script[d + 9..]
            }
        }
        (Some(e), None) => {
            if is_enable {
                &script[e + 8..]
            } else {
                ""
            }
        }
        (None, Some(d)) => {
            if is_enable {
                &script[..d]
            } else {
                &script[d + 9..]
            }
        }
        (None, None) => {
            if is_enable {
                script
            } else {
                ""
            }
        }
    }
}

pub fn split_statements(text: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let is_mnemonic = |word: &str| -> bool {
        let w = word.to_ascii_lowercase();
        MNEMONICS.contains(&w.as_str())
    };

    let is_directive_start = |s: &str| -> bool {
        let s_lower = s.to_ascii_lowercase();
        DIRECTIVE_PREFIXES.iter().any(|d| s_lower.starts_with(d))
    };

    let mut cleaned = String::with_capacity(text.len());
    for line in text.lines() {
        let stripped = strip_comments(line);
        cleaned.push_str(&stripped);
        cleaned.push('\n');
    }

    let bytes = cleaned.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut current = String::new();
    let mut paren_depth: usize = 0;
    let mut bracket_depth: usize = 0;

    while i < len {
        if current.trim().is_empty() {
            current.clear();
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r' || bytes[i] == b'\n') {
                i += 1;
            }
            if i >= len {
                break;
            }
        }

        let slice = &cleaned[i..];

        if current.is_empty() && is_directive_start(slice) {
            let mut end = i;
            let mut p_depth = 0;
            while end < len {
                if bytes[end] == b'(' {
                    p_depth += 1;
                } else if bytes[end] == b')' {
                    p_depth -= 1;
                    if p_depth == 0 {
                        end += 1;
                        break;
                    }
                }
                end += 1;
            }
            statements.push(cleaned[i..end].trim().to_string());
            i = end;
            current.clear();
            continue;
        }

        if current.is_empty() {
            let mut end = i;
            while end < len && bytes[end] != b' ' && bytes[end] != b'\t' && bytes[end] != b'\r' && bytes[end] != b'\n' {
                if bytes[end] == b':' {
                    end += 1;
                    break;
                }
                end += 1;
            }
            if end <= len && cleaned[i..end].ends_with(':') {
                statements.push(cleaned[i..end].trim().to_string());
                i = end;
                current.clear();
                continue;
            }
        }

        let ch = cleaned[i..].chars().next().unwrap();
        let ch_len = ch.len_utf8();

        if ch == '(' {
            paren_depth += 1;
        } else if ch == ')' {
            paren_depth = paren_depth.saturating_sub(1);
        } else if ch == '[' {
            bracket_depth += 1;
        } else if ch == ']' {
            bracket_depth = bracket_depth.saturating_sub(1);
        }

        if paren_depth == 0 && bracket_depth == 0 {
            if ch == '\n' || ch == '\r' {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    statements.push(trimmed.to_string());
                    current.clear();
                }
                i += ch_len;
                continue;
            }

            let trimmed = current.trim();
            if !trimmed.is_empty() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                let mut peek = i;
                while peek < len && (bytes[peek] == b' ' || bytes[peek] == b'\t') {
                    peek += 1;
                }
                if peek < len {
                    let peek_slice = &cleaned[peek..];
                    let is_next_dir = is_directive_start(peek_slice);
                    let is_next_label = {
                        let word = peek_slice.split_whitespace().next().unwrap_or("");
                        word.ends_with(':')
                    };
                    let is_next_mnemonic = {
                        let word = peek_slice.split_whitespace().next().unwrap_or("");
                        let clean_word = word.trim_matches(|c: char| !c.is_alphanumeric());
                        is_mnemonic(clean_word)
                    };

                    if is_next_dir || is_next_label || is_next_mnemonic {
                        statements.push(trimmed.to_string());
                        current.clear();
                        i = peek;
                        continue;
                    }
                }
            }
        }

        current.push(ch);
        i += ch_len;
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        statements.push(trimmed.to_string());
    }

    statements
}

pub fn extract_script_section(script: &str, is_enable: bool) -> Vec<String> {
    let section = get_section_text(script, is_enable);
    split_statements(section)
}

pub fn format_ce_script(script: &str) -> String {
    let enable_stmts = extract_script_section(script, true);
    let disable_stmts = extract_script_section(script, false);
    if enable_stmts.is_empty() && disable_stmts.is_empty() {
        return script.to_string();
    }

    let mut out = String::new();
    out.push_str("[ENABLE]\n");
    for s in &enable_stmts {
        if s.ends_with(':') {
            out.push('\n');
            out.push_str(s);
            out.push('\n');
        } else if s.starts_with("alloc(") || s.starts_with("label(") || s.starts_with("registersymbol(") {
            out.push_str(s);
            out.push('\n');
        } else {
            out.push_str("  ");
            out.push_str(s);
            out.push('\n');
        }
    }

    if !disable_stmts.is_empty() {
        out.push_str("\n[DISABLE]\n");
        for s in &disable_stmts {
            if s.ends_with(':') {
                out.push('\n');
                out.push_str(s);
                out.push('\n');
            } else if s.starts_with("dealloc(") || s.starts_with("unregistersymbol(") {
                out.push_str(s);
                out.push('\n');
            } else {
                out.push_str("  ");
                out.push_str(s);
                out.push('\n');
            }
        }
    }

    let mut cleaned = String::new();
    let mut prev_empty = false;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_empty {
                cleaned.push('\n');
                prev_empty = true;
            }
        } else {
            cleaned.push_str(line);
            cleaned.push('\n');
            prev_empty = false;
        }
    }
    cleaned.trim().to_string()
}

fn strip_comments(line: &str) -> String {
    let mut out = line.to_string();
    if let Some(idx) = out.find("//") {
        out.truncate(idx);
    }
    if let Some(idx) = out.find(";") {
        out.truncate(idx);
    }
    out
}

fn parse_statement(line: &str) -> Result<Option<AsmStatement>, String> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    // Check directives: alloc(name, size [, near_addr])
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("alloc(") && lower.ends_with(')') {
        let inner = &line[6..line.len() - 1];
        let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() < 2 {
            return Err(format!("Invalid alloc syntax: {line}"));
        }
        let name = parts[0].to_string();
        let size = parse_number(parts[1]).ok_or_else(|| format!("Invalid size in: {line}"))?;
        let near_address = if parts.len() >= 3 {
            Some(parts[2].to_string())
        } else {
            None
        };
        return Ok(Some(AsmStatement::Alloc {
            name,
            size,
            near_address,
        }));
    }

    // dealloc(name)
    if lower.starts_with("dealloc(") && lower.ends_with(')') {
        let inner = &line[8..line.len() - 1].trim();
        return Ok(Some(AsmStatement::Dealloc {
            name: inner.to_string(),
        }));
    }

    // label(name)
    if lower.starts_with("label(") && lower.ends_with(')') {
        let inner = &line[6..line.len() - 1].trim();
        return Ok(Some(AsmStatement::LabelDecl(inner.to_string())));
    }

    // registersymbol(name) / unregistersymbol(name)
    if lower.starts_with("registersymbol(") || lower.starts_with("unregistersymbol(") {
        return Ok(None);
    }

    // Label or Target Address definition: `<name>:`
    if line.ends_with(':') {
        let target = line[..line.len() - 1].trim();
        if target.contains('+') || target.contains('.') || target.starts_with("0x") || target.starts_with("0X") {
            return Ok(Some(AsmStatement::TargetAddress(target.to_string())));
        } else {
            return Ok(Some(AsmStatement::LabelDef(target.to_string())));
        }
    }

    // nop [count]
    if lower == "nop" {
        return Ok(Some(AsmStatement::Nop(1)));
    }
    if lower.starts_with("nop ") {
        let count_str = lower[4..].trim();
        let count = parse_number(count_str).unwrap_or(1);
        return Ok(Some(AsmStatement::Nop(count)));
    }

    // jmp <target>
    if lower.starts_with("jmp ") {
        let target = line[4..].trim().to_string();
        return Ok(Some(AsmStatement::JmpRel32 {
            target,
            is_call: false,
        }));
    }

    // call <target>
    if lower.starts_with("call ") {
        let target = line[5..].trim().to_string();
        return Ok(Some(AsmStatement::JmpRel32 {
            target,
            is_call: true,
        }));
    }

    // Conditional jumps
    for (prefix, cond_byte) in [
        ("je ", 0x84),
        ("jz ", 0x84),
        ("jne ", 0x85),
        ("jnz ", 0x85),
        ("ja ", 0x87),
        ("jae ", 0x83),
        ("jb ", 0x82),
        ("jbe ", 0x86),
        ("jg ", 0x8F),
        ("jge ", 0x8D),
        ("jl ", 0x8C),
        ("jle ", 0x8E),
    ] {
        if lower.starts_with(prefix) {
            let target = line[prefix.len()..].trim().to_string();
            return Ok(Some(AsmStatement::CondJmpRel32 { target, cond_byte }));
        }
    }

    // ret
    if lower == "ret" {
        return Ok(Some(AsmStatement::Bytes(vec![0xC3])));
    }

    // db <bytes> or raw hex bytes
    if lower.starts_with("db ") {
        let hex_part = &line[3..];
        let bytes = parse_hex_bytes(hex_part)?;
        return Ok(Some(AsmStatement::Bytes(bytes)));
    }

    // Try assembling instruction
    if let Some(bytes) = assemble_x64_instruction(line)? {
        return Ok(Some(AsmStatement::Bytes(bytes)));
    }

    // If pure hex bytes
    if let Ok(bytes) = parse_hex_bytes(line) {
        if !bytes.is_empty() {
            return Ok(Some(AsmStatement::Bytes(bytes)));
        }
    }

    Err(format!("Unrecognized instruction or directive: '{line}'"))
}

fn parse_number(text: &str) -> Option<usize> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16).ok()
    } else if t.chars().all(|c| c.is_ascii_digit()) {
        t.parse::<usize>().ok()
    } else {
        usize::from_str_radix(t, 16).ok()
    }
}

fn parse_hex_bytes(text: &str) -> Result<Vec<u8>, String> {
    let cleaned = text.replace(['$', ',', ' '], "");
    if cleaned.len() % 2 != 0 {
        return Err(format!("Hex string length must be even: '{text}'"));
    }
    let mut bytes = Vec::with_capacity(cleaned.len() / 2);
    for chunk in cleaned.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(chunk).map_err(|e| e.to_string())?;
        let b = u8::from_str_radix(pair, 16)
            .map_err(|_| format!("Invalid hex byte '{pair}' in '{text}'"))?;
        bytes.push(b);
    }
    Ok(bytes)
}

fn parse_address_offset(text: &str) -> Option<usize> {
    let t = text.trim();
    let hex_clean = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    usize::from_str_radix(hex_clean, 16).ok().or_else(|| t.parse::<usize>().ok())
}

fn resolve_address_expr(
    pid: u32,
    expr: &str,
    symbols: &HashMap<String, usize>,
) -> Result<usize, String> {
    let expr = expr.trim();
    let lower = expr.to_ascii_lowercase();

    // Check symbol table
    if let Some(&addr) = symbols.get(&lower) {
        return Ok(addr);
    }

    // Check if expression is `<base> + <offset>` or `<base> - <offset>`
    if let Some((base_str, off_str)) = expr.split_once('+') {
        let base = resolve_address_expr(pid, base_str.trim(), symbols)?;
        let off = parse_address_offset(off_str.trim())
            .ok_or_else(|| format!("Invalid offset '{}' in '{expr}'", off_str.trim()))?;
        return Ok(base + off);
    }
    if let Some((base_str, off_str)) = expr.split_once('-') {
        let base = resolve_address_expr(pid, base_str.trim(), symbols)?;
        let off = parse_address_offset(off_str.trim())
            .ok_or_else(|| format!("Invalid offset '{}' in '{expr}'", off_str.trim()))?;
        return Ok(base.saturating_sub(off));
    }

    // Direct numeric address
    if let Some(val) = parse_number(expr) {
        return Ok(val);
    }

    // Module name lookup: e.g. `neox_engine.dll`
    #[cfg(windows)]
    {
        if let Ok(modules) = crate::memory_debugger::debugger::process_modules(pid) {
            for (name, base, _) in modules {
                if name.eq_ignore_ascii_case(expr) {
                    return Ok(base);
                }
            }
        }
    }

    Err(format!("Could not resolve address expression '{expr}'"))
}

// Lightweight instruction assembler for common x86_64 instructions
fn assemble_x64_instruction(line: &str) -> Result<Option<Vec<u8>>, String> {
    let line = line.trim();
    let (mnemonic, rest) = match line.split_once(' ') {
        Some((m, r)) => (m.trim().to_ascii_lowercase(), r.trim()),
        None => (line.to_ascii_lowercase(), ""),
    };

    match mnemonic.as_str() {
        "mov" => assemble_mov(rest),
        "movups" => assemble_movups(rest),
        "movss" => assemble_movss(rest),
        "movsd" => assemble_movsd(rest),
        "cvtsi2sd" => assemble_cvtsi2sd(rest),
        "cvtsi2ss" => assemble_cvtsi2ss(rest),
        "xor" => assemble_xor(rest),
        "xorps" => assemble_xorps(rest),
        "xorpd" => assemble_xorpd(rest),
        "push" => assemble_push(rest),
        "pop" => assemble_pop(rest),
        _ => Ok(None),
    }
}

fn assemble_mov(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };

    // mov eax, 1 / mov <r32>, <imm32>
    if let Some(r32) = parse_reg32(dst) {
        if let Some(imm) = parse_imm32(src) {
            let mut bytes = vec![0xB8 + (r32 & 7)];
            bytes.extend_from_slice(&(imm as u32).to_le_bytes());
            return Ok(Some(bytes));
        }
        if let Some(r32_src) = parse_reg32(src) {
            // mov r32, r32 -> 89 /r (src, dst)
            let modrm = 0xC0 | ((r32_src & 7) << 3) | (r32 & 7);
            return Ok(Some(vec![0x89, modrm]));
        }
        if let Some((base, disp)) = parse_memory_operand(src) {
            // mov r32, [r64 + disp] -> 8B /r
            let (modrm, sib, disp_bytes) = encode_modrm(r32, base, disp);
            let mut bytes = vec![0x8B, modrm];
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            return Ok(Some(bytes));
        }
    }

    // mov rax, 1 / mov <r64>, <imm64>
    if let Some(r64) = parse_reg64(dst) {
        if let Some(imm) = parse_imm64(src) {
            let rex = 0x48 | if r64 >= 8 { 1 } else { 0 };
            let mut bytes = vec![rex, 0xB8 + (r64 & 7)];
            bytes.extend_from_slice(&imm.to_le_bytes());
            return Ok(Some(bytes));
        }
    }

    // mov [r64 + disp], r32 / r64
    if let Some((base, disp)) = parse_memory_operand(dst) {
        if let Some(r32) = parse_reg32(src) {
            let (modrm, sib, disp_bytes) = encode_modrm(r32, base, disp);
            let mut bytes = vec![0x89, modrm];
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            return Ok(Some(bytes));
        }
        if let Some(imm) = parse_imm32(src) {
            // mov dword ptr [r64 + disp], imm32 -> C7 /0
            let (modrm, sib, disp_bytes) = encode_modrm(0, base, disp);
            let mut bytes = vec![0xC7, modrm];
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            bytes.extend_from_slice(&(imm as u32).to_le_bytes());
            return Ok(Some(bytes));
        }
    }

    Ok(None)
}

fn assemble_cvtsi2sd(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };
    let xmm = parse_xmm(dst).ok_or_else(|| format!("Invalid xmm register '{dst}'"))?;
    if let Some(r32) = parse_reg32(src) {
        // F2 0F 2A /r
        let modrm = 0xC0 | ((xmm & 7) << 3) | (r32 & 7);
        return Ok(Some(vec![0xF2, 0x0F, 0x2A, modrm]));
    }
    if let Some(r64) = parse_reg64(src) {
        // F2 48 0F 2A /r
        let rex = 0x48 | if r64 >= 8 { 1 } else { 0 } | if xmm >= 8 { 4 } else { 0 };
        let modrm = 0xC0 | ((xmm & 7) << 3) | (r64 & 7);
        return Ok(Some(vec![0xF2, rex, 0x0F, 0x2A, modrm]));
    }
    Ok(None)
}

fn assemble_cvtsi2ss(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };
    let xmm = parse_xmm(dst).ok_or_else(|| format!("Invalid xmm register '{dst}'"))?;
    if let Some(r32) = parse_reg32(src) {
        let modrm = 0xC0 | ((xmm & 7) << 3) | (r32 & 7);
        return Ok(Some(vec![0xF3, 0x0F, 0x2A, modrm]));
    }
    Ok(None)
}

fn assemble_movups(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };

    // movups [r64 + disp], xmm -> 0F 11 /r
    if let Some((base, disp)) = parse_memory_operand(dst) {
        if let Some(xmm) = parse_xmm(src) {
            let (modrm, sib, disp_bytes) = encode_modrm(xmm, base, disp);
            let mut bytes = vec![0x0F, 0x11, modrm];
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            return Ok(Some(bytes));
        }
    }

    // movups xmm, [r64 + disp] -> 0F 10 /r
    if let Some(xmm) = parse_xmm(dst) {
        if let Some((base, disp)) = parse_memory_operand(src) {
            let (modrm, sib, disp_bytes) = encode_modrm(xmm, base, disp);
            let mut bytes = vec![0x0F, 0x10, modrm];
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            return Ok(Some(bytes));
        }
    }

    Ok(None)
}

fn assemble_movss(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };
    if let Some((base, disp)) = parse_memory_operand(dst) {
        if let Some(xmm) = parse_xmm(src) {
            let (modrm, sib, disp_bytes) = encode_modrm(xmm, base, disp);
            let mut bytes = vec![0xF3, 0x0F, 0x11, modrm];
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            return Ok(Some(bytes));
        }
    }
    if let Some(xmm) = parse_xmm(dst) {
        if let Some((base, disp)) = parse_memory_operand(src) {
            let (modrm, sib, disp_bytes) = encode_modrm(xmm, base, disp);
            let mut bytes = vec![0xF3, 0x0F, 0x10, modrm];
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn assemble_movsd(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };
    if let Some((base, disp)) = parse_memory_operand(dst) {
        if let Some(xmm) = parse_xmm(src) {
            let (modrm, sib, disp_bytes) = encode_modrm(xmm, base, disp);
            let mut bytes = vec![0xF2, 0x0F, 0x11, modrm];
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            return Ok(Some(bytes));
        }
    }
    if let Some(xmm) = parse_xmm(dst) {
        if let Some((base, disp)) = parse_memory_operand(src) {
            let (modrm, sib, disp_bytes) = encode_modrm(xmm, base, disp);
            let mut bytes = vec![0xF2, 0x0F, 0x10, modrm];
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn assemble_xor(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };
    if let (Some(r1), Some(r2)) = (parse_reg32(dst), parse_reg32(src)) {
        let modrm = 0xC0 | ((r2 & 7) << 3) | (r1 & 7);
        return Ok(Some(vec![0x31, modrm]));
    }
    Ok(None)
}

fn assemble_xorps(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };
    if let (Some(x1), Some(x2)) = (parse_xmm(dst), parse_xmm(src)) {
        let modrm = 0xC0 | ((x1 & 7) << 3) | (x2 & 7);
        return Ok(Some(vec![0x0F, 0x57, modrm]));
    }
    Ok(None)
}

fn assemble_xorpd(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };
    if let (Some(x1), Some(x2)) = (parse_xmm(dst), parse_xmm(src)) {
        let modrm = 0xC0 | ((x1 & 7) << 3) | (x2 & 7);
        return Ok(Some(vec![0x66, 0x0F, 0x57, modrm]));
    }
    Ok(None)
}

fn assemble_push(operand: &str) -> Result<Option<Vec<u8>>, String> {
    if let Some(r64) = parse_reg64(operand) {
        if r64 < 8 {
            return Ok(Some(vec![0x50 + r64]));
        } else {
            return Ok(Some(vec![0x41, 0x50 + (r64 & 7)]));
        }
    }
    Ok(None)
}

fn assemble_pop(operand: &str) -> Result<Option<Vec<u8>>, String> {
    if let Some(r64) = parse_reg64(operand) {
        if r64 < 8 {
            return Ok(Some(vec![0x58 + r64]));
        } else {
            return Ok(Some(vec![0x41, 0x58 + (r64 & 7)]));
        }
    }
    Ok(None)
}

// Helpers for registers and operands
fn parse_reg32(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "eax" => Some(0),
        "ecx" => Some(1),
        "edx" => Some(2),
        "ebx" => Some(3),
        "esp" => Some(4),
        "ebp" => Some(5),
        "esi" => Some(6),
        "edi" => Some(7),
        _ => None,
    }
}

fn parse_reg64(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "rax" => Some(0),
        "rcx" => Some(1),
        "rdx" => Some(2),
        "rbx" => Some(3),
        "rsp" => Some(4),
        "rbp" => Some(5),
        "rsi" => Some(6),
        "rdi" => Some(7),
        "r8" => Some(8),
        "r9" => Some(9),
        "r10" => Some(10),
        "r11" => Some(11),
        "r12" => Some(12),
        "r13" => Some(13),
        "r14" => Some(14),
        "r15" => Some(15),
        _ => None,
    }
}

fn parse_xmm(name: &str) -> Option<u8> {
    let lower = name.to_ascii_lowercase();
    if let Some(num_str) = lower.strip_prefix("xmm") {
        num_str.parse::<u8>().ok().filter(|&n| n <= 15)
    } else {
        None
    }
}

fn parse_imm32(text: &str) -> Option<i32> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i32::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<i32>().ok().or_else(|| i32::from_str_radix(t, 16).ok())
    }
}

fn parse_imm64(text: &str) -> Option<i64> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<i64>().ok().or_else(|| i64::from_str_radix(t, 16).ok())
    }
}

// Parses `[rcx+0x88]` or `[rcx+00000088]` or `[rdx+4]` or `dword ptr [rcx+88h]`
fn parse_memory_operand(text: &str) -> Option<(u8, i64)> {
    let mut s = text.trim();
    for prefix in ["byte ptr", "word ptr", "dword ptr", "qword ptr", "xmmword ptr"] {
        if s.to_ascii_lowercase().starts_with(prefix) {
            s = s[prefix.len()..].trim();
        }
    }
    if !s.starts_with('[') || !s.ends_with(']') {
        return None;
    }
    let inner = &s[1..s.len() - 1].trim();

    // Check for base + offset or base - offset
    if let Some((base_str, off_str)) = inner.split_once('+') {
        let base = parse_reg64(base_str.trim())?;
        let off = parse_disp(off_str.trim())?;
        Some((base, off))
    } else if let Some((base_str, off_str)) = inner.split_once('-') {
        let base = parse_reg64(base_str.trim())?;
        let off = parse_disp(off_str.trim())?;
        Some((base, -off))
    } else {
        let base = parse_reg64(inner)?;
        Some((base, 0))
    }
}

fn parse_disp(text: &str) -> Option<i64> {
    let mut t = text.trim();
    if t.starts_with('#') {
        return t[1..].trim().parse::<i64>().ok();
    }
    if t.ends_with('h') || t.ends_with('H') {
        t = &t[..t.len() - 1];
        return i64::from_str_radix(t, 16).ok();
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else if (t.len() > 1 && t.starts_with('0')) || t.chars().any(|c| c.is_ascii_alphabetic()) {
        i64::from_str_radix(t, 16).ok().or_else(|| t.parse::<i64>().ok())
    } else {
        t.parse::<i64>().ok().or_else(|| i64::from_str_radix(t, 16).ok())
    }
}

// Encodes ModR/M byte, optional SIB byte, and displacement bytes for x86_64
fn encode_modrm(reg: u8, base: u8, disp: i64) -> (u8, Option<u8>, Vec<u8>) {
    let reg_field = reg & 7;
    let base_field = base & 7;

    if disp == 0 && base_field != 5 /* rbp */ && base_field != 4 /* rsp */ {
        ((0 << 6) | (reg_field << 3) | base_field, None, Vec::new())
    } else if (-128..=127).contains(&disp) && base_field != 4 {
        (
            (1 << 6) | (reg_field << 3) | base_field,
            None,
            vec![disp as u8],
        )
    } else if base_field != 4 {
        (
            (2 << 6) | (reg_field << 3) | base_field,
            None,
            (disp as i32).to_le_bytes().to_vec(),
        )
    } else {
        // rsp needs SIB byte (0x24 = scale 1, no index, base rsp)
        let sib = 0x24;
        if disp == 0 {
            ((0 << 6) | (reg_field << 3) | 4, Some(sib), Vec::new())
        } else if (-128..=127).contains(&disp) {
            ((1 << 6) | (reg_field << 3) | 4, Some(sib), vec![disp as u8])
        } else {
            (
                (2 << 6) | (reg_field << 3) | 4,
                Some(sib),
                (disp as i32).to_le_bytes().to_vec(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assemble_basic_instructions() {
        assert_eq!(
            assemble_x64_instruction("mov eax, 1").unwrap(),
            Some(vec![0xB8, 0x01, 0x00, 0x00, 0x00])
        );
        assert_eq!(
            assemble_x64_instruction("cvtsi2sd xmm0, eax").unwrap(),
            Some(vec![0xF2, 0x0F, 0x2A, 0xC0])
        );
        assert_eq!(
            assemble_x64_instruction("movups [rcx+00000088], xmm0").unwrap(),
            Some(vec![0x0F, 0x11, 0x81, 0x88, 0x00, 0x00, 0x00])
        );
        assert_eq!(
            assemble_x64_instruction("mov dword ptr [rdx+4], 240").unwrap(),
            Some(vec![0xC7, 0x42, 0x04, 0xF0, 0x00, 0x00, 0x00])
        );
    }

    #[test]
    fn test_extract_sections() {
        let script = r#"
[ENABLE]
alloc(newmem,2048)
newmem:
mov eax,1

[DISABLE]
dealloc(newmem)
"#;
        let enable_lines = extract_script_section(script, true);
        assert_eq!(enable_lines.len(), 3);
        let disable_lines = extract_script_section(script, false);
        assert_eq!(disable_lines.len(), 1);
    }

    #[test]
    fn test_parse_user_cheat_engine_script() {
        let script = r#"
[ENABLE]
alloc(newmem,2048,0x7FF82F729350)
label(returnhere)
label(originalcode)
label(exit)

newmem:
mov eax,1
cvtsi2sd xmm0,eax

originalcode:
movups [rcx+00000088],xmm0

exit:
jmp returnhere

0x7FF82F729350:
jmp newmem
nop 2
returnhere:

[DISABLE]
dealloc(newmem)
0x7FF82F729350:
movups [rcx+00000088],xmm0
"#;
        let enable_lines = extract_script_section(script, true);
        assert!(!enable_lines.is_empty());
        let disable_lines = extract_script_section(script, false);
        assert_eq!(disable_lines.len(), 3);
    }

    #[test]
    fn test_flattened_user_script_parsing_and_formatting() {
        let flattened = "[ENABLE] alloc(newmem,2048,neox_engine.dll+50A7E2) label(returnhere) label(originalcode) label(exit) newmem: mov eax,1 cvtsi2sd xmm0,eax originalcode: movups [rcx+00000088],xmm0 exit: jmp returnhere neox_engine.dll+50A7E2: jmp newmem nop 2 returnhere: [DISABLE] dealloc(newmem) neox_engine.dll+50A7E2: movups [rcx+00000088],xmm0";

        let enable_stmts = extract_script_section(flattened, true);
        assert_eq!(enable_stmts.len(), 15, "Expected 15 statements in ENABLE, got {:?}", enable_stmts);
        assert_eq!(enable_stmts[0], "alloc(newmem,2048,neox_engine.dll+50A7E2)");
        assert_eq!(enable_stmts[4], "newmem:");
        assert_eq!(enable_stmts[5], "mov eax,1");
        assert_eq!(enable_stmts[6], "cvtsi2sd xmm0,eax");
        assert_eq!(enable_stmts[8], "movups [rcx+00000088],xmm0");
        assert_eq!(enable_stmts[11], "neox_engine.dll+50A7E2:");
        assert_eq!(enable_stmts[12], "jmp newmem");
        assert_eq!(enable_stmts[13], "nop 2");
        assert_eq!(enable_stmts[14], "returnhere:");

        let disable_stmts = extract_script_section(flattened, false);
        assert_eq!(disable_stmts.len(), 3, "Expected 3 statements in DISABLE, got {:?}", disable_stmts);
        assert_eq!(disable_stmts[0], "dealloc(newmem)");
        assert_eq!(disable_stmts[1], "neox_engine.dll+50A7E2:");
        assert_eq!(disable_stmts[2], "movups [rcx+00000088],xmm0");

        // Verify statements parse into AsmStatement without errors
        for stmt_str in &enable_stmts {
            let parsed = parse_statement(stmt_str).unwrap();
            assert!(parsed.is_some(), "Failed parsing statement: {stmt_str}");
        }
        for stmt_str in &disable_stmts {
            let parsed = parse_statement(stmt_str).unwrap();
            assert!(parsed.is_some(), "Failed parsing statement: {stmt_str}");
        }

        // Verify format_ce_script produces multi-line formatted text
        let formatted = format_ce_script(flattened);
        assert!(formatted.lines().count() >= 15);
        assert!(formatted.contains("[ENABLE]"));
        assert!(formatted.contains("[DISABLE]"));
    }
}
