//! Pure-Rust live Android ARM64 Lua / ToLua / xLua / LuaJIT RAM Dumper & ELF Symbol Resolver.
//! Inspects guest `/proc/<pid>/mem` over ADB to locate native Lua C-API exports and loaded Lua chunks.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use crate::android_bridge::{get_android_process_maps, AndroidMemoryRegion};
use crate::il2cpp_dumper::read_guest_mem_exact;

const LUAJIT_MAGIC: &[u8; 3] = b"\x1bLJ";
const LUA_STD_MAGIC: &[u8; 4] = b"\x1bLua";

#[derive(Clone, Debug, Serialize)]
pub struct LuaSymbolInfo {
    pub module: String,
    pub name: String,
    pub rva: usize,
    pub va: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum LuaChunkKind {
    LuaJit,
    Lua5x,
    Plaintext,
}

impl LuaChunkKind {
    pub fn badge(self) -> &'static str {
        match self {
            Self::LuaJit => "LuaJIT",
            Self::Lua5x => "Lua 5.x",
            Self::Plaintext => "Plaintext",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LuaChunkInfo {
    pub index: usize,
    pub name: String,
    pub format: String,
    pub kind: LuaChunkKind,
    pub guest_va: usize,
    pub region_pathname: String,
    pub byte_len: usize,
    pub strings: Vec<String>,
    pub preview: String,
    #[serde(skip)]
    pub raw_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LuaModuleEntry {
    pub name: String,
    pub base_va: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct AndroidLuaDumpResult {
    pub guest_pid: u32,
    pub lua_modules: Vec<LuaModuleEntry>,
    pub symbols: Vec<LuaSymbolInfo>,
    pub chunks: Vec<LuaChunkInfo>,
    pub scanned_regions: usize,
    pub scanned_bytes: usize,
}

fn read_u16(buf: &[u8], off: usize) -> Option<u16> {
    buf.get(off..off + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    buf.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_u64(buf: &[u8], off: usize) -> Option<u64> {
    buf.get(off..off + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

fn read_uleb128(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    let mut shift = 0u32;
    while *pos < buf.len() && shift < 63 {
        let b = buf[*pos];
        *pos += 1;
        val |= ((b & 0x7F) as u64) << shift;
        if (b & 0x80) == 0 {
            return Some(val);
        }
        shift += 7;
    }
    None
}

fn read_cstr(buf: &[u8], off: usize) -> String {
    if off >= buf.len() {
        return String::new();
    }
    let slice = &buf[off..];
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len().min(256));
    String::from_utf8_lossy(&slice[..end]).into_owned()
}

fn normalize_elf_va(raw: usize, module_base: usize) -> usize {
    if raw >= module_base {
        raw - module_base
    } else {
        raw
    }
}

/// Parses ELF64 dynamic exports from a guest module slice or live guest memory reader.
/// Resolves Lua/ToLua/xLua/SLua/LuaJIT exported symbols (`lua*`, `tolua*`, `xlua*`, `slua*`).
pub fn parse_elf64_lua_exports_with_reader<F>(
    module_name: &str,
    module_base: usize,
    header_mem: &[u8],
    mut read_guest: F,
) -> Vec<LuaSymbolInfo>
where
    F: FnMut(usize, usize) -> Option<Vec<u8>>,
{
    if header_mem.len() < 64 || &header_mem[0..4] != b"\x7FELF" {
        return Vec::new();
    }
    // EI_CLASS == 2 (ELF64), EI_DATA == 1 (little endian)
    if header_mem[4] != 2 || header_mem[5] != 1 {
        return Vec::new();
    }
    let Some(e_phoff) = read_u64(header_mem, 32).map(|v| v as usize) else {
        return Vec::new();
    };
    let Some(e_phentsize) = read_u16(header_mem, 54).map(|v| v as usize) else {
        return Vec::new();
    };
    let Some(e_phnum) = read_u16(header_mem, 56).map(|v| v as usize) else {
        return Vec::new();
    };
    if e_phentsize < 56 || e_phnum == 0 || e_phnum > 128 {
        return Vec::new();
    }

    let mut dyn_rva: Option<usize> = None;
    let mut dyn_sz: usize = 0;
    for i in 0..e_phnum {
        let off = e_phoff + i * e_phentsize;
        if off + 56 > header_mem.len() {
            break;
        }
        let p_type = read_u32(header_mem, off).unwrap_or(0);
        if p_type == 2 {
            // PT_DYNAMIC
            dyn_rva = read_u64(header_mem, off + 16).map(|v| v as usize);
            dyn_sz = read_u64(header_mem, off + 40).unwrap_or(0) as usize;
            break;
        }
    }
    let Some(dyn_rva) = dyn_rva else {
        return Vec::new();
    };
    let dyn_len = dyn_sz.clamp(64, 64 * 1024);
    let dyn_buf_owned;
    let dyn_buf: &[u8] = if dyn_rva + dyn_len <= header_mem.len() {
        &header_mem[dyn_rva..dyn_rva + dyn_len]
    } else if let Some(b) = read_guest(module_base + dyn_rva, dyn_len) {
        dyn_buf_owned = b;
        &dyn_buf_owned
    } else {
        return Vec::new();
    };

    let mut strtab_rva = 0usize;
    let mut symtab_rva = 0usize;
    let mut strsz = 0usize;
    let mut syment = 24usize;
    let mut hash_rva = 0usize;

    let mut pos = 0usize;
    while pos + 16 <= dyn_buf.len() {
        let d_tag = read_u64(dyn_buf, pos).unwrap_or(0);
        let d_val = read_u64(dyn_buf, pos + 8).unwrap_or(0) as usize;
        pos += 16;
        match d_tag {
            0 => break,
            4 => hash_rva = normalize_elf_va(d_val, module_base),     // DT_HASH
            5 => strtab_rva = normalize_elf_va(d_val, module_base),   // DT_STRTAB
            6 => symtab_rva = normalize_elf_va(d_val, module_base),   // DT_SYMTAB
            10 => strsz = d_val,                                      // DT_STRSZ
            11 => {
                if d_val >= 24 {
                    syment = d_val;
                }
            }
            _ => {}
        }
    }

    if symtab_rva == 0 || strtab_rva == 0 || strsz == 0 || strsz > 16 * 1024 * 1024 {
        return Vec::new();
    }

    // Determine symbol count from DT_HASH nchain if present, else strtab-symtab distance
    let sym_count_from_hash = if hash_rva > 0 {
        if hash_rva + 8 <= header_mem.len() {
            read_u32(header_mem, hash_rva + 4).map(|v| v as usize)
        } else {
            read_guest(module_base + hash_rva, 8).and_then(|b| read_u32(&b, 4).map(|v| v as usize))
        }
    } else {
        None
    };

    let sym_count = sym_count_from_hash
        .unwrap_or_else(|| {
            if strtab_rva > symtab_rva {
                (strtab_rva - symtab_rva) / syment
            } else {
                4096
            }
        })
        .min(65_536);
    if sym_count == 0 {
        return Vec::new();
    }

    let symtab_len = sym_count * syment;
    let symtab_owned;
    let symtab_buf: &[u8] = if symtab_rva + symtab_len <= header_mem.len() {
        &header_mem[symtab_rva..symtab_rva + symtab_len]
    } else if let Some(b) = read_guest(module_base + symtab_rva, symtab_len) {
        symtab_owned = b;
        &symtab_owned
    } else {
        return Vec::new();
    };

    let strtab_owned;
    let strtab_buf: &[u8] = if strtab_rva + strsz <= header_mem.len() {
        &header_mem[strtab_rva..strtab_rva + strsz]
    } else if let Some(b) = read_guest(module_base + strtab_rva, strsz) {
        strtab_owned = b;
        &strtab_owned
    } else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for i in 0..sym_count {
        let off = i * syment;
        if off + 24 > symtab_buf.len() {
            break;
        }
        let st_name = read_u32(symtab_buf, off).unwrap_or(0) as usize;
        let st_shndx = read_u16(symtab_buf, off + 6).unwrap_or(0);
        let st_value = read_u64(symtab_buf, off + 8).unwrap_or(0) as usize;
        if st_shndx == 0 || st_value == 0 || st_name >= strtab_buf.len() {
            continue;
        }
        let name = read_cstr(strtab_buf, st_name);
        if name.is_empty() {
            continue;
        }
        let low = name.to_ascii_lowercase();
        if !low.contains("lua") {
            continue;
        }
        let rva = normalize_elf_va(st_value, module_base);
        if rva == 0 {
            continue;
        }
        out.push(LuaSymbolInfo {
            module: module_name.to_owned(),
            name,
            rva,
            va: module_base.saturating_add(rva),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn extract_printable_strings(bytes: &[u8], min_len: usize, max_count: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut cur = Vec::new();

    let flush = |cur: &mut Vec<u8>, seen: &mut HashSet<String>, out: &mut Vec<String>| {
        if cur.len() >= min_len && cur.len() <= 160 {
            if let Ok(s) = std::str::from_utf8(cur) {
                let trimmed = s.trim();
                let has_alpha = trimmed.bytes().any(|b| b.is_ascii_alphabetic() || b == b'_');
                if has_alpha && trimmed.len() >= min_len && seen.insert(trimmed.to_owned()) {
                    out.push(trimmed.to_owned());
                }
            }
        }
        cur.clear();
    };

    for &b in bytes {
        if (0x20..=0x7E).contains(&b) {
            cur.push(b);
        } else {
            flush(&mut cur, &mut seen, &mut out);
            if out.len() >= max_count {
                break;
            }
        }
    }
    if out.len() < max_count {
        flush(&mut cur, &mut seen, &mut out);
    }
    out
}

fn try_parse_luajit_chunk(buf: &[u8], start: usize, base_va: usize, region_path: &str) -> Option<(LuaChunkInfo, usize)> {
    let ver = *buf.get(start + 3)?;
    if ver != 1 && ver != 2 {
        return None;
    }
    let mut pos = start + 4;
    let flags = read_uleb128(buf, &mut pos)?;
    if flags > 0x3F {
        return None;
    }
    let stripped = (flags & 0x02) != 0;
    let name = if stripped {
        format!("luajit_0x{:X}", base_va + start)
    } else {
        let name_len = read_uleb128(buf, &mut pos)? as usize;
        if name_len == 0 || name_len > 512 || pos + name_len > buf.len() {
            return None;
        }
        let raw_name = &buf[pos..pos + name_len];
        pos += name_len;
        let s = std::str::from_utf8(raw_name).ok()?.trim_matches('\0');
        if s.is_empty() {
            format!("luajit_0x{:X}", base_va + start)
        } else {
            s.to_owned()
        }
    };

    // Walk LuaJIT prototype stream (<proto_size:uleb128> + <proto_bytes> ... until 0x00)
    let mut proto_count = 0usize;
    let max_end = (start + 2 * 1024 * 1024).min(buf.len());
    loop {
        if pos >= max_end {
            return None;
        }
        let psize = read_uleb128(buf, &mut pos)? as usize;
        if psize == 0 {
            break;
        }
        if psize > 1024 * 1024 || pos + psize > max_end {
            return None;
        }
        pos += psize;
        proto_count += 1;
        if proto_count > 16_384 {
            return None;
        }
    }
    if proto_count == 0 {
        return None;
    }

    let raw_bytes = buf[start..pos].to_vec();
    let strings = extract_printable_strings(&raw_bytes, 4, 300);
    let format = format!(
        "LuaJIT v{ver}{}",
        if stripped { " (stripped)" } else { "" }
    );
    let preview = if strings.is_empty() {
        format!("[{format} | {proto_count} prototypes | {} bytes]", raw_bytes.len())
    } else {
        format!(
            "-- {name} ({format}, {proto_count} prototypes, {} bytes)\n-- Extracted strings/identifiers:\n{}",
            raw_bytes.len(),
            strings.iter().take(80).cloned().collect::<Vec<_>>().join("\n")
        )
    };

    Some((
        LuaChunkInfo {
            index: 0,
            name,
            format,
            kind: LuaChunkKind::LuaJit,
            guest_va: base_va + start,
            region_pathname: region_path.to_owned(),
            byte_len: raw_bytes.len(),
            strings,
            preview,
            raw_bytes,
        },
        pos,
    ))
}

fn try_parse_std_lua_chunk(buf: &[u8], start: usize, base_va: usize, region_path: &str) -> Option<(LuaChunkInfo, usize)> {
    let ver = *buf.get(start + 4)?;
    if !(0x51..=0x54).contains(&ver) {
        return None;
    }
    let ver_str = match ver {
        0x51 => "Lua 5.1",
        0x52 => "Lua 5.2",
        0x53 => "Lua 5.3",
        _ => "Lua 5.4",
    };
    let hdr_slice = buf.get(start..buf.len().min(start + 64))?;
    // Standard Lua 5.1 header (12 bytes): magic(4), ver(1), fmt(1), endian(1), int(1), size_t(1), instr(1), num(1), integral(1)
    if ver == 0x51 && (hdr_slice.len() < 12 || hdr_slice[5] > 1 || hdr_slice[6] > 1) {
        return None;
    }

    let mut chunk_name = format!("lua_{ver_str}_0x{:X}", base_va + start);
    if ver == 0x51 {
        let size_t_sz = hdr_slice[8] as usize;
        if (size_t_sz == 4 || size_t_sz == 8) && start + 12 + size_t_sz <= buf.len() {
            let slen = if size_t_sz == 4 {
                read_u32(buf, start + 12).unwrap_or(0) as usize
            } else {
                read_u64(buf, start + 12).unwrap_or(0) as usize
            };
            let nstart = start + 12 + size_t_sz;
            if slen > 1 && slen <= 512 && nstart + slen <= buf.len() {
                if let Ok(s) = std::str::from_utf8(&buf[nstart..nstart + slen]) {
                    let s = s.trim_matches('\0');
                    if !s.is_empty() && s.is_ascii() {
                        chunk_name = s.to_owned();
                    }
                }
            }
        }
    }

    // Bound standard Lua bytecode slice up to next magic or max 128 KB printable region boundary
    let max_end = (start + 128 * 1024).min(buf.len());
    let mut end = start + 16;
    while end + 4 <= max_end {
        if &buf[end..end + 4] == LUA_STD_MAGIC || buf[end..].starts_with(LUAJIT_MAGIC) {
            break;
        }
        end += 1;
    }
    let raw_bytes = buf[start..end].to_vec();
    let strings = extract_printable_strings(&raw_bytes, 4, 300);
    let preview = format!(
        "-- {chunk_name} ({ver_str}, ~{} bytes window)\n-- Extracted strings:\n{}",
        raw_bytes.len(),
        strings.iter().take(80).cloned().collect::<Vec<_>>().join("\n")
    );
    Some((
        LuaChunkInfo {
            index: 0,
            name: chunk_name,
            format: ver_str.to_owned(),
            kind: LuaChunkKind::Lua5x,
            guest_va: base_va + start,
            region_pathname: region_path.to_owned(),
            byte_len: raw_bytes.len(),
            strings,
            preview,
            raw_bytes,
        },
        end,
    ))
}

fn scan_plaintext_lua_blocks(
    buf: &[u8],
    base_va: usize,
    region_path: &str,
    seen_hashes: &mut HashSet<u64>,
    out: &mut Vec<LuaChunkInfo>,
) {
    let mut i = 0usize;
    while i + 32 < buf.len() {
        let is_start = buf[i..].starts_with(b"local ")
            || buf[i..].starts_with(b"function ")
            || buf[i..].starts_with(b"--[[")
            || buf[i..].starts_with(b"-- @");
        if is_start && (i == 0 || buf[i - 1] == b'\n' || buf[i - 1] == 0) {
            let max_end = (i + 64 * 1024).min(buf.len());
            let mut end = i;
            while end < max_end {
                let b = buf[end];
                if b == 0 || (b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t') {
                    break;
                }
                end += 1;
            }
            if end - i >= 64
                && let Ok(txt) = std::str::from_utf8(&buf[i..end])
                && txt.contains('\n')
                && (txt.contains("end") || txt.contains("return ") || txt.contains("require("))
            {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                txt.hash(&mut h);
                let digest = h.finish();
                if seen_hashes.insert(digest) {
                    let first_line = txt.lines().next().unwrap_or("").trim();
                    let name = if first_line.starts_with("--") && first_line.len() > 3 {
                        first_line.trim_start_matches('-').trim().chars().take(64).collect()
                    } else {
                        format!("script_0x{:X}.lua", base_va + i)
                    };
                    let strings = extract_printable_strings(txt.as_bytes(), 4, 200);
                    let preview: String = txt.chars().take(4096).collect();
                    out.push(LuaChunkInfo {
                        index: 0,
                        name,
                        format: "Plaintext Lua".to_owned(),
                        kind: LuaChunkKind::Plaintext,
                        guest_va: base_va + i,
                        region_pathname: region_path.to_owned(),
                        byte_len: end - i,
                        strings,
                        preview,
                        raw_bytes: txt.as_bytes().to_vec(),
                    });
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

/// Scans an in-memory buffer for LuaJIT bytecode, standard Lua bytecode, and plaintext Lua blocks.
pub fn scan_lua_chunks_in_buffer(
    buf: &[u8],
    base_va: usize,
    region_path: &str,
    seen_hashes: &mut HashSet<u64>,
    out: &mut Vec<LuaChunkInfo>,
) {
    let mut pos = 0usize;
    while pos + 5 < buf.len() {
        if buf[pos] == 0x1B {
            if buf[pos..].starts_with(LUAJIT_MAGIC)
                && let Some((chunk, next_pos)) = try_parse_luajit_chunk(buf, pos, base_va, region_path)
            {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                chunk.raw_bytes.hash(&mut h);
                if seen_hashes.insert(h.finish()) {
                    out.push(chunk);
                }
                pos = next_pos;
                continue;
            }
            if buf[pos..].starts_with(LUA_STD_MAGIC)
                && let Some((chunk, next_pos)) = try_parse_std_lua_chunk(buf, pos, base_va, region_path)
            {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                chunk.raw_bytes.hash(&mut h);
                if seen_hashes.insert(h.finish()) {
                    out.push(chunk);
                }
                pos = next_pos;
                continue;
            }
        }
        pos += 1;
    }
    scan_plaintext_lua_blocks(buf, base_va, region_path, seen_hashes, out);
}

fn is_scannable_lua_region(r: &AndroidMemoryRegion) -> bool {
    let size = r.end.saturating_sub(r.start);
    if !r.readable || size < 4096 || size > 32 * 1024 * 1024 {
        return false;
    }
    let p = r.pathname.as_str();
    if p.starts_with("/dev/")
        || p.starts_with("/system/")
        || p.starts_with("/vendor/")
        || p.starts_with("/apex/")
        || p.ends_with(".odex")
        || p.ends_with(".oat")
        || p.ends_with(".vdex")
        || p.ends_with(".art")
        || p.ends_with(".ttf")
        || p.ends_with(".otf")
        || p.contains("gralloc")
        || p.contains("kgsl")
    {
        return false;
    }
    // Scan anonymous/heap/writable game buffers and asset/lua file mappings, skip executable .so code pages
    if r.executable && p.ends_with(".so") {
        return false;
    }
    p.is_empty()
        || p.starts_with("[anon:")
        || p.starts_with("[heap]")
        || p.contains("ashmem")
        || p.contains("lua")
        || p.contains("tolua")
        || p.contains("xlua")
        || r.writable
}

/// Dumps Lua native ELF64 symbols and live Lua bytecode/script chunks from a running Android process over ADB.
pub fn dump_live_android_lua<F>(
    adb: &Path,
    target: &str,
    pid: u32,
    mut on_progress: F,
) -> Result<AndroidLuaDumpResult, String>
where
    F: FnMut(&str),
{
    on_progress("Reading Android /proc/<pid>/maps...");
    let (modules, regions) = get_android_process_maps(adb, target, pid)?;

    let mut lua_modules = Vec::new();
    let mut candidate_modules = Vec::new();
    for m in &modules {
        let low = m.name.to_ascii_lowercase();
        let is_lua_lib = low.contains("tolua")
            || low.contains("xlua")
            || low.contains("slua")
            || low.contains("luajit")
            || (low.contains("lua") && low.ends_with(".so"));
        if is_lua_lib {
            lua_modules.push(LuaModuleEntry {
                name: m.name.clone(),
                base_va: m.base_address,
            });
            candidate_modules.push((m.name.clone(), m.base_address));
        }
    }
    // Also inspect libil2cpp.so / libunity.so if no dedicated Lua .so or for statically linked exports
    for m in &modules {
        let low = m.name.to_ascii_lowercase();
        if (low.contains("libil2cpp.so") || low.contains("libunity.so"))
            && !candidate_modules.iter().any(|(_, b)| *b == m.base_address)
        {
            candidate_modules.push((m.name.clone(), m.base_address));
        }
    }

    let mut symbols = Vec::new();
    for (mod_name, mod_base) in &candidate_modules {
        on_progress(&format!("Resolving ELF64 Lua exports in {mod_name}..."));
        if let Ok(hdr) = read_guest_mem_exact(adb, target, pid, *mod_base, 256 * 1024) {
            let mut syms = parse_elf64_lua_exports_with_reader(mod_name, *mod_base, &hdr, |va, len| {
                if len > 8 * 1024 * 1024 {
                    return None;
                }
                read_guest_mem_exact(adb, target, pid, va, len).ok()
            });
            symbols.append(&mut syms);
        }
    }

    let scan_list: Vec<&AndroidMemoryRegion> = regions.iter().filter(|r| is_scannable_lua_region(r)).collect();
    let total_regions = scan_list.len();
    let mut chunks = Vec::new();
    let mut seen_hashes = HashSet::new();
    let mut scanned_bytes = 0usize;
    let mut scanned_regions = 0usize;
    const MAX_TOTAL_SCAN_BYTES: usize = 96 * 1024 * 1024;
    const CHUNK_READ_STEP: usize = 2 * 1024 * 1024;

    for (idx, reg) in scan_list.into_iter().enumerate() {
        if scanned_bytes >= MAX_TOTAL_SCAN_BYTES {
            break;
        }
        if idx % 16 == 0 {
            on_progress(&format!(
                "Scanning RAM regions for Lua bytecode/scripts ({idx}/{total_regions}, {} chunks found)...",
                chunks.len()
            ));
        }
        let reg_size = reg.end.saturating_sub(reg.start);
        let region_budget = reg_size.min(MAX_TOTAL_SCAN_BYTES - scanned_bytes);
        let mut offset = 0usize;
        let mut region_had_read = false;
        while offset < region_budget {
            let step = (region_budget - offset).min(CHUNK_READ_STEP);
            if let Ok(bytes) = read_guest_mem_exact(adb, target, pid, reg.start + offset, step) {
                region_had_read = true;
                scanned_bytes += bytes.len();
                scan_lua_chunks_in_buffer(
                    &bytes,
                    reg.start + offset,
                    &reg.pathname,
                    &mut seen_hashes,
                    &mut chunks,
                );
            }
            offset += step;
        }
        if region_had_read {
            scanned_regions += 1;
        }
    }

    for (i, c) in chunks.iter_mut().enumerate() {
        c.index = i;
    }

    Ok(AndroidLuaDumpResult {
        guest_pid: pid,
        lua_modules,
        symbols,
        chunks,
        scanned_regions,
        scanned_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_elf64_lua_exports() {
        // Minimal ELF64 header + PT_DYNAMIC + DT_HASH + DT_STRTAB + DT_SYMTAB
        let mut buf = vec![0u8; 0x1000];
        buf[0..4].copy_from_slice(b"\x7FELF");
        buf[4] = 2; // ELF64
        buf[5] = 1; // LE
        buf[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff = 64
        buf[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        buf[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum

        // Program header at 64: PT_DYNAMIC = 2, offset/vaddr = 0x200, size = 0x100
        buf[64..68].copy_from_slice(&2u32.to_le_bytes());
        buf[80..88].copy_from_slice(&0x200u64.to_le_bytes()); // p_vaddr
        buf[104..112].copy_from_slice(&0x100u64.to_le_bytes()); // p_memsz

        // DT_HASH at 0x300 (nchain = 3)
        buf[0x300..0x304].copy_from_slice(&1u32.to_le_bytes());
        buf[0x304..0x308].copy_from_slice(&3u32.to_le_bytes());

        // DT_STRTAB at 0x400
        let strs = b"\0luaL_loadbuffer\0unrelated_sym\0tolua_open\0";
        buf[0x400..0x400 + strs.len()].copy_from_slice(strs);

        // DT_SYMTAB at 0x500 (3 entries x 24 bytes)
        // Entry 1: luaL_loadbuffer (name=1, shndx=1, value=0x12340)
        buf[0x500 + 24..0x500 + 28].copy_from_slice(&1u32.to_le_bytes());
        buf[0x500 + 30..0x500 + 32].copy_from_slice(&1u16.to_le_bytes());
        buf[0x500 + 32..0x500 + 40].copy_from_slice(&0x12340u64.to_le_bytes());
        // Entry 2: unrelated_sym (name=17, shndx=1, value=0x22000)
        buf[0x500 + 48..0x500 + 52].copy_from_slice(&17u32.to_le_bytes());
        buf[0x500 + 54..0x500 + 56].copy_from_slice(&1u16.to_le_bytes());
        buf[0x500 + 56..0x500 + 64].copy_from_slice(&0x22000u64.to_le_bytes());

        // Dynamic table at 0x200 (also tests relocated guest VA >= base)
        let base = 0x7000_0000usize;
        let entries: [(u64, u64); 5] = [
            (4, (base + 0x300) as u64),  // DT_HASH
            (5, (base + 0x400) as u64),  // DT_STRTAB
            (6, (base + 0x500) as u64),  // DT_SYMTAB
            (10, strs.len() as u64),     // DT_STRSZ
            (0, 0),
        ];
        for (idx, (tag, val)) in entries.into_iter().enumerate() {
            let off = 0x200 + idx * 16;
            buf[off..off + 8].copy_from_slice(&tag.to_le_bytes());
            buf[off + 8..off + 16].copy_from_slice(&val.to_le_bytes());
        }

        let syms = parse_elf64_lua_exports_with_reader("libtolua.so", base, &buf, |_, _| None);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "luaL_loadbuffer");
        assert_eq!(syms[0].rva, 0x12340);
        assert_eq!(syms[0].va, base + 0x12340);
    }

    #[test]
    fn scans_luajit_std_lua_and_plaintext_chunks() {
        let mut ram = vec![0u8; 32];
        // Synthetic LuaJIT v2 named chunk: \x1bLJ \x02 flags=0x08 namelen=14 "@Battle/Board.lua" ...
        let name = b"@Board/Unit.lua";
        ram.extend_from_slice(b"\x1bLJ\x02\x08");
        ram.push(name.len() as u8);
        ram.extend_from_slice(name);
        let proto = b"\x00\x00\x01\x01DeployUnit\0GetBoardGrid\0";
        ram.push(proto.len() as u8);
        ram.extend_from_slice(proto);
        ram.push(0x00); // EOF prototype terminator
        ram.extend_from_slice(&[0u8; 16]);

        // Plaintext Lua block
        ram.extend_from_slice(b"\nlocal BoardMgr = {}\nfunction BoardMgr:GetCells()\n    return self.cells\nend\n\0");

        let mut seen = HashSet::new();
        let mut chunks = Vec::new();
        scan_lua_chunks_in_buffer(&ram, 0x5000_0000, "[anon:libc_malloc]", &mut seen, &mut chunks);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].kind, LuaChunkKind::LuaJit);
        assert_eq!(chunks[0].name, "@Board/Unit.lua");
        assert!(chunks[0].strings.iter().any(|s| s == "DeployUnit"));
        assert_eq!(chunks[1].kind, LuaChunkKind::Plaintext);
        assert!(chunks[1].preview.contains("BoardMgr:GetCells"));
    }
}
