use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::c_void,
    io,
    mem::{MaybeUninit, size_of},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::model::MemoryValueType;

const PROCESS_VM_READ: u32 = 0x0010;
const PROCESS_VM_WRITE: u32 = 0x0020;
const PROCESS_VM_OPERATION: u32 = 0x0008;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
const MEM_COMMIT: u32 = 0x1000;
const MEM_PRIVATE: u32 = 0x0002_0000;
const MEM_MAPPED: u32 = 0x0004_0000;
const MEM_IMAGE: u32 = 0x0100_0000;
const PAGE_READONLY: u32 = 0x02;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_WRITECOPY: u32 = 0x08;
const PAGE_EXECUTE_READ: u32 = 0x20;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;
const PAGE_GUARD: u32 = 0x100;
const SCAN_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const SCAN_BUCKET_BYTES: usize = 64 * 1024 * 1024;
const PAGE_BYTES: usize = 4096;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(target_arch = "x86_64")]
const AVX_CMP_LE_OQ: i32 = 0x12;
#[cfg(target_arch = "x86_64")]
const AVX_CMP_GE_OQ: i32 = 0x1d;
#[cfg(target_arch = "x86_64")]
const AVX_CMP_GT_OQ: i32 = 0x1e;

#[repr(C)]
struct MemoryBasicInformation {
    base_address: *mut c_void,
    allocation_base: *mut c_void,
    allocation_protect: u32,
    partition_id: u16,
    _padding: u16,
    region_size: usize,
    state: u32,
    protect: u32,
    kind: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct WorkingSetExInformation {
    virtual_address: *mut c_void,
    virtual_attributes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScanValueType {
    I8,
    I16,
    #[default]
    I32,
    F32,
    I64,
    F64,
}

#[derive(Clone)]
pub struct MemoryScanOptions {
    pub writable: bool,
    pub executable: bool,
    pub copy_on_write: bool,
    pub active_memory_only: bool,
    pub mem_private: bool,
    pub mem_image: bool,
    pub mem_mapped: bool,
    pub alignment: Option<usize>,
    pub target_ranges: Option<Arc<Vec<(usize, usize)>>>,
    pub isolate_guest_ram: bool,
    pub vm_allocation_base: Option<usize>,
}

impl Default for MemoryScanOptions {
    fn default() -> Self {
        Self {
            writable: true,
            executable: false,
            copy_on_write: false,
            active_memory_only: true,
            mem_private: true,
            mem_image: true,
            mem_mapped: false,
            alignment: Some(4),
            target_ranges: None,
            isolate_guest_ram: false,
            vm_allocation_base: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextScanCandidate {
    pub address: usize,
    pub previous: String,
    pub current: String,
}

#[derive(Clone, Copy, Debug)]
pub struct MemoryRegionInfo {
    pub allocation_base: usize,
    pub base: usize,
    pub size: usize,
    pub protect: u32,
}

impl ScanValueType {
    pub const fn width(self) -> usize {
        match self {
            Self::I8 => 1,
            Self::I16 => 2,
            Self::I32 | Self::F32 => 4,
            Self::I64 | Self::F64 => 8,
        }
    }

    pub fn decode(self, bytes: &[u8]) -> Option<ScanValue> {
        Some(match self {
            Self::I8 => ScanValue::I8(i8::from_le_bytes(bytes.get(..1)?.try_into().ok()?)),
            Self::I16 => ScanValue::I16(i16::from_le_bytes(bytes.get(..2)?.try_into().ok()?)),
            Self::I32 => ScanValue::I32(i32::from_le_bytes(bytes.get(..4)?.try_into().ok()?)),
            Self::F32 => ScanValue::F32(f32::from_le_bytes(bytes.get(..4)?.try_into().ok()?)),
            Self::I64 => ScanValue::I64(i64::from_le_bytes(bytes.get(..8)?.try_into().ok()?)),
            Self::F64 => ScanValue::F64(f64::from_le_bytes(bytes.get(..8)?.try_into().ok()?)),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScanValue {
    I8(i8),
    I16(i16),
    I32(i32),
    F32(f32),
    I64(i64),
    F64(f64),
}

impl ScanValue {
    #[inline]
    pub fn as_i128(self) -> Option<i128> {
        match self {
            Self::I8(v) => Some(v as i128),
            Self::I16(v) => Some(v as i128),
            Self::I32(v) => Some(v as i128),
            Self::I64(v) => Some(v as i128),
            _ => None,
        }
    }

    #[inline]
    pub fn as_f64(self) -> f64 {
        match self {
            Self::I8(v) => v as f64,
            Self::I16(v) => v as f64,
            Self::I32(v) => v as f64,
            Self::I64(v) => v as f64,
            Self::F32(v) => v as f64,
            Self::F64(v) => v,
        }
    }

    pub const fn value_type(self) -> ScanValueType {
        match self {
            Self::I8(_) => ScanValueType::I8,
            Self::I16(_) => ScanValueType::I16,
            Self::I32(_) => ScanValueType::I32,
            Self::F32(_) => ScanValueType::F32,
            Self::I64(_) => ScanValueType::I64,
            Self::F64(_) => ScanValueType::F64,
        }
    }

    fn bytes(self) -> [u8; 8] {
        let mut bytes = [0; 8];
        match self {
            Self::I8(value) => bytes[..1].copy_from_slice(&value.to_le_bytes()),
            Self::I16(value) => bytes[..2].copy_from_slice(&value.to_le_bytes()),
            Self::I32(value) => bytes[..4].copy_from_slice(&value.to_le_bytes()),
            Self::F32(value) => bytes[..4].copy_from_slice(&value.to_le_bytes()),
            Self::I64(value) => bytes.copy_from_slice(&value.to_le_bytes()),
            Self::F64(value) => bytes.copy_from_slice(&value.to_le_bytes()),
        }
        bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanCandidate {
    pub address: usize,
    current: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityPointerMatch {
    pub location: usize,
    pub entity_index: usize,
    pub entity_base: usize,
}

#[derive(Clone, Debug)]
pub struct EntityListCandidate {
    pub address: usize,
    pub end_address: usize,
    pub pointer_matches: Vec<EntityPointerMatch>,
    pub matched_entities: usize,
    pub coverage: f32,
    pub observed_stride: usize,
    pub spacing_regularity: f32,
    pub readable_pointers: usize,
    pub plausible_xyz: usize,
    pub confidence: f32,
}

#[derive(Clone, Debug)]
pub struct EntityListScanResult {
    pub candidates: Vec<EntityListCandidate>,
    pub pointer_hits: usize,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub struct EntityListPreview {
    pub slot_address: usize,
    pub entity_base: Option<usize>,
    pub matched_input: Option<usize>,
    pub readable: bool,
    pub xyz: Option<[f32; 3]>,
}

#[derive(Clone, Debug)]
pub struct EntityListValidation {
    pub entries: Vec<EntityListPreview>,
    pub matched_entities: usize,
    pub readable_pointers: usize,
    pub plausible_xyz: usize,
}

impl ScanCandidate {
    pub(crate) fn new(address: usize, current: ScanValue) -> Self {
        Self {
            address,
            current: u64::from_le_bytes(current.bytes()),
        }
    }

    #[inline(always)]
    pub(crate) fn new_i32(address: usize, val: i32) -> Self {
        Self {
            address,
            current: val as u32 as u64,
        }
    }

    #[inline(always)]
    pub(crate) fn new_f32(address: usize, val: f32) -> Self {
        Self {
            address,
            current: val.to_bits() as u64,
        }
    }

    #[inline(always)]
    pub(crate) fn new_i64(address: usize, val: i64) -> Self {
        Self {
            address,
            current: val as u64,
        }
    }

    #[inline(always)]
    pub(crate) fn new_f64(address: usize, val: f64) -> Self {
        Self {
            address,
            current: val.to_bits(),
        }
    }

    #[inline(always)]
    pub(crate) fn new_i16(address: usize, val: i16) -> Self {
        Self {
            address,
            current: val as u16 as u64,
        }
    }

    #[inline(always)]
    pub(crate) fn new_i8(address: usize, val: i8) -> Self {
        Self {
            address,
            current: val as u8 as u64,
        }
    }

    #[inline(always)]
    pub fn get_i32(self) -> i32 {
        self.current as u32 as i32
    }

    #[inline(always)]
    pub fn get_f32(self) -> f32 {
        f32::from_bits(self.current as u32)
    }

    #[inline(always)]
    pub fn get_i64(self) -> i64 {
        self.current as i64
    }

    #[inline(always)]
    pub fn get_f64(self) -> f64 {
        f64::from_bits(self.current)
    }

    #[inline(always)]
    pub fn get_i16(self) -> i16 {
        self.current as u16 as i16
    }

    #[inline(always)]
    pub fn get_i8(self) -> i8 {
        self.current as u8 as i8
    }

    pub fn current(self, value_type: ScanValueType) -> ScanValue {
        value_type
            .decode(&self.current.to_le_bytes())
            .expect("stored scan value width")
    }

    pub fn set_current(&mut self, current: ScanValue) {
        self.current = u64::from_le_bytes(current.bytes());
    }
}

const _: () = assert!(std::mem::size_of::<ScanCandidate>() == 16);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PointerPath {
    pub module: String,
    pub module_offset: usize,
    pub offsets: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct PointerPathComparison {
    pub exact: Vec<PointerPath>,
    pub entity_roots: Vec<PointerPath>,
}

const MAX_POINTER_PATH_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct EntityRootKey {
    module_id: u32,
    module_offset: usize,
    depth: u8,
    offsets: [usize; MAX_POINTER_PATH_DEPTH],
}

pub fn compare_pointer_paths(
    paths_a: impl IntoIterator<Item = PointerPath>,
    paths_b: impl IntoIterator<Item = PointerPath>,
    entity_stride: usize,
    result_limit: usize,
) -> PointerPathComparison {
    let paths_a = paths_a.into_iter().collect::<Vec<_>>();
    let paths_b = paths_b.into_iter().collect::<HashSet<_>>();
    let mut exact = Vec::new();
    let mut seen_exact = HashSet::new();
    for path in &paths_a {
        if paths_b.contains(path) && seen_exact.insert(path.clone()) {
            exact.push(path.clone());
            if exact.len() == result_limit {
                break;
            }
        }
    }

    let entity_roots = compare_entity_roots(&paths_a, &paths_b, entity_stride, result_limit);

    PointerPathComparison {
        exact,
        entity_roots,
    }
}

fn compare_entity_roots(
    paths_a: &[PointerPath],
    paths_b: &HashSet<PointerPath>,
    entity_stride: usize,
    result_limit: usize,
) -> Vec<PointerPath> {
    if entity_stride == 0 || result_limit == 0 {
        return Vec::new();
    }

    let mut module_ids = HashMap::<String, u32>::new();
    for path in paths_a.iter().chain(paths_b.iter()) {
        let next_id = module_ids.len() as u32;
        module_ids.entry(path.module.clone()).or_insert(next_id);
    }

    let max_depth = paths_a
        .iter()
        .chain(paths_b.iter())
        .map(|path| path.offsets.len())
        .max()
        .unwrap_or(0)
        .min(MAX_POINTER_PATH_DEPTH);
    let mut roots = Vec::new();
    let mut seen = HashSet::new();

    // ponytail: entity slots can appear at any pointer level, but only one slot
    // is allowed to vary. This keeps comparison linear per depth and avoids the
    // false roots produced by modulo-normalizing every structural offset.
    for varying_index in 0..max_depth {
        let normalized_b = paths_b
            .iter()
            .filter_map(|path| entity_root_key(path, varying_index, entity_stride, &module_ids))
            .collect::<HashSet<_>>();

        for path in paths_a {
            if paths_b.contains(path) {
                continue;
            }
            let Some(key) = entity_root_key(path, varying_index, entity_stride, &module_ids) else {
                continue;
            };
            if !normalized_b.contains(&key) {
                continue;
            }
            let mut root = path.clone();
            root.offsets[varying_index] %= entity_stride;
            if seen.insert(root.clone()) {
                roots.push(root);
                if roots.len() == result_limit {
                    return roots;
                }
            }
        }
    }
    roots
}

fn entity_root_key(
    path: &PointerPath,
    varying_index: usize,
    entity_stride: usize,
    module_ids: &HashMap<String, u32>,
) -> Option<EntityRootKey> {
    if path.offsets.len() > MAX_POINTER_PATH_DEPTH || varying_index >= path.offsets.len() {
        return None;
    }
    let mut offsets = [0; MAX_POINTER_PATH_DEPTH];
    offsets[..path.offsets.len()].copy_from_slice(&path.offsets);
    offsets[varying_index] %= entity_stride;
    Some(EntityRootKey {
        module_id: *module_ids.get(&path.module)?,
        module_offset: path.module_offset,
        depth: path.offsets.len() as u8,
        offsets,
    })
}

#[derive(Clone, Debug)]
pub struct PointerMap {
    pointers: Vec<(usize, usize)>,
    modules: Vec<(String, usize, usize)>,
}

#[derive(Clone, Debug)]
pub struct ViewProjectionCandidate {
    pub address: usize,
    pub matrix: [f32; 16],
    pub score: f32,
}

pub fn scan_view_projection_candidates(
    pid: u32,
    max_bytes: usize,
    result_limit: usize,
    progress: Arc<AtomicUsize>,
) -> io::Result<Vec<ViewProjectionCandidate>> {
    let process = ScanProcess::open(pid, false)?;
    let options = MemoryScanOptions {
        writable: true,
        executable: false,
        copy_on_write: false,
        active_memory_only: true,
        mem_private: true,
        mem_image: true,
        mem_mapped: true,
        alignment: Some(4),
        target_ranges: None,
        isolate_guest_ram: false,
        vm_allocation_base: None,
    };
    let mut candidates = Vec::new();
    let mut remaining = max_bytes;
    let mut buffer = vec![0u8; SCAN_CHUNK_BYTES + 64];
    'regions: for region in scan_regions_for(&process, options) {
        let region_size = region.size.min(remaining);
        for offset in (0..region_size).step_by(SCAN_CHUNK_BYTES) {
            let address = region.base + offset;
            let wanted = (region_size - offset).min(SCAN_CHUNK_BYTES);
            remaining = remaining.saturating_sub(wanted);
            let Ok(read) = process.read(address, &mut buffer[..wanted]) else {
                if remaining == 0 {
                    break 'regions;
                }
                continue;
            };
            if read >= 64 {
                for byte_offset in (0..=read - 64).step_by(4) {
                    let mut matrix = [0.0f32; 16];
                    for (index, value) in matrix.iter_mut().enumerate() {
                        let start = byte_offset + index * 4;
                        *value = f32::from_le_bytes(buffer[start..start + 4].try_into().unwrap());
                    }
                    let Some(score) = view_projection_likelihood(&matrix) else {
                        continue;
                    };
                    candidates.push(ViewProjectionCandidate {
                        address: address + byte_offset,
                        matrix,
                        score,
                    });
                    if candidates.len() >= result_limit.max(1).saturating_mul(8) {
                        candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
                        candidates.truncate(result_limit.max(1).saturating_mul(4));
                    }
                }
            }
            progress.fetch_add(read, Ordering::Relaxed);
            if remaining == 0 {
                break 'regions;
            }
        }
    }
    candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
    candidates.truncate(result_limit.max(1));
    Ok(candidates)
}

fn view_projection_likelihood(matrix: &[f32; 16]) -> Option<f32> {
    if matrix
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1.0e7)
    {
        return None;
    }
    let non_zero = matrix.iter().filter(|value| value.abs() > 1.0e-6).count();
    if !(6..=16).contains(&non_zero) {
        return None;
    }
    // Must have a perspective / homogeneous divide component:
    let has_w = matrix[3].abs() > 1.0e-4
        || matrix[7].abs() > 1.0e-4
        || matrix[11].abs() > 1.0e-4
        || matrix[12].abs() > 1.0e-4
        || matrix[13].abs() > 1.0e-4
        || matrix[14].abs() > 1.0e-4
        || matrix[15].abs() > 1.0e-4;
    if !has_w {
        return None;
    }
    let moderate = matrix
        .iter()
        .filter(|value| value.abs() >= 1.0e-5 && value.abs() <= 10_000.0)
        .count() as f32;
    let zeros = (16 - non_zero) as f32;
    Some(moderate + zeros * 0.35)
}

/// Bounds for pointer scans. Keeping these explicit prevents a deep scan from
/// consuming unbounded memory when a target process has a very large address space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerScanLimits {
    pub max_offset: usize,
    pub max_depth: usize,
    pub result_limit: usize,
    pub max_bytes: usize,
    pub exhaustive: bool,
}

impl PointerScanLimits {
    pub const MAX_RESULT_LIMIT: usize = 250_000;

    pub const SAFE: Self = Self {
        max_offset: 0x1000,
        max_depth: 5,
        result_limit: 256,
        max_bytes: 1024 * 1024 * 1024,
        exhaustive: false,
    };

    pub const DEEP: Self = Self {
        max_offset: 0x4000,
        max_depth: 7,
        result_limit: 16_384,
        max_bytes: 2048 * 1024 * 1024,
        exhaustive: true,
    };

    pub const EXHAUSTIVE: Self = Self {
        max_offset: 0x4000,
        max_depth: 7,
        result_limit: 100_000,
        max_bytes: 4096 * 1024 * 1024,
        exhaustive: true,
    };
}


pub fn capture_pointer_map_with_budget(
    pid: u32,
    modules: &[(String, usize, usize)],
    pointer_width: usize,
    max_bytes: usize,
    progress: Arc<AtomicUsize>,
) -> io::Result<PointerMap> {
    capture_pointer_map_with_budget_cancel(pid, modules, pointer_width, max_bytes, progress, None, None)
}

fn capture_pointer_map_bucket(
    pid: u32,
    regions: Vec<ScanRegion>,
    readable_ranges: &[(usize, usize)],
    min_readable: usize,
    max_readable: usize,
    pointer_width: usize,
    emulator_delta: Option<isize>,
    progress: &AtomicUsize,
    cancel: Option<&AtomicBool>,
) -> io::Result<Vec<(usize, usize)>> {
    let process = ScanProcess::open(pid, false)?;
    let mut pointers = Vec::new();
    let _ = pointers.try_reserve(262_144);
    let mut buffer = vec![0u8; SCAN_CHUNK_BYTES];
    let mut last_hit_base = 0usize;
    let mut last_hit_end = 0usize;
    let buf_ptr = buffer.as_ptr();

    'regions: for region in regions {
        if cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
            break;
        }
        let mut offset = 0;
        while offset < region.size {
            if cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
                break 'regions;
            }
            let address = region.base + offset;
            let wanted = (region.size - offset).min(SCAN_CHUNK_BYTES);
            let Ok(read) = process.read(address, &mut buffer[..wanted]) else {
                offset += wanted;
                continue;
            };
            if read < pointer_width {
                offset += wanted;
                continue;
            }
            if pointer_width == 8 {
                let chunks = read / 8;
                for i in 0..chunks {
                    let value = unsafe { (buf_ptr.add(i * 8) as *const u64).read_unaligned() as usize };
                    let valid_addr = if value >= min_readable && value < max_readable {
                        if (value >= last_hit_base && value < last_hit_end) || {
                            let range = readable_ranges.partition_point(|(base, _)| *base <= value);
                            if range > 0 && value < readable_ranges[range - 1].1 {
                                last_hit_base = readable_ranges[range - 1].0;
                                last_hit_end = readable_ranges[range - 1].1;
                                true
                            } else {
                                false
                            }
                        } {
                            Some(value)
                        } else {
                            None
                        }
                    } else if let Some(delta) = emulator_delta {
                        let translated = (value as isize).wrapping_add(delta) as usize;
                        if translated >= min_readable && translated < max_readable {
                            let range = readable_ranges.partition_point(|(base, _)| *base <= translated);
                            if range > 0 && translated < readable_ranges[range - 1].1 {
                                Some(translated)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(target_ptr) = valid_addr {
                        if pointers.len() == pointers.capacity() {
                            let additional = pointers.capacity().max(500_000).min(4_000_000);
                            if pointers.try_reserve(additional).is_err() {
                                break 'regions;
                            }
                        }
                        pointers.push((target_ptr, address + i * 8));
                    }
                }
            } else {
                let chunks = read / 4;
                for i in 0..chunks {
                    let value = unsafe { (buf_ptr.add(i * 4) as *const u32).read_unaligned() as usize };
                    let valid_addr = if value >= min_readable && value < max_readable {
                        if (value >= last_hit_base && value < last_hit_end) || {
                            let range = readable_ranges.partition_point(|(base, _)| *base <= value);
                            if range > 0 && value < readable_ranges[range - 1].1 {
                                last_hit_base = readable_ranges[range - 1].0;
                                last_hit_end = readable_ranges[range - 1].1;
                                true
                            } else {
                                false
                            }
                        } {
                            Some(value)
                        } else {
                            None
                        }
                    } else if let Some(delta) = emulator_delta {
                        let translated = (value as isize).wrapping_add(delta) as usize;
                        if translated >= min_readable && translated < max_readable {
                            let range = readable_ranges.partition_point(|(base, _)| *base <= translated);
                            if range > 0 && translated < readable_ranges[range - 1].1 {
                                Some(translated)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(target_ptr) = valid_addr {
                        if pointers.len() == pointers.capacity() {
                            let additional = pointers.capacity().max(500_000).min(4_000_000);
                            if pointers.try_reserve(additional).is_err() {
                                break 'regions;
                            }
                        }
                        pointers.push((target_ptr, address + i * 4));
                    }
                }
            }
            progress.fetch_add(read, Ordering::Relaxed);
            offset += wanted;
        }
    }
    pointers.sort_unstable();
    Ok(pointers)
}

fn capture_pointer_map_with_budget_cancel(
    pid: u32,
    modules: &[(String, usize, usize)],
    pointer_width: usize,
    max_bytes: usize,
    progress: Arc<AtomicUsize>,
    cancel: Option<&AtomicBool>,
    emulator_delta: Option<isize>,
) -> io::Result<PointerMap> {
    if !matches!(pointer_width, 4 | 8) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pointer width must be 4 or 8 bytes",
        ));
    }
    crate::platform::set_current_thread_high_priority();
    let process = ScanProcess::open(pid, false)?;
    let mut regions = pointer_scan_regions_for(&process);
    // Module-backed regions are the most likely roots; scan them first when a
    // caller has selected a finite byte budget.
    regions.sort_by_key(|region| {
        !modules
            .iter()
            .any(|(_, base, size)| (*base..base.saturating_add(*size)).contains(&region.base))
    });
    let mut raw_ranges = regions
        .iter()
        .map(|region| (region.base, region.base.saturating_add(region.size)))
        .collect::<Vec<_>>();
    raw_ranges.sort_unstable_by_key(|(base, _)| *base);
    let mut readable_ranges: Vec<(usize, usize)> = Vec::with_capacity(raw_ranges.len());
    for (base, end) in raw_ranges {
        if let Some(last) = readable_ranges.last_mut() {
            if base <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        readable_ranges.push((base, end));
    }
    let min_readable = readable_ranges.first().map(|(base, _)| *base).unwrap_or(0);
    let max_readable = readable_ranges.iter().map(|(_, end)| *end).max().unwrap_or(0);

    let mut limited_regions = Vec::new();
    let mut remaining = max_bytes;
    for region in regions {
        if remaining == 0 {
            break;
        }
        let size = region.size.min(remaining);
        let mut offset = 0;
        while offset < size {
            let chunk_sz = (size - offset).min(SCAN_BUCKET_BYTES);
            limited_regions.push(ScanRegion {
                base: region.base + offset,
                size: chunk_sz,
            });
            offset += chunk_sz;
        }
        remaining -= size;
    }

    if limited_regions.is_empty() {
        return Ok(PointerMap {
            pointers: Vec::new(),
            modules: modules.to_vec(),
        });
    }

    let max_threads = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 32);
    let worker_count = max_threads.min(limited_regions.len().max(1));
    let total_bytes: usize = limited_regions.iter().map(|r| r.size).sum();
    let target_bytes = total_bytes.div_ceil(worker_count).max(1);
    let mut buckets = vec![Vec::new(); worker_count];
    let mut bucket_index = 0;
    let mut bucket_bytes = 0usize;
    for region in limited_regions {
        if bucket_bytes >= target_bytes && bucket_index + 1 < worker_count {
            bucket_index += 1;
            bucket_bytes = 0;
        }
        bucket_bytes = bucket_bytes.saturating_add(region.size);
        buckets[bucket_index].push(region);
    }

    let readable_ref = &readable_ranges;
    let progress_ref = &progress;
    let worker_results = thread::scope(|scope| {
        let mut handles = Vec::with_capacity(buckets.len());
        for worker_regions in buckets {
            handles.push(scope.spawn(move || {
                crate::platform::set_current_thread_high_priority();
                capture_pointer_map_bucket(
                    pid,
                    worker_regions,
                    readable_ref,
                    min_readable,
                    max_readable,
                    pointer_width,
                    emulator_delta,
                    progress_ref,
                    cancel,
                )
            }));
        }
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err(io::Error::other("pointer scan worker panicked"))))
            .collect::<io::Result<Vec<_>>>()
    })?;

    let total_pointers: usize = worker_results.iter().map(|v| v.len()).sum();
    let mut pointers = Vec::with_capacity(total_pointers);
    for worker_ptrs in worker_results {
        pointers.extend(worker_ptrs);
    }
    pointers.sort_unstable();

    Ok(PointerMap {
        pointers,
        modules: modules.to_vec(),
    })
}

pub fn parse_must_end_offsets(text: &str) -> Vec<usize> {
    text.split([',', ';', ' ', '\t', '\n', '\r'])
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let digits = part
                .strip_prefix("0x")
                .or_else(|| part.strip_prefix("0X"))
                .unwrap_or(part);
            usize::from_str_radix(digits, 16).ok()
        })
        .collect()
}

impl PointerMap {
    pub fn paths_to_with_options(
        &self,
        targets: &[usize],
        max_offset: usize,
        max_depth: usize,
        result_limit: usize,
        must_end_offsets: &[usize],
        exhaustive: bool,
    ) -> Vec<PointerPath> {
        find_pointer_paths_to_any(
            &self.pointers,
            targets,
            &self.modules,
            max_offset,
            max_depth,
            result_limit,
            false,
            must_end_offsets,
            exhaustive,
        )
    }

    #[cfg(test)]
    pub fn new_for_test(pointers: Vec<(usize, usize)>, modules: Vec<(String, usize, usize)>) -> Self {
        Self { pointers, modules }
    }
}


pub fn scan_pointer_map_and_paths_to_targets_must_end(
    pid: u32,
    targets: &[usize],
    modules: &[(String, usize, usize)],
    pointer_width: usize,
    max_offset: usize,
    max_depth: usize,
    result_limit: usize,
    max_bytes: usize,
    must_end_offsets: &[usize],
    progress: Arc<AtomicUsize>,
    exhaustive: bool,
    emulator_delta: Option<isize>,
) -> io::Result<(PointerMap, Vec<(usize, Vec<PointerPath>)>)> {
    let effective_bytes = if exhaustive {
        max_bytes.max(4096 * 1024 * 1024)
    } else {
        max_bytes
    };
    let map = capture_pointer_map_with_budget_cancel(
        pid,
        modules,
        pointer_width,
        effective_bytes,
        progress,
        None,
        emulator_delta,
    )?;
    let target_count = targets.len().max(1);
    let per_target_limit = if exhaustive {
        (result_limit.max(50_000) / target_count).clamp(2_000, 50_000)
    } else {
        result_limit
    };
    let paths = targets
        .iter()
        .copied()
        .map(|target| {
            let paths = map.paths_to_with_options(
                &[target],
                max_offset,
                max_depth,
                per_target_limit,
                must_end_offsets,
                exhaustive,
            );
            (target, paths)
        })
        .collect();
    Ok((map, paths))
}

pub fn scan_pointer_paths_with_budget_options(
    pid: u32,
    target: usize,
    modules: &[(String, usize, usize)],
    pointer_width: usize,
    max_offset: usize,
    max_depth: usize,
    result_limit: usize,
    max_bytes: usize,
    include_system_modules: bool,
    progress: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
) -> io::Result<Vec<PointerPath>> {
    scan_pointer_paths_with_budget_options_must_end(
        pid,
        target,
        modules,
        pointer_width,
        max_offset,
        max_depth,
        result_limit,
        max_bytes,
        include_system_modules,
        &[],
        progress,
        cancel,
    )
}

pub fn scan_pointer_paths_with_budget_options_must_end(
    pid: u32,
    target: usize,
    modules: &[(String, usize, usize)],
    pointer_width: usize,
    max_offset: usize,
    max_depth: usize,
    result_limit: usize,
    max_bytes: usize,
    include_system_modules: bool,
    must_end_offsets: &[usize],
    progress: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
) -> io::Result<Vec<PointerPath>> {
    let map = capture_pointer_map_with_budget_cancel(
        pid,
        modules,
        pointer_width,
        max_bytes,
        progress,
        Some(&cancel),
        None,
    )?;
    Ok(find_pointer_paths_to_any(
        &map.pointers,
        &[target],
        modules,
        max_offset,
        max_depth,
        result_limit,
        include_system_modules,
        must_end_offsets,
        false,
    ))
}

#[derive(Clone, Debug)]
struct FastModuleInfo {
    name: String,
    base: usize,
    end: usize,
    is_system: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SmallOffsets {
    len: u8,
    data: [usize; 16],
}

impl SmallOffsets {
    #[inline]
    fn new() -> Self {
        Self {
            len: 0,
            data: [0; 16],
        }
    }

    #[inline]
    fn pushed(mut self, offset: usize) -> Option<Self> {
        if (self.len as usize) < self.data.len() {
            self.data[self.len as usize] = offset;
            self.len += 1;
            Some(self)
        } else {
            None
        }
    }

    #[inline]
    fn to_reversed_vec(&self) -> Vec<usize> {
        let mut v = Vec::with_capacity(self.len as usize);
        for i in (0..self.len as usize).rev() {
            v.push(self.data[i]);
        }
        v
    }
}

fn find_pointer_paths_to_any(
    pointers: &[(usize, usize)],
    targets: &[usize],
    modules: &[(String, usize, usize)],
    max_offset: usize,
    max_depth: usize,
    result_limit: usize,
    include_system_modules: bool,
    must_end_offsets: &[usize],
    exhaustive: bool,
) -> Vec<PointerPath> {
    let effective_result_limit = if exhaustive {
        result_limit.min(100_000)
    } else {
        result_limit
    };
    let max_frontier = if exhaustive {
        100_000
    } else {
        effective_result_limit.saturating_mul(4).clamp(10_000, 100_000)
    };
    let mut results = Vec::new();
    let mut seen_results = HashSet::new();

    let mut fast_modules: Vec<FastModuleInfo> = modules
        .iter()
        .map(|(module, base, size)| {
            let lower = module.to_ascii_lowercase();
            let is_system = lower.contains("ntdll")
                || lower.contains("kernel32")
                || lower.contains("kernelbase")
                || lower.contains("user32")
                || lower.contains("gdi32")
                || lower.contains("msvcp")
                || lower.contains("msvcrt")
                || lower.contains("ucrtbase")
                || lower.contains("vcruntime")
                || lower.contains("comctl32")
                || lower.contains("imm32")
                || lower.contains("shell32");
            FastModuleInfo {
                name: module.clone(),
                base: *base,
                end: base.saturating_add(*size),
                is_system,
            }
        })
        .collect();
    fast_modules.sort_unstable_by_key(|m| m.base);

    let mut legacy_visited = HashSet::new();
    let mut frontier = Vec::new();
    let mut visit_counts: HashMap<usize, u8> = HashMap::new();
    for &target in targets {
        if exhaustive {
            visit_counts.insert(target, 1);
            frontier.push((target, SmallOffsets::new()));
        } else if legacy_visited.insert(target) {
            frontier.push((target, SmallOffsets::new()));
        }
    }

    const MAX_VISITS_PER_NODE: u8 = 4;
    for depth in 0..max_depth.max(1) {
        let mut next = Vec::new();
        let req_offset = must_end_offsets.get(depth).copied();
        for (node, suffix) in frontier {
            let (start, end) = if let Some(req) = req_offset {
                let Some(wanted_value) = node.checked_sub(req) else {
                    continue;
                };
                let s = pointers.partition_point(|(value, _)| *value < wanted_value);
                let e = pointers.partition_point(|(value, _)| *value <= wanted_value);
                (s, e)
            } else {
                let minimum = node.saturating_sub(max_offset);
                let s = pointers.partition_point(|(value, _)| *value < minimum);
                let e = pointers.partition_point(|(value, _)| *value <= node);
                (s, e)
            };
            for &(value, location) in pointers[start..end].iter().rev() {
                let offset = node - value;
                let Some(new_suffix) = suffix.pushed(offset) else {
                    continue;
                };

                if (new_suffix.len as usize) >= must_end_offsets.len() {
                    let mod_idx = fast_modules.partition_point(|m| m.base <= location);
                    if mod_idx > 0 {
                        let m = &fast_modules[mod_idx - 1];
                        if location < m.end && (include_system_modules || !m.is_system) {
                            let module_offset = location - m.base;
                            if seen_results.insert((mod_idx, module_offset, new_suffix)) {
                                results.push(PointerPath {
                                    module: m.name.clone(),
                                    module_offset,
                                    offsets: new_suffix.to_reversed_vec(),
                                });
                                if results.len() >= effective_result_limit.max(1) {
                                    return results;
                                }
                            }
                        }
                    }
                }
                if depth + 1 < max_depth.max(1) && next.len() < max_frontier {
                    if exhaustive {
                        let count = visit_counts.entry(location).or_insert(0);
                        if *count < MAX_VISITS_PER_NODE {
                            *count += 1;
                            next.push((location, new_suffix));
                        }
                    } else if legacy_visited.insert(location) {
                        next.push((location, new_suffix));
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    results
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanComparison {
    Exact,
    Increased,
    Decreased,
    Changed,
    Unchanged,
    Less,
    Greater,
    Between,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScanRegion {
    base: usize,
    size: usize,
}

pub(crate) struct ScanProcess {
    handle: *mut c_void,
}

unsafe impl Send for ScanProcess {}

pub struct PausedProcess {
    #[cfg(windows)]
    threads: Vec<windows_sys::Win32::Foundation::HANDLE>,
}

impl PausedProcess {
    #[cfg(windows)]
    pub fn new(pid: u32) -> io::Result<Self> {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
            System::{
                Diagnostics::ToolHelp::{
                    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                    Thread32Next,
                },
                Threading::{OpenThread, SuspendThread, THREAD_SUSPEND_RESUME},
            },
        };
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let mut threads = Vec::new();
        let mut has_entry = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while has_entry {
            if entry.th32OwnerProcessID == pid {
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !thread.is_null() {
                    if unsafe { SuspendThread(thread) } != u32::MAX {
                        threads.push(thread);
                    } else {
                        unsafe { CloseHandle(thread) };
                    }
                }
            }
            has_entry = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        if threads.is_empty() {
            Err(io::Error::other("unable to pause any target thread"))
        } else {
            Ok(Self { threads })
        }
    }

    #[cfg(not(windows))]
    pub fn new(_pid: u32) -> io::Result<Self> {
        Ok(Self {})
    }
}

impl Drop for PausedProcess {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::ResumeThread};
            for thread in self.threads.drain(..) {
                unsafe {
                    ResumeThread(thread);
                    CloseHandle(thread);
                }
            }
        }
    }
}

impl Drop for ScanProcess {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

impl ScanProcess {
    fn open(pid: u32, write: bool) -> io::Result<Self> {
        #[cfg(windows)]
        let _ = crate::memory_debugger::debugger::enable_debug_privilege();
        let access = PROCESS_QUERY_LIMITED_INFORMATION
            | PROCESS_QUERY_INFORMATION
            | PROCESS_VM_READ
            | if write {
                PROCESS_VM_OPERATION | PROCESS_VM_WRITE
            } else {
                0
            };
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self { handle })
        }
    }

    pub(crate) fn read(&self, address: usize, buffer: &mut [u8]) -> io::Result<usize> {
        let mut read = 0;
        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                address as *const c_void,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut read,
            )
        };
        (ok != 0)
            .then_some(read)
            .ok_or_else(io::Error::last_os_error)
    }
}

thread_local! {
    static CACHED_READ_PROCESS: RefCell<std::collections::HashMap<u32, (Instant, ScanProcess)>> =
        RefCell::new(std::collections::HashMap::new());
    static CACHED_WRITE_PROCESS: RefCell<std::collections::HashMap<u32, (Instant, ScanProcess)>> =
        RefCell::new(std::collections::HashMap::new());
}

pub(crate) fn with_cached_read_process<T>(
    pid: u32,
    read: impl FnOnce(&ScanProcess) -> io::Result<T>,
) -> io::Result<T> {
    CACHED_READ_PROCESS.with(|cached| {
        let mut map = cached.borrow_mut();
        if map.len() > 8 {
            map.retain(|_, (opened_at, _)| opened_at.elapsed() < Duration::from_secs(5));
        }
        let should_open = map
            .get(&pid)
            .map_or(true, |(opened_at, _)| opened_at.elapsed() >= Duration::from_secs(5));
        if should_open {
            let proc = ScanProcess::open(pid, false)?;
            map.insert(pid, (Instant::now(), proc));
        }
        let (_, proc) = map.get(&pid).expect("cached process was just inserted");
        read(proc)
    })
}

fn with_cached_write_process<T>(
    pid: u32,
    write: impl FnOnce(&ScanProcess) -> io::Result<T>,
) -> io::Result<T> {
    CACHED_WRITE_PROCESS.with(|cached| {
        let mut map = cached.borrow_mut();
        if map.len() > 8 {
            map.retain(|_, (opened_at, _)| opened_at.elapsed() < Duration::from_secs(5));
        }
        let should_open = map
            .get(&pid)
            .map_or(true, |(opened_at, _)| opened_at.elapsed() >= Duration::from_secs(5));
        if should_open {
            let proc = ScanProcess::open(pid, true)?;
            map.insert(pid, (Instant::now(), proc));
        }
        let (_, proc) = map.get(&pid).expect("cached process was just inserted");
        write(proc)
    })
}

pub fn read_scan_value(
    pid: u32,
    address: usize,
    value_type: ScanValueType,
) -> io::Result<ScanValue> {
    let mut bytes = [0; 8];
    let width = value_type.width();
    let read = with_cached_read_process(pid, |process| process.read(address, &mut bytes[..width]))?;
    if read != width {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "partial value read",
        ));
    }
    value_type
        .decode(&bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid value"))
}

pub fn query_memory_region(pid: u32, address: usize) -> io::Result<MemoryRegionInfo> {
    let process = ScanProcess::open(pid, false)?;
    let mut information = MaybeUninit::<MemoryBasicInformation>::zeroed();
    let queried = unsafe {
        VirtualQueryEx(
            process.handle,
            address as *const c_void,
            information.as_mut_ptr(),
            size_of::<MemoryBasicInformation>(),
        )
    };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    let information = unsafe { information.assume_init() };
    Ok(MemoryRegionInfo {
        allocation_base: information.allocation_base as usize,
        base: information.base_address as usize,
        size: information.region_size,
        protect: information.protect,
    })
}

pub fn read_memory_bytes(pid: u32, address: usize, length: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0; length];
    let read = with_cached_read_process(pid, |process| process.read(address, &mut bytes))?;
    bytes.truncate(read);
    Ok(bytes)
}

pub fn read_memory_fixed<const N: usize>(pid: u32, address: usize) -> io::Result<[u8; N]> {
    let mut bytes = [0u8; N];
    let read = with_cached_read_process(pid, |process| process.read(address, &mut bytes))?;
    if read == N {
        Ok(bytes)
    } else {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "partial memory read",
        ))
    }
}

pub fn read_text_memory(
    pid: u32,
    address: usize,
    byte_len: usize,
    encoding: TextEncoding,
) -> io::Result<String> {
    let bytes = read_memory_bytes(pid, address, byte_len)?;
    if bytes.len() != byte_len {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "partial text read",
        ));
    }
    Ok(match encoding {
        TextEncoding::Utf8 => {
            let end = bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(bytes.len());
            String::from_utf8_lossy(&bytes[..end]).into_owned()
        }
        TextEncoding::Utf16 => {
            let units = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take_while(|unit| *unit != 0)
                .collect::<Vec<_>>();
            String::from_utf16_lossy(&units)
        }
    })
}

pub fn write_scan_value(pid: u32, address: usize, value: ScanValue) -> io::Result<()> {
    let bytes = value.bytes();
    let width = value.value_type().width();
    with_cached_write_process(pid, |process| {
        let mut written = 0;
        let ok = unsafe {
            WriteProcessMemory(
                process.handle,
                address as *mut c_void,
                bytes.as_ptr().cast(),
                width,
                &mut written,
            )
        };
        if ok == 0 || written != width {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })
}

pub fn write_text_memory(
    pid: u32,
    address: usize,
    text: &str,
    encoding: TextEncoding,
    capacity: usize,
) -> io::Result<()> {
    let encoded = match encoding {
        TextEncoding::Utf8 => text.as_bytes().to_vec(),
        TextEncoding::Utf16 => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    };
    if encoded.len() > capacity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "text needs {} bytes but this address has {capacity}",
                encoded.len()
            ),
        ));
    }
    let mut bytes = vec![0; capacity];
    bytes[..encoded.len()].copy_from_slice(&encoded);
    with_cached_write_process(pid, |process| {
        let mut written = 0;
        let ok = unsafe {
            WriteProcessMemory(
                process.handle,
                address as *mut c_void,
                bytes.as_ptr().cast(),
                bytes.len(),
                &mut written,
            )
        };
        if ok == 0 || written != bytes.len() {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })
}

pub fn write_code_bytes(pid: u32, address: usize, bytes: &[u8]) -> io::Result<()> {
    let process = ScanProcess::open(pid, true)?;
    let mut old_protect = 0u32;
    let protect_ok = unsafe {
        VirtualProtectEx(
            process.handle,
            address as *mut c_void,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old_protect,
        )
    };

    let mut written = 0;
    let ok = unsafe {
        WriteProcessMemory(
            process.handle,
            address as *mut c_void,
            bytes.as_ptr().cast(),
            bytes.len(),
            &mut written,
        )
    };

    if protect_ok != 0 {
        let mut temp = 0u32;
        unsafe {
            VirtualProtectEx(
                process.handle,
                address as *mut c_void,
                bytes.len(),
                old_protect,
                &mut temp,
            );
        }
    }
    unsafe {
        FlushInstructionCache(process.handle, address as *const c_void, bytes.len());
    }

    if ok == 0 || written != bytes.len() {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn scan_entity_lists_with_progress(
    pid: u32,
    entity_bases: &[usize],
    pointer_width: usize,
    max_gap: usize,
    xyz_offsets: [usize; 3],
    progress: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
) -> io::Result<EntityListScanResult> {
    if !matches!(pointer_width, 4 | 8) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pointer width must be 4 or 8 bytes",
        ));
    }
    if entity_bases.len() < 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "at least three entity bases are required",
        ));
    }
    if pointer_width == 4
        && entity_bases
            .iter()
            .any(|&address| address > u32::MAX as usize)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an entity address does not fit the 32-bit target process",
        ));
    }

    let process = ScanProcess::open(pid, false)?;
    let regions = pointer_scan_regions_for(&process);
    total.store(
        regions
            .iter()
            .map(|region| region.size)
            .fold(0usize, usize::saturating_add),
        Ordering::Release,
    );
    let targets = entity_bases
        .iter()
        .copied()
        .enumerate()
        .map(|(index, address)| (address, index))
        .collect::<HashMap<_, _>>();
    let mut target_first_bytes = [false; 256];
    for &address in entity_bases {
        target_first_bytes[address & 0xFF] = true;
    }
    let mut hits = Vec::new();
    let mut buffer = Vec::new();
    let overlap = pointer_width - 1;

    'regions: for region in &regions {
        let end = region.base.saturating_add(region.size);
        let mut chunk_base = region.base;
        while chunk_base < end {
            if cancel.load(Ordering::Acquire) {
                break 'regions;
            }
            let prefix = if chunk_base == region.base {
                0
            } else {
                overlap.min(chunk_base - region.base)
            };
            let read_base = chunk_base - prefix;
            let length = (end - read_base).min(SCAN_CHUNK_BYTES + prefix);
            buffer.resize(length, 0);
            let count = match process.read(read_base, &mut buffer) {
                Ok(count) => count,
                Err(_) => {
                    progress.fetch_add(length.saturating_sub(prefix), Ordering::Relaxed);
                    chunk_base = chunk_base.saturating_add(SCAN_CHUNK_BYTES);
                    continue;
                }
            };
            let haystack = &buffer[..count];
            if count >= pointer_width {
                for offset in prefix..=count - pointer_width {
                    if offset & 0xFFFF == 0 && cancel.load(Ordering::Relaxed) {
                        break 'regions;
                    }
                    if !target_first_bytes[haystack[offset] as usize] {
                        continue;
                    }
                    let value = decode_native_pointer(&haystack[offset..], pointer_width);
                    if let Some(&entity_index) = targets.get(&value) {
                        hits.push(EntityPointerMatch {
                            location: read_base + offset,
                            entity_index,
                            entity_base: value,
                        });
                    }
                }
            }
            progress.fetch_add(count.saturating_sub(prefix), Ordering::Relaxed);
            chunk_base = chunk_base.saturating_add(SCAN_CHUNK_BYTES);
        }
    }

    hits.sort_unstable_by_key(|hit| hit.location);
    hits.dedup_by_key(|hit| hit.location);
    let readable_ranges = regions
        .iter()
        .map(|region| (region.base, region.base.saturating_add(region.size)))
        .collect::<Vec<_>>();
    let readable_entities = entity_bases
        .iter()
        .map(|&address| address_in_ranges(address, &readable_ranges))
        .collect::<Vec<_>>();
    let xyz = entity_bases
        .iter()
        .map(|&base| read_entity_xyz(&process, base, xyz_offsets))
        .collect::<Vec<_>>();
    let pointer_hits = hits.len();
    let candidates = group_entity_pointer_hits(
        &hits,
        entity_bases.len(),
        pointer_width,
        max_gap.max(pointer_width),
        &readable_entities,
        &xyz,
    );
    Ok(EntityListScanResult {
        candidates,
        pointer_hits,
        cancelled: cancel.load(Ordering::Acquire),
    })
}

pub fn validate_entity_list(
    pid: u32,
    list_address: usize,
    slot_count: usize,
    pointer_width: usize,
    entity_bases: &[usize],
    xyz_offsets: [usize; 3],
) -> io::Result<EntityListValidation> {
    if !matches!(pointer_width, 4 | 8) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pointer width must be 4 or 8 bytes",
        ));
    }
    let process = ScanProcess::open(pid, false)?;
    let regions = pointer_scan_regions_for(&process);
    let readable_ranges = regions
        .iter()
        .map(|region| (region.base, region.base.saturating_add(region.size)))
        .collect::<Vec<_>>();
    let expected = entity_bases
        .iter()
        .copied()
        .enumerate()
        .map(|(index, address)| (address, index))
        .collect::<HashMap<_, _>>();
    // ponytail: preview is intentionally bounded; paging is the upgrade path for giant tables.
    let slot_count = slot_count.clamp(1, 512);
    let byte_count = slot_count.saturating_mul(pointer_width);
    let mut bytes = vec![0; byte_count];
    let read = process.read(list_address, &mut bytes)?;
    let mut matched = HashSet::new();
    let mut readable_pointers = 0;
    let mut plausible_xyz = 0;
    let mut entries = Vec::with_capacity(slot_count);
    for slot in 0..slot_count {
        let offset = slot * pointer_width;
        if offset + pointer_width > read {
            break;
        }
        let entity_base = decode_native_pointer(&bytes[offset..], pointer_width);
        let entity_base = (entity_base != 0).then_some(entity_base);
        let matched_input = entity_base.and_then(|address| expected.get(&address).copied());
        if let Some(index) = matched_input {
            matched.insert(index);
        }
        let readable =
            entity_base.is_some_and(|address| address_in_ranges(address, &readable_ranges));
        readable_pointers += usize::from(readable);
        let xyz = entity_base.and_then(|address| read_entity_xyz(&process, address, xyz_offsets));
        plausible_xyz += usize::from(xyz.is_some());
        entries.push(EntityListPreview {
            slot_address: list_address + offset,
            entity_base,
            matched_input,
            readable,
            xyz,
        });
    }
    Ok(EntityListValidation {
        entries,
        matched_entities: matched.len(),
        readable_pointers,
        plausible_xyz,
    })
}

fn decode_native_pointer(bytes: &[u8], pointer_width: usize) -> usize {
    if pointer_width == 4 {
        u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize
    } else {
        u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize
    }
}

fn address_in_ranges(address: usize, ranges: &[(usize, usize)]) -> bool {
    let index = ranges.partition_point(|(base, _)| *base <= address);
    index > 0 && address < ranges[index - 1].1
}

fn read_entity_xyz(
    process: &ScanProcess,
    entity_base: usize,
    offsets: [usize; 3],
) -> Option<[f32; 3]> {
    let mut xyz = [0.0; 3];
    for (index, offset) in offsets.into_iter().enumerate() {
        let mut bytes = [0; 4];
        if process
            .read(entity_base.checked_add(offset)?, &mut bytes)
            .ok()?
            != bytes.len()
        {
            return None;
        }
        let value = f32::from_le_bytes(bytes);
        if !value.is_finite() || value.abs() > 10_000_000.0 || (value != 0.0 && value.abs() < 1e-20)
        {
            return None;
        }
        xyz[index] = value;
    }
    Some(xyz)
}

fn group_entity_pointer_hits(
    hits: &[EntityPointerMatch],
    entity_count: usize,
    pointer_width: usize,
    max_gap: usize,
    readable_entities: &[bool],
    xyz: &[Option<[f32; 3]>],
) -> Vec<EntityListCandidate> {
    let mut candidates = Vec::new();
    let mut start = 0;
    for end in 1..=hits.len() {
        if end < hits.len() && hits[end].location - hits[end - 1].location <= max_gap {
            continue;
        }
        let cluster = &hits[start..end];
        start = end;
        let matched = cluster
            .iter()
            .map(|hit| hit.entity_index)
            .collect::<HashSet<_>>();
        if matched.len() < 2 {
            continue;
        }
        let deltas = cluster
            .windows(2)
            .filter_map(|pair| pair[1].location.checked_sub(pair[0].location))
            .filter(|&delta| delta != 0)
            .collect::<Vec<_>>();
        let observed_stride = deltas
            .iter()
            .copied()
            .reduce(greatest_common_divisor)
            .unwrap_or(pointer_width);
        let aligned = deltas
            .iter()
            .filter(|&&delta| delta % pointer_width == 0)
            .count();
        let mut frequencies = HashMap::new();
        for &delta in &deltas {
            *frequencies.entry(delta).or_insert(0usize) += 1;
        }
        let dominant = frequencies.values().copied().max().unwrap_or(1);
        let denominator = deltas.len().max(1) as f32;
        let spacing_regularity =
            0.7 * aligned as f32 / denominator + 0.3 * dominant as f32 / denominator;
        let readable_pointers = cluster
            .iter()
            .filter(|hit| {
                readable_entities
                    .get(hit.entity_index)
                    .copied()
                    .unwrap_or(false)
            })
            .count();
        let plausible_xyz = matched
            .iter()
            .filter(|&&index| xyz.get(index).is_some_and(Option::is_some))
            .count();
        let coverage = matched.len() as f32 / entity_count.max(1) as f32;
        let confidence = 100.0
            * (0.15 * (matched.len() as f32 / 3.0).min(1.0)
                + 0.35 * coverage
                + 0.20 * spacing_regularity
                + 0.15 * readable_pointers as f32 / cluster.len().max(1) as f32
                + 0.15 * plausible_xyz as f32 / matched.len().max(1) as f32);
        candidates.push(EntityListCandidate {
            address: cluster[0].location,
            end_address: cluster.last().unwrap().location,
            pointer_matches: cluster.to_vec(),
            matched_entities: matched.len(),
            coverage,
            observed_stride,
            spacing_regularity,
            readable_pointers,
            plausible_xyz,
            confidence,
        });
    }
    candidates.sort_by(|left, right| {
        right
            .matched_entities
            .cmp(&left.matched_entities)
            .then_with(|| right.coverage.total_cmp(&left.coverage))
            .then_with(|| right.spacing_regularity.total_cmp(&left.spacing_regularity))
            .then_with(|| right.readable_pointers.cmp(&left.readable_pointers))
            .then_with(|| right.plausible_xyz.cmp(&left.plausible_xyz))
            .then_with(|| right.confidence.total_cmp(&left.confidence))
            .then_with(|| left.address.cmp(&right.address))
    });
    candidates.truncate(512);
    candidates
}

fn greatest_common_divisor(mut left: usize, mut right: usize) -> usize {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

pub fn scan_memory_range_with_progress(
    pid: u32,
    exact: Option<ScanValue>,
    range: Option<(ScanValue, ScanValue)>,
    value_type: ScanValueType,
    result_limit: usize,
    options: MemoryScanOptions,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<ScanCandidate>> {
    let result_limit = result_limit.max(1);
    let process = ScanProcess::open(pid, false)?;
    let regions = scan_regions_for(&process, options.clone())
        .into_iter()
        .flat_map(|region| {
            (0..region.size)
                .step_by(SCAN_BUCKET_BYTES)
                .map(move |offset| ScanRegion {
                    base: region.base + offset,
                    size: (region.size - offset).min(SCAN_BUCKET_BYTES),
                })
        })
        .collect::<Vec<_>>();
    let _slots = regions
        .iter()
        .map(|region| region.size / value_type.width())
        .fold(0usize, usize::saturating_add);
    let alignment = options.alignment.unwrap_or(1).max(1);
    let max_threads = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 32);
    let worker_count = max_threads.min(regions.len().max(1));
    let total_bytes = regions
        .iter()
        .map(|region| region.size)
        .fold(0usize, usize::saturating_add);
    let target_bytes = total_bytes.div_ceil(worker_count).max(1);
    let mut buckets = vec![Vec::new(); worker_count];
    let mut bucket_index = 0;
    let mut bucket_bytes = 0usize;
    for region in regions {
        if bucket_bytes >= target_bytes && bucket_index + 1 < worker_count {
            bucket_index += 1;
            bucket_bytes = 0;
        }
        bucket_bytes = bucket_bytes.saturating_add(region.size);
        buckets[bucket_index].push(region);
    }
    let claimed_results = Arc::new(AtomicUsize::new(0));
    let workers = buckets
        .into_iter()
        .map(|regions| {
            let progress = Arc::clone(&total);
            let claimed_results = Arc::clone(&claimed_results);
            thread::spawn(move || {
                crate::platform::set_current_thread_high_priority();
                scan_region_bucket(
                    pid,
                    regions,
                    exact,
                    range,
                    value_type,
                    alignment,
                    result_limit,
                    claimed_results,
                    progress,
                )
            })
        })
        .collect::<Vec<_>>();
    let completed_buckets = workers
        .into_iter()
        .filter_map(|worker| worker.join().ok()?.ok())
        .collect::<Vec<_>>();
    Ok(merge_scan_buckets(completed_buckets, result_limit))
}

fn merge_scan_buckets(
    completed_buckets: Vec<Vec<ScanCandidate>>,
    result_limit: usize,
) -> Vec<ScanCandidate> {
    let total_len = completed_buckets
        .iter()
        .map(|b| b.len())
        .sum::<usize>()
        .min(result_limit);
    let mut found = Vec::with_capacity(total_len);
    for mut bucket in completed_buckets {
        if found.len() >= result_limit {
            break;
        }
        let needed = result_limit - found.len();
        if bucket.len() > needed {
            bucket.truncate(needed);
        }
        found.append(&mut bucket);
    }
    found
}

pub fn scan_text_memory_with_progress(
    pid: u32,
    text: &str,
    encoding: TextEncoding,
    case_sensitive: bool,
    null_terminated: bool,
    result_limit: usize,
    options: MemoryScanOptions,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<TextScanCandidate>> {
    let pattern = encode_scan_text(text, encoding, case_sensitive, null_terminated);
    if pattern.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "text cannot be empty",
        ));
    }
    crate::platform::set_current_thread_high_priority();
    let process = ScanProcess::open(pid, false)?;
    let mut found = Vec::new();
    let mut buffer = Vec::new();
    let overlap = pattern.len().saturating_sub(1);
    'regions: for region in scan_regions_for(&process, options) {
        let end = region.base.saturating_add(region.size);
        let mut chunk_base = region.base;
        while chunk_base < end {
            let prefix = if chunk_base == region.base {
                0
            } else {
                overlap.min(chunk_base - region.base)
            };
            let read_base = chunk_base - prefix;
            let length = (end - read_base).min(SCAN_CHUNK_BYTES + prefix);
            buffer.resize(length, 0);
            if let Ok(count) = process.read(read_base, &mut buffer) {
                total.fetch_add(count, Ordering::Relaxed);
                let haystack = &buffer[..count];
                if count < pattern.len() {
                    chunk_base = chunk_base.saturating_add(SCAN_CHUNK_BYTES);
                    continue;
                }
                let max_start = count.saturating_sub(pattern.len());
                for offset in 0..=max_start {
                    if offset < prefix
                        || !text_bytes_equal(
                            &haystack[offset..offset + pattern.len()],
                            &pattern,
                            encoding,
                            case_sensitive,
                        )
                    {
                        continue;
                    }
                    found.push(TextScanCandidate {
                        address: read_base + offset,
                        previous: text.to_owned(),
                        current: text.to_owned(),
                    });
                    if found.len() >= result_limit.max(1) {
                        break 'regions;
                    }
                }
            }
            chunk_base = chunk_base.saturating_add(SCAN_CHUNK_BYTES);
        }
    }
    Ok(found)
}

pub fn filter_text_scan_candidates(
    pid: u32,
    candidates: Vec<TextScanCandidate>,
    text: &str,
    encoding: TextEncoding,
    case_sensitive: bool,
    null_terminated: bool,
) -> io::Result<Vec<TextScanCandidate>> {
    let pattern = encode_scan_text(text, encoding, case_sensitive, null_terminated);
    if pattern.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "text cannot be empty",
        ));
    }
    let process = ScanProcess::open(pid, false)?;
    let mut bytes = vec![0; pattern.len()];
    let mut kept = Vec::new();
    for candidate in candidates {
        if process.read(candidate.address, &mut bytes).ok() == Some(pattern.len())
            && text_bytes_equal(&bytes, &pattern, encoding, case_sensitive)
        {
            kept.push(TextScanCandidate {
                address: candidate.address,
                previous: candidate.current,
                current: text.to_owned(),
            });
        }
    }
    Ok(kept)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AobByte {
    Exact(u8),
    Any,
}

pub fn parse_aob_pattern(pattern_str: &str) -> Option<Vec<AobByte>> {
    let sanitized = pattern_str.replace(['O', 'o'], "0");
    let mut bytes = Vec::new();
    for token in sanitized.split_whitespace() {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if token == "?" || token == "??" {
            bytes.push(AobByte::Any);
        } else {
            let clean_token = if token.len() == 1 {
                format!("0{token}")
            } else {
                token.to_string()
            };
            if let Ok(byte) = u8::from_str_radix(&clean_token, 16) {
                bytes.push(AobByte::Exact(byte));
            } else {
                return None;
            }
        }
    }
    if bytes.is_empty() { None } else { Some(bytes) }
}

#[allow(dead_code)]
fn aob_bytes_equal(actual: &[u8], pattern: &[AobByte]) -> bool {
    if actual.len() != pattern.len() {
        return false;
    }
    actual.iter().zip(pattern.iter()).all(|(&b, &p)| match p {
        AobByte::Exact(expected) => b == expected,
        AobByte::Any => true,
    })
}

#[derive(Clone, Debug)]
pub struct CompiledAob {
    #[allow(dead_code)]
    pub pattern: Vec<AobByte>,
    pub values: Vec<u8>,
    pub masks: Vec<u8>,
    pub first_exact_idx: Option<usize>,
    pub first_exact_byte: Option<u8>,
    pub second_exact_idx: Option<usize>,
    pub second_exact_byte: Option<u8>,
}

impl CompiledAob {
    pub fn new(pattern: Vec<AobByte>) -> Self {
        let len = pattern.len();
        let mut values = Vec::with_capacity(len);
        let mut masks = Vec::with_capacity(len);
        for &b in &pattern {
            match b {
                AobByte::Exact(v) => {
                    values.push(v);
                    masks.push(0xFF);
                }
                AobByte::Any => {
                    values.push(0x00);
                    masks.push(0x00);
                }
            }
        }
        let first_exact_idx = pattern.iter().position(|b| matches!(b, AobByte::Exact(_)));
        let first_exact_byte = first_exact_idx.map(|idx| values[idx]);

        let mut second_exact = None;
        if let Some(f_idx) = first_exact_idx {
            let is_common = |b: u8| matches!(b, 0x00 | 0x48 | 0x89 | 0x8B | 0x90 | 0xFF);
            let max_win = (f_idx + 32).min(len);
            for idx in (f_idx + 1..max_win).rev() {
                if masks[idx] == 0xFF && !is_common(values[idx]) {
                    second_exact = Some((idx, values[idx]));
                    break;
                }
            }
            if second_exact.is_none() {
                for idx in (f_idx + 1..max_win).rev() {
                    if masks[idx] == 0xFF {
                        second_exact = Some((idx, values[idx]));
                        break;
                    }
                }
            }
            if second_exact.is_none() {
                for idx in f_idx + 1..len {
                    if masks[idx] == 0xFF {
                        second_exact = Some((idx, values[idx]));
                        break;
                    }
                }
            }
        }
        let (second_exact_idx, second_exact_byte) = match second_exact {
            Some((idx, b)) => (Some(idx), Some(b)),
            None => (None, None),
        };

        Self {
            pattern,
            values,
            masks,
            first_exact_idx,
            first_exact_byte,
            second_exact_idx,
            second_exact_byte,
        }
    }

    pub fn from_pattern_str(pattern_str: &str) -> Option<Self> {
        let pattern = parse_aob_pattern(pattern_str)?;
        Some(Self::new(pattern))
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    #[inline]
    pub fn matches(&self, haystack: &[u8]) -> bool {
        let len = self.len();
        if haystack.len() < len {
            return false;
        }
        let h = &haystack[..len];
        let v = &self.values[..];
        let m = &self.masks[..];

        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx2") && len >= 32 {
                return unsafe { aob_matches_avx2(h.as_ptr(), v.as_ptr(), m.as_ptr(), len) };
            }
        }

        let mut i = 0;
        while i + 8 <= len {
            let h_val = u64::from_ne_bytes(h[i..i + 8].try_into().unwrap());
            let m_val = u64::from_ne_bytes(m[i..i + 8].try_into().unwrap());
            let v_val = u64::from_ne_bytes(v[i..i + 8].try_into().unwrap());
            if (h_val & m_val) != v_val {
                return false;
            }
            i += 8;
        }
        while i < len {
            if (h[i] & m[i]) != v[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unused_unsafe)]
unsafe fn aob_matches_avx2(
    h_ptr: *const u8,
    v_ptr: *const u8,
    m_ptr: *const u8,
    len: usize,
) -> bool {
    use std::arch::x86_64::*;
    let mut i = 0;
    while i + 32 <= len {
        let chunk = unsafe { _mm256_loadu_si256(h_ptr.add(i) as *const __m256i) };
        let mask = unsafe { _mm256_loadu_si256(m_ptr.add(i) as *const __m256i) };
        let val = unsafe { _mm256_loadu_si256(v_ptr.add(i) as *const __m256i) };
        let masked = unsafe { _mm256_and_si256(chunk, mask) };
        let eq = unsafe { _mm256_cmpeq_epi8(masked, val) };
        if (unsafe { _mm256_movemask_epi8(eq) } as u32) != 0xFFFF_FFFF {
            return false;
        }
        i += 32;
    }
    while i + 8 <= len {
        let h_val = unsafe { std::ptr::read_unaligned(h_ptr.add(i) as *const u64) };
        let m_val = unsafe { std::ptr::read_unaligned(m_ptr.add(i) as *const u64) };
        let v_val = unsafe { std::ptr::read_unaligned(v_ptr.add(i) as *const u64) };
        if (h_val & m_val) != v_val {
            return false;
        }
        i += 8;
    }
    while i < len {
        if (unsafe { *h_ptr.add(i) } & unsafe { *m_ptr.add(i) }) != unsafe { *v_ptr.add(i) } {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unused_unsafe)]
unsafe fn scan_aob_chunk_dual_avx2(
    haystack: &[u8],
    compiled: &CompiledAob,
    f_idx: usize,
    f_byte: u8,
    delta: usize,
    s_byte: u8,
    mut on_match: impl FnMut(usize) -> bool,
) -> bool {
    use std::arch::x86_64::*;
    let pat_len = compiled.len();
    let max_cursor = haystack.len().saturating_sub(pat_len - f_idx);
    let mut cursor = f_idx;

    let v_f = unsafe { _mm256_set1_epi8(f_byte as i8) };
    let v_s = unsafe { _mm256_set1_epi8(s_byte as i8) };

    while cursor + delta + 32 <= haystack.len() && cursor + 32 <= max_cursor + 1 {
        let chunk_f = unsafe { _mm256_loadu_si256(haystack.as_ptr().add(cursor) as *const __m256i) };
        let chunk_s = unsafe { _mm256_loadu_si256(haystack.as_ptr().add(cursor + delta) as *const __m256i) };
        let eq_f = unsafe { _mm256_cmpeq_epi8(chunk_f, v_f) };
        let eq_s = unsafe { _mm256_cmpeq_epi8(chunk_s, v_s) };
        let mut mask = unsafe { _mm256_movemask_epi8(_mm256_and_si256(eq_f, eq_s)) } as u32;

        while mask != 0 {
            let bit = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            let cand_offset = (cursor + bit) - f_idx;
            if compiled.matches(&haystack[cand_offset..]) {
                if !on_match(cand_offset) {
                    return false;
                }
            }
        }
        cursor += 32;
    }

    while cursor <= max_cursor {
        if haystack[cursor] == f_byte && haystack[cursor + delta] == s_byte {
            let cand_offset = cursor - f_idx;
            if compiled.matches(&haystack[cand_offset..]) {
                if !on_match(cand_offset) {
                    return false;
                }
            }
        }
        cursor += 1;
    }
    true
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unused_unsafe)]
unsafe fn scan_aob_chunk_single_avx2(
    haystack: &[u8],
    compiled: &CompiledAob,
    f_idx: usize,
    f_byte: u8,
    mut on_match: impl FnMut(usize) -> bool,
) -> bool {
    use std::arch::x86_64::*;
    let pat_len = compiled.len();
    let max_cursor = haystack.len().saturating_sub(pat_len - f_idx);
    let mut cursor = f_idx;

    let v_f = unsafe { _mm256_set1_epi8(f_byte as i8) };

    while cursor + 32 <= max_cursor + 1 {
        let chunk_f = unsafe { _mm256_loadu_si256(haystack.as_ptr().add(cursor) as *const __m256i) };
        let eq_f = unsafe { _mm256_cmpeq_epi8(chunk_f, v_f) };
        let mut mask = unsafe { _mm256_movemask_epi8(eq_f) } as u32;

        while mask != 0 {
            let bit = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            let cand_offset = (cursor + bit) - f_idx;
            if compiled.matches(&haystack[cand_offset..]) {
                if !on_match(cand_offset) {
                    return false;
                }
            }
        }
        cursor += 32;
    }

    while cursor <= max_cursor {
        if haystack[cursor] == f_byte {
            let cand_offset = cursor - f_idx;
            if compiled.matches(&haystack[cand_offset..]) {
                if !on_match(cand_offset) {
                    return false;
                }
            }
        }
        cursor += 1;
    }
    true
}

fn scan_aob_chunk_all_any(
    haystack: &[u8],
    compiled: &CompiledAob,
    mut on_match: impl FnMut(usize) -> bool,
) -> bool {
    let pat_len = compiled.len();
    let max_start = haystack.len().saturating_sub(pat_len);
    for offset in 0..=max_start {
        if !on_match(offset) {
            return false;
        }
    }
    true
}

fn scan_aob_chunk_scalar(
    haystack: &[u8],
    compiled: &CompiledAob,
    mut on_match: impl FnMut(usize) -> bool,
) -> bool {
    let pat_len = compiled.len();
    if let (Some(f_idx), Some(f_byte)) = (compiled.first_exact_idx, compiled.first_exact_byte) {
        let max_cursor = haystack.len().saturating_sub(pat_len - f_idx);
        let mut cursor = f_idx;
        if let (Some(s_idx), Some(s_byte)) = (compiled.second_exact_idx, compiled.second_exact_byte) {
            let delta = s_idx - f_idx;
            while cursor <= max_cursor {
                if haystack[cursor] == f_byte && haystack[cursor + delta] == s_byte {
                    let cand_offset = cursor - f_idx;
                    if compiled.matches(&haystack[cand_offset..]) {
                        if !on_match(cand_offset) {
                            return false;
                        }
                    }
                }
                cursor += 1;
            }
        } else {
            while cursor <= max_cursor {
                if haystack[cursor] == f_byte {
                    let cand_offset = cursor - f_idx;
                    if compiled.matches(&haystack[cand_offset..]) {
                        if !on_match(cand_offset) {
                            return false;
                        }
                    }
                }
                cursor += 1;
            }
        }
    } else {
        return scan_aob_chunk_all_any(haystack, compiled, on_match);
    }
    true
}

pub fn scan_aob_chunk(
    haystack: &[u8],
    compiled: &CompiledAob,
    on_match: impl FnMut(usize) -> bool,
) -> bool {
    if haystack.len() < compiled.len() {
        return true;
    }
    if compiled.is_empty() {
        return true;
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            if let (Some(f_idx), Some(f_byte)) = (compiled.first_exact_idx, compiled.first_exact_byte) {
                if let (Some(s_idx), Some(s_byte)) = (compiled.second_exact_idx, compiled.second_exact_byte) {
                    let delta = s_idx - f_idx;
                    return unsafe {
                        scan_aob_chunk_dual_avx2(haystack, compiled, f_idx, f_byte, delta, s_byte, on_match)
                    };
                } else {
                    return unsafe {
                        scan_aob_chunk_single_avx2(haystack, compiled, f_idx, f_byte, on_match)
                    };
                }
            } else {
                return scan_aob_chunk_all_any(haystack, compiled, on_match);
            }
        }
    }

    scan_aob_chunk_scalar(haystack, compiled, on_match)
}

pub fn is_aob_pattern_input(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }
    let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return false;
    }
    if tokens.len() == 1 {
        let t = tokens[0];
        return t == "??" || t == "?";
    }
    tokens.iter().all(|t| {
        t.eq_ignore_ascii_case("?")
            || t.eq_ignore_ascii_case("??")
            || (t.len() <= 2 && t.chars().all(|c| c.is_ascii_hexdigit()))
    })
}

pub fn scan_aob_memory_with_progress(
    pid: u32,
    pattern_str: &str,
    result_limit: usize,
    options: MemoryScanOptions,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<TextScanCandidate>> {
    let addrs = scan_aob_memory_addresses(pid, pattern_str, result_limit, options, total)?;
    Ok(addrs
        .into_iter()
        .map(|address| TextScanCandidate {
            address,
            previous: pattern_str.to_owned(),
            current: pattern_str.to_owned(),
        })
        .collect())
}

pub fn scan_aob_memory_as_numeric(
    pid: u32,
    pattern_str: &str,
    value_type: ScanValueType,
    result_limit: usize,
    options: MemoryScanOptions,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<ScanCandidate>> {
    let addrs = scan_aob_memory_addresses(pid, pattern_str, result_limit, options, total)?;
    let mut candidates = Vec::with_capacity(addrs.len());
    for address in addrs {
        let current = read_scan_value(pid, address, value_type).unwrap_or(ScanValue::I32(0));
        candidates.push(ScanCandidate::new(address, current));
    }
    Ok(candidates)
}

pub fn scan_aob_memory_as_text(
    pid: u32,
    pattern_str: &str,
    encoding: TextEncoding,
    result_limit: usize,
    options: MemoryScanOptions,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<TextScanCandidate>> {
    let addrs = scan_aob_memory_addresses(pid, pattern_str, result_limit, options, total)?;
    let mut candidates = Vec::with_capacity(addrs.len());
    let read_len = if encoding == TextEncoding::Utf16 { 64 } else { 32 };
    for address in addrs {
        let text = read_text_memory(pid, address, read_len, encoding).unwrap_or_default();
        candidates.push(TextScanCandidate {
            address,
            previous: text.clone(),
            current: text,
        });
    }
    Ok(candidates)
}

pub fn scan_aob_memory_addresses(
    pid: u32,
    pattern_str: &str,
    result_limit: usize,
    options: MemoryScanOptions,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<usize>> {
    let pattern = parse_aob_pattern(pattern_str).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid AOB pattern format (e.g. F3 41 ?? 10 50 10)",
        )
    })?;
    let compiled = CompiledAob::new(pattern);
    let process = ScanProcess::open(pid, false)?;
    let regions = scan_regions_for(&process, options);
    scan_aob_regions_multithreaded(pid, regions, &compiled, result_limit, total)
}

fn scan_aob_regions_multithreaded(
    pid: u32,
    regions: Vec<ScanRegion>,
    compiled: &CompiledAob,
    result_limit: usize,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<usize>> {
    let sliced_regions = regions
        .into_iter()
        .flat_map(|region| {
            (0..region.size)
                .step_by(SCAN_BUCKET_BYTES)
                .map(move |offset| ScanRegion {
                    base: region.base + offset,
                    size: (region.size - offset).min(SCAN_BUCKET_BYTES),
                })
        })
        .collect::<Vec<_>>();

    if sliced_regions.is_empty() {
        return Ok(Vec::new());
    }

    let max_threads = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 32);
    let worker_count = max_threads.min(sliced_regions.len().max(1));
    let total_bytes = sliced_regions
        .iter()
        .map(|region| region.size)
        .fold(0usize, usize::saturating_add);
    let target_bytes = total_bytes.div_ceil(worker_count).max(1);
    let mut buckets = vec![Vec::new(); worker_count];
    let mut bucket_index = 0;
    let mut bucket_bytes = 0usize;
    for region in sliced_regions {
        if bucket_bytes >= target_bytes && bucket_index + 1 < worker_count {
            bucket_index += 1;
            bucket_bytes = 0;
        }
        bucket_bytes = bucket_bytes.saturating_add(region.size);
        buckets[bucket_index].push(region);
    }

    let claimed_results = Arc::new(AtomicUsize::new(0));
    let compiled_arc = Arc::new(compiled.clone());
    let workers = buckets
        .into_iter()
        .map(|b_regions| {
            let progress = Arc::clone(&total);
            let claimed_results = Arc::clone(&claimed_results);
            let compiled = Arc::clone(&compiled_arc);
            thread::spawn(move || {
                scan_aob_bucket(
                    pid,
                    b_regions,
                    &compiled,
                    result_limit,
                    claimed_results,
                    progress,
                )
            })
        })
        .collect::<Vec<_>>();

    let mut all_results = Vec::new();
    for worker in workers {
        if let Ok(Ok(bucket_res)) = worker.join() {
            all_results.extend(bucket_res);
            if all_results.len() >= result_limit {
                all_results.truncate(result_limit);
                break;
            }
        }
    }
    all_results.sort_unstable();
    all_results.dedup();
    if all_results.len() > result_limit {
        all_results.truncate(result_limit);
    }
    Ok(all_results)
}

fn scan_aob_bucket(
    pid: u32,
    regions: Vec<ScanRegion>,
    compiled: &CompiledAob,
    result_limit: usize,
    claimed_results: Arc<AtomicUsize>,
    progress: Arc<AtomicUsize>,
) -> io::Result<Vec<usize>> {
    crate::platform::set_current_thread_high_priority();
    let process = ScanProcess::open(pid, false)?;
    let mut found = Vec::new();
    let mut buffer = Vec::new();
    let overlap = compiled.len().saturating_sub(1);

    'regions: for region in regions {
        let end = region.base.saturating_add(region.size);
        let mut chunk_base = region.base;
        while chunk_base < end {
            if claimed_results.load(Ordering::Relaxed) >= result_limit {
                break 'regions;
            }
            let prefix = if chunk_base == region.base {
                0
            } else {
                overlap.min(chunk_base - region.base)
            };
            let read_base = chunk_base - prefix;
            let length = (end - read_base).min(SCAN_CHUNK_BYTES + prefix);
            buffer.resize(length, 0);
            if let Ok(count) = process.read(read_base, &mut buffer) {
                progress.fetch_add(count, Ordering::Relaxed);
                let haystack = &buffer[..count];
                if count < compiled.len() {
                    chunk_base = chunk_base.saturating_add(SCAN_CHUNK_BYTES);
                    continue;
                }

                let cont = scan_aob_chunk(haystack, compiled, |cand_offset| {
                    let match_addr = read_base + cand_offset;
                    found.push(match_addr);
                    claimed_results.fetch_add(1, Ordering::Relaxed) + 1 < result_limit
                });
                if !cont {
                    break 'regions;
                }
            }
            chunk_base = chunk_base.saturating_add(SCAN_CHUNK_BYTES);
        }
    }
    Ok(found)
}

fn intersect_scan_region(region: ScanRegion, start: usize, end: usize) -> Option<ScanRegion> {
    let intersection_start = region.base.max(start);
    let intersection_end = region.base.saturating_add(region.size).min(end);
    (intersection_start < intersection_end).then_some(ScanRegion {
        base: intersection_start,
        size: intersection_end - intersection_start,
    })
}

pub fn scan_aob_memory_range_with_progress(
    pid: u32,
    pattern_str: &str,
    base: usize,
    size: usize,
    result_limit: usize,
    options: MemoryScanOptions,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<TextScanCandidate>> {
    let pattern = parse_aob_pattern(pattern_str).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid AOB pattern format (e.g. F3 41 ?? 10 50 10)",
        )
    })?;
    let compiled = CompiledAob::new(pattern);
    let process = ScanProcess::open(pid, false)?;
    let end = base.saturating_add(size);
    let regions: Vec<ScanRegion> = scan_regions_for(&process, options)
        .into_iter()
        .filter_map(|region| intersect_scan_region(region, base, end))
        .collect();
    let addrs = scan_aob_regions_multithreaded(pid, regions, &compiled, result_limit, total)?;
    Ok(addrs
        .into_iter()
        .map(|address| TextScanCandidate {
            address,
            previous: pattern_str.to_owned(),
            current: pattern_str.to_owned(),
        })
        .collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AobWildcardScenarioOutcome {
    pub label: String,
    pub pattern: String,
    pub wildcard_count: usize,
    pub match_count: usize,
    pub sample_address: Option<usize>,
}

pub fn scan_aob_wildcard_batch(
    pid: u32,
    scenarios: Vec<(String, String, usize)>,
    match_limit: usize,
    options: MemoryScanOptions,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(usize, usize) + Send + Sync,
) -> io::Result<Vec<AobWildcardScenarioOutcome>> {
    let total_scenarios = scenarios.len();
    if total_scenarios == 0 {
        return Ok(Vec::new());
    }

    let process = ScanProcess::open(pid, false)?;
    let regions = scan_regions_for(&process, options.clone());
    if regions.is_empty() {
        return Ok(scenarios
            .into_iter()
            .map(|(label, pattern_str, wildcard_count)| AobWildcardScenarioOutcome {
                label,
                pattern: pattern_str,
                wildcard_count,
                match_count: 0,
                sample_address: None,
            })
            .collect());
    }

    struct Candidate {
        label: String,
        pattern_str: String,
        wildcard_count: usize,
        compiled: CompiledAob,
        matches: Vec<usize>,
    }

    let mut candidates = Vec::with_capacity(total_scenarios);
    for (label, pattern_str, wildcard_count) in scenarios {
        let pattern = parse_aob_pattern(&pattern_str).unwrap_or_default();
        let compiled = CompiledAob::new(pattern);
        candidates.push(Candidate {
            label,
            pattern_str,
            wildcard_count,
            compiled,
            matches: Vec::new(),
        });
    }

    let max_pattern_len = candidates
        .iter()
        .map(|c| c.compiled.len())
        .max()
        .unwrap_or(0);
    let overlap = max_pattern_len.saturating_sub(1);

    let total_bytes: usize = regions.iter().map(|r| r.size).fold(0usize, usize::saturating_add);
    let mut bytes_scanned = 0usize;
    let mut buffer = Vec::new();

    // Stream across all process memory regions without arbitrary buffer truncation
    'regions: for region in &regions {
        if cancel.load(Ordering::Relaxed) {
            break 'regions;
        }

        let end = region.base.saturating_add(region.size);
        let mut chunk_base = region.base;
        while chunk_base < end {
            if cancel.load(Ordering::Relaxed) {
                break 'regions;
            }

            // Early exit if every pattern has reached match_limit
            if candidates
                .iter()
                .all(|c| c.compiled.is_empty() || c.matches.len() >= match_limit)
            {
                break 'regions;
            }

            let prefix = if chunk_base == region.base {
                0
            } else {
                overlap.min(chunk_base - region.base)
            };
            let read_base = chunk_base - prefix;
            let length = (end - read_base).min(SCAN_CHUNK_BYTES + prefix);
            buffer.resize(length, 0);

            if let Ok(count) = process.read(read_base, &mut buffer) {
                bytes_scanned = bytes_scanned.saturating_add(count.saturating_sub(prefix));
                let haystack = &buffer[..count];

                for cand in &mut candidates {
                    if cand.matches.len() >= match_limit || cand.compiled.is_empty() {
                        continue;
                    }
                    let pat_len = cand.compiled.len();
                    if count < pat_len {
                        continue;
                    }

                    scan_aob_chunk(haystack, &cand.compiled, |cand_offset| {
                        let match_addr = read_base + cand_offset;
                        // Avoid duplicates from chunk overlap prefix
                        if chunk_base > region.base && match_addr <= chunk_base - pat_len {
                            return true;
                        }
                        cand.matches.push(match_addr);
                        cand.matches.len() < match_limit
                    });
                }
            } else {
                bytes_scanned = bytes_scanned.saturating_add(length.saturating_sub(prefix));
            }

            let completed = candidates
                .iter()
                .filter(|c| c.matches.len() >= match_limit)
                .count();
            let memory_progress = if total_bytes > 0 {
                (bytes_scanned * total_scenarios / total_bytes).min(total_scenarios)
            } else {
                0
            };
            on_progress(completed.max(memory_progress), total_scenarios);

            chunk_base = chunk_base.saturating_add(SCAN_CHUNK_BYTES);
        }
    }

    on_progress(total_scenarios, total_scenarios);

    let outcomes: Vec<AobWildcardScenarioOutcome> = candidates
        .into_iter()
        .map(|c| {
            let match_count = c.matches.len();
            let sample_address = c.matches.first().copied();
            AobWildcardScenarioOutcome {
                label: c.label,
                pattern: c.pattern_str,
                wildcard_count: c.wildcard_count,
                match_count,
                sample_address,
            }
        })
        .collect();

    Ok(outcomes)
}

pub fn filter_aob_scan_candidates_numeric(
    pid: u32,
    candidates: Vec<ScanCandidate>,
    pattern_str: &str,
    value_type: ScanValueType,
) -> io::Result<Vec<ScanCandidate>> {
    let compiled = CompiledAob::from_pattern_str(pattern_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid AOB pattern format"))?;
    let process = ScanProcess::open(pid, false)?;
    let mut bytes = vec![0; compiled.len()];
    let mut kept = Vec::new();
    for candidate in candidates {
        if process.read(candidate.address, &mut bytes).ok() == Some(compiled.len())
            && compiled.matches(&bytes)
        {
            let current = read_scan_value(pid, candidate.address, value_type)
                .unwrap_or(ScanValue::I32(0));
            kept.push(ScanCandidate::new(candidate.address, current));
        }
    }
    Ok(kept)
}

pub fn filter_aob_scan_candidates_text(
    pid: u32,
    candidates: Vec<TextScanCandidate>,
    pattern_str: &str,
    encoding: TextEncoding,
) -> io::Result<Vec<TextScanCandidate>> {
    let compiled = CompiledAob::from_pattern_str(pattern_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid AOB pattern format"))?;
    let process = ScanProcess::open(pid, false)?;
    let mut bytes = vec![0; compiled.len()];
    let mut kept = Vec::new();
    let read_len = if encoding == TextEncoding::Utf16 { 64 } else { 32 };
    for candidate in candidates {
        if process.read(candidate.address, &mut bytes).ok() == Some(compiled.len())
            && compiled.matches(&bytes)
        {
            let text = read_text_memory(pid, candidate.address, read_len, encoding).unwrap_or_default();
            kept.push(TextScanCandidate {
                address: candidate.address,
                previous: candidate.current,
                current: text,
            });
        }
    }
    Ok(kept)
}

fn encode_scan_text(
    text: &str,
    encoding: TextEncoding,
    case_sensitive: bool,
    null_terminated: bool,
) -> Vec<u8> {
    let text = if case_sensitive {
        text.to_owned()
    } else {
        text.to_lowercase()
    };
    let mut bytes = match encoding {
        TextEncoding::Utf8 => text.into_bytes(),
        TextEncoding::Utf16 => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    };
    if null_terminated {
        bytes.extend(std::iter::repeat_n(
            0,
            if encoding == TextEncoding::Utf16 {
                2
            } else {
                1
            },
        ));
    }
    bytes
}

fn text_bytes_equal(
    actual: &[u8],
    expected: &[u8],
    encoding: TextEncoding,
    case_sensitive: bool,
) -> bool {
    if case_sensitive {
        return actual == expected;
    }
    match encoding {
        TextEncoding::Utf8 => actual.eq_ignore_ascii_case(expected),
        TextEncoding::Utf16 => {
            actual
                .chunks_exact(2)
                .zip(expected.chunks_exact(2))
                .all(|(left, right)| {
                    let left = u16::from_le_bytes([left[0], left[1]]);
                    let right = u16::from_le_bytes([right[0], right[1]]);
                    left == right
                        || (left <= 0x7F
                            && right <= 0x7F
                            && (left as u8).eq_ignore_ascii_case(&(right as u8)))
                })
        }
    }
}

pub fn filter_scan_candidates_with_progress(
    pid: u32,
    mut candidates: Vec<ScanCandidate>,
    value_type: ScanValueType,
    comparison: ScanComparison,
    exact: Option<ScanValue>,
    range: Option<(ScanValue, ScanValue)>,
    progress: Option<Arc<AtomicUsize>>,
) -> io::Result<Vec<ScanCandidate>> {
    if candidates.is_empty() {
        return Ok(candidates);
    }
    let worker_count = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(1, 32)
        .min(candidates.len().div_ceil(500).max(1));
    let chunk_len = candidates.len().div_ceil(worker_count);
    let progress = progress.as_deref();
    let kept = thread::scope(|scope| {
        candidates
            .chunks_mut(chunk_len)
            .map(|chunk| {
                scope.spawn(move || {
                    crate::platform::set_current_thread_high_priority();
                    filter_candidate_slice(
                        pid,
                        chunk,
                        value_type,
                        comparison,
                        exact,
                        range,
                        progress,
                    )
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .map_err(|_| io::Error::other("memory filter worker panicked"))?
            })
            .collect::<io::Result<Vec<_>>>()
    })?;
    let mut write = 0;
    for (chunk_index, kept) in kept.into_iter().enumerate() {
        let start = chunk_index * chunk_len;
        if start != write {
            candidates.copy_within(start..start + kept, write);
        }
        write += kept;
    }
    candidates.truncate(write);
    Ok(candidates)
}

fn filter_candidate_slice(
    pid: u32,
    candidates: &mut [ScanCandidate],
    value_type: ScanValueType,
    comparison: ScanComparison,
    exact: Option<ScanValue>,
    range: Option<(ScanValue, ScanValue)>,
    progress: Option<&AtomicUsize>,
) -> io::Result<usize> {
    const BATCH_BYTES: usize = 256 * 1024;
    let process = ScanProcess::open(pid, false)?;
    let mut buffer = vec![0u8; BATCH_BYTES];
    let mut index = 0;
    let mut write = 0;
    let mut accumulated_progress = 0usize;
    let buf_ptr = buffer.as_ptr();

    macro_rules! filter_int_candidates {
        ($candidates:ident, $index:ident, $end:ident, $batch_base:ident, $count:ident, $write:ident, $buf_ptr:ident, $ty:ty, $get_fn:ident, $new_fn:ident, $read_expr:expr, $variant:ident) => {{
            let width = std::mem::size_of::<$ty>();
            let exact_val = match exact {
                Some(ScanValue::$variant(v)) => Some(v),
                _ => None,
            };
            let (min_val, max_val) = match range {
                Some((ScanValue::$variant(min), ScanValue::$variant(max))) => (Some(min), Some(max)),
                _ => (None, None),
            };

            match comparison {
                ScanComparison::Unchanged => {
                    for read in $index..$end {
                        let candidate = $candidates[read];
                        let offset = candidate.address - $batch_base;
                        if offset + width <= $count {
                            let cur: $ty = unsafe { $read_expr(offset) };
                            let keep = cur == candidate.$get_fn();
                            $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                            $write += keep as usize;
                        }
                    }
                }
                ScanComparison::Changed => {
                    for read in $index..$end {
                        let candidate = $candidates[read];
                        let offset = candidate.address - $batch_base;
                        if offset + width <= $count {
                            let cur: $ty = unsafe { $read_expr(offset) };
                            let keep = cur != candidate.$get_fn();
                            $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                            $write += keep as usize;
                        }
                    }
                }
                ScanComparison::Increased => {
                    for read in $index..$end {
                        let candidate = $candidates[read];
                        let offset = candidate.address - $batch_base;
                        if offset + width <= $count {
                            let cur: $ty = unsafe { $read_expr(offset) };
                            let keep = cur > candidate.$get_fn();
                            $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                            $write += keep as usize;
                        }
                    }
                }
                ScanComparison::Decreased => {
                    for read in $index..$end {
                        let candidate = $candidates[read];
                        let offset = candidate.address - $batch_base;
                        if offset + width <= $count {
                            let cur: $ty = unsafe { $read_expr(offset) };
                            let keep = cur < candidate.$get_fn();
                            $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                            $write += keep as usize;
                        }
                    }
                }
                ScanComparison::Exact => {
                    if let Some(e) = exact_val {
                        for read in $index..$end {
                            let candidate = $candidates[read];
                            let offset = candidate.address - $batch_base;
                            if offset + width <= $count {
                                let cur: $ty = unsafe { $read_expr(offset) };
                                let keep = cur == e;
                                $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                                $write += keep as usize;
                            }
                        }
                    }
                }
                ScanComparison::Greater => {
                    if let Some(e) = exact_val {
                        for read in $index..$end {
                            let candidate = $candidates[read];
                            let offset = candidate.address - $batch_base;
                            if offset + width <= $count {
                                let cur: $ty = unsafe { $read_expr(offset) };
                                let keep = cur > e;
                                $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                                $write += keep as usize;
                            }
                        }
                    }
                }
                ScanComparison::Less => {
                    if let Some(e) = exact_val {
                        for read in $index..$end {
                            let candidate = $candidates[read];
                            let offset = candidate.address - $batch_base;
                            if offset + width <= $count {
                                let cur: $ty = unsafe { $read_expr(offset) };
                                let keep = cur < e;
                                $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                                $write += keep as usize;
                            }
                        }
                    }
                }
                ScanComparison::Between => {
                    if let (Some(min), Some(max)) = (min_val, max_val) {
                        for read in $index..$end {
                            let candidate = $candidates[read];
                            let offset = candidate.address - $batch_base;
                            if offset + width <= $count {
                                let cur: $ty = unsafe { $read_expr(offset) };
                                let keep = cur >= min && cur <= max;
                                $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                                $write += keep as usize;
                            }
                        }
                    }
                }
            }
        }};
    }

    macro_rules! filter_float_candidates {
        ($candidates:ident, $index:ident, $end:ident, $batch_base:ident, $count:ident, $write:ident, $buf_ptr:ident, $ty:ty, $get_fn:ident, $new_fn:ident, $variant:ident, $rel_eps:expr, $abs_eps:expr) => {{
            let width = std::mem::size_of::<$ty>();
            let exact_val = match exact {
                Some(ScanValue::$variant(v)) => Some(v),
                _ => None,
            };
            let (min_val, max_val) = match range {
                Some((ScanValue::$variant(min), ScanValue::$variant(max))) => (Some(min), Some(max)),
                _ => (None, None),
            };

            match comparison {
                ScanComparison::Unchanged => {
                    for read in $index..$end {
                        let candidate = $candidates[read];
                        let offset = candidate.address - $batch_base;
                        if offset + width <= $count {
                            let cur = unsafe { ($buf_ptr.add(offset) as *const $ty).read_unaligned() };
                            let keep = cur.to_bits() == candidate.$get_fn().to_bits();
                            $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                            $write += keep as usize;
                        }
                    }
                }
                ScanComparison::Changed => {
                    for read in $index..$end {
                        let candidate = $candidates[read];
                        let offset = candidate.address - $batch_base;
                        if offset + width <= $count {
                            let cur = unsafe { ($buf_ptr.add(offset) as *const $ty).read_unaligned() };
                            let keep = cur.to_bits() != candidate.$get_fn().to_bits();
                            $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                            $write += keep as usize;
                        }
                    }
                }
                ScanComparison::Increased => {
                    for read in $index..$end {
                        let candidate = $candidates[read];
                        let offset = candidate.address - $batch_base;
                        if offset + width <= $count {
                            let cur = unsafe { ($buf_ptr.add(offset) as *const $ty).read_unaligned() };
                            let keep = cur > candidate.$get_fn();
                            $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                            $write += keep as usize;
                        }
                    }
                }
                ScanComparison::Decreased => {
                    for read in $index..$end {
                        let candidate = $candidates[read];
                        let offset = candidate.address - $batch_base;
                        if offset + width <= $count {
                            let cur = unsafe { ($buf_ptr.add(offset) as *const $ty).read_unaligned() };
                            let keep = cur < candidate.$get_fn();
                            $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                            $write += keep as usize;
                        }
                    }
                }
                ScanComparison::Exact => {
                    if let Some(e) = exact_val {
                        for read in $index..$end {
                            let candidate = $candidates[read];
                            let offset = candidate.address - $batch_base;
                            if offset + width <= $count {
                                let cur = unsafe { ($buf_ptr.add(offset) as *const $ty).read_unaligned() };
                                let keep = (cur - e).abs() <= (e.abs() * $rel_eps).max($abs_eps);
                                $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                                $write += keep as usize;
                            }
                        }
                    }
                }
                ScanComparison::Greater => {
                    if let Some(e) = exact_val {
                        for read in $index..$end {
                            let candidate = $candidates[read];
                            let offset = candidate.address - $batch_base;
                            if offset + width <= $count {
                                let cur = unsafe { ($buf_ptr.add(offset) as *const $ty).read_unaligned() };
                                let keep = cur > e;
                                $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                                $write += keep as usize;
                            }
                        }
                    }
                }
                ScanComparison::Less => {
                    if let Some(e) = exact_val {
                        for read in $index..$end {
                            let candidate = $candidates[read];
                            let offset = candidate.address - $batch_base;
                            if offset + width <= $count {
                                let cur = unsafe { ($buf_ptr.add(offset) as *const $ty).read_unaligned() };
                                let keep = cur < e;
                                $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                                $write += keep as usize;
                            }
                        }
                    }
                }
                ScanComparison::Between => {
                    if let (Some(min), Some(max)) = (min_val, max_val) {
                        for read in $index..$end {
                            let candidate = $candidates[read];
                            let offset = candidate.address - $batch_base;
                            if offset + width <= $count {
                                let cur = unsafe { ($buf_ptr.add(offset) as *const $ty).read_unaligned() };
                                let keep = cur >= min && cur <= max;
                                $candidates[$write] = ScanCandidate::$new_fn(candidate.address, cur);
                                $write += keep as usize;
                            }
                        }
                    }
                }
            }
        }};
    }

    while index < candidates.len() {
        let batch_base = candidates[index].address & !(PAGE_BYTES - 1);
        let mut end = index + 1;
        while end < candidates.len() && candidates[end].address < batch_base + BATCH_BYTES {
            end += 1;
        }
        let batch_size = {
            let last_addr = candidates[end - 1].address + value_type.width();
            (last_addr - batch_base).div_ceil(PAGE_BYTES) * PAGE_BYTES
        }.min(BATCH_BYTES);

        let read_result = process.read(batch_base, &mut buffer[..batch_size]);
        let count = match read_result {
            Ok(count) => count,
            Err(_) => {
                let mut valid_count = 0;
                for page_offset in (0..batch_size).step_by(PAGE_BYTES) {
                    let page_addr = batch_base + page_offset;
                    let page_end = (page_offset + PAGE_BYTES).min(batch_size);
                    if let Ok(n) = process.read(page_addr, &mut buffer[page_offset..page_end]) {
                        valid_count = valid_count.max(page_offset + n);
                    } else {
                        buffer[page_offset..page_end].fill(0);
                    }
                }
                valid_count
            }
        };
        if count > 0 {
            accumulated_progress += count;
            if accumulated_progress >= 512 * 1024 {
                if let Some(progress) = progress {
                    progress.fetch_add(accumulated_progress, Ordering::Relaxed);
                }
                accumulated_progress = 0;
            }

            match value_type {
                ScanValueType::I32 => {
                    filter_int_candidates!(candidates, index, end, batch_base, count, write, buf_ptr, i32, get_i32, new_i32, |off| (buf_ptr.add(off) as *const i32).read_unaligned(), I32);
                }
                ScanValueType::F32 => {
                    filter_float_candidates!(candidates, index, end, batch_base, count, write, buf_ptr, f32, get_f32, new_f32, F32, 1e-6, 1e-5);
                }
                ScanValueType::I64 => {
                    filter_int_candidates!(candidates, index, end, batch_base, count, write, buf_ptr, i64, get_i64, new_i64, |off| (buf_ptr.add(off) as *const i64).read_unaligned(), I64);
                }
                ScanValueType::F64 => {
                    filter_float_candidates!(candidates, index, end, batch_base, count, write, buf_ptr, f64, get_f64, new_f64, F64, 1e-12, 1e-9);
                }
                ScanValueType::I16 => {
                    filter_int_candidates!(candidates, index, end, batch_base, count, write, buf_ptr, i16, get_i16, new_i16, |off| (buf_ptr.add(off) as *const i16).read_unaligned(), I16);
                }
                ScanValueType::I8 => {
                    filter_int_candidates!(candidates, index, end, batch_base, count, write, buf_ptr, i8, get_i8, new_i8, |off| *buf_ptr.add(off) as i8, I8);
                }
            }
        }
        index = end;
    }
    if accumulated_progress > 0 {
        if let Some(progress) = progress {
            progress.fetch_add(accumulated_progress, Ordering::Relaxed);
        }
    }
    Ok(write)
}

fn scan_regions_for(process: &ScanProcess, options: MemoryScanOptions) -> Vec<ScanRegion> {
    let mut regions = Vec::new();
    let mut address = 0usize;
    let mut information_buffer = if options.active_memory_only {
        Some(vec![
            WorkingSetExInformation {
                virtual_address: std::ptr::null_mut(),
                virtual_attributes: 0,
            };
            4096
        ])
    } else {
        None
    };
    loop {
        let mut information = MaybeUninit::<MemoryBasicInformation>::zeroed();
        let queried = unsafe {
            VirtualQueryEx(
                process.handle,
                address as *const c_void,
                information.as_mut_ptr(),
                size_of::<MemoryBasicInformation>(),
            )
        };
        if queried == 0 {
            break;
        }
        let information = unsafe { information.assume_init() };
        let base = information.base_address as usize;
        let Some(next) = base.checked_add(information.region_size) else {
            break;
        };
        let protection = information.protect & 0xFF;
        let writable = matches!(protection, PAGE_READWRITE | PAGE_EXECUTE_READWRITE);
        let executable = matches!(
            protection,
            PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
        );
        let copy_on_write = matches!(protection, PAGE_WRITECOPY | PAGE_EXECUTE_WRITECOPY);
        let readable = matches!(
            protection,
            PAGE_READONLY
                | PAGE_READWRITE
                | PAGE_WRITECOPY
                | PAGE_EXECUTE_READ
                | PAGE_EXECUTE_READWRITE
                | PAGE_EXECUTE_WRITECOPY
        );
        let is_kind_allowed = match information.kind {
            MEM_PRIVATE => options.mem_private,
            MEM_IMAGE => options.mem_image,
            MEM_MAPPED => options.mem_mapped,
            _ => true,
        };
        if options.isolate_guest_ram {
            if information.kind == MEM_IMAGE {
                if next <= address {
                    break;
                }
                address = next;
                continue;
            }
            if let Some(target_alloc) = options.vm_allocation_base {
                if information.allocation_base as usize != target_alloc {
                    if next <= address {
                        break;
                    }
                    address = next;
                    continue;
                }
            } else if information.region_size < 16 * 1024 * 1024 {
                if next <= address {
                    break;
                }
                address = next;
                continue;
            }
        }
        if information.state == MEM_COMMIT
            && is_kind_allowed
            && readable
            && (!options.writable || writable || (options.copy_on_write && copy_on_write))
            && (options.executable || !executable)
            && (options.copy_on_write || !copy_on_write)
            && information.protect & PAGE_GUARD == 0
        {
            let region = ScanRegion {
                base,
                size: information.region_size,
            };
            if let Some(target_ranges) = options.target_ranges.as_ref() {
                let reg_end = region.base.saturating_add(region.size);
                for &(t_start, t_end) in target_ranges.as_ref() {
                    let start = region.base.max(t_start);
                    let end = reg_end.min(t_end);
                    if start < end {
                        let sub_region = ScanRegion {
                            base: start,
                            size: end - start,
                        };
                        if let Some(buf) = information_buffer.as_mut() {
                            regions.extend(active_scan_regions(process, sub_region, buf));
                        } else {
                            regions.push(sub_region);
                        }
                    }
                }
            } else if let Some(buf) = information_buffer.as_mut() {
                regions.extend(active_scan_regions(process, region, buf));
            } else {
                regions.push(region);
            }
        }
        if next <= address {
            break;
        }
        address = next;
    }
    regions
}

/// Queries the AllocationBase for an address in a process.
#[cfg(windows)]
pub fn query_allocation_base(pid: u32, address: usize) -> Option<usize> {
    let process = ScanProcess::open(pid, false).ok()?;
    let mut information = MaybeUninit::<MemoryBasicInformation>::zeroed();
    let queried = unsafe {
        VirtualQueryEx(
            process.handle,
            address as *const c_void,
            information.as_mut_ptr(),
            size_of::<MemoryBasicInformation>(),
        )
    };
    if queried != 0 {
        let info = unsafe { information.assume_init() };
        let base = info.allocation_base as usize;
        if base != 0 {
            return Some(base);
        }
    }
    None
}

#[cfg(not(windows))]
pub fn query_allocation_base(_pid: u32, _address: usize) -> Option<usize> {
    None
}

fn active_scan_regions(
    process: &ScanProcess,
    region: ScanRegion,
    information: &mut [WorkingSetExInformation],
) -> Vec<ScanRegion> {
    const MAX_COALESCE_GAP: usize = 64 * 1024;
    let query_pages = information.len().max(1);
    let end = region.base.saturating_add(region.size);
    let mut page = region.base;
    let mut active_start = None;
    let mut last_active_end = 0;
    let mut active: Vec<ScanRegion> = Vec::new();
    while page < end {
        let count = (end - page).div_ceil(PAGE_BYTES).min(query_pages);
        for (index, item) in information[..count].iter_mut().enumerate() {
            item.virtual_address = page.saturating_add(index * PAGE_BYTES) as *mut c_void;
            item.virtual_attributes = 0;
        }
        let queried = unsafe {
            K32QueryWorkingSetEx(
                process.handle,
                information.as_mut_ptr().cast(),
                (count * size_of::<WorkingSetExInformation>()) as u32,
            )
        } != 0;
        for (index, entry) in information[..count].iter().enumerate() {
            let address = page.saturating_add(index * PAGE_BYTES);
            let resident = queried && entry.virtual_attributes & 1 != 0;
            if resident {
                if active_start.is_none() {
                    if last_active_end > 0
                        && address.saturating_sub(last_active_end) <= MAX_COALESCE_GAP
                        && let Some(prev) = active.pop()
                    {
                        active_start = Some(prev.base);
                    } else {
                        active_start = Some(address);
                    }
                }
                last_active_end = address + PAGE_BYTES;
            } else if let Some(start) = active_start.take() {
                active.push(ScanRegion {
                    base: start,
                    size: address - start,
                });
            }
        }
        page = page.saturating_add(count * PAGE_BYTES);
    }
    if let Some(start) = active_start {
        active.push(ScanRegion {
            base: start,
            size: end.min(last_active_end) - start,
        });
    }
    active
}

fn pointer_scan_regions_for(process: &ScanProcess) -> Vec<ScanRegion> {
    let mut regions = Vec::new();
    let mut address = 0usize;
    loop {
        let mut information = MaybeUninit::<MemoryBasicInformation>::zeroed();
        let queried = unsafe {
            VirtualQueryEx(
                process.handle,
                address as *const c_void,
                information.as_mut_ptr(),
                size_of::<MemoryBasicInformation>(),
            )
        };
        if queried == 0 {
            break;
        }
        let information = unsafe { information.assume_init() };
        let base = information.base_address as usize;
        let Some(next) = base.checked_add(information.region_size) else {
            break;
        };
        if information.state == MEM_COMMIT
            && matches!(information.kind, MEM_PRIVATE | MEM_MAPPED | MEM_IMAGE)
            && matches!(
                information.protect & 0xFF,
                PAGE_READONLY
                    | PAGE_READWRITE
                    | PAGE_WRITECOPY
                    | PAGE_EXECUTE_READ
                    | PAGE_EXECUTE_READWRITE
                    | PAGE_EXECUTE_WRITECOPY
            )
            && information.protect & PAGE_GUARD == 0
        {
            regions.push(ScanRegion {
                base,
                size: information.region_size,
            });
        }
        if next <= address {
            break;
        }
        address = next;
    }
    regions
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn scan_exact_i32_avx2_step4(
    chunk_base: usize,
    ptr: *const u8,
    count: usize,
    val: i32,
    found: &mut Vec<ScanCandidate>,
) {
    let target = _mm256_set1_epi32(val);
    let full_blocks = count / 32;
    for block in 0..full_blocks {
        let offset = block * 32;
        let v = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);
        let eq = _mm256_cmpeq_epi32(v, target);
        let mask = _mm256_movemask_ps(_mm256_castsi256_ps(eq)) as u32;
        if mask != 0 {
            for i in 0..8 {
                if (mask & (1 << i)) != 0 {
                    found.push(ScanCandidate::new_i32(chunk_base + offset + i * 4, val));
                }
            }
        }
    }
    let processed = full_blocks * 32;
    for offset in (processed..=count.saturating_sub(4)).step_by(4) {
        let cur = (ptr.add(offset) as *const i32).read_unaligned();
        if cur == val {
            found.push(ScanCandidate::new_i32(chunk_base + offset, val));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn scan_exact_i32_avx2_step1(
    chunk_base: usize,
    ptr: *const u8,
    count: usize,
    val: i32,
    found: &mut Vec<ScanCandidate>,
) {
    let b0 = val as u8;
    let b0_vec = _mm256_set1_epi8(b0 as i8);
    let full_blocks = count / 32;
    for block in 0..full_blocks {
        let offset = block * 32;
        let v = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);
        let eq = _mm256_cmpeq_epi8(v, b0_vec);
        let mut mask = _mm256_movemask_epi8(eq) as u32;
        while mask != 0 {
            let i = mask.trailing_zeros() as usize;
            let cand_offset = offset + i;
            if cand_offset + 4 <= count {
                let cur = (ptr.add(cand_offset) as *const i32).read_unaligned();
                if cur == val {
                    found.push(ScanCandidate::new_i32(chunk_base + cand_offset, val));
                }
            }
            mask &= mask - 1;
        }
    }
    let processed = full_blocks * 32;
    for offset in processed..=count.saturating_sub(4) {
        let cur = (ptr.add(offset) as *const i32).read_unaligned();
        if cur == val {
            found.push(ScanCandidate::new_i32(chunk_base + offset, val));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn scan_exact_i64_avx2_step8(
    chunk_base: usize,
    ptr: *const u8,
    count: usize,
    val: i64,
    found: &mut Vec<ScanCandidate>,
) {
    let target = _mm256_set1_epi64x(val);
    let full_blocks = count / 32;
    for block in 0..full_blocks {
        let offset = block * 32;
        let v = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);
        let eq = _mm256_cmpeq_epi64(v, target);
        let mask = _mm256_movemask_pd(_mm256_castsi256_pd(eq)) as u32;
        if mask != 0 {
            for i in 0..4 {
                if (mask & (1 << i)) != 0 {
                    found.push(ScanCandidate::new_i64(chunk_base + offset + i * 8, val));
                }
            }
        }
    }
    let processed = full_blocks * 32;
    for offset in (processed..=count.saturating_sub(8)).step_by(8) {
        let cur = (ptr.add(offset) as *const i64).read_unaligned();
        if cur == val {
            found.push(ScanCandidate::new_i64(chunk_base + offset, val));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn scan_exact_i64_avx2_step1(
    chunk_base: usize,
    ptr: *const u8,
    count: usize,
    val: i64,
    found: &mut Vec<ScanCandidate>,
) {
    let b0 = val as u8;
    let b0_vec = _mm256_set1_epi8(b0 as i8);
    let full_blocks = count / 32;
    for block in 0..full_blocks {
        let offset = block * 32;
        let v = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);
        let eq = _mm256_cmpeq_epi8(v, b0_vec);
        let mut mask = _mm256_movemask_epi8(eq) as u32;
        while mask != 0 {
            let i = mask.trailing_zeros() as usize;
            let cand_offset = offset + i;
            if cand_offset + 8 <= count {
                let cur = (ptr.add(cand_offset) as *const i64).read_unaligned();
                if cur == val {
                    found.push(ScanCandidate::new_i64(chunk_base + cand_offset, val));
                }
            }
            mask &= mask - 1;
        }
    }
    let processed = full_blocks * 32;
    for offset in processed..=count.saturating_sub(8) {
        let cur = (ptr.add(offset) as *const i64).read_unaligned();
        if cur == val {
            found.push(ScanCandidate::new_i64(chunk_base + offset, val));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn scan_exact_i16_avx2_step2(
    chunk_base: usize,
    ptr: *const u8,
    count: usize,
    val: i16,
    found: &mut Vec<ScanCandidate>,
) {
    let target = _mm256_set1_epi16(val);
    let full_blocks = count / 32;
    for block in 0..full_blocks {
        let offset = block * 32;
        let v = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);
        let eq = _mm256_cmpeq_epi16(v, target);
        let mask = _mm256_movemask_epi8(eq) as u32;
        if mask != 0 {
            for i in 0..16 {
                if (mask & (1 << (i * 2))) != 0 {
                    found.push(ScanCandidate::new_i16(chunk_base + offset + i * 2, val));
                }
            }
        }
    }
    let processed = full_blocks * 32;
    for offset in (processed..=count.saturating_sub(2)).step_by(2) {
        let cur = (ptr.add(offset) as *const i16).read_unaligned();
        if cur == val {
            found.push(ScanCandidate::new_i16(chunk_base + offset, val));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn scan_exact_i16_avx2_step1(
    chunk_base: usize,
    ptr: *const u8,
    count: usize,
    val: i16,
    found: &mut Vec<ScanCandidate>,
) {
    let b0 = val as u8;
    let b0_vec = _mm256_set1_epi8(b0 as i8);
    let full_blocks = count / 32;
    for block in 0..full_blocks {
        let offset = block * 32;
        let v = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);
        let eq = _mm256_cmpeq_epi8(v, b0_vec);
        let mut mask = _mm256_movemask_epi8(eq) as u32;
        while mask != 0 {
            let i = mask.trailing_zeros() as usize;
            let cand_offset = offset + i;
            if cand_offset + 2 <= count {
                let cur = (ptr.add(cand_offset) as *const i16).read_unaligned();
                if cur == val {
                    found.push(ScanCandidate::new_i16(chunk_base + cand_offset, val));
                }
            }
            mask &= mask - 1;
        }
    }
    let processed = full_blocks * 32;
    for offset in processed..=count.saturating_sub(2) {
        let cur = (ptr.add(offset) as *const i16).read_unaligned();
        if cur == val {
            found.push(ScanCandidate::new_i16(chunk_base + offset, val));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn scan_exact_i8_avx2_step1(
    chunk_base: usize,
    ptr: *const u8,
    count: usize,
    val: i8,
    found: &mut Vec<ScanCandidate>,
) {
    let target = _mm256_set1_epi8(val);
    let full_blocks = count / 32;
    for block in 0..full_blocks {
        let offset = block * 32;
        let v = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);
        let eq = _mm256_cmpeq_epi8(v, target);
        let mut mask = _mm256_movemask_epi8(eq) as u32;
        while mask != 0 {
            let i = mask.trailing_zeros() as usize;
            found.push(ScanCandidate::new_i8(chunk_base + offset + i, val));
            mask &= mask - 1;
        }
    }
    let processed = full_blocks * 32;
    for offset in processed..count {
        let cur = *ptr.add(offset) as i8;
        if cur == val {
            found.push(ScanCandidate::new_i8(chunk_base + offset, val));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn scan_exact_f32_avx2_step4(
    chunk_base: usize,
    ptr: *const u8,
    count: usize,
    val: f32,
    found: &mut Vec<ScanCandidate>,
) {
    let tol = (val.abs() * 1e-6).max(1e-5);
    let min_val = val - tol;
    let max_val = val + tol;
    let min_v = _mm256_set1_ps(min_val);
    let max_v = _mm256_set1_ps(max_val);
    let full_blocks = count / 32;
    for block in 0..full_blocks {
        let offset = block * 32;
        let v = _mm256_loadu_ps(ptr.add(offset) as *const f32);
        let ge = _mm256_cmp_ps(v, min_v, AVX_CMP_GE_OQ);
        let le = _mm256_cmp_ps(v, max_v, AVX_CMP_LE_OQ);
        let and = _mm256_and_ps(ge, le);
        let mask = _mm256_movemask_ps(and) as u32;
        if mask != 0 {
            for i in 0..8 {
                if (mask & (1 << i)) != 0 {
                    let cur = (ptr.add(offset + i * 4) as *const f32).read_unaligned();
                    found.push(ScanCandidate::new_f32(chunk_base + offset + i * 4, cur));
                }
            }
        }
    }
    let processed = full_blocks * 32;
    for offset in (processed..=count.saturating_sub(4)).step_by(4) {
        let cur = (ptr.add(offset) as *const f32).read_unaligned();
        if cur >= min_val && cur <= max_val {
            found.push(ScanCandidate::new_f32(chunk_base + offset, cur));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn scan_exact_f64_avx2_step8(
    chunk_base: usize,
    ptr: *const u8,
    count: usize,
    val: f64,
    found: &mut Vec<ScanCandidate>,
) {
    let tol = (val.abs() * 1e-12).max(1e-9);
    let min_val = val - tol;
    let max_val = val + tol;
    let min_v = _mm256_set1_pd(min_val);
    let max_v = _mm256_set1_pd(max_val);
    let full_blocks = count / 32;
    for block in 0..full_blocks {
        let offset = block * 32;
        let v = _mm256_loadu_pd(ptr.add(offset) as *const f64);
        let ge = _mm256_cmp_pd(v, min_v, AVX_CMP_GE_OQ);
        let le = _mm256_cmp_pd(v, max_v, AVX_CMP_LE_OQ);
        let and = _mm256_and_pd(ge, le);
        let mask = _mm256_movemask_pd(and) as u32;
        if mask != 0 {
            for i in 0..4 {
                if (mask & (1 << i)) != 0 {
                    let cur = (ptr.add(offset + i * 8) as *const f64).read_unaligned();
                    found.push(ScanCandidate::new_f64(chunk_base + offset + i * 8, cur));
                }
            }
        }
    }
    let processed = full_blocks * 32;
    for offset in (processed..=count.saturating_sub(8)).step_by(8) {
        let cur = (ptr.add(offset) as *const f64).read_unaligned();
        if cur >= min_val && cur <= max_val {
            found.push(ScanCandidate::new_f64(chunk_base + offset, cur));
        }
    }
}

#[inline]
fn scan_chunk_fast(
    chunk_base: usize,
    slice: &[u8],
    count: usize,
    exact: Option<ScanValue>,
    range: Option<(ScanValue, ScanValue)>,
    value_type: ScanValueType,
    alignment: usize,
    found: &mut Vec<ScanCandidate>,
) {
    let width = value_type.width();
    let step = alignment.max(1);
    if count < width {
        return;
    }
    let ptr = slice.as_ptr();

    // 1. Exact value scan
    if let (Some(expected), None) = (exact, range) {
        #[cfg(target_arch = "x86_64")]
        if is_x86_feature_detected!("avx2") {
            match expected {
                ScanValue::I32(val) => {
                    if step == 4 {
                        unsafe { scan_exact_i32_avx2_step4(chunk_base, ptr, count, val, found) };
                        return;
                    } else if step == 1 {
                        unsafe { scan_exact_i32_avx2_step1(chunk_base, ptr, count, val, found) };
                        return;
                    }
                }
                ScanValue::F32(val) => {
                    if step == 4 {
                        unsafe { scan_exact_f32_avx2_step4(chunk_base, ptr, count, val, found) };
                        return;
                    }
                }
                ScanValue::I64(val) => {
                    if step == 8 {
                        unsafe { scan_exact_i64_avx2_step8(chunk_base, ptr, count, val, found) };
                        return;
                    } else if step == 1 {
                        unsafe { scan_exact_i64_avx2_step1(chunk_base, ptr, count, val, found) };
                        return;
                    }
                }
                ScanValue::F64(val) => {
                    if step == 8 {
                        unsafe { scan_exact_f64_avx2_step8(chunk_base, ptr, count, val, found) };
                        return;
                    }
                }
                ScanValue::I16(val) => {
                    if step == 2 {
                        unsafe { scan_exact_i16_avx2_step2(chunk_base, ptr, count, val, found) };
                        return;
                    } else if step == 1 {
                        unsafe { scan_exact_i16_avx2_step1(chunk_base, ptr, count, val, found) };
                        return;
                    }
                }
                ScanValue::I8(val) => {
                    if step == 1 {
                        unsafe { scan_exact_i8_avx2_step1(chunk_base, ptr, count, val, found) };
                        return;
                    }
                }
            }
        }

        match expected {
            ScanValue::I32(val) => {
                for offset in (0..=count - 4).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const i32).read_unaligned() };
                    if cur == val {
                        found.push(ScanCandidate::new_i32(chunk_base + offset, val));
                    }
                }
                return;
            }
            ScanValue::F32(val) => {
                let tol = (val.abs() * 1e-6).max(1e-5);
                let min_val = val - tol;
                let max_val = val + tol;
                for offset in (0..=count - 4).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const f32).read_unaligned() };
                    if cur >= min_val && cur <= max_val {
                        found.push(ScanCandidate::new_f32(chunk_base + offset, cur));
                    }
                }
                return;
            }
            ScanValue::I16(val) => {
                for offset in (0..=count - 2).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const i16).read_unaligned() };
                    if cur == val {
                        found.push(ScanCandidate::new_i16(chunk_base + offset, val));
                    }
                }
                return;
            }
            ScanValue::I64(val) => {
                for offset in (0..=count - 8).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const i64).read_unaligned() };
                    if cur == val {
                        found.push(ScanCandidate::new_i64(chunk_base + offset, val));
                    }
                }
                return;
            }
            ScanValue::F64(val) => {
                let tol = (val.abs() * 1e-12).max(1e-9);
                let min_val = val - tol;
                let max_val = val + tol;
                for offset in (0..=count - 8).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const f64).read_unaligned() };
                    if cur >= min_val && cur <= max_val {
                        found.push(ScanCandidate::new_f64(chunk_base + offset, cur));
                    }
                }
                return;
            }
            ScanValue::I8(val) => {
                let target = val as u8;
                for offset in (0..count).step_by(step) {
                    let cur = unsafe { *ptr.add(offset) };
                    if cur == target {
                        found.push(ScanCandidate::new_i8(chunk_base + offset, val));
                    }
                }
                return;
            }
        }
    }

    // 2. Between / Range scan
    if let (None, Some((min_val, max_val))) = (exact, range) {
        match (min_val, max_val) {
            (ScanValue::I32(min_i), ScanValue::I32(max_i)) => {
                for offset in (0..=count - 4).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const i32).read_unaligned() };
                    if cur >= min_i && cur <= max_i {
                        found.push(ScanCandidate::new_i32(chunk_base + offset, cur));
                    }
                }
                return;
            }
            (ScanValue::F32(min_f), ScanValue::F32(max_f)) => {
                for offset in (0..=count - 4).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const f32).read_unaligned() };
                    if cur >= min_f && cur <= max_f {
                        found.push(ScanCandidate::new_f32(chunk_base + offset, cur));
                    }
                }
                return;
            }
            (ScanValue::I16(min_i), ScanValue::I16(max_i)) => {
                for offset in (0..=count - 2).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const i16).read_unaligned() };
                    if cur >= min_i && cur <= max_i {
                        found.push(ScanCandidate::new_i16(chunk_base + offset, cur));
                    }
                }
                return;
            }
            (ScanValue::I64(min_i), ScanValue::I64(max_i)) => {
                for offset in (0..=count - 8).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const i64).read_unaligned() };
                    if cur >= min_i && cur <= max_i {
                        found.push(ScanCandidate::new_i64(chunk_base + offset, cur));
                    }
                }
                return;
            }
            (ScanValue::F64(min_f), ScanValue::F64(max_f)) => {
                for offset in (0..=count - 8).step_by(step) {
                    let cur = unsafe { (ptr.add(offset) as *const f64).read_unaligned() };
                    if cur >= min_f && cur <= max_f {
                        found.push(ScanCandidate::new_f64(chunk_base + offset, cur));
                    }
                }
                return;
            }
            (ScanValue::I8(min_i), ScanValue::I8(max_i)) => {
                for offset in (0..count).step_by(step) {
                    let cur = unsafe { *ptr.add(offset) as i8 };
                    if cur >= min_i && cur <= max_i {
                        found.push(ScanCandidate::new_i8(chunk_base + offset, cur));
                    }
                }
                return;
            }
            _ => {}
        }
    }

    // 3. Unknown value scan (exact is None, range is None)
    match value_type {
        ScanValueType::I32 => {
            let num_items = (count.saturating_sub(4)) / step + 1;
            found.reserve(num_items);
            for offset in (0..=count - 4).step_by(step) {
                let cur = unsafe { (ptr.add(offset) as *const i32).read_unaligned() };
                found.push(ScanCandidate::new_i32(chunk_base + offset, cur));
            }
        }
        ScanValueType::F32 => {
            let num_items = (count.saturating_sub(4)) / step + 1;
            found.reserve(num_items);
            for offset in (0..=count - 4).step_by(step) {
                let cur = unsafe { (ptr.add(offset) as *const f32).read_unaligned() };
                if cur.is_finite() && (cur == 0.0 || (cur.abs() >= 1e-30 && cur.abs() <= 1e30)) {
                    found.push(ScanCandidate::new_f32(chunk_base + offset, cur));
                }
            }
        }
        ScanValueType::I16 => {
            let num_items = (count.saturating_sub(2)) / step + 1;
            found.reserve(num_items);
            for offset in (0..=count - 2).step_by(step) {
                let cur = unsafe { (ptr.add(offset) as *const i16).read_unaligned() };
                found.push(ScanCandidate::new_i16(chunk_base + offset, cur));
            }
        }
        ScanValueType::I64 => {
            let num_items = (count.saturating_sub(8)) / step + 1;
            found.reserve(num_items);
            for offset in (0..=count - 8).step_by(step) {
                let cur = unsafe { (ptr.add(offset) as *const i64).read_unaligned() };
                found.push(ScanCandidate::new_i64(chunk_base + offset, cur));
            }
        }
        ScanValueType::F64 => {
            let num_items = (count.saturating_sub(8)) / step + 1;
            found.reserve(num_items);
            for offset in (0..=count - 8).step_by(step) {
                let cur = unsafe { (ptr.add(offset) as *const f64).read_unaligned() };
                if cur.is_finite() && (cur == 0.0 || (cur.abs() >= 1e-300 && cur.abs() <= 1e300)) {
                    found.push(ScanCandidate::new_f64(chunk_base + offset, cur));
                }
            }
        }
        ScanValueType::I8 => {
            found.reserve(count / step);
            for offset in (0..count).step_by(step) {
                let cur = unsafe { *ptr.add(offset) as i8 };
                found.push(ScanCandidate::new_i8(chunk_base + offset, cur));
            }
        }
    }
}

fn scan_region_bucket(
    pid: u32,
    regions: Vec<ScanRegion>,
    exact: Option<ScanValue>,
    range: Option<(ScanValue, ScanValue)>,
    value_type: ScanValueType,
    alignment: usize,
    result_limit: usize,
    claimed_results: Arc<AtomicUsize>,
    progress: Arc<AtomicUsize>,
) -> io::Result<Vec<ScanCandidate>> {
    let process = ScanProcess::open(pid, false)?;
    let mut found = Vec::with_capacity(65536.min(result_limit));
    let mut buffer = vec![0u8; SCAN_CHUNK_BYTES];
    'regions: for region in regions {
        let end = region.base.saturating_add(region.size);
        let mut chunk_base = region.base;
        while chunk_base < end {
            let length = (end - chunk_base).min(SCAN_CHUNK_BYTES);
            if let Ok(count) = process.read(chunk_base, &mut buffer[..length]) {
                progress.fetch_add(count, Ordering::Relaxed);
                let chunk_start = found.len();
                scan_chunk_fast(
                    chunk_base,
                    &buffer,
                    count,
                    exact,
                    range,
                    value_type,
                    alignment,
                    &mut found,
                );
                let chunk_matches = found.len() - chunk_start;
                if chunk_matches > 0 {
                    let allowed =
                        claim_result_slots(&claimed_results, chunk_matches, result_limit);
                    found.truncate(chunk_start + allowed);
                    if allowed < chunk_matches {
                        break 'regions;
                    }
                }
            }
            chunk_base = chunk_base.saturating_add(length);
        }
    }
    Ok(found)
}

#[derive(Clone, Debug)]
pub struct RawMemorySnapshotChunk {
    pub base: usize,
    pub data: Vec<u8>,
    pub mask: Option<Vec<u64>>,
}

#[derive(Clone, Debug, Default)]
pub struct RawMemorySnapshot {
    pub chunks: Vec<RawMemorySnapshotChunk>,
    pub total_slots: usize,
    pub alignment: usize,
}

#[derive(Clone, Debug)]
pub enum SnapshotFilterResult {
    Snapshot(RawMemorySnapshot),
    Candidates(Vec<ScanCandidate>),
}

pub fn capture_memory_snapshot(
    pid: u32,
    value_type: ScanValueType,
    options: MemoryScanOptions,
    progress: Arc<AtomicUsize>,
) -> io::Result<RawMemorySnapshot> {
    let process = ScanProcess::open(pid, false)?;
    let regions = scan_regions_for(&process, options.clone());
    let alignment = options.alignment.unwrap_or(value_type.width()).max(1);
    let width = value_type.width();

    if regions.is_empty() {
        return Ok(RawMemorySnapshot {
            chunks: Vec::new(),
            total_slots: 0,
            alignment,
        });
    }

    let max_threads = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 32);
    let worker_count = max_threads.min(regions.len().max(1));
    let total_bytes = regions
        .iter()
        .map(|region| region.size)
        .fold(0usize, usize::saturating_add);
    let target_bytes = total_bytes.div_ceil(worker_count).max(1);
    let mut buckets = vec![Vec::new(); worker_count];
    let mut bucket_index = 0;
    let mut bucket_bytes = 0usize;
    for region in regions {
        if bucket_bytes >= target_bytes && bucket_index + 1 < worker_count {
            bucket_index += 1;
            bucket_bytes = 0;
        }
        bucket_bytes = bucket_bytes.saturating_add(region.size);
        buckets[bucket_index].push(region);
    }

    let total_slots = Arc::new(AtomicUsize::new(0));
    let workers = buckets
        .into_iter()
        .map(|worker_regions| {
            let progress = Arc::clone(&progress);
            let total_slots = Arc::clone(&total_slots);
            thread::spawn(move || -> io::Result<Vec<RawMemorySnapshotChunk>> {
                crate::platform::set_current_thread_high_priority();
                let process = ScanProcess::open(pid, false)?;
                let mut chunks = Vec::new();
                for region in worker_regions {
                    let end = region.base.saturating_add(region.size);
                    let mut chunk_base = region.base;
                    while chunk_base < end {
                        let length = (end - chunk_base).min(SCAN_CHUNK_BYTES);
                        let mut data = vec![0u8; length];
                        if let Ok(count) = process.read(chunk_base, &mut data) {
                            progress.fetch_add(count, Ordering::Relaxed);
                            if count >= width {
                                data.truncate(count);
                                let slots = (count - width) / alignment + 1;
                                total_slots.fetch_add(slots, Ordering::Relaxed);
                                chunks.push(RawMemorySnapshotChunk {
                                    base: chunk_base,
                                    data,
                                    mask: None,
                                });
                            }
                        }
                        chunk_base = chunk_base.saturating_add(length);
                    }
                }
                Ok(chunks)
            })
        })
        .collect::<Vec<_>>();

    let mut all_chunks = Vec::new();
    for worker in workers {
        if let Ok(Ok(chunks)) = worker.join() {
            all_chunks.extend(chunks);
        }
    }
    all_chunks.sort_unstable_by_key(|c| c.base);

    Ok(RawMemorySnapshot {
        chunks: all_chunks,
        total_slots: total_slots.load(Ordering::Relaxed),
        alignment,
    })
}

pub fn update_snapshot_baseline(
    pid: u32,
    snapshot: &RawMemorySnapshot,
) -> io::Result<RawMemorySnapshot> {
    if snapshot.chunks.is_empty() {
        return Ok(snapshot.clone());
    }
    let max_threads = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 32);
    let worker_count = max_threads.min(snapshot.chunks.len().max(1));
    let chunk_len = snapshot.chunks.len().div_ceil(worker_count);

    let updated_chunks = thread::scope(|scope| {
        snapshot
            .chunks
            .chunks(chunk_len)
            .map(|chunk_slice| {
                scope.spawn(move || -> io::Result<Vec<RawMemorySnapshotChunk>> {
                    crate::platform::set_current_thread_high_priority();
                    let process = ScanProcess::open(pid, false)?;
                    let mut worker_chunks = Vec::with_capacity(chunk_slice.len());
                    for chunk in chunk_slice {
                        let mut data = chunk.data.clone();
                        let len = data.len();
                        let _ = process.read(chunk.base, &mut data[..len]);
                        worker_chunks.push(RawMemorySnapshotChunk {
                            base: chunk.base,
                            data,
                            mask: chunk.mask.clone(),
                        });
                    }
                    Ok(worker_chunks)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|w| w.join().map_err(|_| io::Error::other("worker panicked"))?)
            .collect::<io::Result<Vec<_>>>()
    })?;

    Ok(RawMemorySnapshot {
        chunks: updated_chunks.into_iter().flatten().collect(),
        total_slots: snapshot.total_slots,
        alignment: snapshot.alignment,
    })
}

pub fn filter_memory_snapshot_with_progress(
    pid: u32,
    mut snapshot: RawMemorySnapshot,
    value_type: ScanValueType,
    comparison: ScanComparison,
    exact: Option<ScanValue>,
    range: Option<(ScanValue, ScanValue)>,
    object_threshold: usize,
    progress: Option<Arc<AtomicUsize>>,
) -> io::Result<SnapshotFilterResult> {
    if snapshot.chunks.is_empty() {
        return Ok(SnapshotFilterResult::Candidates(Vec::new()));
    }
    let max_threads = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 32);
    let worker_count = max_threads.min(snapshot.chunks.len().max(1));
    let chunk_len = snapshot.chunks.len().div_ceil(worker_count);
    let alignment = snapshot.alignment.max(1);

    let results = thread::scope(|scope| {
        snapshot
            .chunks
            .chunks_mut(chunk_len)
            .map(|chunk_slice| {
                let progress = progress.clone();
                scope.spawn(move || -> io::Result<Vec<(RawMemorySnapshotChunk, usize)>> {
                    crate::platform::set_current_thread_high_priority();
                    let process = ScanProcess::open(pid, false)?;
                    let mut fresh_buffer = vec![0u8; SCAN_CHUNK_BYTES];
                    let mut mask_buffer = Vec::new();
                    let mut kept = Vec::new();

                    for snap_chunk in chunk_slice {
                        let len = snap_chunk.data.len();
                        if len < value_type.width() {
                            continue;
                        }
                        if fresh_buffer.len() < len {
                            fresh_buffer.resize(len, 0);
                        }
                        if let Ok(count) = process.read(snap_chunk.base, &mut fresh_buffer[..len]) {
                            if let Some(p) = progress.as_ref() {
                                p.fetch_add(count, Ordering::Relaxed);
                            }
                            let width = value_type.width();
                            let valid_len = count.min(snap_chunk.data.len());
                            if valid_len < width {
                                continue;
                            }
                            let slots_count = (valid_len - width) / alignment + 1;
                            let words = slots_count.div_ceil(64);
                            if mask_buffer.len() < words {
                                mask_buffer.resize(words, 0);
                            }
                            let chunk_matches = filter_snapshot_chunk_mask(
                                &fresh_buffer[..valid_len],
                                &snap_chunk.data[..valid_len],
                                snap_chunk.mask.as_deref(),
                                &mut mask_buffer[..words],
                                slots_count,
                                alignment,
                                value_type,
                                comparison,
                                exact,
                                range,
                            );
                            if chunk_matches > 0 {
                                snap_chunk.data[..valid_len].copy_from_slice(&fresh_buffer[..valid_len]);
                                if snap_chunk.data.len() > valid_len {
                                    snap_chunk.data.truncate(valid_len);
                                }
                                let mask = mask_buffer[..words].to_vec();
                                let kept_chunk = RawMemorySnapshotChunk {
                                    base: snap_chunk.base,
                                    data: std::mem::take(&mut snap_chunk.data),
                                    mask: Some(mask),
                                };
                                kept.push((kept_chunk, chunk_matches));
                            }
                        }
                    }
                    Ok(kept)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|w| w.join().map_err(|_| io::Error::other("worker panicked"))?)
            .collect::<io::Result<Vec<_>>>()
    })?;

    let all_kept = results.into_iter().flatten().collect::<Vec<_>>();
    let total_matches: usize = all_kept.iter().map(|(_, m)| *m).sum();

    if total_matches > object_threshold {
        let chunks = all_kept.into_iter().map(|(c, _)| c).collect();
        Ok(SnapshotFilterResult::Snapshot(RawMemorySnapshot {
            chunks,
            total_slots: total_matches,
            alignment,
        }))
    } else {
        let mut candidates = Vec::with_capacity(total_matches);
        for (c, _) in all_kept {
            if let Some(mask) = c.mask.as_deref() {
                extract_candidates_from_chunk(
                    c.base,
                    &c.data,
                    mask,
                    value_type,
                    alignment,
                    &mut candidates,
                );
            }
        }
        Ok(SnapshotFilterResult::Candidates(candidates))
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn filter_snapshot_chunk_i32_avx2(
    fresh: &[u8],
    snap: &[u8],
    old_mask: Option<&[u64]>,
    new_mask: &mut [u64],
    slots_count: usize,
    comparison: ScanComparison,
    exact_val: Option<i32>,
    range_val: Option<(i32, i32)>,
) -> usize {
    let fresh_ptr = fresh.as_ptr();
    let snap_ptr = snap.as_ptr();
    let full_words = slots_count / 64;
    let mut total_matches = 0usize;

    let target_v = _mm256_set1_epi32(exact_val.unwrap_or(0));
    let (min_v, max_v) = match range_val {
        Some((min, max)) => (_mm256_set1_epi32(min), _mm256_set1_epi32(max)),
        None => (_mm256_setzero_si256(), _mm256_setzero_si256()),
    };
    let all_ones = _mm256_set1_epi32(-1);

    macro_rules! avx2_i32_loop {
        ($cmp_fn:expr) => {{
            for word_idx in 0..full_words {
                let old_word = match old_mask {
                    Some(old) => {
                        let w = if word_idx < old.len() { old[word_idx] } else { 0 };
                        if w == 0 {
                            new_mask[word_idx] = 0;
                            continue;
                        }
                        w
                    }
                    None => u64::MAX,
                };

                let base_offset = word_idx * 256;
                let mut word_res = 0u64;

                for v in 0..8 {
                    if ((old_word >> (v * 8)) & 0xFF) == 0 {
                        continue;
                    }
                    let off = base_offset + v * 32;
                    let f = _mm256_loadu_si256(fresh_ptr.add(off) as *const __m256i);
                    let s = _mm256_loadu_si256(snap_ptr.add(off) as *const __m256i);
                    let cmp = $cmp_fn(f, s);
                    let byte_mask = _mm256_movemask_ps(_mm256_castsi256_ps(cmp)) as u8;
                    word_res |= (byte_mask as u64) << (v * 8);
                }

                let final_word = word_res & old_word;
                new_mask[word_idx] = final_word;
                total_matches += final_word.count_ones() as usize;
            }
        }};
    }

    match comparison {
        ScanComparison::Unchanged => {
            avx2_i32_loop!(|f, s| _mm256_cmpeq_epi32(f, s));
        }
        ScanComparison::Changed => {
            avx2_i32_loop!(|f, s| _mm256_xor_si256(_mm256_cmpeq_epi32(f, s), all_ones));
        }
        ScanComparison::Increased => {
            avx2_i32_loop!(|f, s| _mm256_cmpgt_epi32(f, s));
        }
        ScanComparison::Decreased => {
            avx2_i32_loop!(|f, s| _mm256_cmpgt_epi32(s, f));
        }
        ScanComparison::Exact => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_i32_loop!(|f, _s| _mm256_cmpeq_epi32(f, target_v));
        }
        ScanComparison::Greater => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_i32_loop!(|f, _s| _mm256_cmpgt_epi32(f, target_v));
        }
        ScanComparison::Less => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_i32_loop!(|f, _s| _mm256_cmpgt_epi32(target_v, f));
        }
        ScanComparison::Between => {
            if range_val.is_none() {
                return 0;
            }
            avx2_i32_loop!(|f, _s| {
                let not_lt_min = _mm256_xor_si256(_mm256_cmpgt_epi32(min_v, f), all_ones);
                let not_gt_max = _mm256_xor_si256(_mm256_cmpgt_epi32(f, max_v), all_ones);
                _mm256_and_si256(not_lt_min, not_gt_max)
            });
        }
    }

    let processed_slots = full_words * 64;
    if processed_slots < slots_count {
        let last_word_idx = full_words;
        let mut active_bits = match old_mask {
            Some(old) => if last_word_idx < old.len() { old[last_word_idx] } else { 0 },
            None => u64::MAX,
        };
        let mut word_matches = 0u64;
        let rem_count = slots_count - processed_slots;
        let valid_mask = if rem_count >= 64 { u64::MAX } else { (1u64 << rem_count) - 1 };
        active_bits &= valid_mask;

        while active_bits != 0 {
            let bit_idx = active_bits.trailing_zeros() as usize;
            let slot_idx = processed_slots + bit_idx;
            let offset = slot_idx * 4;
            let cur = (fresh_ptr.add(offset) as *const i32).read_unaligned();
            let prev = (snap_ptr.add(offset) as *const i32).read_unaligned();
            let matches = match comparison {
                ScanComparison::Unchanged => cur == prev,
                ScanComparison::Changed => cur != prev,
                ScanComparison::Increased => cur > prev,
                ScanComparison::Decreased => cur < prev,
                ScanComparison::Exact => exact_val.is_some_and(|e| cur == e),
                ScanComparison::Greater => exact_val.is_some_and(|e| cur > e),
                ScanComparison::Less => exact_val.is_some_and(|e| cur < e),
                ScanComparison::Between => {
                    range_val.is_some_and(|(min, max)| cur >= min && cur <= max)
                }
            };
            if matches {
                word_matches |= 1u64 << bit_idx;
            }
            active_bits &= active_bits - 1;
        }
        new_mask[last_word_idx] = word_matches;
        total_matches += word_matches.count_ones() as usize;
    }

    total_matches
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn filter_snapshot_chunk_f32_avx2(
    fresh: &[u8],
    snap: &[u8],
    old_mask: Option<&[u64]>,
    new_mask: &mut [u64],
    slots_count: usize,
    comparison: ScanComparison,
    exact_val: Option<f32>,
    range_val: Option<(f32, f32)>,
) -> usize {
    let fresh_ptr = fresh.as_ptr();
    let snap_ptr = snap.as_ptr();
    let full_words = slots_count / 64;
    let mut total_matches = 0usize;

    let target_v = _mm256_set1_ps(exact_val.unwrap_or(0.0));
    let (min_v, max_v) = match range_val {
        Some((min, max)) => (_mm256_set1_ps(min), _mm256_set1_ps(max)),
        None => (_mm256_setzero_ps(), _mm256_setzero_ps()),
    };
    let tol_v = if let Some(e) = exact_val {
        _mm256_set1_ps((e.abs() * 1e-6).max(1e-5))
    } else {
        _mm256_setzero_ps()
    };
    let abs_mask = _mm256_castsi256_ps(_mm256_set1_epi32(0x7FFF_FFFF));
    let all_ones = _mm256_set1_epi32(-1);

    macro_rules! avx2_f32_loop {
        ($cmp_fn:expr) => {{
            for word_idx in 0..full_words {
                let old_word = match old_mask {
                    Some(old) => {
                        let w = if word_idx < old.len() { old[word_idx] } else { 0 };
                        if w == 0 {
                            new_mask[word_idx] = 0;
                            continue;
                        }
                        w
                    }
                    None => u64::MAX,
                };

                let base_offset = word_idx * 256;
                let mut word_res = 0u64;

                for v in 0..8 {
                    if ((old_word >> (v * 8)) & 0xFF) == 0 {
                        continue;
                    }
                    let off = base_offset + v * 32;
                    let f = _mm256_loadu_ps(fresh_ptr.add(off) as *const f32);
                    let s = _mm256_loadu_ps(snap_ptr.add(off) as *const f32);
                    let cmp = $cmp_fn(f, s);
                    let byte_mask = _mm256_movemask_ps(cmp) as u8;
                    word_res |= (byte_mask as u64) << (v * 8);
                }

                let final_word = word_res & old_word;
                new_mask[word_idx] = final_word;
                total_matches += final_word.count_ones() as usize;
            }
        }};
    }

    match comparison {
        ScanComparison::Unchanged => {
            avx2_f32_loop!(|f, s| {
                let f_i = _mm256_castps_si256(f);
                let s_i = _mm256_castps_si256(s);
                _mm256_castsi256_ps(_mm256_cmpeq_epi32(f_i, s_i))
            });
        }
        ScanComparison::Changed => {
            avx2_f32_loop!(|f, s| {
                let f_i = _mm256_castps_si256(f);
                let s_i = _mm256_castps_si256(s);
                _mm256_castsi256_ps(_mm256_xor_si256(_mm256_cmpeq_epi32(f_i, s_i), all_ones))
            });
        }
        ScanComparison::Increased => {
            avx2_f32_loop!(|f, s| _mm256_cmp_ps(f, s, AVX_CMP_GT_OQ));
        }
        ScanComparison::Decreased => {
            avx2_f32_loop!(|f, s| _mm256_cmp_ps(s, f, AVX_CMP_GT_OQ));
        }
        ScanComparison::Exact => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_f32_loop!(|f, _s| {
                let diff = _mm256_and_ps(_mm256_sub_ps(f, target_v), abs_mask);
                _mm256_cmp_ps(diff, tol_v, AVX_CMP_LE_OQ)
            });
        }
        ScanComparison::Greater => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_f32_loop!(|f, _s| _mm256_cmp_ps(f, target_v, AVX_CMP_GT_OQ));
        }
        ScanComparison::Less => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_f32_loop!(|f, _s| _mm256_cmp_ps(target_v, f, AVX_CMP_GT_OQ));
        }
        ScanComparison::Between => {
            if range_val.is_none() {
                return 0;
            }
            avx2_f32_loop!(|f, _s| {
                let ge_min = _mm256_cmp_ps(f, min_v, AVX_CMP_GE_OQ);
                let le_max = _mm256_cmp_ps(f, max_v, AVX_CMP_LE_OQ);
                _mm256_and_ps(ge_min, le_max)
            });
        }
    }

    let processed_slots = full_words * 64;
    if processed_slots < slots_count {
        let last_word_idx = full_words;
        let mut active_bits = match old_mask {
            Some(old) => if last_word_idx < old.len() { old[last_word_idx] } else { 0 },
            None => u64::MAX,
        };
        let mut word_matches = 0u64;
        let rem_count = slots_count - processed_slots;
        let valid_mask = if rem_count >= 64 { u64::MAX } else { (1u64 << rem_count) - 1 };
        active_bits &= valid_mask;

        while active_bits != 0 {
            let bit_idx = active_bits.trailing_zeros() as usize;
            let slot_idx = processed_slots + bit_idx;
            let offset = slot_idx * 4;
            let cur = (fresh_ptr.add(offset) as *const f32).read_unaligned();
            let prev = (snap_ptr.add(offset) as *const f32).read_unaligned();
            let matches = match comparison {
                ScanComparison::Unchanged => cur.to_bits() == prev.to_bits(),
                ScanComparison::Changed => cur.to_bits() != prev.to_bits(),
                ScanComparison::Increased => cur > prev,
                ScanComparison::Decreased => cur < prev,
                ScanComparison::Exact => exact_val.is_some_and(|e| (cur - e).abs() <= (e.abs() * 1e-6).max(1e-5)),
                ScanComparison::Greater => exact_val.is_some_and(|e| cur > e),
                ScanComparison::Less => exact_val.is_some_and(|e| cur < e),
                ScanComparison::Between => {
                    range_val.is_some_and(|(min, max)| cur >= min && cur <= max)
                }
            };
            if matches {
                word_matches |= 1u64 << bit_idx;
            }
            active_bits &= active_bits - 1;
        }
        new_mask[last_word_idx] = word_matches;
        total_matches += word_matches.count_ones() as usize;
    }

    total_matches
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn filter_snapshot_chunk_i16_avx2(
    fresh: &[u8],
    snap: &[u8],
    old_mask: Option<&[u64]>,
    new_mask: &mut [u64],
    slots_count: usize,
    comparison: ScanComparison,
    exact_val: Option<i16>,
    range_val: Option<(i16, i16)>,
) -> usize {
    let fresh_ptr = fresh.as_ptr();
    let snap_ptr = snap.as_ptr();
    let full_words = slots_count / 64;
    let mut total_matches = 0usize;

    let target_v = _mm256_set1_epi16(exact_val.unwrap_or(0));
    let (min_v, max_v) = match range_val {
        Some((min, max)) => (_mm256_set1_epi16(min), _mm256_set1_epi16(max)),
        None => (_mm256_setzero_si256(), _mm256_setzero_si256()),
    };
    let all_ones = _mm256_set1_epi16(-1);

    macro_rules! avx2_i16_loop {
        ($cmp_fn:expr) => {{
            for word_idx in 0..full_words {
                let old_word = match old_mask {
                    Some(old) => {
                        let w = if word_idx < old.len() { old[word_idx] } else { 0 };
                        if w == 0 {
                            new_mask[word_idx] = 0;
                            continue;
                        }
                        w
                    }
                    None => u64::MAX,
                };

                let base_offset = word_idx * 128;
                let mut word_res = 0u64;

                for v in 0..4 {
                    if ((old_word >> (v * 16)) & 0xFFFF) == 0 {
                        continue;
                    }
                    let off = base_offset + v * 32;
                    let f = _mm256_loadu_si256(fresh_ptr.add(off) as *const __m256i);
                    let s = _mm256_loadu_si256(snap_ptr.add(off) as *const __m256i);
                    let cmp = $cmp_fn(f, s);
                    let byte_mask = _mm256_movemask_epi8(cmp) as u32;
                    let mut mask16 = 0u16;
                    for bit in 0..16 {
                        if (byte_mask & (1 << (bit * 2))) != 0 {
                            mask16 |= 1 << bit;
                        }
                    }
                    word_res |= (mask16 as u64) << (v * 16);
                }

                let final_word = word_res & old_word;
                new_mask[word_idx] = final_word;
                total_matches += final_word.count_ones() as usize;
            }
        }};
    }

    match comparison {
        ScanComparison::Unchanged => {
            avx2_i16_loop!(|f, s| _mm256_cmpeq_epi16(f, s));
        }
        ScanComparison::Changed => {
            avx2_i16_loop!(|f, s| _mm256_xor_si256(_mm256_cmpeq_epi16(f, s), all_ones));
        }
        ScanComparison::Increased => {
            avx2_i16_loop!(|f, s| _mm256_cmpgt_epi16(f, s));
        }
        ScanComparison::Decreased => {
            avx2_i16_loop!(|f, s| _mm256_cmpgt_epi16(s, f));
        }
        ScanComparison::Exact => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_i16_loop!(|f, _s| _mm256_cmpeq_epi16(f, target_v));
        }
        ScanComparison::Greater => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_i16_loop!(|f, _s| _mm256_cmpgt_epi16(f, target_v));
        }
        ScanComparison::Less => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_i16_loop!(|f, _s| _mm256_cmpgt_epi16(target_v, f));
        }
        ScanComparison::Between => {
            if range_val.is_none() {
                return 0;
            }
            avx2_i16_loop!(|f, _s| {
                let not_lt_min = _mm256_xor_si256(_mm256_cmpgt_epi16(min_v, f), all_ones);
                let not_gt_max = _mm256_xor_si256(_mm256_cmpgt_epi16(f, max_v), all_ones);
                _mm256_and_si256(not_lt_min, not_gt_max)
            });
        }
    }

    let processed_slots = full_words * 64;
    if processed_slots < slots_count {
        let last_word_idx = full_words;
        let mut active_bits = match old_mask {
            Some(old) => if last_word_idx < old.len() { old[last_word_idx] } else { 0 },
            None => u64::MAX,
        };
        let rem_count = slots_count - processed_slots;
        let valid_mask = if rem_count >= 64 { u64::MAX } else { (1u64 << rem_count) - 1 };
        active_bits &= valid_mask;

        let mut word_matches = 0u64;
        while active_bits != 0 {
            let bit_idx = active_bits.trailing_zeros() as usize;
            let slot_idx = processed_slots + bit_idx;
            let offset = slot_idx * 2;
            let cur = (fresh_ptr.add(offset) as *const i16).read_unaligned();
            let prev = (snap_ptr.add(offset) as *const i16).read_unaligned();
            let matches = match comparison {
                ScanComparison::Unchanged => cur == prev,
                ScanComparison::Changed => cur != prev,
                ScanComparison::Increased => cur > prev,
                ScanComparison::Decreased => cur < prev,
                ScanComparison::Exact => exact_val.map_or(false, |e| cur == e),
                ScanComparison::Greater => exact_val.map_or(false, |e| cur > e),
                ScanComparison::Less => exact_val.map_or(false, |e| cur < e),
                ScanComparison::Between => range_val.map_or(false, |(min, max)| cur >= min && cur <= max),
            };
            if matches {
                word_matches |= 1u64 << bit_idx;
            }
            active_bits &= active_bits - 1;
        }
        new_mask[last_word_idx] = word_matches;
        total_matches += word_matches.count_ones() as usize;
    }
    total_matches
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn filter_snapshot_chunk_i8_avx2(
    fresh: &[u8],
    snap: &[u8],
    old_mask: Option<&[u64]>,
    new_mask: &mut [u64],
    slots_count: usize,
    comparison: ScanComparison,
    exact_val: Option<i8>,
    range_val: Option<(i8, i8)>,
) -> usize {
    let fresh_ptr = fresh.as_ptr();
    let snap_ptr = snap.as_ptr();
    let full_words = slots_count / 64;
    let mut total_matches = 0usize;

    let target_v = _mm256_set1_epi8(exact_val.unwrap_or(0));
    let (min_v, max_v) = match range_val {
        Some((min, max)) => (_mm256_set1_epi8(min), _mm256_set1_epi8(max)),
        None => (_mm256_setzero_si256(), _mm256_setzero_si256()),
    };
    let all_ones = _mm256_set1_epi8(-1);

    macro_rules! avx2_i8_loop {
        ($cmp_fn:expr) => {{
            for word_idx in 0..full_words {
                let old_word = match old_mask {
                    Some(old) => {
                        let w = if word_idx < old.len() { old[word_idx] } else { 0 };
                        if w == 0 {
                            new_mask[word_idx] = 0;
                            continue;
                        }
                        w
                    }
                    None => u64::MAX,
                };

                let base_offset = word_idx * 64;
                let mut word_res = 0u64;

                for v in 0..2 {
                    if ((old_word >> (v * 32)) & 0xFFFF_FFFF) == 0 {
                        continue;
                    }
                    let off = base_offset + v * 32;
                    let f = _mm256_loadu_si256(fresh_ptr.add(off) as *const __m256i);
                    let s = _mm256_loadu_si256(snap_ptr.add(off) as *const __m256i);
                    let cmp = $cmp_fn(f, s);
                    let mask_32 = _mm256_movemask_epi8(cmp) as u32;
                    word_res |= (mask_32 as u64) << (v * 32);
                }

                let final_word = word_res & old_word;
                new_mask[word_idx] = final_word;
                total_matches += final_word.count_ones() as usize;
            }
        }};
    }

    match comparison {
        ScanComparison::Unchanged => {
            avx2_i8_loop!(|f, s| _mm256_cmpeq_epi8(f, s));
        }
        ScanComparison::Changed => {
            avx2_i8_loop!(|f, s| _mm256_xor_si256(_mm256_cmpeq_epi8(f, s), all_ones));
        }
        ScanComparison::Increased => {
            avx2_i8_loop!(|f, s| _mm256_cmpgt_epi8(f, s));
        }
        ScanComparison::Decreased => {
            avx2_i8_loop!(|f, s| _mm256_cmpgt_epi8(s, f));
        }
        ScanComparison::Exact => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_i8_loop!(|f, _s| _mm256_cmpeq_epi8(f, target_v));
        }
        ScanComparison::Greater => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_i8_loop!(|f, _s| _mm256_cmpgt_epi8(f, target_v));
        }
        ScanComparison::Less => {
            if exact_val.is_none() {
                return 0;
            }
            avx2_i8_loop!(|f, _s| _mm256_cmpgt_epi8(target_v, f));
        }
        ScanComparison::Between => {
            if range_val.is_none() {
                return 0;
            }
            avx2_i8_loop!(|f, _s| {
                let not_lt_min = _mm256_xor_si256(_mm256_cmpgt_epi8(min_v, f), all_ones);
                let not_gt_max = _mm256_xor_si256(_mm256_cmpgt_epi8(f, max_v), all_ones);
                _mm256_and_si256(not_lt_min, not_gt_max)
            });
        }
    }

    let processed_slots = full_words * 64;
    if processed_slots < slots_count {
        let last_word_idx = full_words;
        let mut active_bits = match old_mask {
            Some(old) => if last_word_idx < old.len() { old[last_word_idx] } else { 0 },
            None => u64::MAX,
        };
        let mut word_matches = 0u64;
        let rem_count = slots_count - processed_slots;
        let valid_mask = if rem_count >= 64 { u64::MAX } else { (1u64 << rem_count) - 1 };
        active_bits &= valid_mask;

        while active_bits != 0 {
            let bit_idx = active_bits.trailing_zeros() as usize;
            let slot_idx = processed_slots + bit_idx;
            let offset = slot_idx;
            let cur = *fresh_ptr.add(offset) as i8;
            let prev = *snap_ptr.add(offset) as i8;
            let matches = match comparison {
                ScanComparison::Unchanged => cur == prev,
                ScanComparison::Changed => cur != prev,
                ScanComparison::Increased => cur > prev,
                ScanComparison::Decreased => cur < prev,
                ScanComparison::Exact => exact_val.is_some_and(|e| cur == e),
                ScanComparison::Greater => exact_val.is_some_and(|e| cur > e),
                ScanComparison::Less => exact_val.is_some_and(|e| cur < e),
                ScanComparison::Between => {
                    range_val.is_some_and(|(min, max)| cur >= min && cur <= max)
                }
            };
            if matches {
                word_matches |= 1u64 << bit_idx;
            }
            active_bits &= active_bits - 1;
        }
        new_mask[last_word_idx] = word_matches;
        total_matches += word_matches.count_ones() as usize;
    }

    total_matches
}

#[inline(always)]
fn run_mask_loop<F: Fn(usize) -> bool>(
    old_mask: Option<&[u64]>,
    new_mask: &mut [u64],
    slots_count: usize,
    alignment: usize,
    check: F,
) -> usize {
    let words = slots_count.div_ceil(64);
    let mut total_matches = 0usize;
    if let Some(old) = old_mask {
        for word_idx in 0..words {
            let mut active_bits = if word_idx < old.len() { old[word_idx] } else { 0 };
            if active_bits == 0 {
                new_mask[word_idx] = 0;
                continue;
            }
            let mut word_matches = 0u64;
            while active_bits != 0 {
                let bit_idx = active_bits.trailing_zeros() as usize;
                let slot_idx = word_idx * 64 + bit_idx;
                if slot_idx < slots_count {
                    let offset = slot_idx * alignment;
                    if check(offset) {
                        word_matches |= 1u64 << bit_idx;
                    }
                }
                active_bits &= active_bits - 1;
            }
            new_mask[word_idx] = word_matches;
            total_matches += word_matches.count_ones() as usize;
        }
    } else {
        for word_idx in 0..words {
            let start_slot = word_idx * 64;
            let end_slot = (start_slot + 64).min(slots_count);
            let mut word_matches = 0u64;
            for slot_idx in start_slot..end_slot {
                let bit_idx = slot_idx - start_slot;
                let offset = slot_idx * alignment;
                if check(offset) {
                    word_matches |= 1u64 << bit_idx;
                }
            }
            new_mask[word_idx] = word_matches;
            total_matches += word_matches.count_ones() as usize;
        }
    }
    total_matches
}

fn filter_snapshot_chunk_mask(
    fresh: &[u8],
    snap: &[u8],
    old_mask: Option<&[u64]>,
    new_mask: &mut [u64],
    slots_count: usize,
    alignment: usize,
    value_type: ScanValueType,
    comparison: ScanComparison,
    exact: Option<ScanValue>,
    range: Option<(ScanValue, ScanValue)>,
) -> usize {
    let width = value_type.width();
    let count = fresh.len().min(snap.len());
    if count < width || slots_count == 0 {
        return 0;
    }
    let fresh_ptr = fresh.as_ptr();
    let snap_ptr = snap.as_ptr();

    macro_rules! dispatch_snapshot_int {
        ($ty:ty, $variant:ident, $read_expr:expr) => {{
            let exact_val = match exact {
                Some(ScanValue::$variant(v)) => Some(v),
                _ => None,
            };
            let (min_val, max_val) = match range {
                Some((ScanValue::$variant(min), ScanValue::$variant(max))) => (Some(min), Some(max)),
                _ => (None, None),
            };
            match comparison {
                ScanComparison::Unchanged => {
                    run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                        let cur: $ty = unsafe { $read_expr(offset, fresh_ptr) };
                        let prev: $ty = unsafe { $read_expr(offset, snap_ptr) };
                        cur == prev
                    })
                }
                ScanComparison::Changed => {
                    run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                        let cur: $ty = unsafe { $read_expr(offset, fresh_ptr) };
                        let prev: $ty = unsafe { $read_expr(offset, snap_ptr) };
                        cur != prev
                    })
                }
                ScanComparison::Increased => {
                    run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                        let cur: $ty = unsafe { $read_expr(offset, fresh_ptr) };
                        let prev: $ty = unsafe { $read_expr(offset, snap_ptr) };
                        cur > prev
                    })
                }
                ScanComparison::Decreased => {
                    run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                        let cur: $ty = unsafe { $read_expr(offset, fresh_ptr) };
                        let prev: $ty = unsafe { $read_expr(offset, snap_ptr) };
                        cur < prev
                    })
                }
                ScanComparison::Exact => {
                    if let Some(e) = exact_val {
                        run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                            let cur: $ty = unsafe { $read_expr(offset, fresh_ptr) };
                            cur == e
                        })
                    } else {
                        0
                    }
                }
                ScanComparison::Greater => {
                    if let Some(e) = exact_val {
                        run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                            let cur: $ty = unsafe { $read_expr(offset, fresh_ptr) };
                            cur > e
                        })
                    } else {
                        0
                    }
                }
                ScanComparison::Less => {
                    if let Some(e) = exact_val {
                        run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                            let cur: $ty = unsafe { $read_expr(offset, fresh_ptr) };
                            cur < e
                        })
                    } else {
                        0
                    }
                }
                ScanComparison::Between => {
                    if let (Some(min), Some(max)) = (min_val, max_val) {
                        run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                            let cur: $ty = unsafe { $read_expr(offset, fresh_ptr) };
                            cur >= min && cur <= max
                        })
                    } else {
                        0
                    }
                }
            }
        }};
    }

    macro_rules! dispatch_snapshot_float {
        ($ty:ty, $variant:ident, $rel_eps:expr, $abs_eps:expr) => {{
            let exact_val = match exact {
                Some(ScanValue::$variant(v)) => Some(v),
                _ => None,
            };
            let (min_val, max_val) = match range {
                Some((ScanValue::$variant(min), ScanValue::$variant(max))) => (Some(min), Some(max)),
                _ => (None, None),
            };
            match comparison {
                ScanComparison::Unchanged => {
                    run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                        let cur = unsafe { (fresh_ptr.add(offset) as *const $ty).read_unaligned() };
                        let prev = unsafe { (snap_ptr.add(offset) as *const $ty).read_unaligned() };
                        cur.to_bits() == prev.to_bits()
                    })
                }
                ScanComparison::Changed => {
                    run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                        let cur = unsafe { (fresh_ptr.add(offset) as *const $ty).read_unaligned() };
                        let prev = unsafe { (snap_ptr.add(offset) as *const $ty).read_unaligned() };
                        cur.to_bits() != prev.to_bits()
                    })
                }
                ScanComparison::Increased => {
                    run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                        let cur = unsafe { (fresh_ptr.add(offset) as *const $ty).read_unaligned() };
                        let prev = unsafe { (snap_ptr.add(offset) as *const $ty).read_unaligned() };
                        cur > prev
                    })
                }
                ScanComparison::Decreased => {
                    run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                        let cur = unsafe { (fresh_ptr.add(offset) as *const $ty).read_unaligned() };
                        let prev = unsafe { (snap_ptr.add(offset) as *const $ty).read_unaligned() };
                        cur < prev
                    })
                }
                ScanComparison::Exact => {
                    if let Some(e) = exact_val {
                        run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                            let cur = unsafe { (fresh_ptr.add(offset) as *const $ty).read_unaligned() };
                            (cur - e).abs() <= (e.abs() * $rel_eps).max($abs_eps)
                        })
                    } else {
                        0
                    }
                }
                ScanComparison::Greater => {
                    if let Some(e) = exact_val {
                        run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                            let cur = unsafe { (fresh_ptr.add(offset) as *const $ty).read_unaligned() };
                            cur > e
                        })
                    } else {
                        0
                    }
                }
                ScanComparison::Less => {
                    if let Some(e) = exact_val {
                        run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                            let cur = unsafe { (fresh_ptr.add(offset) as *const $ty).read_unaligned() };
                            cur < e
                        })
                    } else {
                        0
                    }
                }
                ScanComparison::Between => {
                    if let (Some(min), Some(max)) = (min_val, max_val) {
                        run_mask_loop(old_mask, new_mask, slots_count, alignment, |offset| {
                            let cur = unsafe { (fresh_ptr.add(offset) as *const $ty).read_unaligned() };
                            cur >= min && cur <= max
                        })
                    } else {
                        0
                    }
                }
            }
        }};
    }

    match value_type {
        ScanValueType::I32 => {
            #[cfg(target_arch = "x86_64")]
            if alignment == 4 && is_x86_feature_detected!("avx2") {
                let exact_val = match exact {
                    Some(ScanValue::I32(v)) => Some(v),
                    _ => None,
                };
                let range_val = match range {
                    Some((ScanValue::I32(min), ScanValue::I32(max))) => Some((min, max)),
                    _ => None,
                };
                return unsafe {
                    filter_snapshot_chunk_i32_avx2(
                        fresh,
                        snap,
                        old_mask,
                        new_mask,
                        slots_count,
                        comparison,
                        exact_val,
                        range_val,
                    )
                };
            }
            dispatch_snapshot_int!(i32, I32, |off, ptr: *const u8| (ptr.add(off) as *const i32).read_unaligned())
        }
        ScanValueType::F32 => {
            #[cfg(target_arch = "x86_64")]
            if alignment == 4 && is_x86_feature_detected!("avx2") {
                let exact_val = match exact {
                    Some(ScanValue::F32(v)) => Some(v),
                    _ => None,
                };
                let range_val = match range {
                    Some((ScanValue::F32(min), ScanValue::F32(max))) => Some((min, max)),
                    _ => None,
                };
                return unsafe {
                    filter_snapshot_chunk_f32_avx2(
                        fresh,
                        snap,
                        old_mask,
                        new_mask,
                        slots_count,
                        comparison,
                        exact_val,
                        range_val,
                    )
                };
            }
            dispatch_snapshot_float!(f32, F32, 1e-6, 1e-5)
        }
        ScanValueType::I8 => {
            #[cfg(target_arch = "x86_64")]
            if alignment == 1 && is_x86_feature_detected!("avx2") {
                let exact_val = match exact {
                    Some(ScanValue::I8(v)) => Some(v),
                    _ => None,
                };
                let range_val = match range {
                    Some((ScanValue::I8(min), ScanValue::I8(max))) => Some((min, max)),
                    _ => None,
                };
                return unsafe {
                    filter_snapshot_chunk_i8_avx2(
                        fresh,
                        snap,
                        old_mask,
                        new_mask,
                        slots_count,
                        comparison,
                        exact_val,
                        range_val,
                    )
                };
            }
            dispatch_snapshot_int!(i8, I8, |off, ptr: *const u8| *ptr.add(off) as i8)
        }
        ScanValueType::I64 => {
            dispatch_snapshot_int!(i64, I64, |off, ptr: *const u8| (ptr.add(off) as *const i64).read_unaligned())
        }
        ScanValueType::F64 => {
            dispatch_snapshot_float!(f64, F64, 1e-12, 1e-9)
        }
        ScanValueType::I16 => {
            #[cfg(target_arch = "x86_64")]
            if alignment == 2 && is_x86_feature_detected!("avx2") {
                let exact_val = match exact {
                    Some(ScanValue::I16(v)) => Some(v),
                    _ => None,
                };
                let range_val = match range {
                    Some((ScanValue::I16(min), ScanValue::I16(max))) => Some((min, max)),
                    _ => None,
                };
                return unsafe {
                    filter_snapshot_chunk_i16_avx2(
                        fresh,
                        snap,
                        old_mask,
                        new_mask,
                        slots_count,
                        comparison,
                        exact_val,
                        range_val,
                    )
                };
            }
            dispatch_snapshot_int!(i16, I16, |off, ptr: *const u8| (ptr.add(off) as *const i16).read_unaligned())
        }
    }
}

fn extract_candidates_from_chunk(
    chunk_base: usize,
    data: &[u8],
    mask: &[u64],
    value_type: ScanValueType,
    alignment: usize,
    found: &mut Vec<ScanCandidate>,
) {
    let width = value_type.width();
    let data_ptr = data.as_ptr();
    let data_len = data.len();

    macro_rules! extract_loop {
        ($extract_fn:expr) => {
            for (word_idx, &word) in mask.iter().enumerate() {
                if word == 0 {
                    continue;
                }
                let mut bits = word;
                while bits != 0 {
                    let bit_idx = bits.trailing_zeros() as usize;
                    let slot_idx = word_idx * 64 + bit_idx;
                    let offset = slot_idx * alignment;
                    if offset + width <= data_len {
                        let address = chunk_base + offset;
                        found.push(unsafe { $extract_fn(address, offset) });
                    }
                    bits &= bits - 1;
                }
            }
        };
    }

    match value_type {
        ScanValueType::I8 => {
            extract_loop!(|addr, off| ScanCandidate::new_i8(addr, *data_ptr.add(off) as i8));
        }
        ScanValueType::I16 => {
            extract_loop!(|addr, off| ScanCandidate::new_i16(addr, (data_ptr.add(off) as *const i16).read_unaligned()));
        }
        ScanValueType::I32 => {
            extract_loop!(|addr, off| ScanCandidate::new_i32(addr, (data_ptr.add(off) as *const i32).read_unaligned()));
        }
        ScanValueType::F32 => {
            extract_loop!(|addr, off| ScanCandidate::new_f32(addr, (data_ptr.add(off) as *const f32).read_unaligned()));
        }
        ScanValueType::I64 => {
            extract_loop!(|addr, off| ScanCandidate::new_i64(addr, (data_ptr.add(off) as *const i64).read_unaligned()));
        }
        ScanValueType::F64 => {
            extract_loop!(|addr, off| ScanCandidate::new_f64(addr, (data_ptr.add(off) as *const f64).read_unaligned()));
        }
    }
}

fn claim_result_slots(total: &AtomicUsize, requested: usize, limit: usize) -> usize {
    let previous = total
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_add(requested).min(limit))
        })
        .unwrap_or_else(|current| current);
    limit.saturating_sub(previous).min(requested)
}

pub fn read_value(pid: u32, address: usize, value_type: MemoryValueType) -> io::Result<String> {
    let mut bytes = [0u8; 8];
    let width = match value_type {
        MemoryValueType::I8 => 1,
        MemoryValueType::I16 => 2,
        MemoryValueType::I32 | MemoryValueType::F32 => 4,
        MemoryValueType::I64 | MemoryValueType::F64 => 8,
    };
    let read = with_cached_read_process(pid, |process| process.read(address, &mut bytes[..width]))?;
    if read != width {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "partial process-memory read",
        ));
    }

    Ok(match value_type {
        MemoryValueType::I8 => i8::from_le_bytes(bytes[..1].try_into().unwrap()).to_string(),
        MemoryValueType::I16 => i16::from_le_bytes(bytes[..2].try_into().unwrap()).to_string(),
        MemoryValueType::I32 => i32::from_le_bytes(bytes[..4].try_into().unwrap()).to_string(),
        MemoryValueType::F32 => f32::from_le_bytes(bytes[..4].try_into().unwrap()).to_string(),
        MemoryValueType::I64 => i64::from_le_bytes(bytes).to_string(),
        MemoryValueType::F64 => f64::from_le_bytes(bytes).to_string(),
    })
}

unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
    fn ReadProcessMemory(
        process: *mut c_void,
        address: *const c_void,
        buffer: *mut c_void,
        size: usize,
        bytes_read: *mut usize,
    ) -> i32;
    fn WriteProcessMemory(
        process: *mut c_void,
        address: *mut c_void,
        buffer: *const c_void,
        size: usize,
        bytes_written: *mut usize,
    ) -> i32;
    fn VirtualQueryEx(
        process: *mut c_void,
        address: *const c_void,
        information: *mut MemoryBasicInformation,
        length: usize,
    ) -> usize;
    fn K32QueryWorkingSetEx(process: *mut c_void, information: *mut c_void, length: u32) -> i32;
    fn VirtualProtectEx(
        process: *mut c_void,
        address: *mut c_void,
        size: usize,
        new_protect: u32,
        old_protect: *mut u32,
    ) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
    fn GetExitCodeProcess(process: *mut c_void, exit_code: *mut u32) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

#[cfg(windows)]
pub fn is_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        ok != 0 && exit_code == 259 // STILL_ACTIVE = 259
    }
}

#[cfg(not(windows))]
pub fn is_process_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group_test_hits(locations: &[(usize, usize)], max_gap: usize) -> Vec<EntityListCandidate> {
        let hits = locations
            .iter()
            .map(|&(location, entity_index)| EntityPointerMatch {
                location,
                entity_index,
                entity_base: 0x5000 + entity_index * 0x100,
            })
            .collect::<Vec<_>>();
        group_entity_pointer_hits(
            &hits,
            3,
            8,
            max_gap,
            &[true; 3],
            &[Some([1.0, 2.0, 3.0]); 3],
        )
    }

    #[test]
    fn aob_relocation_matches_wildcards_and_stays_inside_the_module_range() {
        let pattern = parse_aob_pattern("F3 41 ?? 10").unwrap();
        assert!(aob_bytes_equal(&[0xF3, 0x41, 0x0F, 0x10], &pattern));
        assert_eq!(
            intersect_scan_region(
                ScanRegion {
                    base: 0x1000,
                    size: 0x1000,
                },
                0x1800,
                0x2800,
            ),
            Some(ScanRegion {
                base: 0x1800,
                size: 0x800,
            })
        );
    }

    #[test]
    fn aob_wildcard_scenario_outcome_fields() {
        let outcome = AobWildcardScenarioOutcome {
            label: "Wildcard 4 byte (+04..+07)".to_string(),
            pattern: "48 89 5C 24 ?? ?? ?? ?? 48".to_string(),
            wildcard_count: 4,
            match_count: 1,
            sample_address: Some(0x7FF70001000),
        };
        assert_eq!(outcome.wildcard_count, 4);
        assert_eq!(outcome.match_count, 1);
        assert_eq!(outcome.sample_address, Some(0x7FF70001000));
    }

    #[test]
    fn scan_bucket_merge_respects_limit() {
        let candidate = |address| ScanCandidate::new(address, ScanValue::I32(address as i32));
        let merged = merge_scan_buckets(
            vec![
                vec![candidate(1), candidate(2)],
                vec![candidate(3), candidate(4)],
            ],
            3,
        );
        assert_eq!(
            merged.iter().map(|item| item.address).collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    #[test]
    fn entity_list_groups_three_contiguous_pointers() {
        let candidates = group_test_hits(&[(0x1000, 0), (0x1008, 1), (0x1010, 2)], 0x20);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].matched_entities, 3);
        assert_eq!(candidates[0].observed_stride, 8);
        assert!(candidates[0].confidence > 95.0);
    }

    #[test]
    fn entity_list_does_not_group_hits_outside_max_gap() {
        let candidates = group_test_hits(&[(0x1000, 0), (0x1008, 1), (0x2000, 2)], 0x20);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].matched_entities, 2);
        assert_eq!(candidates[0].end_address, 0x1008);
    }

    #[test]
    fn entity_list_sparse_null_slot_keeps_one_candidate() {
        let candidates = group_test_hits(&[(0x1000, 0), (0x1008, 1), (0x1018, 2)], 0x20);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].matched_entities, 3);
        assert_eq!(candidates[0].observed_stride, 8);
    }

    #[test]
    fn pointer_map_comparison_keeps_entity_root_when_slot_changes() {
        let path_a = PointerPath {
            module: "game.exe".to_owned(),
            module_offset: 0x1234,
            offsets: vec![0x20, 0x90],
        };
        let path_b = PointerPath {
            module: "game.exe".to_owned(),
            module_offset: 0x1234,
            offsets: vec![0x20, 0x120],
        };

        let comparison = compare_pointer_paths([path_a], [path_b], 0x48, 32);

        assert!(comparison.exact.is_empty());
        assert_eq!(comparison.entity_roots.len(), 1);
        assert_eq!(comparison.entity_roots[0].offsets, vec![0x20, 0]);
    }

    #[test]
    fn normal_pointer_map_comparison_ignores_entity_stride_matching() {
        let path_a = PointerPath {
            module: "game.exe".to_owned(),
            module_offset: 0x1234,
            offsets: vec![0x20, 0x90],
        };
        let path_b = PointerPath {
            module: "game.exe".to_owned(),
            module_offset: 0x1234,
            offsets: vec![0x20, 0x120],
        };

        let comparison = compare_pointer_paths([path_a], [path_b], 0, 32);

        assert!(comparison.exact.is_empty());
        assert!(comparison.entity_roots.is_empty());
    }

    #[test]
    fn pointer_map_comparison_finds_entity_slot_at_intermediate_level() {
        let path_a = PointerPath {
            module: "game.exe".to_owned(),
            module_offset: 0x1234,
            offsets: vec![0x20, 0x90, 0x18],
        };
        let path_b = PointerPath {
            module: "game.exe".to_owned(),
            module_offset: 0x1234,
            offsets: vec![0x20, 0x120, 0x18],
        };

        let comparison = compare_pointer_paths([path_a], [path_b], 0x48, 32);

        assert!(comparison.exact.is_empty());
        assert_eq!(comparison.entity_roots.len(), 1);
        assert_eq!(comparison.entity_roots[0].offsets, vec![0x20, 0, 0x18]);
    }

    #[test]
    fn pointer_map_comparison_rejects_multiple_changed_pointer_levels() {
        let path_a = PointerPath {
            module: "game.exe".to_owned(),
            module_offset: 0x1234,
            offsets: vec![0x20, 0x90, 0x18],
        };
        let path_b = PointerPath {
            module: "game.exe".to_owned(),
            module_offset: 0x1234,
            offsets: vec![0x68, 0x120, 0x18],
        };

        let comparison = compare_pointer_paths([path_a], [path_b], 0x48, 32);

        assert!(comparison.exact.is_empty());
        assert!(comparison.entity_roots.is_empty());
    }

    #[test]
    fn result_slots_are_claimed_per_chunk_without_exceeding_limit() {
        let total = AtomicUsize::new(0);
        assert_eq!(claim_result_slots(&total, 8, 10), 8);
        assert_eq!(claim_result_slots(&total, 8, 10), 2);
        assert_eq!(claim_result_slots(&total, 1, 10), 0);
        assert_eq!(total.load(Ordering::Relaxed), 10);
    }

    #[test]
    fn reads_and_writes_double_in_current_process() {
        let mut value = Box::new(3.5f64);
        let address = std::ptr::from_mut(&mut *value).addr();
        let pid = std::process::id();
        assert_eq!(
            read_scan_value(pid, address, ScanValueType::F64).unwrap(),
            ScanValue::F64(3.5)
        );
        write_scan_value(pid, address, ScanValue::F64(7.25)).unwrap();
        assert_eq!(*value, 7.25);
    }

    #[test]
    fn reads_and_writes_float_in_current_process() {
        let mut value = Box::new(123.4f32);
        let address = std::ptr::from_mut(&mut *value).addr();
        let pid = std::process::id();
        assert_eq!(
            read_scan_value(pid, address, ScanValueType::F32).unwrap(),
            ScanValue::F32(123.4)
        );
        write_scan_value(pid, address, ScanValue::F32(-7.25)).unwrap();
        assert_eq!(*value, -7.25);
    }

    #[test]
    fn pointer_paths_keep_offsets_in_dereference_order() {
        let paths = find_pointer_paths_to_any(
            &[(0x1FF0, 0x1010), (0x2FE0, 0x2000)],
            &[0x3000],
            &[("game.exe".to_owned(), 0x1000, 0x100)],
            0x100,
            3,
            8,
            false,
            &[],
            false,
        );
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].module, "game.exe");
        assert_eq!(paths[0].module_offset, 0x10);
        assert_eq!(paths[0].offsets, vec![0x10, 0x20]);
    }

    #[test]
    fn pointer_paths_must_end_with_specific_offsets() {
        let pointers = [(0x1FF0, 0x1010), (0x2FE0, 0x2000)];
        let modules = [("game.exe".to_owned(), 0x1000, 0x100)];

        // Matching both offsets: Offset 0 = 0x20 (hop to target 0x3000), Offset 1 = 0x10
        let paths = find_pointer_paths_to_any(
            &pointers,
            &[0x3000],
            &modules,
            0x100,
            2,
            8,
            false,
            &[0x20, 0x10],
            false,
        );
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].offsets, vec![0x10, 0x20]);

        // Matching only the final offset: Offset 0 = 0x20
        let paths_one = find_pointer_paths_to_any(
            &pointers,
            &[0x3000],
            &modules,
            0x100,
            3,
            8,
            false,
            &[0x20],
            false,
        );
        assert_eq!(paths_one.len(), 1);
        assert_eq!(paths_one[0].offsets, vec![0x10, 0x20]);

        // Wrong final offset: must return 0 results
        let paths_mismatch = find_pointer_paths_to_any(
            &pointers,
            &[0x3000],
            &modules,
            0x100,
            3,
            8,
            false,
            &[0x99],
            false,
        );
        assert!(paths_mismatch.is_empty());

        // Requires 3 offsets but max path length is 2: must return 0 results
        let paths_too_deep = find_pointer_paths_to_any(
            &pointers,
            &[0x3000],
            &modules,
            0x100,
            3,
            8,
            false,
            &[0x20, 0x10, 0x5],
            false,
        );
        assert!(paths_too_deep.is_empty());
    }

    #[test]
    fn exhaustive_pointer_paths_do_not_drop_branches_sharing_nodes() {
        // Target is 0x5000.
        // Node 0x2000 has pointer with value 0x4F00 (offset 0x100 to target 0x5000).
        // Node 0x2050 has pointer with value 0x4E00 (offset 0x200 to target 0x5000).
        // Node 0x4000 has pointer with value 0x1F00:
        //   - offset to 0x2000 is 0x100
        //   - offset to 0x2050 is 0x150
        // Module game.exe at 0x1000..0x1100 has pointer at 0x1010 with value 0x3F00 (offset 0x100 to 0x4000).
        let pointers = vec![
            (0x1F00, 0x4000),
            (0x3F00, 0x1010),
            (0x4E00, 0x2050),
            (0x4F00, 0x2000),
        ];
        let modules = [("game.exe".to_owned(), 0x1000, 0x100)];

        // Legacy non-exhaustive mode: 0x4000 is visited once and dropped for the other branch
        let legacy_paths = find_pointer_paths_to_any(
            &pointers,
            &[0x5000],
            &modules,
            0x200,
            3,
            8,
            false,
            &[],
            false,
        );
        assert_eq!(legacy_paths.len(), 1);

        // Exhaustive mode: does NOT prune 0x4000 for different suffixes, finding BOTH paths!
        let exhaustive_paths = find_pointer_paths_to_any(
            &pointers,
            &[0x5000],
            &modules,
            0x200,
            3,
            8,
            false,
            &[],
            true,
        );
        assert_eq!(exhaustive_paths.len(), 2);
    }

    #[test]
    fn exhaustive_pointer_paths_handles_cycles_without_explosion() {
        // Cycle: 0x2000 points to 0x3000, 0x3000 points to 0x2000.
        // 0x2000 also points to 0x5000 (target).
        // Module game.exe at 0x1000..0x1100 has pointer at 0x1010 pointing to 0x2000.
        let pointers = vec![
            (0x1FFE, 0x1010), // module points to 0x2000 (offset 2)
            (0x1FFE, 0x3000), // 0x3000 points to 0x2000 (offset 2)
            (0x2FFE, 0x2000), // 0x2000 points to 0x3000 (offset 2)
            (0x4FFE, 0x2000), // 0x2000 points to 0x5000 (offset 2)
        ];
        let modules = [("game.exe".to_owned(), 0x1000, 0x100)];

        let paths = find_pointer_paths_to_any(
            &pointers,
            &[0x5000],
            &modules,
            0x10,
            5,
            16,
            false,
            &[],
            true,
        );
        assert!(!paths.is_empty());
        assert_eq!(paths[0].module, "game.exe");
    }

    #[test]
    fn test_parse_must_end_offsets() {
        assert_eq!(parse_must_end_offsets("0, 58"), vec![0x0, 0x58]);
        assert_eq!(parse_must_end_offsets("0x0, 0x58"), vec![0x0, 0x58]);
        assert_eq!(parse_must_end_offsets("10; 20 30"), vec![0x10, 0x20, 0x30]);
        assert_eq!(parse_must_end_offsets(""), Vec::<usize>::new());
    }

    #[test]
    fn snapshot_mask_filtering_and_candidate_extraction() {
        let mut snap_bytes = Vec::new();
        for v in [100i32, 200, 300, 400] {
            snap_bytes.extend_from_slice(&v.to_le_bytes());
        }

        let mut fresh_bytes = Vec::new();
        for v in [100i32, 250, 290, 400] {
            fresh_bytes.extend_from_slice(&v.to_le_bytes());
        }

        // Round 1: Changed (from None mask -> all 4 slots active)
        let mut mask_1 = vec![0u64; 1];
        let matches_1 = filter_snapshot_chunk_mask(
            &fresh_bytes,
            &snap_bytes,
            None,
            &mut mask_1,
            4,
            4,
            ScanValueType::I32,
            ScanComparison::Changed,
            None,
            None,
        );
        assert_eq!(matches_1, 2);
        assert_eq!(mask_1[0], (1 << 1) | (1 << 2));

        // Round 2: Increased (using mask_1 as old_mask)
        let mut mask_2 = vec![0u64; 1];
        let matches_2 = filter_snapshot_chunk_mask(
            &fresh_bytes,
            &snap_bytes,
            Some(&mask_1),
            &mut mask_2,
            4,
            4,
            ScanValueType::I32,
            ScanComparison::Increased,
            None,
            None,
        );
        assert_eq!(matches_2, 1);
        assert_eq!(mask_2[0], 1 << 1);

        // Candidate extraction from mask_2
        let mut candidates = Vec::new();
        extract_candidates_from_chunk(
            0x1000,
            &fresh_bytes,
            &mask_2,
            ScanValueType::I32,
            4,
            &mut candidates,
        );
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].address, 0x1000 + 4);
        assert_eq!(candidates[0].get_i32(), 250);
    }


    #[test]
    fn snapshot_mask_filtering_i16_avx2_verification() {
        let count: usize = 128;
        let mut snap_bytes = Vec::new();
        let mut fresh_bytes = Vec::new();
        for i in 0..count {
            let s_val = (i * 10) as i16;
            // Introduce some equal, increased, decreased, exact
            let f_val = match i % 4 {
                0 => s_val,         // Unchanged
                1 => s_val + 5,     // Increased
                2 => s_val - 5,     // Decreased
                _ => 999i16,        // Exact test
            };
            snap_bytes.extend_from_slice(&s_val.to_le_bytes());
            fresh_bytes.extend_from_slice(&f_val.to_le_bytes());
        }

        // Test Unchanged
        let mut mask_unchanged = vec![0u64; count.div_ceil(64)];
        let m_unc = filter_snapshot_chunk_mask(
            &fresh_bytes,
            &snap_bytes,
            None,
            &mut mask_unchanged,
            count,
            2,
            ScanValueType::I16,
            ScanComparison::Unchanged,
            None,
            None,
        );
        assert_eq!(m_unc, count / 4);

        // Test Increased
        let mut mask_inc = vec![0u64; count.div_ceil(64)];
        let m_inc = filter_snapshot_chunk_mask(
            &fresh_bytes,
            &snap_bytes,
            None,
            &mut mask_inc,
            count,
            2,
            ScanValueType::I16,
            ScanComparison::Increased,
            None,
            None,
        );
        let expected_inc = (0..count).filter(|&i| {
            let s = (i * 10) as i16;
            let f = match i % 4 { 0 => s, 1 => s + 5, 2 => s - 5, _ => 999i16 };
            f > s
        }).count();
        assert_eq!(m_inc, expected_inc);

        // Test Decreased
        let mut mask_dec = vec![0u64; count.div_ceil(64)];
        let m_dec = filter_snapshot_chunk_mask(
            &fresh_bytes,
            &snap_bytes,
            None,
            &mut mask_dec,
            count,
            2,
            ScanValueType::I16,
            ScanComparison::Decreased,
            None,
            None,
        );
        let expected_dec = (0..count).filter(|&i| {
            let s = (i * 10) as i16;
            let f = match i % 4 { 0 => s, 1 => s + 5, 2 => s - 5, _ => 999i16 };
            f < s
        }).count();
        assert_eq!(m_dec, expected_dec);
    }

    #[test]
    fn aob_pattern_matching_and_text_decoding() {
        let pattern = parse_aob_pattern("48 65 ?? 6C 6F").expect("valid pattern");
        let data = b"Hello";
        assert!(aob_bytes_equal(data, &pattern));

        let data_wildcard = b"Hexlo";
        assert!(aob_bytes_equal(data_wildcard, &pattern));

        let data_bad = b"Hxllo";
        assert!(!aob_bytes_equal(data_bad, &pattern));

        let utf8_str = String::from_utf8_lossy(b"PlayerOne\0\0\0");
        assert_eq!(utf8_str.trim_matches(char::from(0)), "PlayerOne");

        let u16_data: Vec<u16> = "PlayerTwo\0".encode_utf16().collect();
        let u8_bytes: Vec<u8> = u16_data.iter().flat_map(|c| c.to_le_bytes()).collect();
        let u16_decoded: Vec<u16> = u8_bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let decoded = String::from_utf16_lossy(&u16_decoded);
        assert_eq!(decoded.trim_matches(char::from(0)), "PlayerTwo");
    }

    #[test]
    fn test_emulator_delta_pointer_translation() {
        let gva: usize = 0x4000_1000_0000;
        let hva: usize = 0x0000_2000_0000;
        let delta: isize = (hva as isize).wrapping_sub(gva as isize);

        let translated = (gva as isize).wrapping_add(delta) as usize;
        assert_eq!(translated, hva);
    }

    #[test]
    fn test_scan_chunk_fast_exact_simd_and_scalar() {
        let mut buffer = vec![0u8; 256];

        // 1. i32 test: place 123456 at aligned offset 16, 32, 64, and unaligned offset 73
        let val_i32: i32 = 123456;
        let bytes_i32 = val_i32.to_le_bytes();
        buffer[16..20].copy_from_slice(&bytes_i32);
        buffer[32..36].copy_from_slice(&bytes_i32);
        buffer[64..68].copy_from_slice(&bytes_i32);
        buffer[73..77].copy_from_slice(&bytes_i32);

        // Step 4 (aligned): should find 16, 32, 64 (73 is unaligned so ignored when step=4)
        let mut found_i32_step4 = Vec::new();
        scan_chunk_fast(0x1000, &buffer, buffer.len(), Some(ScanValue::I32(val_i32)), None, ScanValueType::I32, 4, &mut found_i32_step4);
        let addrs_step4: Vec<usize> = found_i32_step4.iter().map(|c| c.address).collect();
        assert_eq!(addrs_step4, vec![0x1000 + 16, 0x1000 + 32, 0x1000 + 64]);

        // Step 1 (unaligned): should find 16, 32, 64, and 73!
        let mut found_i32_step1 = Vec::new();
        scan_chunk_fast(0x1000, &buffer, buffer.len(), Some(ScanValue::I32(val_i32)), None, ScanValueType::I32, 1, &mut found_i32_step1);
        let addrs_step1: Vec<usize> = found_i32_step1.iter().map(|c| c.address).collect();
        assert_eq!(addrs_step1, vec![0x1000 + 16, 0x1000 + 32, 0x1000 + 64, 0x1000 + 73]);

        // 2. i64 test: place 0x0123456789ABCDEF at offset 8, 40, and unaligned 73
        let mut buf_i64 = vec![0u8; 256];
        let val_i64: i64 = 0x0123456789ABCDEF;
        let bytes_i64 = val_i64.to_le_bytes();
        buf_i64[8..16].copy_from_slice(&bytes_i64);
        buf_i64[40..48].copy_from_slice(&bytes_i64);
        buf_i64[73..81].copy_from_slice(&bytes_i64);

        let mut found_i64_step8 = Vec::new();
        scan_chunk_fast(0x2000, &buf_i64, buf_i64.len(), Some(ScanValue::I64(val_i64)), None, ScanValueType::I64, 8, &mut found_i64_step8);
        let addrs_i64_step8: Vec<usize> = found_i64_step8.iter().map(|c| c.address).collect();
        assert_eq!(addrs_i64_step8, vec![0x2000 + 8, 0x2000 + 40]);

        let mut found_i64_step1 = Vec::new();
        scan_chunk_fast(0x2000, &buf_i64, buf_i64.len(), Some(ScanValue::I64(val_i64)), None, ScanValueType::I64, 1, &mut found_i64_step1);
        let addrs_i64_step1: Vec<usize> = found_i64_step1.iter().map(|c| c.address).collect();
        assert_eq!(addrs_i64_step1, vec![0x2000 + 8, 0x2000 + 40, 0x2000 + 73]);

        // 3. f32 test: place 123.456 at offset 12 and 36
        let mut buf_f32 = vec![0u8; 128];
        let val_f32: f32 = 123.456;
        let bytes_f32 = val_f32.to_le_bytes();
        buf_f32[12..16].copy_from_slice(&bytes_f32);
        buf_f32[36..40].copy_from_slice(&bytes_f32);

        let mut found_f32 = Vec::new();
        scan_chunk_fast(0x3000, &buf_f32, buf_f32.len(), Some(ScanValue::F32(val_f32)), None, ScanValueType::F32, 4, &mut found_f32);
        let addrs_f32: Vec<usize> = found_f32.iter().map(|c| c.address).collect();
        assert_eq!(addrs_f32, vec![0x3000 + 12, 0x3000 + 36]);

        // 4. i8 test: place 42 at offsets 5, 33, 34, 99
        let mut buf_i8 = vec![0u8; 128];
        buf_i8[5] = 42;
        buf_i8[33] = 42;
        buf_i8[34] = 42;
        buf_i8[99] = 42;

        let mut found_i8 = Vec::new();
        scan_chunk_fast(0x4000, &buf_i8, buf_i8.len(), Some(ScanValue::I8(42)), None, ScanValueType::I8, 1, &mut found_i8);
        let addrs_i8: Vec<usize> = found_i8.iter().map(|c| c.address).collect();
        assert_eq!(addrs_i8, vec![0x4000 + 5, 0x4000 + 33, 0x4000 + 34, 0x4000 + 99]);
    }

    #[test]
    fn test_compiled_aob_and_simd_scan_chunk() {
        // 1. CompiledAob matches equivalence with aob_bytes_equal
        let pat_str = "48 89 5C 24 ?? 48 89 74 24 ?? 57 48 83 EC ??";
        let pat = parse_aob_pattern(pat_str).expect("valid pattern");
        let compiled = CompiledAob::new(pat.clone());
        assert_eq!(compiled.len(), 15);
        assert_eq!(compiled.first_exact_idx, Some(0));
        assert_eq!(compiled.first_exact_byte, Some(0x48));

        let matching_data = [
            0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xEC, 0x20,
        ];
        assert!(compiled.matches(&matching_data));
        assert!(aob_bytes_equal(&matching_data, &pat));

        let mismatch_data = [
            0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xEE, 0x20,
        ];
        assert!(!compiled.matches(&mismatch_data));
        assert!(!aob_bytes_equal(&mismatch_data, &pat));

        // 2. Long pattern (>= 32 bytes) testing AVX2 bitmask comparison
        let long_pat_str = "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 41 54 41 55 41 56 41 57 48 83 EC ?? 48 8B ?? ?? ?? ?? ??";
        let long_pat = parse_aob_pattern(long_pat_str).expect("valid long pattern");
        let long_compiled = CompiledAob::new(long_pat.clone());
        assert!(long_compiled.len() >= 32);

        let mut long_data = vec![0u8; long_compiled.len()];
        for (i, b) in long_pat.iter().enumerate() {
            if let AobByte::Exact(v) = b {
                long_data[i] = *v;
            } else {
                long_data[i] = 0xAA;
            }
        }
        assert!(long_compiled.matches(&long_data));
        assert!(aob_bytes_equal(&long_data, &long_pat));

        // Corrupt one exact byte at index 28 (0x48)
        long_data[28] ^= 0xFF;
        assert!(!long_compiled.matches(&long_data));
        assert!(!aob_bytes_equal(&long_data, &long_pat));

        // 3. scan_aob_chunk on 1024-byte haystack with multiple occurrences (aligned & unaligned)
        let mut haystack = vec![0x90u8; 1024];
        let target_offsets = vec![0, 20, 48, 77, 256, 400, 990];
        for &off in &target_offsets {
            haystack[off..off + matching_data.len()].copy_from_slice(&matching_data);
        }

        let mut found_offsets = Vec::new();
        scan_aob_chunk(&haystack, &compiled, |off| {
            found_offsets.push(off);
            true
        });
        assert_eq!(found_offsets, target_offsets);

        // 4. Single-exact-byte pattern: "?? ?? 7E ??"
        let single_pat_str = "?? ?? 7E ??";
        let single_pat = parse_aob_pattern(single_pat_str).unwrap();
        let single_compiled = CompiledAob::new(single_pat);
        let mut single_haystack = vec![0x00u8; 256];
        single_haystack[10] = 0x7E; // match at offset 8 (byte 2 is index 10)
        single_haystack[42] = 0x7E; // match at offset 40
        single_haystack[120] = 0x7E; // match at offset 118

        let mut single_found = Vec::new();
        scan_aob_chunk(&single_haystack, &single_compiled, |off| {
            single_found.push(off);
            true
        });
        assert_eq!(single_found, vec![8, 40, 118]);
    }
}

