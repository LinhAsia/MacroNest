#![cfg(windows)]
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use iced_x86::{
    BlockEncoder, BlockEncoderOptions, Code, Decoder, DecoderOptions, FlowControl, Formatter,
    Instruction, InstructionBlock, InstructionInfoFactory, IntelFormatter, MemoryOperand, OpAccess,
    OpKind, Register,
};

use std::sync::atomic::AtomicUsize;

use crate::{
    android_bridge::{AndroidMemoryModule, AndroidMemoryRegion},
    memory_debugger::debugger::WatchEvent,
    process_memory::{MemoryScanOptions, read_memory_bytes, scan_aob_memory_addresses, write_code_bytes},
};

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 0x3E;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;

#[derive(Clone, Debug)]
pub struct DiscoveredElfModule {
    pub name: String,
    pub base_gva: usize,
    pub base_hva: usize,
    pub size: usize,
}

/// Scans `pid` memory at 4KB page boundaries for x86_64 ELF64 shared libraries (`\x7fELF\x02\x01\x01`).
/// Used as a self-contained fallback when ADB / Android Bridge modules are unavailable.
pub fn discover_x64_elfs_in_process_memory(pid: u32) -> Vec<DiscoveredElfModule> {
    let options = MemoryScanOptions {
        writable: false,
        executable: true,
        copy_on_write: true,
        active_memory_only: false,
        mem_private: true,
        mem_image: false,
        mem_mapped: true,
        alignment: Some(4096),
        target_ranges: None,
        isolate_guest_ram: false,
        vm_allocation_base: None,
    };
    let progress = Arc::new(AtomicUsize::new(0));
    let addrs = scan_aob_memory_addresses(
        pid,
        "7F 45 4C 46 02 01 01",
        256,
        options,
        progress,
    )
    .unwrap_or_default();

    let mut out = Vec::new();
    for base_hva in addrs {
        let Ok(hdr) = read_memory_bytes(pid, base_hva, 4096) else {
            continue;
        };
        if hdr.len() < 64 || hdr[0..4] != ELF_MAGIC || hdr[4] != ELFCLASS64 {
            continue;
        }
        let e_type = u16::from_le_bytes([hdr[16], hdr[17]]);
        let e_machine = u16::from_le_bytes([hdr[18], hdr[19]]);
        if e_machine != EM_X86_64 || (e_type != ET_DYN && e_type != 2) {
            continue;
        }
        let e_phoff = u64::from_le_bytes(hdr[32..40].try_into().unwrap_or_default()) as usize;
        let e_phentsize = u16::from_le_bytes([hdr[54], hdr[55]]) as usize;
        let e_phnum = u16::from_le_bytes([hdr[56], hdr[57]]) as usize;
        if e_phentsize < 56 || e_phoff + e_phentsize.saturating_mul(e_phnum) > hdr.len() {
            continue;
        }
        let mut min_vaddr = u64::MAX;
        let mut max_end = 0u64;
        let mut has_exec = false;
        for i in 0..e_phnum {
            let off = e_phoff + i * e_phentsize;
            let ph = &hdr[off..off + e_phentsize];
            if u32::from_le_bytes(ph[0..4].try_into().unwrap_or_default()) != PT_LOAD {
                continue;
            }
            let p_flags = u32::from_le_bytes(ph[4..8].try_into().unwrap_or_default());
            let p_vaddr = u64::from_le_bytes(ph[16..24].try_into().unwrap_or_default());
            let p_memsz = u64::from_le_bytes(ph[40..48].try_into().unwrap_or_default());
            if (p_flags & PF_X) != 0 {
                has_exec = true;
            }
            min_vaddr = min_vaddr.min(p_vaddr);
            max_end = max_end.max(p_vaddr.saturating_add(p_memsz));
        }
        if !has_exec || min_vaddr == u64::MAX {
            continue;
        }
        let span = max_end.saturating_sub(min_vaddr) as usize;
        if span >= 256 * 1024 {
            out.push(DiscoveredElfModule {
                name: format!("elf_0x{:X}.so", base_hva),
                base_gva: base_hva,
                base_hva,
                size: span,
            });
        }
    }
    // Largest .so first (libil2cpp.so is always largest game binary)
    out.sort_by(|a, b| b.size.cmp(&a.size));
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElfCaveRange {
    pub module_name: String,
    pub start_hva: usize,
    pub len: usize,
    pub is_exec: bool,
    pub is_write: bool,
}

#[derive(Clone, Debug)]
struct CaveAllocator {
    exec_caves: Vec<(usize, usize)>, // (current_hva, remaining_bytes)
    data_caves: Vec<(usize, usize)>, // (current_hva, remaining_bytes)
}

impl CaveAllocator {
    fn discover(
        pid: u32,
        anchor_hva: usize,
        modules: &[AndroidMemoryModule],
        all_regions: &[AndroidMemoryRegion],
        delta: isize,
        extra_elfs: &[DiscoveredElfModule],
    ) -> Self {
        let mut module_bases: Vec<(String, usize, usize)> = modules
            .iter()
            .filter_map(|m| {
                let base_hva = (m.base_address as isize).checked_add(delta)?;
                if base_hva > 0 {
                    Some((m.name.clone(), base_hva as usize, m.size))
                } else {
                    None
                }
            })
            .collect();

        for elf in extra_elfs {
            if !module_bases.iter().any(|(_, b, _)| *b == elf.base_hva) {
                module_bases.push((elf.name.clone(), elf.base_hva, elf.size));
            }
        }

        // Fallback: probe backwards page-by-page from anchor_hva to locate the enclosing ELF64 header.
        if module_bases.is_empty() {
            let aligned = anchor_hva & !0xFFF;
            for page_idx in 0..16384usize {
                let probe = match aligned.checked_sub(page_idx * 4096) {
                    Some(addr) if addr > 0x10000 => addr,
                    _ => break,
                };
                if let Ok(hdr) = read_memory_bytes(pid, probe, 64) {
                    if hdr.len() >= 20 && hdr[0..4] == ELF_MAGIC {
                        module_bases.push(("module.so".to_string(), probe, 64 * 1024 * 1024));
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        // Prioritize modules closest to anchor_hva (within +/- 1.8 GB so rel32 always reaches)
        module_bases.sort_by_key(|(_, base_hva, _)| anchor_hva.abs_diff(*base_hva));

        let region_allows = |hva: usize, len: usize, want_exec: bool, want_write: bool| -> bool {
            if all_regions.is_empty() {
                return true;
            }
            let gva = if delta != 0 {
                (hva as isize - delta) as usize
            } else {
                hva
            };
            let gva_end = gva.saturating_add(len);
            all_regions.iter().any(|r| {
                gva >= r.start
                    && gva_end <= r.end
                    && (!want_exec || r.executable)
                    && (!want_write || (r.writable && !r.executable))
            })
        };

        let mut exec_caves = Vec::new();
        let mut data_caves = Vec::new();

        for (mod_name, base_hva, _) in &module_bases {
            if anchor_hva.abs_diff(*base_hva) > 0x7000_0000 {
                continue;
            }
            if let Ok(header_bytes) = read_memory_bytes(pid, *base_hva, 4096) {
                for cave in parse_elf64_x64_caves(mod_name, *base_hva, &header_bytes) {
                    if anchor_hva.abs_diff(cave.start_hva) > 0x7000_0000 {
                        continue;
                    }
                    if !region_allows(cave.start_hva, cave.len, cave.is_exec, cave.is_write) {
                        continue;
                    }
                    if let Ok(existing) = read_memory_bytes(pid, cave.start_hva, cave.len) {
                        let is_clean = if cave.is_exec {
                            existing.iter().all(|&b| b == 0x00 || b == 0xCC || b == 0x90)
                        } else {
                            existing.iter().all(|&b| b == 0x00)
                        };
                        if is_clean {
                            if cave.is_exec {
                                exec_caves.push((cave.start_hva, cave.len));
                            } else if cave.is_write {
                                data_caves.push((cave.start_hva, cave.len));
                            }
                        }
                    }
                }
            }
        }

        // Additional fallback for executable caves: scan ONLY explicit 0xCC (INT3) padding >= 96 bytes
        // at the end of r-xp regions within rel32 reach. Never touch [anon:.bss] for data caves!
        if exec_caves.iter().map(|(_, l)| *l).sum::<usize>() < 1024 && !all_regions.is_empty() {
            for r in all_regions {
                if !r.executable || r.writable {
                    continue;
                }
                let Some(end_hva_isize) = (r.end as isize).checked_add(delta) else {
                    continue;
                };
                if end_hva_isize <= 0 {
                    continue;
                }
                let end_hva = end_hva_isize as usize;
                let probe_len = r.end.saturating_sub(r.start).min(4096);
                let probe_start = end_hva.saturating_sub(probe_len);
                if anchor_hva.abs_diff(probe_start) > 0x7000_0000 {
                    continue;
                }
                if let Ok(tail) = read_memory_bytes(pid, probe_start, probe_len) {
                    if let Some((off, len)) = find_trailing_padding(&tail, 96, true) {
                        if tail[off..off + len].iter().all(|&b| b == 0xCC) {
                            let aligned_off = (off + 15) & !15;
                            if off + len > aligned_off && (off + len - aligned_off) >= 96 {
                                let start = probe_start + aligned_off;
                                if !exec_caves.iter().any(|(s, l)| start >= *s && start < *s + *l) {
                                    exec_caves.push((start, off + len - aligned_off));
                                }
                            }
                        }
                    }
                }
            }
        }

        Self {
            exec_caves,
            data_caves,
        }
    }

    fn total_exec_bytes(&self) -> usize {
        self.exec_caves.iter().map(|(_, l)| *l).sum()
    }

    fn total_data_bytes(&self) -> usize {
        self.data_caves.iter().map(|(_, l)| *l).sum()
    }

    fn max_data_cave_len(&self) -> usize {
        self.data_caves.iter().map(|(_, l)| *l).max().unwrap_or(0)
    }

    fn peek_exec(&self, min_size: usize) -> Option<(usize, usize)> {
        self.exec_caves
            .iter()
            .enumerate()
            .find(|(_, (_, rem))| *rem >= min_size)
            .map(|(idx, (cur, _))| (idx, *cur))
    }

    fn commit_exec(&mut self, idx: usize, used_size: usize) {
        let aligned = (used_size + 15) & !15;
        if let Some((cur, rem)) = self.exec_caves.get_mut(idx) {
            let adv = aligned.min(*rem);
            *cur += adv;
            *rem -= adv;
        }
    }

    fn alloc_exec(&mut self, size: usize) -> Option<usize> {
        let aligned = (size + 15) & !15;
        for (cur, rem) in &mut self.exec_caves {
            if *rem >= aligned {
                let addr = *cur;
                *cur += aligned;
                *rem -= aligned;
                return Some(addr);
            }
        }
        None
    }

    fn alloc_data(&mut self, size: usize) -> Option<usize> {
        let aligned = (size + 15) & !15;
        for (cur, rem) in &mut self.data_caves {
            if *rem >= aligned {
                let addr = *cur;
                *cur += aligned;
                *rem -= aligned;
                return Some(addr);
            }
        }
        None
    }
}

fn find_trailing_padding(bytes: &[u8], min_len: usize, allow_cc: bool) -> Option<(usize, usize)> {
    let mut count = 0usize;
    for &b in bytes.iter().rev() {
        let is_pad = b == 0x00 || (allow_cc && b == 0xCC);
        if is_pad {
            count += 1;
        } else {
            break;
        }
    }
    if count >= min_len {
        Some((bytes.len() - count, count))
    } else {
        None
    }
}

/// Parses ELF64 `PT_LOAD` segment tail alignment gaps (`[seg_end .. page_end]`)
/// for an x86_64 `.so` module loaded at `base_hva`.
pub fn parse_elf64_x64_caves(
    module_name: &str,
    base_hva: usize,
    header_bytes: &[u8],
) -> Vec<ElfCaveRange> {
    let mut caves = Vec::new();
    if header_bytes.len() < 64 || header_bytes[0..4] != ELF_MAGIC {
        return caves;
    }
    if header_bytes[4] != ELFCLASS64 {
        return caves;
    }
    let e_machine = u16::from_le_bytes([header_bytes[18], header_bytes[19]]);
    if e_machine != EM_X86_64 {
        return caves;
    }
    let e_phoff = u64::from_le_bytes(header_bytes[32..40].try_into().unwrap_or_default()) as usize;
    let e_phentsize = u16::from_le_bytes([header_bytes[54], header_bytes[55]]) as usize;
    let e_phnum = u16::from_le_bytes([header_bytes[56], header_bytes[57]]) as usize;

    if e_phentsize < 56 || e_phoff + e_phentsize.saturating_mul(e_phnum) > header_bytes.len() {
        return caves;
    }

    let mut min_vaddr = u64::MAX;
    let mut segments = Vec::new();
    for i in 0..e_phnum {
        let off = e_phoff + i * e_phentsize;
        let ph = &header_bytes[off..off + e_phentsize];
        let p_type = u32::from_le_bytes(ph[0..4].try_into().unwrap_or_default());
        if p_type != PT_LOAD {
            continue;
        }
        let p_flags = u32::from_le_bytes(ph[4..8].try_into().unwrap_or_default());
        let p_vaddr = u64::from_le_bytes(ph[16..24].try_into().unwrap_or_default());
        let p_memsz = u64::from_le_bytes(ph[40..48].try_into().unwrap_or_default());
        if p_vaddr < min_vaddr {
            min_vaddr = p_vaddr;
        }
        segments.push((p_flags, p_vaddr, p_memsz));
    }

    if min_vaddr == u64::MAX {
        return caves;
    }

    let min_vaddr_page = min_vaddr & !0xFFF;
    for (p_flags, p_vaddr, p_memsz) in segments {
        if p_memsz == 0 {
            continue;
        }
        let seg_end_rel = (p_vaddr - min_vaddr_page).saturating_add(p_memsz) as usize;
        let seg_end_hva = base_hva.saturating_add(seg_end_rel);
        let page_end_hva = (seg_end_hva + 0xFFF) & !0xFFF;
        let aligned_start = (seg_end_hva + 15) & !15;
        if page_end_hva > aligned_start {
            let len = page_end_hva - aligned_start;
            if len >= 64 {
                caves.push(ElfCaveRange {
                    module_name: module_name.to_string(),
                    start_hva: aligned_start,
                    len,
                    is_exec: (p_flags & PF_X) != 0,
                    is_write: (p_flags & PF_W) != 0 && (p_flags & PF_X) == 0,
                });
            }
        }
    }

    caves
}

/// Extracts a `MemoryOperand` suitable for `lea rax, [mem]` given `stack_adjust` bytes
/// already pushed on the stack before `lea` executes.
fn extract_lea_memory_operand(instr: &Instruction, stack_adjust: i64) -> Option<MemoryOperand> {
    if instr.has_rep_prefix() || instr.has_repne_prefix() || instr.is_vsib() {
        return None;
    }
    let has_mem_op = (0..instr.op_count()).any(|k| instr.op_kind(k) == OpKind::Memory);
    if !has_mem_op {
        return None;
    }
    let seg = instr.segment_prefix();
    if seg == Register::FS || seg == Register::GS {
        return None;
    }

    if instr.is_ip_rel_memory_operand() {
        let target_ip = instr.ip_rel_memory_address();
        return Some(MemoryOperand::with_base_displ(Register::RIP, target_ip as i64));
    }

    let base = instr.memory_base();
    let index = instr.memory_index();
    let scale = instr.memory_index_scale();
    if base != Register::None && !base.is_gpr() {
        return None;
    }
    if index != Register::None && (!index.is_gpr() || index == Register::RSP) {
        return None;
    }

    let mut displ = instr.memory_displacement64() as i64;
    if base == Register::RSP {
        displ = displ.wrapping_add(stack_adjust);
    }

    let displ_size = if base == Register::None && index == Register::None {
        8
    } else if displ == 0 {
        0
    } else if (-128..=127).contains(&displ) {
        1
    } else {
        8
    };

    Some(MemoryOperand::new(
        base,
        index,
        scale,
        displ,
        displ_size,
        false,
        Register::None,
    ))
}

fn format_instruction_intel(instr: &Instruction) -> String {
    let mut formatter = IntelFormatter::new();
    let mut out = String::new();
    formatter.format(instr, &mut out);
    out
}

fn encode_jmp_rel32_patch(from_ip: usize, to_ip: usize, total_len: usize) -> Result<Vec<u8>, String> {
    if total_len < 5 {
        return Err("Need at least 5 bytes for JMP patch".to_string());
    }
    let rel = (to_ip as isize) - ((from_ip + 5) as isize);
    let rel32 = i32::try_from(rel).map_err(|_| "Jump target exceeds +/- 2GB range".to_string())?;
    let mut patch = Vec::with_capacity(total_len);
    patch.push(0xE9);
    patch.extend_from_slice(&rel32.to_le_bytes());
    patch.resize(total_len, 0x90);
    Ok(patch)
}

/// Decodes stolen instruction(s) at `inst_hva` covering at least 5 bytes on instruction boundary.
fn decode_stolen_instructions(
    code_bytes: &[u8],
    inst_hva: usize,
) -> Result<(Vec<Instruction>, usize, usize), String> {
    let mut decoder = Decoder::with_ip(64, code_bytes, inst_hva as u64, DecoderOptions::NONE);
    let mut stolen = Vec::new();
    let mut total_len = 0usize;
    let mut mem_instr_idx = None;

    while decoder.can_decode() && total_len < 5 {
        let instr = decoder.decode();
        if instr.is_invalid() || instr.code() == Code::Int3 {
            return Err("Invalid or padding instruction at hook site".to_string());
        }
        if !stolen.is_empty() && instr.flow_control() != FlowControl::Next {
            return Err("Instruction < 5 bytes borders a branch/return; unsafe to steal next instruction".to_string());
        }
        let has_mem = (0..instr.op_count()).any(|k| instr.op_kind(k) == OpKind::Memory);
        if mem_instr_idx.is_none() && has_mem {
            mem_instr_idx = Some(stolen.len());
        }
        total_len += instr.len();
        stolen.push(instr);
    }

    if total_len < 5 {
        return Err("Could not decode >= 5 instruction bytes at target".to_string());
    }
    let mem_idx = mem_instr_idx.ok_or_else(|| "Instruction does not have a memory operand".to_string())?;
    Ok((stolen, total_len, mem_idx))
}

/// Builds the x86_64 code-cave stub + jmp patch for capturing every `data_address`
/// accessed by `stolen[mem_idx]` into a 64-entry ring buffer at `data_cave_hva`.
/// Layout at `data_cave_hva`:
/// - `[0..8]`: `u64 write_seq`
/// - `[8..16]`: reserved
/// - `[16..528]`: `[u64; 64]` ring buffer of captured `data_address` (GVA)
pub fn build_code_access_hook_bytes(
    inst_hva: usize,
    code_bytes: &[u8],
    code_cave_hva: usize,
    data_cave_hva: usize,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let (stolen, stolen_len, mem_idx) = decode_stolen_instructions(code_bytes, inst_hva)?;
    if mem_idx != 0 {
        return Err("First instruction at target address has no memory operand".to_string());
    }
    let orig_bytes = code_bytes[..stolen_len].to_vec();
    // Skip System V AMD64 128-byte Red Zone + 32 bytes (pushfq, rax, rcx, rdx) = 160
    let lea_mem_op = extract_lea_memory_operand(&stolen[0], 160)
        .ok_or_else(|| "Unsupported memory operand for address capture".to_string())?;

    let write_seq_va = data_cave_hva as i64;
    let ring_buf_va = (data_cave_hva + 16) as i64;
    let return_va = (inst_hva + stolen_len) as u64;

    let mut instrs = Vec::new();
    instrs.push(
        Instruction::with2(
            Code::Lea_r64_m,
            Register::RSP,
            MemoryOperand::with_base_index_scale_displ_size(
                Register::RSP,
                Register::None,
                1,
                -128,
                1,
            ),
        )
        .map_err(|e| e.to_string())?,
    );
    instrs.push(Instruction::with(Code::Pushfq));
    instrs.push(Instruction::with1(Code::Push_r64, Register::RAX).map_err(|e| e.to_string())?);
    instrs.push(Instruction::with1(Code::Push_r64, Register::RCX).map_err(|e| e.to_string())?);
    instrs.push(Instruction::with1(Code::Push_r64, Register::RDX).map_err(|e| e.to_string())?);

    instrs.push(Instruction::with2(Code::Lea_r64_m, Register::RAX, lea_mem_op).map_err(|e| e.to_string())?);
    instrs.push(Instruction::with2(Code::Mov_r32_imm32, Register::ECX, 1i32).map_err(|e| e.to_string())?);
    let mut xadd = Instruction::with2(
        Code::Xadd_rm64_r64,
        MemoryOperand::with_base_displ(Register::RIP, write_seq_va),
        Register::RCX,
    )
    .map_err(|e| e.to_string())?;
    xadd.set_has_lock_prefix(true);
    instrs.push(xadd);

    instrs.push(Instruction::with2(Code::And_rm32_imm8, Register::ECX, 63i32).map_err(|e| e.to_string())?);
    instrs.push(
        Instruction::with2(
            Code::Lea_r64_m,
            Register::RDX,
            MemoryOperand::with_base_displ(Register::RIP, ring_buf_va),
        )
        .map_err(|e| e.to_string())?,
    );
    instrs.push(
        Instruction::with2(
            Code::Mov_rm64_r64,
            MemoryOperand::with_base_index_scale_displ_size(
                Register::RDX,
                Register::RCX,
                8,
                0,
                0,
            ),
            Register::RAX,
        )
        .map_err(|e| e.to_string())?,
    );

    instrs.push(Instruction::with1(Code::Pop_r64, Register::RDX).map_err(|e| e.to_string())?);
    instrs.push(Instruction::with1(Code::Pop_r64, Register::RCX).map_err(|e| e.to_string())?);
    instrs.push(Instruction::with1(Code::Pop_r64, Register::RAX).map_err(|e| e.to_string())?);
    instrs.push(Instruction::with(Code::Popfq));
    instrs.push(
        Instruction::with2(
            Code::Lea_r64_m,
            Register::RSP,
            MemoryOperand::with_base_index_scale_displ_size(
                Register::RSP,
                Register::None,
                1,
                128,
                8,
            ),
        )
        .map_err(|e| e.to_string())?,
    );

    for s in stolen {
        instrs.push(s);
    }
    instrs.push(Instruction::with_branch(Code::Jmp_rel32_64, return_va).map_err(|e| e.to_string())?);

    let encoded = BlockEncoder::encode(
        64,
        InstructionBlock::new(&instrs, code_cave_hva as u64),
        BlockEncoderOptions::NONE,
    )
    .map_err(|e| format!("Failed to assemble code-cave hook: {e}"))?;

    let jmp_patch = encode_jmp_rel32_patch(inst_hva, code_cave_hva, stolen_len)?;
    Ok((encoded.code_buffer, jmp_patch, orig_bytes))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMatchMode {
    ExactGva { target_gva_slot_va: usize },
    PageOffset { page_off: u32 },
}

/// Builds a candidate verification hook for `InstructionWatchDialog`.
/// Increments `[counter_slot_va]` ONLY when runtime effective address matches `mode`.
pub fn build_candidate_filter_hook_bytes(
    inst_hva: usize,
    code_bytes: &[u8],
    code_cave_hva: usize,
    mode: FilterMatchMode,
    counter_slot_va: usize,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let (stolen, stolen_len, mem_idx) = decode_stolen_instructions(code_bytes, inst_hva)?;
    if mem_idx != 0 {
        return Err("Candidate instruction first op is not memory".to_string());
    }
    let orig_bytes = code_bytes[..stolen_len].to_vec();
    // Skip System V AMD64 128-byte Red Zone + 24 bytes (pushfq, rax, rdx) = 152
    let lea_mem_op = extract_lea_memory_operand(&stolen[0], 152)
        .ok_or_else(|| "Unsupported candidate memory operand".to_string())?;
    let access_size = stolen[0].memory_size().size().max(4) as i32;
    let return_va = (inst_hva + stolen_len) as u64;

    // 1. Pre-branch prefix
    let mut pre_instrs = vec![
        Instruction::with2(
            Code::Lea_r64_m,
            Register::RSP,
            MemoryOperand::with_base_index_scale_displ_size(
                Register::RSP,
                Register::None,
                1,
                -128,
                1,
            ),
        )
        .map_err(|e| e.to_string())?,
        Instruction::with(Code::Pushfq),
        Instruction::with1(Code::Push_r64, Register::RAX).map_err(|e| e.to_string())?,
        Instruction::with1(Code::Push_r64, Register::RDX).map_err(|e| e.to_string())?,
        Instruction::with2(Code::Lea_r64_m, Register::RAX, lea_mem_op).map_err(|e| e.to_string())?,
    ];
    match mode {
        FilterMatchMode::ExactGva { target_gva_slot_va } => {
            pre_instrs.push(
                Instruction::with2(
                    Code::Mov_r64_rm64,
                    Register::RDX,
                    MemoryOperand::with_base_displ(Register::RIP, target_gva_slot_va as i64),
                )
                .map_err(|e| e.to_string())?,
            );
            pre_instrs.push(
                Instruction::with2(Code::Sub_r64_rm64, Register::RDX, Register::RAX)
                    .map_err(|e| e.to_string())?,
            );
            pre_instrs.push(
                Instruction::with2(Code::Cmp_rm64_imm8, Register::RDX, access_size)
                    .map_err(|e| e.to_string())?,
            );
        }
        FilterMatchMode::PageOffset { page_off } => {
            pre_instrs.push(
                Instruction::with2(Code::And_rm32_imm32, Register::EAX, 0xFFFi32)
                    .map_err(|e| e.to_string())?,
            );
            pre_instrs.push(
                Instruction::with2(Code::Mov_r32_imm32, Register::EDX, (page_off & 0xFFF) as i32)
                    .map_err(|e| e.to_string())?,
            );
            pre_instrs.push(
                Instruction::with2(Code::Sub_r32_rm32, Register::EDX, Register::EAX)
                    .map_err(|e| e.to_string())?,
            );
            pre_instrs.push(
                Instruction::with2(Code::Cmp_rm32_imm8, Register::EDX, access_size)
                    .map_err(|e| e.to_string())?,
            );
        }
    }

    let pre_encoded = BlockEncoder::encode(
        64,
        InstructionBlock::new(&pre_instrs, code_cave_hva as u64),
        BlockEncoderOptions::NONE,
    )
    .map_err(|e| e.to_string())?;
    let pre_len = pre_encoded.code_buffer.len();

    // 2. Conditional body (lock inc qword ptr [counter_slot_va])
    let mut inc_instr = Instruction::with1(
        Code::Inc_rm64,
        MemoryOperand::with_base_displ(Register::RIP, counter_slot_va as i64),
    )
    .map_err(|e| e.to_string())?;
    inc_instr.set_has_lock_prefix(true);
    let inc_ip = (code_cave_hva + pre_len + 2) as u64; // +2 for jae rel8
    let inc_encoded = BlockEncoder::encode(
        64,
        InstructionBlock::new(std::slice::from_ref(&inc_instr), inc_ip),
        BlockEncoderOptions::NONE,
    )
    .map_err(|e| e.to_string())?;
    let inc_len = inc_encoded.code_buffer.len();
    if inc_len > 120 {
        return Err("Unexpected inc instruction length".to_string());
    }

    // 3. Post-branch epilogue + original instruction + jmp back
    let mut post_instrs = vec![
        Instruction::with1(Code::Pop_r64, Register::RDX).map_err(|e| e.to_string())?,
        Instruction::with1(Code::Pop_r64, Register::RAX).map_err(|e| e.to_string())?,
        Instruction::with(Code::Popfq),
        Instruction::with2(
            Code::Lea_r64_m,
            Register::RSP,
            MemoryOperand::with_base_index_scale_displ_size(
                Register::RSP,
                Register::None,
                1,
                128,
                8,
            ),
        )
        .map_err(|e| e.to_string())?,
    ];
    for s in stolen {
        post_instrs.push(s);
    }
    post_instrs.push(Instruction::with_branch(Code::Jmp_rel32_64, return_va).map_err(|e| e.to_string())?);
    let post_ip = (code_cave_hva + pre_len + 2 + inc_len) as u64;
    let post_encoded = BlockEncoder::encode(
        64,
        InstructionBlock::new(&post_instrs, post_ip),
        BlockEncoderOptions::NONE,
    )
    .map_err(|e| e.to_string())?;

    let mut full_stub = Vec::with_capacity(pre_len + 2 + inc_len + post_encoded.code_buffer.len());
    full_stub.extend_from_slice(&pre_encoded.code_buffer);
    full_stub.push(0x73); // JAE rel8
    full_stub.push(inc_len as u8);
    full_stub.extend_from_slice(&inc_encoded.code_buffer);
    full_stub.extend_from_slice(&post_encoded.code_buffer);

    let jmp_patch = encode_jmp_rel32_patch(inst_hva, code_cave_hva, stolen_len)?;
    Ok((full_stub, jmp_patch, orig_bytes))
}

struct PatchedSite {
    hva: usize,
    original_bytes: Vec<u8>,
}

/// Real-time instruction access/write watcher for an emulator code entry (`CodeAccessDialog`).
pub struct EmulatorCodeAccessHook {
    stop: Arc<AtomicBool>,
    restored: Arc<AtomicBool>,
    pid: u32,
    patches: Arc<parking_lot::Mutex<Vec<PatchedSite>>>,
    handle: Option<JoinHandle<()>>,
}

impl EmulatorCodeAccessHook {
    pub fn start(
        pid: u32,
        inst_hva: usize,
        _delta: isize,
        modules: Vec<AndroidMemoryModule>,
        all_regions: Vec<AndroidMemoryRegion>,
        tx: mpsc::Sender<WatchEvent>,
    ) -> Result<Self, String> {
        let code_bytes = read_memory_bytes(pid, inst_hva, 32)
            .map_err(|e| format!("Cannot read instruction bytes at 0x{inst_hva:X}: {e}"))?;

        let extra_elfs = if modules.is_empty() {
            discover_x64_elfs_in_process_memory(pid)
        } else {
            Vec::new()
        };
        let mut caves = CaveAllocator::discover(pid, inst_hva, &modules, &all_regions, _delta, &extra_elfs);
        let code_cave_hva = caves
            .alloc_exec(160)
            .ok_or_else(|| "No unused executable ELF cave found near instruction (+/- 1.8GB)".to_string())?;
        let data_cave_hva = caves
            .alloc_data(528)
            .ok_or_else(|| "No unused writable .bss/data cave found near code cave".to_string())?;

        let (stub_bytes, jmp_patch, orig_bytes) =
            build_code_access_hook_bytes(inst_hva, &code_bytes, code_cave_hva, data_cave_hva)?;

        // Zero ring buffer + write code cave BEFORE patching target instruction
        let zero_buf = vec![0u8; 528];
        write_code_bytes(pid, data_cave_hva, &zero_buf)
            .map_err(|e| format!("Failed to initialize ring buffer cave: {e}"))?;
        write_code_bytes(pid, code_cave_hva, &stub_bytes)
            .map_err(|e| format!("Failed to write hook stub to executable cave: {e}"))?;
        write_code_bytes(pid, inst_hva, &jmp_patch)
            .map_err(|e| format!("Failed to patch instruction at 0x{inst_hva:X}: {e}"))?;

        let patches = Arc::new(parking_lot::Mutex::new(vec![
            PatchedSite {
                hva: inst_hva,
                original_bytes: orig_bytes,
            },
            PatchedSite {
                hva: code_cave_hva,
                original_bytes: vec![0u8; stub_bytes.len()],
            },
            PatchedSite {
                hva: data_cave_hva,
                original_bytes: vec![0u8; 528],
            },
        ]));

        let stop = Arc::new(AtomicBool::new(false));
        let restored = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let restored_clone = Arc::clone(&restored);
        let patches_clone = Arc::clone(&patches);

        let handle = thread::spawn(move || {
            let _ = tx.send(WatchEvent::Started {
                armed_threads: 1,
                total_threads: 1,
            });
            let mut last_seq = 0u64;

            while !stop_clone.load(Ordering::Acquire) {
                if let Ok(buf) = read_memory_bytes(pid, data_cave_hva, 528) {
                    let write_seq = u64::from_le_bytes(buf[0..8].try_into().unwrap_or_default());
                    if write_seq > last_seq {
                        if write_seq - last_seq > 64 {
                            last_seq = write_seq - 64;
                        }
                        while last_seq < write_seq {
                            let slot = (last_seq & 63) as usize;
                            let off = 16 + slot * 8;
                            let data_addr =
                                u64::from_le_bytes(buf[off..off + 8].try_into().unwrap_or_default())
                                    as usize;
                            if data_addr != 0 {
                                let _ = tx.send(WatchEvent::AccessHit {
                                    instruction_address: inst_hva,
                                    data_address: data_addr,
                                });
                            }
                            last_seq += 1;
                        }
                    }
                }
                thread::sleep(Duration::from_millis(12));
            }

            restore_patches(pid, &patches_clone, &restored_clone);
            let _ = tx.send(WatchEvent::Stopped);
        });

        Ok(Self {
            stop,
            restored,
            pid,
            patches,
            handle: Some(handle),
        })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        restore_patches(self.pid, &self.patches, &self.restored);
    }
}

impl Drop for EmulatorCodeAccessHook {
    fn drop(&mut self) {
        self.stop();
    }
}

fn restore_patches(
    pid: u32,
    patches: &Arc<parking_lot::Mutex<Vec<PatchedSite>>>,
    restored: &Arc<AtomicBool>,
) {
    if restored.swap(true, Ordering::AcqRel) {
        return;
    }
    let list = patches.lock();
    // Restore original instructions FIRST, then clear caves
    for site in list.iter() {
        let _ = write_code_bytes(pid, site.hva, &site.original_bytes);
    }
}

/// Candidate instruction discovered in a game module for runtime verification.
pub struct CandidateInfo {
    inst_hva: usize,
    mod_desc: String,
    asm_text: String,
    code_bytes: Vec<u8>,
    field_offset: i64,
}

pub struct CandidateScanOutcome {
    pub candidates: Vec<CandidateInfo>,
    pub used_elfs: Vec<DiscoveredElfModule>,
    pub cand_offsets: Vec<u32>,
    pub mod_count: usize,
}

fn pointer_into_modules(ptr_val: usize, mods: &[(String, usize, usize, usize)], delta: isize) -> bool {
    mods.iter().any(|(_, base_gva, base_hva, sz)| {
        let in_gva = ptr_val >= *base_gva && ptr_val < base_gva.saturating_add(*sz);
        let in_hva = ptr_val >= *base_hva && ptr_val < base_hva.saturating_add(*sz);
        let in_shifted = if delta != 0 {
            let s = (ptr_val as isize).wrapping_add(delta) as usize;
            s >= *base_hva && s < base_hva.saturating_add(*sz)
        } else {
            false
        };
        in_gva || in_hva || in_shifted
    })
}

fn is_game_module_name(name: &str) -> bool {
    let l = name.to_lowercase();
    l.contains("il2cpp")
        || l.contains("unity")
        || l.contains("unreal")
        || l.contains("ue4")
        || l.contains("cocos")
        || l.contains("godot")
        || l.contains("neox")
        || l.contains("game")
        || l.contains("main")
}

/// Extracts strictly executable (`PF_X`) code ranges within `[base_hva .. base_hva + mod_size]`.
/// Never scans `.rodata`, `.data`, `.bss`, ELF headers, or non-executable pages.
fn get_elf_exec_ranges(
    pid: u32,
    base_gva: usize,
    hva_base: usize,
    mod_size: usize,
    delta: isize,
    all_regions: &[AndroidMemoryRegion],
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let cap = mod_size.min(128 * 1024 * 1024);
    if cap < 4096 {
        return ranges;
    }

    if let Ok(hdr) = read_memory_bytes(pid, hva_base, 4096) {
        if hdr.len() >= 64
            && hdr[0..4] == ELF_MAGIC
            && hdr[4] == ELFCLASS64
            && u16::from_le_bytes([hdr[18], hdr[19]]) == EM_X86_64
        {
            let e_phoff = u64::from_le_bytes(hdr[32..40].try_into().unwrap_or_default()) as usize;
            let e_phentsize = u16::from_le_bytes([hdr[54], hdr[55]]) as usize;
            let e_phnum = u16::from_le_bytes([hdr[56], hdr[57]]) as usize;
            if e_phentsize >= 56 && e_phoff + e_phentsize.saturating_mul(e_phnum) <= hdr.len() {
                let mut min_vaddr = u64::MAX;
                let mut exec_segs = Vec::new();
                for i in 0..e_phnum {
                    let off = e_phoff + i * e_phentsize;
                    let ph = &hdr[off..off + e_phentsize];
                    if u32::from_le_bytes(ph[0..4].try_into().unwrap_or_default()) != PT_LOAD {
                        continue;
                    }
                    let p_flags = u32::from_le_bytes(ph[4..8].try_into().unwrap_or_default());
                    let p_vaddr = u64::from_le_bytes(ph[16..24].try_into().unwrap_or_default());
                    let p_filesz = u64::from_le_bytes(ph[32..40].try_into().unwrap_or_default());
                    min_vaddr = min_vaddr.min(p_vaddr);
                    if (p_flags & PF_X) != 0 && p_filesz >= 64 {
                        exec_segs.push((p_vaddr, p_filesz));
                    }
                }
                if min_vaddr != u64::MAX {
                    let min_page = min_vaddr & !0xFFF;
                    for (p_vaddr, p_filesz) in exec_segs {
                        let rel = p_vaddr.saturating_sub(min_page) as usize;
                        let mut start_off = rel;
                        let mut len = (p_filesz as usize).min(cap.saturating_sub(rel));
                        // Skip page 0 (ELF header / dynamic tables) when text shares base page
                        if p_vaddr == min_page && len > 4096 {
                            start_off += 4096;
                            len -= 4096;
                        }
                        if len >= 64 {
                            ranges.push((hva_base + start_off, len));
                        }
                    }
                }
            }
        }
    }

    // Intersect or fallback strictly to r-xp regions from /proc/<pid>/maps when available
    if !all_regions.is_empty() {
        let mod_end = base_gva.saturating_add(cap);
        let rx_regions: Vec<(usize, usize)> = all_regions
            .iter()
            .filter(|r| {
                r.executable && !r.writable && r.start < mod_end && r.end > base_gva
            })
            .filter_map(|r| {
                let s = r.start.max(base_gva);
                let e = r.end.min(mod_end);
                if e <= s {
                    return None;
                }
                let sh = (s as isize).checked_add(delta)?;
                if sh <= 0 {
                    return None;
                }
                Some((sh as usize, e - s))
            })
            .collect();

        if ranges.is_empty() {
            return rx_regions;
        } else if !rx_regions.is_empty() {
            let mut clipped = Vec::new();
            for (rs, rl) in ranges {
                let re = rs.saturating_add(rl);
                for &(xs, xl) in &rx_regions {
                    let xe = xs.saturating_add(xl);
                    let is = rs.max(xs);
                    let ie = re.min(xe);
                    if ie > is + 64 {
                        clipped.push((is, ie - is));
                    }
                }
            }
            return clipped;
        }
    }

    ranges
}

/// Scans game modules byte-by-byte in RAM (0 syscalls per candidate match) strictly within
/// verified executable (`PF_X`) segments, synchronizing instruction boundaries via linear decode.
pub fn scan_verified_xref_candidates(
    pid: u32,
    target_gva: usize,
    target_hva: usize,
    writes_only: bool,
    delta: isize,
    modules: &[AndroidMemoryModule],
    all_regions: &[AndroidMemoryRegion],
    host_modules: &[(String, usize, usize)],
) -> CandidateScanOutcome {
    let mut candidate_mods: Vec<(String, usize, usize, usize)> = Vec::new();
    for m in modules {
        let hva_base = (m.base_address as isize).wrapping_add(delta) as usize;
        candidate_mods.push((m.name.clone(), m.base_address, hva_base, m.size));
    }

    let mut used_elfs = Vec::new();
    if candidate_mods.is_empty() {
        used_elfs = discover_x64_elfs_in_process_memory(pid);
        for elf in &used_elfs {
            candidate_mods.push((elf.name.clone(), elf.base_gva, elf.base_hva, elf.size));
        }
    }
    if candidate_mods.is_empty() {
        for (name, base, size) in host_modules {
            candidate_mods.push((name.clone(), *base, *base, *size));
        }
    }

    candidate_mods.sort_by_key(|(name, _, _, sz)| {
        let l = name.to_lowercase();
        if l.contains("il2cpp") {
            (0u8, usize::MAX - *sz)
        } else if l.contains("unity") {
            (1, usize::MAX - *sz)
        } else if l.contains("neox") || l.contains("game") || l.contains("main") {
            (2, usize::MAX - *sz)
        } else if l.ends_with(".so") || l.ends_with(".dll") {
            (3, usize::MAX - *sz)
        } else {
            (4, usize::MAX - *sz)
        }
    });

    // Probe offsets against ALL modules (for vtable/Il2CppClass pointer identification),
    // then restrict instruction hooking strictly to game modules when any exist so we never
    // hook noisy Android system libraries (libc.so, libart.so, etc.).
    let probe_mods = candidate_mods.clone();
    let has_game_mod = candidate_mods
        .iter()
        .any(|(n, _, _, _)| is_game_module_name(n));
    let scan_mods: Vec<(String, usize, usize, usize)> = if has_game_mod {
        candidate_mods
            .into_iter()
            .filter(|(n, _, _, sz)| is_game_module_name(n) || *sz >= 8 * 1024 * 1024)
            .collect()
    } else {
        candidate_mods
            .into_iter()
            .filter(|(n, _, _, sz)| !n.starts_with("elf_0x") || *sz >= 2 * 1024 * 1024)
            .take(12)
            .collect()
    };

    // 1. Discover candidate field offsets:
    //    - Probe backwards up to 8KB from target_hva for direct vptr OR 1-hop Il2CppClass* headers
    //    - Include page masks (& 0xFFF, & 0x1FFF, & 0x7FFF, & 0xFFFF)
    let mut cand_offsets: Vec<u32> = Vec::new();
    let probe_len = 0x2000usize;
    let probe_base = target_hva.saturating_sub(probe_len);
    if let Ok(obj_probe) = read_memory_bytes(pid, probe_base, probe_len) {
        for off in (0..obj_probe.len()).step_by(8).rev() {
            if off + 8 > obj_probe.len() {
                continue;
            }
            let ptr_val =
                u64::from_le_bytes(obj_probe[off..off + 8].try_into().unwrap_or_default()) as usize;
            if ptr_val < 0x10000 || ptr_val > 0x0000_7FFF_FFFF_FFFF {
                continue;
            }
            let field_off = (target_hva - (probe_base + off)) as u32;
            if field_off == 0 || field_off > 0xFFFF {
                continue;
            }
            let mut is_header = pointer_into_modules(ptr_val, &probe_mods, delta);
            // 1-hop Il2CppClass* check (aligned heap pointer whose first 32 bytes contain .so pointers)
            if !is_header && (ptr_val & 7) == 0 {
                let ptr_hva = if delta != 0 {
                    let h = (ptr_val as isize).wrapping_add(delta) as usize;
                    if read_memory_bytes(pid, h, 32).is_ok() {
                        Some(h)
                    } else if read_memory_bytes(pid, ptr_val, 32).is_ok() {
                        Some(ptr_val)
                    } else {
                        None
                    }
                } else {
                    Some(ptr_val)
                };
                if let Some(hva) = ptr_hva {
                    if let Ok(klass_bytes) = read_memory_bytes(pid, hva, 32) {
                        for k_off in (0..32).step_by(8) {
                            let q = u64::from_le_bytes(
                                klass_bytes[k_off..k_off + 8].try_into().unwrap_or_default(),
                            ) as usize;
                            if pointer_into_modules(q, &probe_mods, delta) {
                                is_header = true;
                                break;
                            }
                        }
                    }
                }
            }
            if is_header && !cand_offsets.contains(&field_off) {
                cand_offsets.push(field_off);
            }
        }
    }

    for addr_source in [target_gva, target_hva] {
        for mask in [0xFFFusize, 0x1FFF, 0x7FFF, 0xFFFF] {
            let off = (addr_source & mask) as u32;
            if off > 0 && !cand_offsets.contains(&off) {
                cand_offsets.push(off);
            }
        }
    }

    let mod_count = probe_mods.len();
    let max_cands = 64usize;
    let mut results = Vec::new();
    let mut seen_hva = HashSet::new();
    let mut factory = InstructionInfoFactory::new();
    let chunk_size = 256 * 1024;
    let sync_lead = 256usize;
    let tail_pad = 32usize;

    let disp32_needles: Vec<(u32, [u8; 4])> = cand_offsets
        .iter()
        .filter(|&&o| o > 0x7F)
        .map(|&o| (o, o.to_le_bytes()))
        .collect();
    let disp8_offsets: Vec<u32> = cand_offsets.iter().copied().filter(|&o| o <= 0x7F).collect();

    for (mod_name, base_gva, hva_base, mod_size) in &scan_mods {
        if results.len() >= max_cands {
            break;
        }
        let exec_ranges = get_elf_exec_ranges(pid, *base_gva, *hva_base, *mod_size, delta, all_regions);
        for (range_start, range_len) in exec_ranges {
            if results.len() >= max_cands {
                break;
            }
            let mut offset = 0usize;
            while offset + 4 <= range_len && results.len() < max_cands {
                let lead = offset.min(sync_lead);
                let read_addr = range_start + offset - lead;
                let scan_span = chunk_size.min(range_len - offset);
                let tail = tail_pad.min(range_len - (offset + scan_span));
                let read_len = lead + scan_span + tail;

                if let Ok(bytes) = read_memory_bytes(pid, read_addr, read_len) {
                    let scan_end = (lead + scan_span).min(bytes.len().saturating_sub(3));
                    for i in lead..scan_end {
                        if results.len() >= max_cands {
                            break;
                        }
                        let slice4 = &bytes[i..i + 4];
                        for &(field_off, needle) in &disp32_needles {
                            if slice4 == needle {
                                if let Some(cand) = verify_candidate_in_buffer(
                                    &bytes,
                                    read_addr,
                                    i,
                                    field_off as i64,
                                    writes_only,
                                    delta,
                                    *base_gva,
                                    mod_name,
                                    &mut factory,
                                    &mut seen_hva,
                                ) {
                                    results.push(cand);
                                }
                            }
                        }

                        // Check disp8 only when preceded by ModRM with mod=01 (0x40..0x7F)
                        if !disp8_offsets.is_empty() && i >= 2 {
                            let b = bytes[i];
                            let prev = bytes[i - 1];
                            if (prev & 0xC0) == 0x40 && disp8_offsets.contains(&(b as u32)) {
                                if let Some(cand) = verify_candidate_in_buffer(
                                    &bytes,
                                    read_addr,
                                    i,
                                    b as i64,
                                    writes_only,
                                    delta,
                                    *base_gva,
                                    mod_name,
                                    &mut factory,
                                    &mut seen_hva,
                                ) {
                                    results.push(cand);
                                }
                            }
                        }
                    }
                }
                offset += chunk_size;
            }
        }
    }

    CandidateScanOutcome {
        candidates: results,
        used_elfs,
        cand_offsets,
        mod_count,
    }
}

fn is_unsafe_followup_instr(instr: &Instruction) -> bool {
    if instr.is_invalid() {
        return true;
    }
    matches!(
        instr.code(),
        Code::Hlt
            | Code::Int1
            | Code::Int_imm8
            | Code::Syscall
            | Code::Sysenter
            | Code::Sysexitd
            | Code::Sysexitq
            | Code::Sysretd
            | Code::Sysretq
            | Code::Cli
            | Code::Sti
            | Code::Iretd
            | Code::Iretq
            | Code::Iretw
            | Code::In_AL_DX
            | Code::In_AL_imm8
            | Code::In_AX_DX
            | Code::In_AX_imm8
            | Code::In_EAX_DX
            | Code::In_EAX_imm8
            | Code::Out_DX_AL
            | Code::Out_imm8_AL
            | Code::Out_DX_AX
            | Code::Out_imm8_AX
            | Code::Out_DX_EAX
            | Code::Out_imm8_EAX
    )
}

fn verify_candidate_in_buffer(
    bytes: &[u8],
    chunk_addr: usize,
    match_idx: usize,
    field_offset: i64,
    writes_only: bool,
    delta: isize,
    base_gva: usize,
    mod_name: &str,
    factory: &mut InstructionInfoFactory,
    seen_hva: &mut HashSet<usize>,
) -> Option<CandidateInfo> {
    let match_hva = chunk_addr + match_idx;

    // Synchronize instruction stream from at most 256 bytes earlier (or right after last 0xCC 0xCC padding).
    let mut sync_start = match_idx.saturating_sub(256);
    if match_idx > 16 {
        let search_end = match_idx - 12;
        let mut p = search_end;
        while p > sync_start + 1 {
            if bytes[p] == 0xCC && bytes[p - 1] == 0xCC {
                let mut after = p + 1;
                while after < search_end && bytes[after] == 0xCC {
                    after += 1;
                }
                sync_start = after;
                break;
            }
            p -= 1;
        }
    }

    let want_disp_size = if field_offset > 0x7F { 4 } else { 1 };
    let mut cur = sync_start;
    while cur <= match_idx && cur < bytes.len() {
        let ip = chunk_addr + cur;
        let mut decoder = Decoder::with_ip(64, &bytes[cur..], ip as u64, DecoderOptions::NONE);
        while decoder.can_decode() {
            let instr = decoder.decode();
            if instr.is_invalid() {
                cur = (instr.ip() as usize - chunk_addr) + 1;
                break;
            }
            let inst_addr = instr.ip() as usize;
            let inst_off = inst_addr - chunk_addr;
            if inst_addr > match_hva {
                return None;
            }
            if inst_addr <= match_hva && match_hva < inst_addr + instr.len() {
                let const_offs = decoder.get_constant_offsets(&instr);
                if !const_offs.has_displacement()
                    || const_offs.displacement_size() != want_disp_size
                    || inst_addr + const_offs.displacement_offset() != match_hva
                {
                    return None;
                }
                // Only hook single instructions >= 5 bytes so we NEVER steal a branch target next door
                if instr.len() < 5
                    || instr.flow_control() != FlowControl::Next
                    || instr.is_stack_instruction()
                    || instr.is_ip_rel_memory_operand()
                    || instr.has_lock_prefix()
                    || instr.has_rep_prefix()
                    || instr.has_repne_prefix()
                    || instr.segment_prefix() != Register::None
                {
                    return None;
                }
                let mem_base = instr.memory_base();
                if mem_base == Register::RSP || mem_base == Register::RIP {
                    return None;
                }
                if instr.op_kind(0) != OpKind::Memory
                    && (instr.op_count() < 2 || instr.op_kind(1) != OpKind::Memory)
                {
                    return None;
                }
                if (instr.memory_displacement64() as i64) != field_offset {
                    return None;
                }
                if extract_lea_memory_operand(&instr, 152).is_none() {
                    return None;
                }

                // Forward sanity check: verify subsequent instructions in this block decode cleanly
                let next_off = inst_off + instr.len();
                if next_off + 24 > bytes.len() {
                    return None;
                }
                let mut fwd = Decoder::with_ip(
                    64,
                    &bytes[next_off..],
                    (inst_addr + instr.len()) as u64,
                    DecoderOptions::NONE,
                );
                for _ in 0..4 {
                    if !fwd.can_decode() {
                        break;
                    }
                    let nxt = fwd.decode();
                    if nxt.code() == Code::Int3 || nxt.flow_control() == FlowControl::Return {
                        break;
                    }
                    if is_unsafe_followup_instr(&nxt) {
                        return None;
                    }
                }

                let info = factory.info(&instr);
                let mem_matches_mode = info.used_memory().iter().any(|m| {
                    if writes_only {
                        matches!(
                            m.access(),
                            OpAccess::Write | OpAccess::ReadWrite | OpAccess::CondWrite
                        )
                    } else {
                        matches!(
                            m.access(),
                            OpAccess::Read
                                | OpAccess::Write
                                | OpAccess::ReadWrite
                                | OpAccess::CondRead
                                | OpAccess::CondWrite
                        )
                    }
                });
                if !mem_matches_mode {
                    return None;
                }

                if inst_off + 24 > bytes.len() {
                    return None;
                }
                let code_slice = bytes[inst_off..inst_off + 24].to_vec();
                if decode_stolen_instructions(&code_slice, inst_addr).is_err() {
                    return None;
                }

                if seen_hva.insert(inst_addr) {
                    let inst_gva = if delta != 0 {
                        (inst_addr as isize - delta) as usize
                    } else {
                        inst_addr
                    };
                    let mod_off = inst_gva.saturating_sub(base_gva);
                    return Some(CandidateInfo {
                        inst_hva: inst_addr,
                        mod_desc: format!("{mod_name}+0x{mod_off:X}"),
                        asm_text: format_instruction_intel(&instr),
                        code_bytes: code_slice,
                        field_offset,
                    });
                }
                return None;
            }
            cur = inst_off + instr.len();
        }
    }
    None
}

/// Real-time watch session for `InstructionWatchDialog` on Android emulators.
/// Combines value mutation polling with live runtime hook verification of candidate instructions
/// so ONLY instructions that actually touch `target_gva` are reported, with real hit counts!
pub struct EmulatorInstructionWatch {
    stop: Arc<AtomicBool>,
    restored: Arc<AtomicBool>,
    pid: u32,
    patches: Arc<parking_lot::Mutex<Vec<PatchedSite>>>,
    handle: Option<JoinHandle<()>>,
}

impl EmulatorInstructionWatch {
    pub fn start(
        pid: u32,
        watch_targets: Vec<(usize, usize)>, // (gva_display, hva)
        writes_only: bool,
        tx: mpsc::Sender<WatchEvent>,
        modules: Vec<AndroidMemoryModule>,
        all_regions: Vec<AndroidMemoryRegion>,
        is_android: bool,
        emu_name: Option<String>,
        delta: isize,
        host_modules: Vec<(String, usize, usize)>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let restored = Arc::new(AtomicBool::new(false));
        let patches = Arc::new(parking_lot::Mutex::new(Vec::new()));

        let stop_clone = Arc::clone(&stop);
        let restored_clone = Arc::clone(&restored);
        let patches_clone = Arc::clone(&patches);

        let handle = thread::spawn(move || {
            let _ = tx.send(WatchEvent::Started {
                armed_threads: 1,
                total_threads: 1,
            });

            // 1. Scan & install runtime filter hooks for candidate instructions
            struct ActiveHookMeta {
                cand: CandidateInfo,
                counter_slot_idx: usize,
                last_hits: u64,
            }
            let mut active_hooks: Vec<ActiveHookMeta> = Vec::new();
            let mut data_cave_base = 0usize;
            let mut data_cave_size = 0usize;

            if let Some(&(target_gva, target_hva)) = watch_targets.first() {
                let outcome = scan_verified_xref_candidates(
                    pid,
                    target_gva,
                    target_hva,
                    writes_only,
                    delta,
                    &modules,
                    &all_regions,
                    &host_modules,
                );
                let total_cands = outcome.candidates.len();
                let mod_count = outcome.mod_count;
                let off_preview = if outcome.cand_offsets.is_empty() {
                    "none".to_string()
                } else {
                    outcome
                        .cand_offsets
                        .iter()
                        .take(5)
                        .map(|o| format!("0x{o:X}"))
                        .collect::<Vec<_>>()
                        .join(",")
                };

                let mut exec_avail = 0usize;
                let mut data_avail = 0usize;
                let use_exact_gva = delta != 0
                    && target_gva != target_hva
                    && (all_regions.is_empty()
                        || all_regions
                            .iter()
                            .any(|r| target_gva >= r.start && target_gva < r.end));
                let mode_label = if use_exact_gva {
                    "ExactGva"
                } else {
                    "PageOffset(&FFF)"
                };

                if !outcome.candidates.is_empty() {
                    let mut caves = CaveAllocator::discover(
                        pid,
                        outcome.candidates[0].inst_hva,
                        &modules,
                        &all_regions,
                        delta,
                        &outcome.used_elfs,
                    );
                    exec_avail = caves.total_exec_bytes();
                    data_avail = caves.total_data_bytes();

                    let max_slots = caves.max_data_cave_len().saturating_sub(8) / 8;
                    let num_cands = outcome.candidates.len().min(48).min(max_slots);
                    let needed_data = 8 + num_cands * 8;
                    if num_cands > 0 {
                        if let Some(d_cave) = caves.alloc_data(needed_data) {
                        data_cave_base = d_cave;
                        data_cave_size = needed_data;
                        let mut init_data = vec![0u8; needed_data];
                        init_data[0..8].copy_from_slice(&(target_gva as u64).to_le_bytes());
                        if write_code_bytes(pid, d_cave, &init_data).is_ok() {
                            patches_clone.lock().push(PatchedSite {
                                hva: d_cave,
                                original_bytes: vec![0u8; needed_data],
                            });

                            let match_mode = if use_exact_gva {
                                FilterMatchMode::ExactGva {
                                    target_gva_slot_va: d_cave,
                                }
                            } else {
                                FilterMatchMode::PageOffset {
                                    page_off: (target_hva & 0xFFF) as u32,
                                }
                            };

                            for (idx, cand) in outcome.candidates.into_iter().take(num_cands).enumerate()
                            {
                                if stop_clone.load(Ordering::Relaxed) {
                                    break;
                                }
                                let Some((cave_idx, c_cave)) = caves.peek_exec(96) else {
                                    break;
                                };
                                let counter_va = d_cave + 8 + idx * 8;
                                if let Ok((stub, patch, orig)) = build_candidate_filter_hook_bytes(
                                    cand.inst_hva,
                                    &cand.code_bytes,
                                    c_cave,
                                    match_mode,
                                    counter_va,
                                ) {
                                    if write_code_bytes(pid, c_cave, &stub).is_ok()
                                        && write_code_bytes(pid, cand.inst_hva, &patch).is_ok()
                                    {
                                        caves.commit_exec(cave_idx, stub.len());
                                        {
                                            let mut p = patches_clone.lock();
                                            p.push(PatchedSite {
                                                hva: cand.inst_hva,
                                                original_bytes: orig,
                                            });
                                            p.push(PatchedSite {
                                                hva: c_cave,
                                                original_bytes: vec![0u8; stub.len()],
                                            });
                                        }
                                        active_hooks.push(ActiveHookMeta {
                                            cand,
                                            counter_slot_idx: idx,
                                            last_hits: 0,
                                        });
                                    }
                                }
                            }
                        }
                        }
                    }
                }

                let armed = active_hooks.len();
                let hint = if mod_count == 0 {
                    " | No ELF/x64 modules found in target"
                } else if total_cands == 0 {
                    " | 0 matching instructions for probed offsets"
                } else if armed == 0 {
                    " | 0 hooks armed (no usable code/data cave near target)"
                } else {
                    ""
                };
                let _ = tx.send(WatchEvent::Status(format!(
                    "Hooks: {armed}/{total_cands} | Mods: {mod_count} | Offsets: [{off_preview}] | Caves: {exec_avail}B/{data_avail}B | Mode: {mode_label}{hint}"
                )));
            }

            // 2. Poll target memory changes + real-time hook counters
            let mut prev_values: HashMap<usize, Vec<u8>> = HashMap::new();
            let mut mutation_counts: HashMap<usize, usize> = HashMap::new();
            for &(_, hva) in &watch_targets {
                if let Ok(bytes) = read_memory_bytes(pid, hva, 8) {
                    prev_values.insert(hva, bytes);
                }
            }

            while !stop_clone.load(Ordering::Acquire) {
                // Poll real-time candidate counters
                if !active_hooks.is_empty() && data_cave_base != 0 {
                    if let Ok(counters_buf) = read_memory_bytes(pid, data_cave_base, data_cave_size) {
                        for hook in &mut active_hooks {
                            let off = 8 + hook.counter_slot_idx * 8;
                            if off + 8 <= counters_buf.len() {
                                let count = u64::from_le_bytes(
                                    counters_buf[off..off + 8].try_into().unwrap_or_default(),
                                );
                                if count > hook.last_hits {
                                    let delta_hits = (count - hook.last_hits).min(16);
                                    hook.last_hits = count;
                                    let details = format!(
                                        "Real-Time Captured Instruction!\nInstruction: {}\nCode Address: {} (HVA: 0x{:X})\nField Offset: 0x{:X}\nLive Hits: {}\nClick 'Add to code list' to use 'Watch Writes' after game restarts!",
                                        hook.cand.asm_text,
                                        hook.cand.mod_desc,
                                        hook.cand.inst_hva,
                                        hook.cand.field_offset,
                                        count
                                    );
                                    for _ in 0..delta_hits {
                                        let _ = tx.send(WatchEvent::AddressHit {
                                            instruction_address: hook.cand.inst_hva,
                                            instruction: format!(
                                                "{}  ({})",
                                                hook.cand.asm_text, hook.cand.mod_desc
                                            ),
                                            details: details.clone(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                // Poll target address value mutations
                for &(display_addr, hva) in &watch_targets {
                    let Ok(cur_bytes) = read_memory_bytes(pid, hva, 8) else {
                        continue;
                    };
                    let changed = match prev_values.get(&hva) {
                        Some(prev) => prev != &cur_bytes,
                        None => true,
                    };
                    if changed {
                        let prev = prev_values
                            .get(&hva)
                            .cloned()
                            .unwrap_or_else(|| cur_bytes.clone());
                        prev_values.insert(hva, cur_bytes.clone());

                        let prev_u32 = u32::from_le_bytes(prev[..4].try_into().unwrap_or_default());
                        let cur_u32 =
                            u32::from_le_bytes(cur_bytes[..4].try_into().unwrap_or_default());
                        let prev_f32 =
                            f32::from_le_bytes(prev[..4].try_into().unwrap_or_default());
                        let cur_f32 =
                            f32::from_le_bytes(cur_bytes[..4].try_into().unwrap_or_default());

                        let diff_i64 = cur_u32 as i64 - prev_u32 as i64;
                        let is_float_likely = (cur_f32 - prev_f32).abs() > 0.0001
                            && cur_f32.is_finite()
                            && prev_f32.is_finite()
                            && cur_f32.abs() < 1e7
                            && prev_f32.abs() < 1e7;

                        let summary = if is_float_likely {
                            format!(
                                "Write: {:.2} -> {:.2} (Diff: {:.2})",
                                prev_f32,
                                cur_f32,
                                cur_f32 - prev_f32
                            )
                        } else {
                            format!("Write: {} -> {} (Diff: {diff_i64:+})", prev_u32, cur_u32)
                        };

                        let is_valid_gva = display_addr <= 0x0000_7FFF_FFFF_FFFF;
                        let mod_desc = if is_android && is_valid_gva {
                            modules
                                .iter()
                                .find(|m| {
                                    display_addr >= m.base_address && display_addr < m.end_address
                                })
                                .map(|m| {
                                    format!("{}+0x{:X}", m.name, display_addr - m.base_address)
                                })
                                .or_else(|| {
                                    all_regions
                                        .iter()
                                        .find(|r| display_addr >= r.start && display_addr < r.end)
                                        .map(|r| {
                                            format!("{}+0x{:X}", r.pathname, display_addr - r.start)
                                        })
                                })
                                .unwrap_or_else(|| format!("GVA: 0x{display_addr:08X}"))
                        } else if is_android {
                            "Heap / Dynamic Guest Memory".to_string()
                        } else {
                            format!(
                                "Emulator Memory ({})",
                                emu_name.as_deref().unwrap_or("VM")
                            )
                        };

                        let raw_hex = cur_bytes[..4]
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        let target_desc = if is_android && is_valid_gva {
                            format!("Target GVA: 0x{display_addr:08X} (Host HVA: 0x{hva:08X})")
                        } else if is_android {
                            format!("Host HVA: 0x{hva:08X} (Android Guest Heap)")
                        } else {
                            format!("Host HVA: 0x{hva:08X} (Emulator RAM)")
                        };
                        let details = format!(
                            "{target_desc}\nModule: {mod_desc}\nOld Value: {prev_u32} (0x{prev_u32:X}, f32: {prev_f32:.2})\nNew Value: {cur_u32} (0x{cur_u32:X}, f32: {cur_f32:.2})\nRaw Bytes: {raw_hex}"
                        );

                        let count_entry = mutation_counts.entry(hva).or_insert(0);
                        *count_entry += 1;
                        let count = *count_entry;

                        let _ = tx.send(WatchEvent::ValueMutation {
                            target_address: hva,
                            summary,
                            details,
                            count,
                        });
                    }
                }

                thread::sleep(Duration::from_millis(15));
            }

            restore_patches(pid, &patches_clone, &restored_clone);
            let _ = tx.send(WatchEvent::Stopped);
        });

        Self {
            stop,
            restored,
            pid,
            patches,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        restore_patches(self.pid, &self.patches, &self.restored);
    }
}

impl Drop for EmulatorInstructionWatch {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all_intel(bytes: &[u8], base_ip: usize) -> Vec<String> {
        let mut dec = Decoder::with_ip(64, bytes, base_ip as u64, DecoderOptions::NONE);
        let mut out = Vec::new();
        while dec.can_decode() {
            let instr = dec.decode();
            out.push(format_instruction_intel(&instr));
        }
        out
    }

    #[test]
    fn test_build_code_access_hook_for_vmovdqa_and_mov() {
        // vmovdqa [rbp+2D0h], xmm1 (8 bytes)
        let vmovdqa_bytes = [0xC5, 0xF9, 0x7F, 0x8D, 0xD0, 0x02, 0x00, 0x00, 0x90, 0x90];
        let inst_hva = 0x17099A46B74usize;
        let code_cave = 0x1709A000000usize;
        let data_cave = 0x1709A500000usize;

        let (stub, jmp_patch, orig) =
            build_code_access_hook_bytes(inst_hva, &vmovdqa_bytes, code_cave, data_cave).unwrap();
        assert_eq!(orig.len(), 8);
        assert_eq!(jmp_patch.len(), 8);
        assert_eq!(jmp_patch[0], 0xE9);

        let lines = decode_all_intel(&stub, code_cave);
        let joined = lines.join("\n");
        assert!(joined.contains("pushfq"), "{joined}");
        assert!(joined.contains("lea rax,[rbp+2D0h]"), "{joined}");
        assert!(joined.contains("lock xadd"), "{joined}");
        assert!(joined.contains("vmovdqa [rbp+2D0h],xmm1"), "{joined}");
        assert!(joined.contains("jmp"), "{joined}");

        // mov [r15+2E0h], eax (7 bytes)
        let mov_bytes = [0x41, 0x89, 0x87, 0xE0, 0x02, 0x00, 0x00, 0xC3];
        let (stub2, jmp2, orig2) = build_code_access_hook_bytes(
            0x17195725855,
            &mov_bytes,
            0x17196000000,
            0x17196500000,
        )
        .unwrap();
        assert_eq!(orig2.len(), 7);
        assert_eq!(jmp2.len(), 7);
        let lines2 = decode_all_intel(&stub2, 0x17196000000).join("\n");
        assert!(lines2.contains("lea rax,[r15+2E0h]"), "{lines2}");
        assert!(lines2.contains("mov [r15+2E0h],eax"), "{lines2}");
    }

    #[test]
    fn test_build_code_access_hook_adjusts_rsp_displacement() {
        // mov eax, [rsp+18h] (4 bytes) + nop; nop (2 bytes) => steals 5+ bytes
        let bytes = [0x8B, 0x44, 0x24, 0x18, 0x90, 0x90];
        let inst_hva = 0x17000010000usize;
        let code_cave = 0x17000200000usize;
        let data_cave = 0x17000300000usize;

        let (stub, jmp_patch, orig) =
            build_code_access_hook_bytes(inst_hva, &bytes, code_cave, data_cave).unwrap();
        assert_eq!(orig.len(), 5);
        assert_eq!(jmp_patch.len(), 5);

        let lines = decode_all_intel(&stub, code_cave).join("\n");
        // Red zone (128 = 0x80) + 4 pushes (32 = 0x20) = 0xA0 => 0x18 + 0xA0 = 0xB8
        assert!(lines.contains("lea rsp,[rsp-80h]"), "{lines}");
        assert!(lines.contains("lea rax,[rsp+0B8h]"), "{lines}");
        assert!(lines.contains("lea rsp,[rsp+80h]"), "{lines}");
        // Original instruction runs after pops so still [rsp+18h]
        assert!(lines.contains("mov eax,[rsp+18h]"), "{lines}");
    }

    #[test]
    fn test_build_candidate_filter_hook_bytes() {
        // mov [r15+2E0h], eax
        let mov_bytes = [0x41, 0x89, 0x87, 0xE0, 0x02, 0x00, 0x00, 0xC3];
        let inst_hva = 0x17195725855usize;
        let code_cave = 0x17196000000usize;
        let target_slot = 0x17196500000usize;
        let counter_slot = 0x17196500008usize;

        let (stub, patch, orig) = build_candidate_filter_hook_bytes(
            inst_hva,
            &mov_bytes,
            code_cave,
            FilterMatchMode::ExactGva {
                target_gva_slot_va: target_slot,
            },
            counter_slot,
        )
        .unwrap();
        assert_eq!(orig.len(), 7);
        assert_eq!(patch[0], 0xE9);

        let lines = decode_all_intel(&stub, code_cave).join("\n");
        assert!(lines.contains("lea rsp,[rsp-80h]"), "{lines}");
        assert!(lines.contains("lea rax,[r15+2E0h]"), "{lines}");
        assert!(lines.contains("sub rdx,rax"), "{lines}");
        assert!(lines.contains("cmp rdx,4"), "{lines}");
        assert!(lines.contains("jae"), "{lines}");
        assert!(lines.contains("lock inc"), "{lines}");
        assert!(lines.contains("17196500008h"), "{lines}");
        assert!(lines.contains("lea rsp,[rsp+80h]"), "{lines}");
        assert!(lines.contains("mov [r15+2E0h],eax"), "{lines}");
    }

    #[test]
    fn test_verify_candidate_rejects_mid_instruction_and_accepts_synced_code() {
        // Padding + push rbp; mov rbp, rsp; movabs rax, 0x00000550_8789_0011; ret (where mid-bytes 89 87 50 05 00 00 look like mov [rdi+550h], eax!)
        let mut buf = vec![0xCCu8; 16];
        // Real prologue + movabs rax, 0x0000055087890011 + mov [r15+550h], eax + add eax,1 + ret + int3 padding
        let fake_movabs: [u8; 14] = [
            0x55, 0x48, 0x89, 0xE5, // push rbp; mov rbp, rsp
            0x48, 0xB8, 0x11, 0x00, 0x89, 0x87, 0x50, 0x05, 0x00, 0x00, // movabs rax, ... (50 05 00 00 at offset +10 inside imm64!)
        ];
        buf.extend_from_slice(&fake_movabs);
        let imm_match_idx = 16 + 10;
        let real_inst_off = buf.len();
        buf.extend_from_slice(&[
            0x41, 0x89, 0x87, 0x50, 0x05, 0x00, 0x00, // mov [r15+550h], eax (disp32 at +3)
            0x83, 0xC0, 0x01,                         // add eax, 1
            0x5D,                                     // pop rbp
            0xC3,                                     // ret
        ]);
        buf.extend_from_slice(&vec![0xCCu8; 32]);

        let mut factory = InstructionInfoFactory::new();
        let mut seen = HashSet::new();
        let base = 0x1000_0000usize;

        // 1. Mid-instruction 50 05 00 00 inside movabs MUST be rejected!
        let bad = verify_candidate_in_buffer(
            &buf,
            base,
            imm_match_idx,
            0x550,
            true,
            0,
            base,
            "libil2cpp.so",
            &mut factory,
            &mut seen,
        );
        assert!(bad.is_none(), "mid-instruction immediate must never be hooked");

        // 2. Real synchronized mov [r15+550h], eax MUST be accepted!
        let real_match_idx = real_inst_off + 3;
        let good = verify_candidate_in_buffer(
            &buf,
            base,
            real_match_idx,
            0x550,
            true,
            0,
            base,
            "libil2cpp.so",
            &mut factory,
            &mut seen,
        )
        .expect("real synced instruction should be accepted");
        assert_eq!(good.inst_hva, base + real_inst_off);
        assert!(good.asm_text.contains("r15+550h"), "{}", good.asm_text);
    }

    #[test]
    fn test_build_candidate_filter_hook_page_offset_mode() {
        let mov_bytes = [0x41, 0x89, 0x87, 0xE0, 0x02, 0x00, 0x00, 0xC3];
        let inst_hva = 0x17195725855usize;
        let code_cave = 0x17196000000usize;
        let counter_slot = 0x17196500008usize;

        let (stub, patch, orig) = build_candidate_filter_hook_bytes(
            inst_hva,
            &mov_bytes,
            code_cave,
            FilterMatchMode::PageOffset { page_off: 0x2E0 },
            counter_slot,
        )
        .unwrap();
        assert_eq!(orig.len(), 7);
        assert_eq!(patch[0], 0xE9);

        let lines = decode_all_intel(&stub, code_cave).join("\n");
        assert!(lines.contains("and eax,0FFFh"), "{lines}");
        assert!(lines.contains("mov edx,2E0h"), "{lines}");
        assert!(lines.contains("sub edx,eax"), "{lines}");
        assert!(lines.contains("cmp edx,4"), "{lines}");
        assert!(lines.contains("jae"), "{lines}");
        assert!(lines.contains("lock inc"), "{lines}");
        assert!(lines.contains("mov [r15+2E0h],eax"), "{lines}");
    }

    #[test]
    fn test_parse_elf64_x64_caves() {
        let mut hdr = vec![0u8; 4096];
        hdr[0..4].copy_from_slice(&ELF_MAGIC);
        hdr[4] = ELFCLASS64;
        hdr[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
        hdr[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff
        hdr[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        hdr[56..58].copy_from_slice(&2u16.to_le_bytes());  // e_phnum

        // Phdr 0: PF_R | PF_X, vaddr = 0, memsz = 0x10200 (leaves ~0xDE0 bytes to 0x11000)
        let ph0 = &mut hdr[64..120];
        ph0[0..4].copy_from_slice(&PT_LOAD.to_le_bytes());
        ph0[4..8].copy_from_slice(&(PF_X | 4).to_le_bytes());
        ph0[16..24].copy_from_slice(&0u64.to_le_bytes());
        ph0[40..48].copy_from_slice(&0x10200u64.to_le_bytes());

        // Phdr 1: PF_R | PF_W, vaddr = 0x20000, memsz = 0x5100 (leaves ~0xEF0 bytes to 0x26000)
        let ph1 = &mut hdr[120..176];
        ph1[0..4].copy_from_slice(&PT_LOAD.to_le_bytes());
        ph1[4..8].copy_from_slice(&(PF_W | 4).to_le_bytes());
        ph1[16..24].copy_from_slice(&0x20000u64.to_le_bytes());
        ph1[40..48].copy_from_slice(&0x5100u64.to_le_bytes());

        let base = 0x1000_0000usize;
        let caves = parse_elf64_x64_caves("libil2cpp.so", base, &hdr);
        assert_eq!(caves.len(), 2);
        assert!(caves[0].is_exec);
        assert_eq!(caves[0].start_hva, base + 0x10200);
        assert_eq!(caves[0].len, 0xE00);
        assert!(caves[1].is_write);
        assert_eq!(caves[1].start_hva, base + 0x25100);
        assert_eq!(caves[1].len, 0xF00);
    }
}
