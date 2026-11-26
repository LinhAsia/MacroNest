//! Pure-Rust live Android IL2CPP RAM Dumper & Inspector.
//! Supports standard IL2CPP metadata (0xFAB11BAF) and unpacked NetEase HTProtect RAM (HTPX).

use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::android_bridge::AndroidMemoryRegion;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const IL2CPP_MAGIC: u32 = 0xFAB1_1BAF;
const HTPX_MAGIC: u32 = 0x5850_5448; // "HTPX"

#[derive(Clone, Debug, Serialize)]
pub struct Il2CppMethodInfo {
    pub name: String,
    pub token: u32,
    pub rva: usize,
    pub va: usize,
    pub param_count: u16,
    pub flags: u16,
}

#[derive(Clone, Debug, Serialize)]
pub struct Il2CppFieldInfo {
    pub name: String,
    pub token: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Il2CppClassInfo {
    pub type_index: usize,
    pub assembly_name: String,
    pub namespace: String,
    pub name: String,
    pub methods: Vec<Il2CppMethodInfo>,
    pub fields: Vec<Il2CppFieldInfo>,
}

impl Il2CppClassInfo {
    pub fn full_name(&self) -> String {
        if self.namespace.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.namespace, self.name)
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Il2CppAssemblyInfo {
    pub name: String,
    pub type_start: usize,
    pub type_count: usize,
    pub method_pointer_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Il2CppDumpResult {
    pub guest_pid: u32,
    pub libil2cpp_base: usize,
    pub metadata_base: usize,
    pub metadata_version: u32,
    pub assemblies: Vec<Il2CppAssemblyInfo>,
    pub classes: Vec<Il2CppClassInfo>,
    #[serde(skip)]
    pub raw_metadata: Vec<u8>,
}

#[derive(Clone, Copy)]
struct MetadataHeaderOffsets {
    string_offset: usize,
    string_size: usize,
    methods_offset: usize,
    methods_size: usize,
    fields_offset: usize,
    fields_size: usize,
    types_offset: usize,
    types_size: usize,
    images_offset: usize,
    images_size: usize,
}

fn read_u16(buf: &[u8], off: usize) -> Option<u16> {
    buf.get(off..off + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    buf.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_i32(buf: &[u8], off: usize) -> Option<i32> {
    buf.get(off..off + 4)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_u64(buf: &[u8], off: usize) -> Option<u64> {
    buf.get(off..off + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

fn read_cstr(table: &[u8], idx: usize) -> String {
    if idx >= table.len() {
        return String::new();
    }
    let slice = &table[idx..];
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..end]).into_owned()
}

pub fn run_adb_bytes(adb: &Path, target: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(adb);
    if !target.trim().is_empty() {
        cmd.arg("-s").arg(target.trim());
    }
    let out = cmd
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to run adb: {e}"))?;
    if !out.status.success() && out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        return Err(if err.is_empty() {
            format!("adb failed with status {}", out.status)
        } else {
            err
        });
    }
    Ok(out.stdout)
}

pub fn read_guest_mem_exact(
    adb: &Path,
    target: &str,
    pid: u32,
    start_addr: usize,
    byte_len: usize,
) -> Result<Vec<u8>, String> {
    if byte_len == 0 {
        return Ok(Vec::new());
    }
    let page_start = start_addr / 4096;
    let lead = start_addr % 4096;
    let total_needed = lead + byte_len;
    let page_count = total_needed.div_ceil(4096);
    let shell_cmd = format!(
        "dd if=/proc/{pid}/mem bs=4096 skip={page_start} count={page_count} 2>/dev/null"
    );
    let raw = run_adb_bytes(adb, target, &["exec-out", &shell_cmd])?;
    if raw.len() < total_needed {
        return Err(format!(
            "Short read at 0x{start_addr:X}: expected {total_needed} bytes, got {}",
            raw.len()
        ));
    }
    Ok(raw[lead..lead + byte_len].to_vec())
}

fn fetch_guest_maps(adb: &Path, target: &str, pid: u32) -> Result<Vec<AndroidMemoryRegion>, String> {
    let _ = run_adb_bytes(adb, target, &["root"]);
    let raw = run_adb_bytes(adb, target, &["shell", &format!("cat /proc/{pid}/maps")])?;
    let text = String::from_utf8_lossy(&raw);
    let mut regions = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let Some((s_hex, e_hex)) = parts[0].split_once('-') else {
            continue;
        };
        let Ok(start) = usize::from_str_radix(s_hex, 16) else {
            continue;
        };
        let Ok(end) = usize::from_str_radix(e_hex, 16) else {
            continue;
        };
        if end <= start {
            continue;
        }
        let perms = parts[1];
        let pathname = if parts.len() >= 6 {
            parts[5..].join(" ")
        } else {
            String::new()
        };
        regions.push(AndroidMemoryRegion {
            start,
            end,
            readable: perms.starts_with('r'),
            writable: perms.as_bytes().get(1) == Some(&b'w'),
            executable: perms.as_bytes().get(2) == Some(&b'x'),
            pathname,
        });
    }
    if regions.is_empty() {
        return Err(format!("Could not read /proc/{pid}/maps (make sure ADB root is active)"));
    }
    Ok(regions)
}

fn parse_metadata_header(meta: &[u8]) -> Result<(u32, MetadataHeaderOffsets), String> {
    if meta.len() < 264 {
        return Err("Metadata buffer too small".to_owned());
    }
    let magic = read_u32(meta, 0).ok_or("Missing magic")?;
    let raw_ver = read_u32(meta, 4).ok_or("Missing version")?;
    let first_off = read_u32(meta, 8).ok_or("Missing stringLiteralOffset")? as usize;
    let version = if magic == IL2CPP_MAGIC {
        raw_ver
    } else if magic == HTPX_MAGIC {
        if first_off >= 0x100 { 29 } else { 24 }
    } else {
        return Err(format!("Unsupported metadata magic 0x{magic:08X}"));
    };

    let pair = |off: usize| -> Result<(usize, usize), String> {
        let o = read_u32(meta, off).ok_or("Header OOB")? as usize;
        let s = read_u32(meta, off + 4).ok_or("Header OOB")? as usize;
        if o.saturating_add(s) > meta.len() {
            return Err(format!(
                "Metadata section out of bounds at 0x{off:X}: 0x{o:X}+0x{s:X} > 0x{:X}",
                meta.len()
            ));
        }
        Ok((o, s))
    };

    let (string_offset, string_size) = pair(0x18)?;
    let (methods_offset, methods_size) = pair(0x30)?;
    let (fields_offset, fields_size) = pair(0x60)?;
    let (types_offset, types_size) = pair(0xA0)?;
    let (images_offset, images_size) = pair(0xA8)?;

    Ok((
        version,
        MetadataHeaderOffsets {
            string_offset,
            string_size,
            methods_offset,
            methods_size,
            fields_offset,
            fields_size,
            types_offset,
            types_size,
            images_offset,
            images_size,
        },
    ))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

pub fn parse_il2cpp_metadata_and_rvas(
    meta: &[u8],
    guest_pid: u32,
    libil2cpp_base: usize,
    memory_slices: &[(usize, Vec<u8>)],
) -> Result<Il2CppDumpResult, String> {
    let (mut version, hdr) = parse_metadata_header(meta)?;
    let str_table = &meta[hdr.string_offset..hdr.string_offset + hdr.string_size];

    let image_stride = 40usize;
    if hdr.images_size == 0 || !hdr.images_size.is_multiple_of(image_stride) {
        return Err(format!("Unexpected imagesSize {}", hdr.images_size));
    }
    let image_count = hdr.images_size / image_stride;

    let mut raw_images = Vec::with_capacity(image_count);
    for i in 0..image_count {
        let base = hdr.images_offset + i * image_stride;
        let name_idx = read_u32(meta, base).unwrap_or(0) as usize;
        let type_start = read_i32(meta, base + 8).unwrap_or(0).max(0) as usize;
        let type_count = read_u32(meta, base + 12).unwrap_or(0) as usize;
        let name = read_cstr(str_table, name_idx);
        raw_images.push((name, type_start, type_count));
    }

    // Detect method definition stride (36 in v31 with returnParameterToken at +0x0C, 32 in v24..v29).
    let method_stride = if hdr.methods_size.is_multiple_of(36)
        && read_u32(meta, hdr.methods_offset + 24)
            .is_some_and(|tok| (tok >> 24) == 0x06)
    {
        version = version.max(31);
        36usize
    } else if hdr.methods_size.is_multiple_of(32) {
        32usize
    } else if hdr.methods_size.is_multiple_of(36) {
        36usize
    } else {
        return Err(format!("Unsupported methodsSize {}", hdr.methods_size));
    };
    let method_count_total = hdr.methods_size / method_stride;

    let type_stride = if hdr.types_size.is_multiple_of(88) {
        88usize
    } else if hdr.types_size.is_multiple_of(92) {
        92usize
    } else if hdr.types_size.is_multiple_of(96) {
        96usize
    } else {
        return Err(format!("Unsupported typeDefinitionsSize {}", hdr.types_size));
    };
    let type_count_total = hdr.types_size / type_stride;

    let field_stride = 12usize;
    let field_count_total = hdr.fields_size / field_stride;

    let read_q = |va: usize| -> Option<u64> {
        for (slice_base, buf) in memory_slices {
            if va >= *slice_base && va + 8 <= slice_base + buf.len() {
                return read_u64(buf, va - *slice_base);
            }
        }
        None
    };

    // Locate CodeGenModule pointers for each image via image_name string VA inside memory slices.
    let mut assembly_method_tables: HashMap<String, (usize, usize)> = HashMap::new();
    if let Some((first_img_name, _, _)) = raw_images.first() {
        let mut needle = first_img_name.as_bytes().to_vec();
        needle.push(0);

        let mut str_va_candidates = Vec::new();
        for (slice_base, buf) in memory_slices {
            if let Some(rel) = find_bytes(buf, &needle) {
                str_va_candidates.push(slice_base + rel);
            }
        }

        'cgm_search: for str_va in str_va_candidates {
            for (slice_base, buf) in memory_slices {
                for off in (0..buf.len().saturating_sub(24)).step_by(8) {
                    if read_u64(buf, off) == Some(str_va as u64) {
                        let m_cnt = read_u64(buf, off + 8).unwrap_or(0) as usize;
                        let m_ptrs = read_u64(buf, off + 16).unwrap_or(0) as usize;
                        if m_cnt > 0 && m_cnt < 250_000 && read_q(m_ptrs).is_some() {
                            let cgm0_va = slice_base + off;
                            // Locate pointer array of CodeGenModule* where entry[0] == cgm0_va.
                            for (arr_base, arr_buf) in memory_slices {
                                for aoff in (0..arr_buf.len().saturating_sub(image_count * 8)).step_by(8) {
                                    if read_u64(arr_buf, aoff) == Some(cgm0_va as u64) {
                                        let mut valid_mods = 0usize;
                                        for idx in 0..image_count {
                                            let mod_va = read_q(arr_base + aoff + idx * 8).unwrap_or(0) as usize;
                                            if let Some(name_ptr) = read_q(mod_va)
                                                && let Some(cnt) = read_q(mod_va + 8)
                                                && let Some(ptrs) = read_q(mod_va + 16)
                                                && name_ptr > 0
                                            {
                                                valid_mods += 1;
                                                let mod_name = read_guest_cstr(memory_slices, name_ptr as usize)
                                                    .unwrap_or_else(|| raw_images[idx].0.clone());
                                                assembly_method_tables.insert(
                                                    mod_name,
                                                    (cnt as usize, ptrs as usize),
                                                );
                                            }
                                        }
                                        if valid_mods > 0 {
                                            break 'cgm_search;
                                        }
                                    }
                                }
                            }
                            assembly_method_tables.insert(first_img_name.clone(), (m_cnt, m_ptrs));
                            break 'cgm_search;
                        }
                    }
                }
            }
        }
    }

    let mut type_to_assembly = vec![String::new(); type_count_total];
    let mut assemblies = Vec::with_capacity(raw_images.len());
    for (name, t_start, t_cnt) in raw_images {
        let end = (t_start + t_cnt).min(type_count_total);
        for slot in &mut type_to_assembly[t_start..end] {
            *slot = name.clone();
        }
        let mp_cnt = assembly_method_tables.get(&name).map_or(0, |x| x.0);
        assemblies.push(Il2CppAssemblyInfo {
            name,
            type_start: t_start,
            type_count: t_cnt,
            method_pointer_count: mp_cnt,
        });
    }

    let mut classes = Vec::with_capacity(type_count_total);
    for t_idx in 0..type_count_total {
        let t_off = hdr.types_offset + t_idx * type_stride;
        let name_idx = read_u32(meta, t_off).unwrap_or(0) as usize;
        let ns_idx = read_u32(meta, t_off + 4).unwrap_or(0) as usize;
        let field_start = read_i32(meta, t_off + 32).unwrap_or(-1);
        let method_start = read_i32(meta, t_off + 36).unwrap_or(-1);
        let method_count = read_u16(meta, t_off + 64).unwrap_or(0) as usize;
        let field_count = read_u16(meta, t_off + 68).unwrap_or(0) as usize;

        let assembly_name = type_to_assembly.get(t_idx).cloned().unwrap_or_default();
        let (mp_cnt, mp_base) = assembly_method_tables
            .get(&assembly_name)
            .copied()
            .unwrap_or((0, 0));

        let mut methods = Vec::with_capacity(method_count);
        if method_start >= 0 && method_count > 0 {
            let m_start = method_start as usize;
            let m_end = (m_start + method_count).min(method_count_total);
            for m_idx in m_start..m_end {
                let m_off = hdr.methods_offset + m_idx * method_stride;
                let m_name_idx = read_u32(meta, m_off).unwrap_or(0) as usize;
                let (token, flags, param_count) = if method_stride == 36 {
                    (
                        read_u32(meta, m_off + 24).unwrap_or(0),
                        read_u16(meta, m_off + 28).unwrap_or(0),
                        read_u16(meta, m_off + 34).unwrap_or(0),
                    )
                } else {
                    (
                        read_u32(meta, m_off + 20).unwrap_or(0),
                        read_u16(meta, m_off + 24).unwrap_or(0),
                        read_u16(meta, m_off + 30).unwrap_or(0),
                    )
                };
                let rid = (token & 0x00FF_FFFF) as usize;
                let va = if rid > 0 && rid <= mp_cnt && mp_base != 0 {
                    read_q(mp_base + (rid - 1) * 8).unwrap_or(0) as usize
                } else {
                    0
                };
                let rva = if va > libil2cpp_base {
                    va - libil2cpp_base
                } else {
                    0
                };
                methods.push(Il2CppMethodInfo {
                    name: read_cstr(str_table, m_name_idx),
                    token,
                    rva,
                    va,
                    param_count,
                    flags,
                });
            }
        }

        let mut fields = Vec::with_capacity(field_count);
        if field_start >= 0 && field_count > 0 {
            let f_start = field_start as usize;
            let f_end = (f_start + field_count).min(field_count_total);
            for f_idx in f_start..f_end {
                let f_off = hdr.fields_offset + f_idx * field_stride;
                let f_name_idx = read_u32(meta, f_off).unwrap_or(0) as usize;
                let f_token = read_u32(meta, f_off + 8).unwrap_or(0);
                fields.push(Il2CppFieldInfo {
                    name: read_cstr(str_table, f_name_idx),
                    token: f_token,
                });
            }
        }

        classes.push(Il2CppClassInfo {
            type_index: t_idx,
            assembly_name,
            namespace: read_cstr(str_table, ns_idx),
            name: read_cstr(str_table, name_idx),
            methods,
            fields,
        });
    }

    let mut clean_meta = meta.to_vec();
    if read_u32(&clean_meta, 0) == Some(HTPX_MAGIC) {
        clean_meta[0..4].copy_from_slice(&IL2CPP_MAGIC.to_le_bytes());
        clean_meta[4..8].copy_from_slice(&version.to_le_bytes());
    }

    Ok(Il2CppDumpResult {
        guest_pid,
        libil2cpp_base,
        metadata_base: 0,
        metadata_version: version,
        assemblies,
        classes,
        raw_metadata: clean_meta,
    })
}

fn read_guest_cstr(slices: &[(usize, Vec<u8>)], va: usize) -> Option<String> {
    for (base, buf) in slices {
        if va >= *base && va < base + buf.len() {
            let start = va - *base;
            let rest = &buf[start..];
            let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len().min(128));
            return Some(String::from_utf8_lossy(&rest[..end]).into_owned());
        }
    }
    None
}

pub fn dump_live_android_il2cpp(
    adb: &Path,
    target: &str,
    pid: u32,
    on_progress: impl Fn(&str),
) -> Result<Il2CppDumpResult, String> {
    on_progress("Reading /proc/<pid>/maps...");
    let regions = fetch_guest_maps(adb, target, pid)?;

    let meta_region = regions
        .iter()
        .find(|r| r.pathname.ends_with("global-metadata.dat"))
        .ok_or("global-metadata.dat is not mapped in guest process yet")?;

    let il2cpp_regions: Vec<&AndroidMemoryRegion> = regions
        .iter()
        .filter(|r| r.pathname.ends_with("libil2cpp.so"))
        .collect();
    let libil2cpp_base = il2cpp_regions
        .iter()
        .map(|r| r.start)
        .min()
        .ok_or("libil2cpp.so is not mapped in guest process")?;
    let libil2cpp_end = il2cpp_regions
        .iter()
        .filter(|r| r.start >= libil2cpp_base && r.start - libil2cpp_base < 0x1000_0000)
        .map(|r| r.end)
        .max()
        .unwrap_or(libil2cpp_base + 0x0500_0000);

    let meta_len = meta_region.end - meta_region.start;
    on_progress(&format!(
        "Reading global-metadata.dat ({:.1} MB) from 0x{:X}...",
        meta_len as f64 / (1024.0 * 1024.0),
        meta_region.start
    ));
    let meta = read_guest_mem_exact(adb, target, pid, meta_region.start, meta_len)?;
    let (_, hdr) = parse_metadata_header(&meta)?;

    // Read first readable slice of libil2cpp.so (up to 16 MB) for .rodata image names
    let rodata_len = (libil2cpp_end - libil2cpp_base).min(16 * 1024 * 1024);
    on_progress("Reading libil2cpp.so .rodata strings...");
    let rodata_bytes = read_guest_mem_exact(adb, target, pid, libil2cpp_base, rodata_len)?;
    let mut slices = vec![(libil2cpp_base, rodata_bytes)];

    // Read rw-p regions inside/adjacent to libil2cpp.so for CodeGenModule & methodPointers
    let mut rw_ranges: Vec<(usize, usize)> = regions
        .iter()
        .filter(|r| {
            r.readable
                && r.writable
                && r.start >= libil2cpp_base
                && r.end <= libil2cpp_end + 4 * 1024 * 1024
                && (r.end - r.start) <= 16 * 1024 * 1024
        })
        .map(|r| (r.start, r.end))
        .collect();
    rw_ranges.sort_unstable();
    let mut merged_rw: Vec<(usize, usize)> = Vec::with_capacity(rw_ranges.len());
    for (s, e) in rw_ranges {
        if let Some(last) = merged_rw.last_mut()
            && s <= last.1
            && (e - last.0) <= 16 * 1024 * 1024
        {
            last.1 = last.1.max(e);
        } else {
            merged_rw.push((s, e));
        }
    }
    for (s, e) in merged_rw {
        on_progress(&format!("Reading CodeGen/data slice 0x{s:X}-0x{e:X}..."));
        if let Ok(bytes) = read_guest_mem_exact(adb, target, pid, s, e - s) {
            slices.push((s, bytes));
        }
    }

    on_progress("Parsing IL2CPP classes, methods & RVAs...");
    let mut result = parse_il2cpp_metadata_and_rvas(&meta, pid, libil2cpp_base, &slices)?;
    result.metadata_base = meta_region.start;
    let _ = hdr;
    Ok(result)
}

pub fn format_dump_cs(dump: &Il2CppDumpResult) -> String {
    let mut out = String::with_capacity(dump.classes.len() * 256);
    out.push_str(&format!(
        "// MacroNest IL2CPP RAM Dump\n// PID: {} | libil2cpp.so: 0x{:X} | metadata: 0x{:X} (v{})\n\n",
        dump.guest_pid, dump.libil2cpp_base, dump.metadata_base, dump.metadata_version
    ));
    for (i, asm) in dump.assemblies.iter().enumerate() {
        out.push_str(&format!(
            "// Image {i}: {} (types: {}, methodPointers: {})\n",
            asm.name, asm.type_count, asm.method_pointer_count
        ));
    }
    out.push('\n');
    for cls in &dump.classes {
        out.push_str(&format!("// Assembly: {}\n", cls.assembly_name));
        if cls.namespace.is_empty() {
            out.push_str(&format!("class {}\n{{\n", cls.name));
        } else {
            out.push_str(&format!("namespace {}\nclass {}\n{{\n", cls.namespace, cls.name));
        }
        for f in &cls.fields {
            out.push_str(&format!("    field {}; // token 0x{:X}\n", f.name, f.token));
        }
        for m in &cls.methods {
            let params = (0..m.param_count)
                .map(|p| format!("arg{p}"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "    // RVA: 0x{:X} VA: 0x{:X} Token: 0x{:X}\n    {}({});\n",
                m.rva, m.va, m.token, m.name, params
            ));
        }
        out.push_str("}\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn push_i32(v: &mut Vec<u8>, x: i32) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn push_u16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    #[test]
    fn parses_htpx_v31_metadata_and_resolves_method_rva() {
        let str_table = b"Assembly-CSharp.dll\0Game\0BoardManager\0DeployTatari\0boardSlots\0";
        let off_asm = 0u32;
        let off_ns = 20u32;
        let off_cls = 25u32;
        let off_meth = 38u32;
        let off_fld = 51u32;

        let mut meta = vec![0u8; 0x100];
        meta[0..4].copy_from_slice(&HTPX_MAGIC.to_le_bytes());
        let str_off = 0x100u32;
        meta.extend_from_slice(str_table);

        // MethodDefinition (36 bytes): name=DeployTatari, token=0x06000001, param_count=2
        let meth_off = meta.len() as u32;
        push_u32(&mut meta, off_meth);
        push_i32(&mut meta, 0);
        push_u32(&mut meta, 0);
        push_u32(&mut meta, 0x0800_0000);
        push_i32(&mut meta, -1);
        push_i32(&mut meta, -1);
        push_u32(&mut meta, 0x0600_0001);
        push_u16(&mut meta, 0x0086);
        push_u16(&mut meta, 0);
        push_u16(&mut meta, 0xFFFF);
        push_u16(&mut meta, 2);

        // FieldDefinition (12 bytes): name=boardSlots, token=0x04000001
        let fld_off = meta.len() as u32;
        push_u32(&mut meta, off_fld);
        push_i32(&mut meta, 0);
        push_u32(&mut meta, 0x0400_0001);

        // TypeDefinition (88 bytes)
        let td_off = meta.len() as u32;
        let mut td = vec![0u8; 88];
        td[0..4].copy_from_slice(&off_cls.to_le_bytes());
        td[4..8].copy_from_slice(&off_ns.to_le_bytes());
        td[32..36].copy_from_slice(&0i32.to_le_bytes());
        td[36..40].copy_from_slice(&0i32.to_le_bytes());
        td[64..66].copy_from_slice(&1u16.to_le_bytes());
        td[68..70].copy_from_slice(&1u16.to_le_bytes());
        meta.extend_from_slice(&td);

        // ImageDefinition (40 bytes)
        let img_off = meta.len() as u32;
        let mut img = vec![0u8; 40];
        img[0..4].copy_from_slice(&off_asm.to_le_bytes());
        img[8..12].copy_from_slice(&0i32.to_le_bytes());
        img[12..16].copy_from_slice(&1u32.to_le_bytes());
        meta.extend_from_slice(&img);

        let total_len = meta.len() as u32;
        meta[4..8].copy_from_slice(&total_len.to_le_bytes());
        meta[0x08..0x0C].copy_from_slice(&0x100u32.to_le_bytes());
        meta[0x18..0x1C].copy_from_slice(&str_off.to_le_bytes());
        meta[0x1C..0x20].copy_from_slice(&(str_table.len() as u32).to_le_bytes());
        meta[0x30..0x34].copy_from_slice(&meth_off.to_le_bytes());
        meta[0x34..0x38].copy_from_slice(&36u32.to_le_bytes());
        meta[0x60..0x64].copy_from_slice(&fld_off.to_le_bytes());
        meta[0x64..0x68].copy_from_slice(&12u32.to_le_bytes());
        meta[0xA0..0xA4].copy_from_slice(&td_off.to_le_bytes());
        meta[0xA4..0xA8].copy_from_slice(&88u32.to_le_bytes());
        meta[0xA8..0xAC].copy_from_slice(&img_off.to_le_bytes());
        meta[0xAC..0xB0].copy_from_slice(&40u32.to_le_bytes());

        // Synthetic memory slices: .rodata with "Assembly-CSharp.dll\0" and .data with CodeGenModule
        let lib_base = 0x4000_0000usize;
        let rodata = b"padding\0Assembly-CSharp.dll\0".to_vec();
        let str_va = lib_base + 8;

        let data_base = 0x4010_0000usize;
        let mp_va = data_base;
        let cgm_va = data_base + 16;
        let _arr_va = data_base + 48;
        let mut data = Vec::new();
        data.extend_from_slice(&((lib_base + 0x12340) as u64).to_le_bytes()); // methodPointers[0]
        data.extend_from_slice(&0u64.to_le_bytes());
        // CodeGenModule[0]
        data.extend_from_slice(&(str_va as u64).to_le_bytes());
        data.extend_from_slice(&1u64.to_le_bytes());
        data.extend_from_slice(&(mp_va as u64).to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        // CodeGenModule* array[1]
        data.extend_from_slice(&(cgm_va as u64).to_le_bytes());

        let dump = parse_il2cpp_metadata_and_rvas(
            &meta,
            1829,
            lib_base,
            &[(lib_base, rodata), (data_base, data)],
        )
        .expect("parse synthetic v31 metadata");

        assert_eq!(dump.classes.len(), 1);
        assert_eq!(dump.classes[0].full_name(), "Game.BoardManager");
        assert_eq!(dump.classes[0].fields[0].name, "boardSlots");
        assert_eq!(dump.classes[0].methods[0].name, "DeployTatari");
        assert_eq!(dump.classes[0].methods[0].rva, 0x12340);
        assert_eq!(dump.classes[0].methods[0].param_count, 2);
        assert_eq!(&dump.raw_metadata[0..4], &IL2CPP_MAGIC.to_le_bytes());
    }

    #[test]
    #[ignore]
    fn live_dump_clash_of_critters_on_mumu() {
        let adb = crate::android_bridge::find_adb_executable().expect("adb");
        let target = "127.0.0.1:16416";
        let _ = run_adb_bytes(&adb, "", &["connect", target]);
        let _ = run_adb_bytes(&adb, target, &["root"]);
        let pid_raw = run_adb_bytes(&adb, target, &["shell", "pidof com.farlightgames.pgame.gp"]).expect("pidof");
        let pid: u32 = String::from_utf8_lossy(&pid_raw).trim().parse().expect("parse pid");

        let dump = dump_live_android_il2cpp(&adb, target, pid, |msg| println!("[progress] {msg}"))
            .expect("live dump failed");
        println!(
            "Live dump OK: {} assemblies, {} classes, version={}, libil2cpp=0x{:X}",
            dump.assemblies.len(),
            dump.classes.len(),
            dump.metadata_version,
            dump.libil2cpp_base
        );
        let acs = dump
            .assemblies
            .iter()
            .find(|a| a.name == "Assembly-CSharp.dll")
            .expect("Assembly-CSharp.dll missing");
        println!("Assembly-CSharp.dll: {} types, {} methodPointers", acs.type_count, acs.method_pointer_count);
        assert!(acs.method_pointer_count > 10_000);

        let sample_cls = dump
            .classes
            .iter()
            .find(|c| c.assembly_name == "Assembly-CSharp.dll" && c.methods.iter().any(|m| m.rva > 0))
            .expect("sample class");
        let sample_m = sample_cls.methods.iter().find(|m| m.rva > 0).unwrap();
        println!(
            "Sample: {}::{} RVA=0x{:X} VA=0x{:X}",
            sample_cls.full_name(),
            sample_m.name,
            sample_m.rva,
            sample_m.va
        );
        let bytes = read_guest_mem_exact(&adb, target, pid, sample_m.va, 32).expect("read arm64 bytes");
        let insns = crate::arm64_disasm::decode_arm64_block(&bytes, sample_m.va);
        for (va, hex, asm) in insns {
            println!("  0x{va:X}: {hex}  {asm}");
        }
    }
}
