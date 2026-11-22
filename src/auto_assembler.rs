use std::collections::HashMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

pub static REGISTERED_SYMBOLS: Lazy<Mutex<HashMap<String, (u32, usize)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn get_registered_symbol(name: &str) -> Option<(u32, usize)> {
    REGISTERED_SYMBOLS.lock().get(&name.to_ascii_lowercase()).copied()
}

pub fn register_symbol(name: &str, pid: u32, address: usize) {
    REGISTERED_SYMBOLS.lock().insert(name.to_ascii_lowercase(), (pid, address));
}

pub fn unregister_symbol(name: &str) {
    REGISTERED_SYMBOLS.lock().remove(&name.to_ascii_lowercase());
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveHookSession {
    pub pid: u32,
    pub allocated_blocks: Vec<(String, usize, usize)>, // (name, address, size)
    pub original_patches: Vec<(usize, Vec<u8>)>,       // (address, original_bytes)
    pub registered_symbols: Vec<String>,
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
    RipRelMov {
        is_write: bool,
        is_64: bool,
        reg: u8,
        label: String,
    },
    RipRelMovss {
        is_write: bool,
        xmm: u8,
        label: String,
    },
    RegisterSymbol(String),
    UnregisterSymbol(String),
    Nop(usize),
    AobScanModule {
        symbol: String,
        module: String,
        pattern: String,
    },
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
    RipRelMov {
        is_write: bool,
        is_64: bool,
        reg: u8,
        label: String,
    },
    RipRelMovss {
        is_write: bool,
        xmm: u8,
        label: String,
    },
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

    // Process aobscan, alloc and dealloc directives first
    for stmt in &statements {
        match stmt {
            AsmStatement::AobScanModule {
                symbol,
                module,
                pattern,
            } => {
                let found_addr = scan_aob_for_script(pid, module, pattern)?;
                symbols.insert(symbol.to_ascii_lowercase(), found_addr);
            }
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
    let mut registered_symbols = Vec::new();

    for stmt in statements {
        match stmt {
            AsmStatement::Alloc { .. }
            | AsmStatement::Dealloc { .. }
            | AsmStatement::LabelDecl(_)
            | AsmStatement::AobScanModule { .. } => {
                // Already handled or pre-declared
            }
            AsmStatement::RegisterSymbol(name) => {
                if let Some(&addr) = symbols.get(&name.to_ascii_lowercase()) {
                    register_symbol(&name, pid, addr);
                    registered_symbols.push(name);
                }
            }
            AsmStatement::UnregisterSymbol(name) => {
                unregister_symbol(&name);
            }
            AsmStatement::RipRelMov { is_write, is_64, reg, label } => {
                if let Some(b) = blocks.last_mut() {
                    b.items.push(BlockItem::RipRelMov { is_write, is_64, reg, label });
                }
            }
            AsmStatement::RipRelMovss { is_write, xmm, label } => {
                if let Some(b) = blocks.last_mut() {
                    b.items.push(BlockItem::RipRelMovss { is_write, xmm, label });
                }
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
                                    BlockItem::RipRelMov { is_64, reg, .. } => if *is_64 || *reg >= 8 { 7 } else { 6 },
                                    BlockItem::RipRelMovss { xmm, .. } => if *xmm >= 8 { 9 } else { 8 },
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
                BlockItem::RipRelMov { is_64, reg, .. } => cur += if *is_64 || *reg >= 8 { 7 } else { 6 },
                BlockItem::RipRelMovss { xmm, .. } => cur += if *xmm >= 8 { 9 } else { 8 },
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
                BlockItem::RipRelMov { is_write, is_64, reg, label } => {
                    let target_addr = resolve_address_expr(pid, label, &symbols)
                        .map_err(|e| format!("Cannot resolve symbol '{label}': {e}"))?;
                    let len = if *is_64 || *reg >= 8 { 7 } else { 6 };
                    let next_ip = current_ip + len;
                    let rel32 = (target_addr as i64) - (next_ip as i64);
                    if rel32 < i32::MIN as i64 || rel32 > i32::MAX as i64 {
                        return Err(format!(
                            "Symbol '{label}' is out of 32-bit relative range (delta {rel32})"
                        ));
                    }
                    if *is_64 {
                        let rex = 0x48 | if *reg >= 8 { 4 } else { 0 };
                        block_bytes.push(rex);
                    } else if *reg >= 8 {
                        block_bytes.push(0x44);
                    }
                    block_bytes.push(if *is_write { 0x89 } else { 0x8B });
                    let modrm = 0x05 | ((*reg & 7) << 3);
                    block_bytes.push(modrm);
                    block_bytes.extend_from_slice(&(rel32 as i32).to_le_bytes());
                    current_ip = next_ip;
                }
                BlockItem::RipRelMovss { is_write, xmm, label } => {
                    let target_addr = resolve_address_expr(pid, label, &symbols)
                        .map_err(|e| format!("Cannot resolve symbol '{label}': {e}"))?;
                    let len = if *xmm >= 8 { 9 } else { 8 };
                    let next_ip = current_ip + len;
                    let rel32 = (target_addr as i64) - (next_ip as i64);
                    if rel32 < i32::MIN as i64 || rel32 > i32::MAX as i64 {
                        return Err(format!(
                            "Symbol '{label}' is out of 32-bit relative range (delta {rel32})"
                        ));
                    }
                    block_bytes.push(0xF3);
                    if *xmm >= 8 {
                        block_bytes.push(0x44);
                    }
                    block_bytes.push(0x0F);
                    block_bytes.push(if *is_write { 0x11 } else { 0x10 });
                    let modrm = 0x05 | ((*xmm & 7) << 3);
                    block_bytes.push(modrm);
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

    // Automatically register all allocated blocks and labels so the user never has to write registersymbol manually
    for (name, addr) in &symbols {
        register_symbol(name, pid, *addr);
        if !registered_symbols.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            registered_symbols.push(name.clone());
        }
    }

    Ok(ActiveHookSession {
        pid,
        allocated_blocks,
        original_patches,
        registered_symbols,
    })
}

pub fn restore_hook_session(pid: u32, session: &ActiveHookSession) {
    // 1. Unregister all symbols registered in this session
    for sym in &session.registered_symbols {
        unregister_symbol(sym);
    }
    // 2. Restore original code bytes at hook sites
    for (addr, bytes) in &session.original_patches {
        let _ = crate::process_memory::write_code_bytes(pid, *addr, bytes);
    }
    // 3. Free all allocated memory blocks
    for (_name, addr, _size) in &session.allocated_blocks {
        let _ = crate::process_memory::free_allocated_memory(pid, *addr);
    }
}

fn scan_aob_for_script(pid: u32, module: &str, pattern: &str) -> Result<usize, String> {
    #[cfg(windows)]
    {
        let total = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let options = crate::process_memory::MemoryScanOptions::default();
        if !module.is_empty() {
            let modules = crate::memory_debugger::debugger::process_modules(pid)
                .map_err(|e| format!("Could not read modules for AOB scan: {e}"))?;
            let mut mod_base = 0;
            let mut mod_size = 0;
            for (mname, base, size) in modules {
                if mname.eq_ignore_ascii_case(module) {
                    mod_base = base;
                    mod_size = size;
                    break;
                }
            }
            if mod_base == 0 {
                return Err(format!("Module '{module}' not found for aobscanmodule"));
            }
            let matches = crate::process_memory::scan_aob_memory_range_with_progress(
                pid,
                pattern,
                mod_base,
                mod_size,
                1,
                options,
                total,
            )
            .map_err(|e| format!("AOB scan failed: {e}"))?;

            if let Some(first) = matches.first() {
                Ok(first.address)
            } else {
                Err(format!("AOB pattern '{pattern}' not found in module '{module}'"))
            }
        } else {
            let matches = crate::process_memory::scan_aob_memory_addresses(
                pid,
                pattern,
                1,
                options,
                total,
            )
            .map_err(|e| format!("AOB scan failed: {e}"))?;

            if let Some(&first) = matches.first() {
                Ok(first)
            } else {
                Err(format!("AOB pattern '{pattern}' not found in process"))
            }
        }
    }
    #[cfg(not(windows))]
    Err("AOB scan not supported on this platform".to_string())
}

const MNEMONICS: &[&str] = &[
    "movups", "movaps", "movss", "movsd", "mov",
    "cvtsi2sd", "cvtsi2ss", "cvttsd2si", "cvttss2si",
    "addss", "subss", "mulss", "divss",
    "addsd", "subsd", "mulsd", "divsd",
    "addps", "subps", "mulps", "divps",
    "addpd", "subpd", "mulpd", "divpd",
    "jmp", "call", "ret", "nop", "db",
    "xorps", "xorpd", "xor", "push", "pop",
    "sub", "add", "lea", "cmp", "test", "and", "or", "inc", "dec",
    "je", "jz", "jne", "jnz", "ja", "jae", "jb", "jbe", "jg", "jge", "jl", "jle", "js", "jns",
];

const DIRECTIVE_PREFIXES: &[&str] = &[
    "alloc(", "dealloc(", "label(", "registersymbol(", "unregistersymbol(", "assert(",
    "aobscanmodule(", "aobscan(",
];

fn find_section_header(script: &str, tag: &str) -> Option<usize> {
    let mut offset = 0;
    for line in script.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("//") && !trimmed.starts_with(';') {
            let upper = trimmed.to_ascii_uppercase();
            if let Some(idx) = upper.find(tag) {
                let before = &trimmed[..idx];
                if !before.contains("//") && !before.contains(';') {
                    let line_start = offset + (line.len() - trimmed.len());
                    return Some(line_start + idx);
                }
            }
        }
        offset += line.len();
    }
    None
}

fn get_section_text(script: &str, is_enable: bool) -> &str {
    let enable_pos = find_section_header(script, "[ENABLE]");
    let disable_pos = find_section_header(script, "[DISABLE]");

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

pub fn generate_code_injection_template(pid: Option<u32>, target_expr: &str) -> String {
    let addr_str = if target_expr.trim().is_empty() {
        "neox_engine.dll+50A7E2"
    } else {
        target_expr.trim()
    };

    let mut orig_code = "read original instruction here".to_string();
    let mut nop_str = "nop\n".to_string();
    let mut alt_bytes = String::new();

    #[cfg(windows)]
    if let Some(pid) = pid {
        let symbols = HashMap::new();
        if let Ok(addr) = resolve_address_expr(pid, addr_str, &symbols) {
            if let Ok(lines) = crate::memory_debugger::debugger::disassemble_from(
                pid,
                addr,
                crate::model::MemoryDebuggerArchitecture::Auto,
                6,
            ) {
                let mut total_len = 0;
                let mut instrs = Vec::new();
                let mut hex_bytes: Vec<String> = Vec::new();
                for (_inst_addr, bytes, asm) in lines {
                    let parts: Vec<&str> = bytes.split_whitespace().collect();
                    total_len += parts.len();
                    hex_bytes.extend(parts.into_iter().map(ToString::to_string));
                    instrs.push(asm);
                    if total_len >= 5 {
                        break;
                    }
                }
                if total_len >= 5 {
                    orig_code = instrs.join("\n");
                    if !hex_bytes.is_empty() {
                        alt_bytes = format!("//alt: db {}\n", hex_bytes.join(" "));
                    }
                    let nops = total_len - 5;
                    nop_str = if nops > 1 {
                        format!("nop {nops}\n")
                    } else if nops == 1 {
                        "nop\n".to_string()
                    } else {
                        String::new()
                    };
                }
            }
        }
    }

    format!(
r#"[ENABLE]
//code from here to '[DISABLE]' will be used to enable the cheat
alloc(newmem,2048,{addr_str})
label(returnhere)
label(originalcode)
label(exit)

newmem: //this is allocated memory, you have read,write,execute access
//place your code here

originalcode:
{orig_code}

exit:
jmp returnhere

{addr_str}:
jmp newmem
{nop_str}returnhere:

[DISABLE]
//code from here till the end of the code will be used to disable the cheat
dealloc(newmem)
{addr_str}:
{orig_code}
{alt_bytes}"#)
}

pub fn generate_framework_template() -> String {
    r#"[ENABLE]
//code from here to '[DISABLE]' will be used to enable the cheat



[DISABLE]
//code from here till the end of the code will be used to disable the cheat
"#.to_string()
}

pub fn generate_aob_injection_template(target_expr: &str) -> String {
    let (mod_name, aob) = if target_expr.contains('+') {
        let parts: Vec<&str> = target_expr.split('+').collect();
        (parts[0].trim(), "48 89 5C 24 ??")
    } else {
        ("game.exe", "48 89 5C 24 ??")
    };

    format!(
r#"[ENABLE]
aobscanmodule(INJECT,{mod_name},{aob})
alloc(newmem,2048,INJECT)
label(returnhere)
label(originalcode)
label(exit)

newmem: //this is allocated memory, you have read,write,execute access
//place your code here

originalcode:
//read original instruction here

exit:
jmp returnhere

INJECT:
jmp newmem
returnhere:

[DISABLE]
//code from here till the end of the code will be used to disable the cheat
dealloc(newmem)
INJECT:
//read original instruction here
"#)
}

pub fn generate_nop_patch_template(target_expr: &str) -> String {
    let addr_str = if target_expr.trim().is_empty() {
        "neox_engine.dll+50A7E2"
    } else {
        target_expr.trim()
    };

    format!(
r#"[ENABLE]
{addr_str}:
nop 2

[DISABLE]
{addr_str}:
// restore original instruction here
"#)
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

    // aobscanmodule(name, module, pattern)
    if lower.starts_with("aobscanmodule(") && lower.ends_with(')') {
        let inner = &line[14..line.len() - 1];
        let parts: Vec<&str> = inner.splitn(3, ',').map(|s| s.trim()).collect();
        if parts.len() == 3 {
            return Ok(Some(AsmStatement::AobScanModule {
                symbol: parts[0].to_string(),
                module: parts[1].to_string(),
                pattern: parts[2].to_string(),
            }));
        }
        return Err(format!("Invalid aobscanmodule syntax (expected name, module, pattern): {line}"));
    }

    // aobscan(name, pattern)
    if lower.starts_with("aobscan(") && lower.ends_with(')') {
        let inner = &line[8..line.len() - 1];
        let parts: Vec<&str> = inner.splitn(2, ',').map(|s| s.trim()).collect();
        if parts.len() == 2 {
            return Ok(Some(AsmStatement::AobScanModule {
                symbol: parts[0].to_string(),
                module: String::new(),
                pattern: parts[1].to_string(),
            }));
        }
        return Err(format!("Invalid aobscan syntax (expected name, pattern): {line}"));
    }

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

    // registersymbol(name)
    if lower.starts_with("registersymbol(") && lower.ends_with(')') {
        let inner = line[15..line.len() - 1].trim();
        return Ok(Some(AsmStatement::RegisterSymbol(inner.to_string())));
    }

    // unregistersymbol(name)
    if lower.starts_with("unregistersymbol(") && lower.ends_with(')') {
        let inner = line[17..line.len() - 1].trim();
        return Ok(Some(AsmStatement::UnregisterSymbol(inner.to_string())));
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

    // dw <value> (e.g. `dw 100`)
    if lower.starts_with("dw ") {
        let val_str = line[3..].trim();
        let bytes = parse_data_word(val_str)?;
        return Ok(Some(AsmStatement::Bytes(bytes)));
    }

    // dd <value> (e.g. `dd (float)100.0` or `dd 100` or `dd 0x42C80000`)
    if lower.starts_with("dd ") {
        let val_str = line[3..].trim();
        let bytes = parse_data_dword(val_str)?;
        return Ok(Some(AsmStatement::Bytes(bytes)));
    }

    // dq <value> (e.g. `dq 100` or `dq (double)100.0`)
    if lower.starts_with("dq ") {
        let val_str = line[3..].trim();
        let bytes = parse_data_qword(val_str)?;
        return Ok(Some(AsmStatement::Bytes(bytes)));
    }

    // movss xmm, [label] or movss [label], xmm (RIP-relative addressing in x64)
    if lower.starts_with("movss ") {
        let rest = line[6..].trim();
        if let Some((d, s)) = rest.split_once(',') {
            let (d, s) = (d.trim(), s.trim());
            let d_clean = strip_ptr_prefix(d);
            let s_clean = strip_ptr_prefix(s);
            if let Some(xmm) = parse_xmm(d) {
                if s_clean.starts_with('[') && s_clean.ends_with(']') {
                    let inner = s_clean[1..s_clean.len() - 1].trim();
                    if parse_memory_operand(s_clean).is_none() && !inner.is_empty() {
                        return Ok(Some(AsmStatement::RipRelMovss {
                            is_write: false,
                            xmm,
                            label: inner.to_string(),
                        }));
                    }
                }
            } else if let Some(xmm) = parse_xmm(s) {
                if d_clean.starts_with('[') && d_clean.ends_with(']') {
                    let inner = d_clean[1..d_clean.len() - 1].trim();
                    if parse_memory_operand(d_clean).is_none() && !inner.is_empty() {
                        return Ok(Some(AsmStatement::RipRelMovss {
                            is_write: true,
                            xmm,
                            label: inner.to_string(),
                        }));
                    }
                }
            }
        }
    }

    // mov r32/r64, [label] or mov [label], r32/r64 (RIP-relative addressing in x64)
    if lower.starts_with("mov ") {
        let rest = line[4..].trim();
        if let Some((d, s)) = rest.split_once(',') {
            let (d, s) = (d.trim(), s.trim());
            let d_clean = strip_ptr_prefix(d);
            let s_clean = strip_ptr_prefix(s);
            if let Some(r32) = parse_reg32(d) {
                if s_clean.starts_with('[') && s_clean.ends_with(']') {
                    let inner = s_clean[1..s_clean.len() - 1].trim();
                    if parse_memory_operand(s_clean).is_none() && !inner.is_empty() {
                        return Ok(Some(AsmStatement::RipRelMov {
                            is_write: false,
                            is_64: false,
                            reg: r32,
                            label: inner.to_string(),
                        }));
                    }
                }
            } else if let Some(r32) = parse_reg32(s) {
                if d_clean.starts_with('[') && d_clean.ends_with(']') {
                    let inner = d_clean[1..d_clean.len() - 1].trim();
                    if parse_memory_operand(d_clean).is_none() && !inner.is_empty() {
                        return Ok(Some(AsmStatement::RipRelMov {
                            is_write: true,
                            is_64: false,
                            reg: r32,
                            label: inner.to_string(),
                        }));
                    }
                }
            } else if let Some(r64) = parse_reg64(d) {
                if s_clean.starts_with('[') && s_clean.ends_with(']') {
                    let inner = s_clean[1..s_clean.len() - 1].trim();
                    if parse_memory_operand(s_clean).is_none() && !inner.is_empty() {
                        return Ok(Some(AsmStatement::RipRelMov {
                            is_write: false,
                            is_64: true,
                            reg: r64,
                            label: inner.to_string(),
                        }));
                    }
                }
            } else if let Some(r64) = parse_reg64(s) {
                if d_clean.starts_with('[') && d_clean.ends_with(']') {
                    let inner = d_clean[1..d_clean.len() - 1].trim();
                    if parse_memory_operand(d_clean).is_none() && !inner.is_empty() {
                        return Ok(Some(AsmStatement::RipRelMov {
                            is_write: true,
                            is_64: true,
                            reg: r64,
                            label: inner.to_string(),
                        }));
                    }
                }
            }
        }
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

fn strip_ptr_prefix(text: &str) -> &str {
    let mut s = text.trim();
    for prefix in ["byte ptr", "word ptr", "dword ptr", "qword ptr", "xmmword ptr"] {
        if s.to_ascii_lowercase().starts_with(prefix) {
            s = s[prefix.len()..].trim();
        }
    }
    s
}

fn parse_data_word(val_str: &str) -> Result<Vec<u8>, String> {
    let lower = val_str.to_ascii_lowercase();
    let num_str = lower.strip_prefix("(short)")
        .or_else(|| lower.strip_prefix("(word)"))
        .unwrap_or(&lower)
        .trim();
    if let Some(num) = parse_number(num_str) {
        return Ok((num as u16).to_le_bytes().to_vec());
    }
    if let Ok(bytes) = parse_hex_bytes(num_str) {
        if bytes.len() == 2 {
            return Ok(bytes);
        }
    }
    Err(format!("Could not parse word in dw: '{val_str}'"))
}

fn parse_data_dword(val_str: &str) -> Result<Vec<u8>, String> {
    let lower = val_str.to_ascii_lowercase();
    if let Some(float_str) = lower.strip_prefix("(float)") {
        let val: f32 = float_str.trim().parse()
            .map_err(|e| format!("Invalid float value '{float_str}' in dd: {e}"))?;
        return Ok(val.to_le_bytes().to_vec());
    }
    let num_str = lower.strip_prefix("(int)")
        .or_else(|| lower.strip_prefix("(dword)"))
        .or_else(|| lower.strip_prefix("(uint)"))
        .unwrap_or(&lower)
        .trim();
    if let Some(num) = parse_number(num_str) {
        return Ok((num as u32).to_le_bytes().to_vec());
    }
    if num_str.contains('.') {
        if let Ok(val) = num_str.parse::<f32>() {
            return Ok(val.to_le_bytes().to_vec());
        }
    }
    if let Ok(bytes) = parse_hex_bytes(num_str) {
        if bytes.len() == 4 {
            return Ok(bytes);
        }
    }
    Err(format!("Could not parse dword in dd: '{val_str}'"))
}

fn parse_data_qword(val_str: &str) -> Result<Vec<u8>, String> {
    let lower = val_str.to_ascii_lowercase();
    if let Some(double_str) = lower.strip_prefix("(double)") {
        let val: f64 = double_str.trim().parse()
            .map_err(|e| format!("Invalid double value '{double_str}' in dq: {e}"))?;
        return Ok(val.to_le_bytes().to_vec());
    }
    let num_str = lower.strip_prefix("(qword)")
        .or_else(|| lower.strip_prefix("(int64)"))
        .unwrap_or(&lower)
        .trim();
    if let Some(num) = parse_number(num_str) {
        return Ok((num as u64).to_le_bytes().to_vec());
    }
    if num_str.contains('.') {
        if let Ok(val) = num_str.parse::<f64>() {
            return Ok(val.to_le_bytes().to_vec());
        }
    }
    if let Ok(bytes) = parse_hex_bytes(num_str) {
        if bytes.len() == 8 {
            return Ok(bytes);
        }
    }
    Err(format!("Could not parse qword in dq: '{val_str}'"))
}

fn clean_address_target(expr: &str) -> &str {
    let mut s = expr.trim();
    for prefix in ["short ", "near ptr ", "near ", "far ptr ", "far "] {
        if s.to_ascii_lowercase().starts_with(prefix) {
            s = s[prefix.len()..].trim();
        }
    }
    s
}

fn parse_number(text: &str) -> Option<usize> {
    let t = clean_address_target(text);
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16).ok()
    } else if t.ends_with('h') || t.ends_with('H') {
        usize::from_str_radix(&t[..t.len() - 1], 16).ok()
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
    let mut t = text.trim();
    if t.ends_with('h') || t.ends_with('H') {
        t = &t[..t.len() - 1];
        return usize::from_str_radix(t, 16).ok();
    }
    let hex_clean = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    usize::from_str_radix(hex_clean, 16).ok().or_else(|| t.parse::<usize>().ok())
}

fn resolve_address_expr(
    pid: u32,
    expr: &str,
    symbols: &HashMap<String, usize>,
) -> Result<usize, String> {
    let expr = clean_address_target(expr);
    let lower = expr.to_ascii_lowercase();

    // Check symbol table
    if let Some(&addr) = symbols.get(&lower) {
        return Ok(addr);
    }

    // Check globally registered symbols
    if let Some((_, addr)) = get_registered_symbol(&lower) {
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
        "addss" => assemble_sse_binary_op(Some(0xF3), 0x58, rest),
        "subss" => assemble_sse_binary_op(Some(0xF3), 0x5C, rest),
        "mulss" => assemble_sse_binary_op(Some(0xF3), 0x59, rest),
        "divss" => assemble_sse_binary_op(Some(0xF3), 0x5E, rest),
        "addsd" => assemble_sse_binary_op(Some(0xF2), 0x58, rest),
        "subsd" => assemble_sse_binary_op(Some(0xF2), 0x5C, rest),
        "mulsd" => assemble_sse_binary_op(Some(0xF2), 0x59, rest),
        "divsd" => assemble_sse_binary_op(Some(0xF2), 0x5E, rest),
        "addps" => assemble_sse_binary_op(None, 0x58, rest),
        "subps" => assemble_sse_binary_op(None, 0x5C, rest),
        "mulps" => assemble_sse_binary_op(None, 0x59, rest),
        "divps" => assemble_sse_binary_op(None, 0x5E, rest),
        "addpd" => assemble_sse_binary_op(Some(0x66), 0x58, rest),
        "subpd" => assemble_sse_binary_op(Some(0x66), 0x5C, rest),
        "mulpd" => assemble_sse_binary_op(Some(0x66), 0x59, rest),
        "divpd" => assemble_sse_binary_op(Some(0x66), 0x5E, rest),
        "xor" => assemble_xor(rest),
        "xorps" => assemble_xorps(rest),
        "xorpd" => assemble_xorpd(rest),
        "push" => assemble_push(rest),
        "pop" => assemble_pop(rest),
        "add" => assemble_add(rest),
        "sub" => assemble_sub(rest),
        _ => Ok(None),
    }
}

fn assemble_mov(operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };

    // mov al, 1 / mov <r8>, <imm8>
    if let Some(r8) = parse_reg8(dst) {
        if let Some(imm) = parse_imm32(src) {
            let mut bytes = Vec::new();
            if r8 >= 8 {
                bytes.push(0x41);
            } else if r8 >= 4 {
                bytes.push(0x40);
            }
            bytes.push(0xB0 + (r8 & 7));
            bytes.push(imm as u8);
            return Ok(Some(bytes));
        }
        if let Some(r8_src) = parse_reg8(src) {
            let mut bytes = Vec::new();
            let rex_r = if r8_src >= 8 { 4 } else { 0 };
            let rex_b = if r8 >= 8 { 1 } else { 0 };
            let rex_needed = rex_r != 0 || rex_b != 0 || (r8 >= 4 && r8 < 8) || (r8_src >= 4 && r8_src < 8);
            if rex_needed {
                bytes.push(0x40 | rex_r | rex_b);
            }
            bytes.push(0x88);
            let modrm = 0xC0 | ((r8_src & 7) << 3) | (r8 & 7);
            bytes.push(modrm);
            return Ok(Some(bytes));
        }
    }

    // mov ax, 1 / mov <r16>, <imm16>
    if let Some(r16) = parse_reg16(dst) {
        if let Some(imm) = parse_imm32(src) {
            let mut bytes = vec![0x66];
            if r16 >= 8 {
                bytes.push(0x41);
            }
            bytes.push(0xB8 + (r16 & 7));
            bytes.extend_from_slice(&(imm as u16).to_le_bytes());
            return Ok(Some(bytes));
        }
    }

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
        let rex_r = if xmm >= 8 { 4 } else { 0 };
        let rex_b = if r32 >= 8 { 1 } else { 0 };
        let mut bytes = vec![0xF3];
        if rex_r != 0 || rex_b != 0 {
            bytes.push(0x40 | rex_r | rex_b);
        }
        bytes.push(0x0F);
        bytes.push(0x2A);
        bytes.push(0xC0 | ((xmm & 7) << 3) | (r32 & 7));
        return Ok(Some(bytes));
    }
    if let Some(r64) = parse_reg64(src) {
        let rex_r = if xmm >= 8 { 4 } else { 0 };
        let rex_b = if r64 >= 8 { 1 } else { 0 };
        let mut bytes = vec![0xF3, 0x48 | rex_r | rex_b, 0x0F, 0x2A];
        bytes.push(0xC0 | ((xmm & 7) << 3) | (r64 & 7));
        return Ok(Some(bytes));
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

fn assemble_sse_binary_op(prefix: Option<u8>, opcode: u8, operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };
    if let (Some(x1), Some(x2)) = (parse_xmm(dst), parse_xmm(src)) {
        let mut bytes = Vec::new();
        if let Some(p) = prefix {
            bytes.push(p);
        }
        let rex_r = if x1 >= 8 { 4 } else { 0 };
        let rex_b = if x2 >= 8 { 1 } else { 0 };
        if rex_r != 0 || rex_b != 0 {
            bytes.push(0x40 | rex_r | rex_b);
        }
        bytes.push(0x0F);
        bytes.push(opcode);
        let modrm = 0xC0 | ((x1 & 7) << 3) | (x2 & 7);
        bytes.push(modrm);
        return Ok(Some(bytes));
    }
    if let Some(x1) = parse_xmm(dst) {
        if let Some((base, disp)) = parse_memory_operand(src) {
            let mut bytes = Vec::new();
            if let Some(p) = prefix {
                bytes.push(p);
            }
            let rex_r = if x1 >= 8 { 4 } else { 0 };
            let rex_b = if base >= 8 { 1 } else { 0 };
            if rex_r != 0 || rex_b != 0 {
                bytes.push(0x40 | rex_r | rex_b);
            }
            bytes.push(0x0F);
            bytes.push(opcode);
            let (modrm, sib, disp_bytes) = encode_modrm(x1, base, disp);
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            bytes.extend_from_slice(&disp_bytes);
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn assemble_add(operands: &str) -> Result<Option<Vec<u8>>, String> {
    assemble_int_arithmetic(0, operands)
}

fn assemble_sub(operands: &str) -> Result<Option<Vec<u8>>, String> {
    assemble_int_arithmetic(5, operands)
}

fn assemble_int_arithmetic(sub_code: u8, operands: &str) -> Result<Option<Vec<u8>>, String> {
    let (dst, src) = match operands.split_once(',') {
        Some((d, s)) => (d.trim(), s.trim()),
        None => return Ok(None),
    };
    // reg64, imm
    if let Some(r64) = parse_reg64(dst) {
        if let Some(imm) = parse_imm32(src) {
            let rex = 0x48 | if r64 >= 8 { 1 } else { 0 };
            let r_low = r64 & 7;
            if (-128..=127).contains(&imm) {
                return Ok(Some(vec![rex, 0x83, 0xC0 | (sub_code << 3) | r_low, imm as u8]));
            } else {
                let mut b = vec![rex, 0x81, 0xC0 | (sub_code << 3) | r_low];
                b.extend_from_slice(&imm.to_le_bytes());
                return Ok(Some(b));
            }
        }
        if let Some(src_r64) = parse_reg64(src) {
            let rex_r = if src_r64 >= 8 { 4 } else { 0 };
            let rex_b = if r64 >= 8 { 1 } else { 0 };
            let rex = 0x48 | rex_r | rex_b;
            let opcode = match sub_code {
                0 => 0x01, // add
                _ => 0x29, // sub
            };
            let modrm = 0xC0 | ((src_r64 & 7) << 3) | (r64 & 7);
            return Ok(Some(vec![rex, opcode, modrm]));
        }
    }
    // reg32, imm
    if let Some(r32) = parse_reg32(dst) {
        if let Some(imm) = parse_imm32(src) {
            let rex = if r32 >= 8 { Some(0x41) } else { None };
            let r_low = r32 & 7;
            let mut b = Vec::new();
            if let Some(r) = rex { b.push(r); }
            if (-128..=127).contains(&imm) {
                b.push(0x83);
                b.push(0xC0 | (sub_code << 3) | r_low);
                b.push(imm as u8);
            } else {
                b.push(0x81);
                b.push(0xC0 | (sub_code << 3) | r_low);
                b.extend_from_slice(&imm.to_le_bytes());
            }
            return Ok(Some(b));
        }
    }
    // [memory], imm (e.g. sub qword ptr [r12], 1)
    if let Some((base, disp)) = parse_memory_operand(dst) {
        if let Some(imm) = parse_imm32(src) {
            let is_qword = dst.to_ascii_lowercase().starts_with("qword ptr");
            let rex_w = if is_qword { 8 } else { 0 };
            let rex_b = if base >= 8 { 1 } else { 0 };
            let rex = if rex_w != 0 || rex_b != 0 { Some(0x40 | rex_w | rex_b) } else { None };
            let (modrm_byte, sib, disp_bytes) = encode_modrm(sub_code, base, disp);
            let mut b = Vec::new();
            if let Some(r) = rex { b.push(r); }
            if (-128..=127).contains(&imm) {
                b.push(0x83);
                b.push(modrm_byte);
                if let Some(s) = sib { b.push(s); }
                b.extend_from_slice(&disp_bytes);
                b.push(imm as u8);
            } else {
                b.push(0x81);
                b.push(modrm_byte);
                if let Some(s) = sib { b.push(s); }
                b.extend_from_slice(&disp_bytes);
                b.extend_from_slice(&imm.to_le_bytes());
            }
            return Ok(Some(b));
        }
    }
    Ok(None)
}

// Helpers for registers and operands
fn parse_reg8(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "al" => Some(0),
        "cl" => Some(1),
        "dl" => Some(2),
        "bl" => Some(3),
        "spl" | "ah" => Some(4),
        "bpl" | "ch" => Some(5),
        "sil" | "dh" => Some(6),
        "dil" | "bh" => Some(7),
        "r8b" => Some(8),
        "r9b" => Some(9),
        "r10b" => Some(10),
        "r11b" => Some(11),
        "r12b" => Some(12),
        "r13b" => Some(13),
        "r14b" => Some(14),
        "r15b" => Some(15),
        _ => None,
    }
}

fn parse_reg16(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "ax" => Some(0),
        "cx" => Some(1),
        "dx" => Some(2),
        "bx" => Some(3),
        "sp" => Some(4),
        "bp" => Some(5),
        "si" => Some(6),
        "di" => Some(7),
        "r8w" => Some(8),
        "r9w" => Some(9),
        "r10w" => Some(10),
        "r11w" => Some(11),
        "r12w" => Some(12),
        "r13w" => Some(13),
        "r14w" => Some(14),
        "r15w" => Some(15),
        _ => None,
    }
}

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
    let s = strip_ptr_prefix(text);
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
            assemble_x64_instruction("mov al, 1").unwrap(),
            Some(vec![0xB0, 0x01])
        );
        assert_eq!(
            assemble_x64_instruction("mov al,1").unwrap(),
            Some(vec![0xB0, 0x01])
        );
        assert_eq!(
            assemble_x64_instruction("mov eax, 1").unwrap(),
            Some(vec![0xB8, 0x01, 0x00, 0x00, 0x00])
        );
        assert_eq!(
            assemble_x64_instruction("cvtsi2ss xmm7, eax").unwrap(),
            Some(vec![0xF3, 0x0F, 0x2A, 0xF8])
        );
        assert_eq!(
            assemble_x64_instruction("cvtsi2ss xmm6, rdx").unwrap(),
            Some(vec![0xF3, 0x48, 0x0F, 0x2A, 0xF2])
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
        assert_eq!(
            assemble_x64_instruction("addss xmm0, xmm1").unwrap(),
            Some(vec![0xF3, 0x0F, 0x58, 0xC1])
        );
        assert_eq!(
            assemble_x64_instruction("subss xmm0, xmm1").unwrap(),
            Some(vec![0xF3, 0x0F, 0x5C, 0xC1])
        );
        assert_eq!(
            assemble_x64_instruction("mulss xmm0, xmm1").unwrap(),
            Some(vec![0xF3, 0x0F, 0x59, 0xC1])
        );
        assert_eq!(
            assemble_x64_instruction("sub qword ptr [r12], 1").unwrap(),
            Some(vec![0x49, 0x83, 0x2C, 0x24, 0x01])
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

    #[test]
    fn test_script_with_comments_containing_section_names() {
        let script = r#"
[ENABLE]
//code from here to '[DISABLE]' will be used to enable the cheat
alloc(newmem,2048,neox_engine.dll+50A7E2)
label(returnhere)
label(originalcode)
label(exit)

newmem:
mov eax,1
cvtsi2sd xmm0,eax

originalcode:
movups [rcx+88h],xmm0

exit:
jmp returnhere

neox_engine.dll+50A7E2:
jmp newmem
nop 2
returnhere:

[DISABLE]
//code from here till the end of the code will be used to disable the cheat
dealloc(newmem)
neox_engine.dll+50A7E2:
movups [rcx+88h],xmm0
//alt: db 0F 11 81 88 00 00 00
"#;
        let enable_stmts = extract_script_section(script, true);
        assert_eq!(enable_stmts.len(), 15, "Got: {:?}", enable_stmts);
        assert_eq!(enable_stmts[0], "alloc(newmem,2048,neox_engine.dll+50A7E2)");
        assert_eq!(enable_stmts[8], "movups [rcx+88h],xmm0");

        let disable_stmts = extract_script_section(script, false);
        assert_eq!(disable_stmts.len(), 3, "Got: {:?}", disable_stmts);
        assert_eq!(disable_stmts[0], "dealloc(newmem)");
        assert_eq!(disable_stmts[2], "movups [rcx+88h],xmm0");

        // Verify assembling movups [rcx+88h], xmm0 produces same bytes as [rcx+00000088]
        let b1 = assemble_x64_instruction("movups [rcx+88h], xmm0").unwrap().unwrap();
        let b2 = assemble_x64_instruction("movups [rcx+00000088], xmm0").unwrap().unwrap();
        assert_eq!(b1, b2);
    }

    #[test]
    fn test_jump_target_cleaning_and_resolution() {
        let syms = HashMap::new();
        let target = "short 00007FF8317CBAC3h";
        let resolved = resolve_address_expr(0, target, &syms).unwrap();
        assert_eq!(resolved, 0x7FF8317CBAC3);

        let target2 = "near ptr 1234H";
        let resolved2 = resolve_address_expr(0, target2, &syms).unwrap();
        assert_eq!(resolved2, 0x1234);

        let parsed = parse_statement("je short 00007FF8317CBAC3h").unwrap();
        assert!(matches!(parsed, Some(AsmStatement::CondJmpRel32 { .. })));
    }

    #[test]
    fn test_registered_symbols_and_riprel_movss() {
        let parsed_dd = parse_statement("dd (float)100.0").unwrap().unwrap();
        assert_eq!(parsed_dd, AsmStatement::Bytes(100.0f32.to_le_bytes().to_vec()));

        let parsed_reg = parse_statement("registersymbol(fov_val)").unwrap().unwrap();
        assert_eq!(parsed_reg, AsmStatement::RegisterSymbol("fov_val".to_string()));

        let parsed_unreg = parse_statement("unregistersymbol(fov_val)").unwrap().unwrap();
        assert_eq!(parsed_unreg, AsmStatement::UnregisterSymbol("fov_val".to_string()));

        let parsed_movss = parse_statement("movss xmm6, [fov_val]").unwrap().unwrap();
        assert_eq!(
            parsed_movss,
            AsmStatement::RipRelMovss {
                is_write: false,
                xmm: 6,
                label: "fov_val".to_string(),
            }
        );

        let parsed_mov = parse_statement("mov eax, [fov_val]").unwrap().unwrap();
        assert_eq!(
            parsed_mov,
            AsmStatement::RipRelMov {
                is_write: false,
                is_64: false,
                reg: 0,
                label: "fov_val".to_string(),
            }
        );

        let parsed_mov_ptr = parse_statement("mov eax, dword ptr [fov_val]").unwrap().unwrap();
        assert_eq!(parsed_mov_ptr, parsed_mov);

        let parsed_mov_write = parse_statement("mov [fov_val], eax").unwrap().unwrap();
        assert_eq!(
            parsed_mov_write,
            AsmStatement::RipRelMov {
                is_write: true,
                is_64: false,
                reg: 0,
                label: "fov_val".to_string(),
            }
        );

        let parsed_mov64 = parse_statement("mov rax, [fov_val]").unwrap().unwrap();
        assert_eq!(
            parsed_mov64,
            AsmStatement::RipRelMov {
                is_write: false,
                is_64: true,
                reg: 0,
                label: "fov_val".to_string(),
            }
        );

        let parsed_dw = parse_statement("dw 100").unwrap().unwrap();
        assert_eq!(parsed_dw, AsmStatement::Bytes(100u16.to_le_bytes().to_vec()));

        let parsed_dd_int = parse_statement("dd 100").unwrap().unwrap();
        assert_eq!(parsed_dd_int, AsmStatement::Bytes(100u32.to_le_bytes().to_vec()));

        let parsed_dq = parse_statement("dq 100").unwrap().unwrap();
        assert_eq!(parsed_dq, AsmStatement::Bytes(100u64.to_le_bytes().to_vec()));

        let parsed_cvtsi2ss = parse_statement("cvtsi2ss xmm6, eax").unwrap().unwrap();
        assert_eq!(parsed_cvtsi2ss, AsmStatement::Bytes(vec![0xF3, 0x0F, 0x2A, 0xF0]));

        // Test user's exact snippet:
        // fov_val:
        // dd 100
        // fov:
        // mov eax, [fov_val]
        // cvtsi2ss xmm6, eax
        let snippet = ["fov_val:", "dd 100", "fov:", "mov eax, [fov_val]", "cvtsi2ss xmm6, eax"];
        for line in snippet {
            let res = parse_statement(line).unwrap();
            assert!(res.is_some(), "Failed parsing user snippet line: {line}");
        }

        register_symbol("fov_val", 1234, 0x7FF812340000);
        assert_eq!(get_registered_symbol("fov_val"), Some((1234, 0x7FF812340000)));
        assert_eq!(get_registered_symbol("FOV_VAL"), Some((1234, 0x7FF812340000)));

        let syms = HashMap::new();
        assert_eq!(resolve_address_expr(1234, "fov_val", &syms).unwrap(), 0x7FF812340000);

        unregister_symbol("fov_val");
        assert_eq!(get_registered_symbol("fov_val"), None);

        // Test end-to-end script with registered symbol and RIP-rel movss
        let script = r#"
[ENABLE]
alloc(newmem,2048,0x7FF810000000)
alloc(fov_val,4,0x7FF810000000)
registersymbol(fov_val)
label(returnhere)
label(originalcode)
label(exit)

newmem:
movss xmm6,[fov_val]

originalcode:
movss [rbx+84h],xmm6

exit:
jmp returnhere

fov_val:
dd (float)100.0

0x7FF810000000:
jmp newmem
nop 3
returnhere:

[DISABLE]
dealloc(newmem)
dealloc(fov_val)
unregistersymbol(fov_val)
0x7FF810000000:
movss [rbx+84h],xmm6
"#;
        let enable_stmts = extract_script_section(script, true);
        for stmt_str in &enable_stmts {
            let parsed = parse_statement(stmt_str).unwrap();
            assert!(parsed.is_some(), "Failed parsing statement: {stmt_str}");
        }

        let disable_stmts = extract_script_section(script, false);
        for stmt_str in &disable_stmts {
            let parsed = parse_statement(stmt_str).unwrap();
            assert!(parsed.is_some(), "Failed parsing statement: {stmt_str}");
        }
    }
}
