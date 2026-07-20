use std::{
    ffi::c_void,
    io,
    mem::{MaybeUninit, size_of},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use crate::model::MemoryValueType;

const PROCESS_VM_READ: u32 = 0x0010;
const PROCESS_VM_WRITE: u32 = 0x0020;
const PROCESS_VM_OPERATION: u32 = 0x0008;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanValueType {
    I8,
    I16,
    I32,
    F32,
    I64,
    F64,
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

    fn decode(self, bytes: &[u8]) -> Option<ScanValue> {
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
    pub previous: ScanValue,
    pub current: ScanValue,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PointerPath {
    pub module: String,
    pub module_offset: usize,
    pub offsets: Vec<usize>,
}

pub struct PointerMap {
    pointers: Vec<(usize, usize)>,
    modules: Vec<(String, usize, usize)>,
}

pub fn capture_pointer_map(
    pid: u32,
    modules: &[(String, usize, usize)],
    progress: Arc<AtomicUsize>,
) -> io::Result<PointerMap> {
    const MAX_POINTERS: usize = 12_000_000;
    let process = ScanProcess::open(pid, false)?;
    let regions = pointer_scan_regions_for(&process);
    let readable_ranges = regions
        .iter()
        .map(|region| (region.base, region.base.saturating_add(region.size)))
        .collect::<Vec<_>>();
    let mut pointers = Vec::new();
    let mut buffer = vec![0u8; SCAN_CHUNK_BYTES];
    for region in regions {
        for offset in (0..region.size).step_by(SCAN_CHUNK_BYTES) {
            let address = region.base + offset;
            let wanted = (region.size - offset).min(SCAN_CHUNK_BYTES);
            let Ok(read) = process.read(address, &mut buffer[..wanted]) else {
                continue;
            };
            for byte_offset in (0..read.saturating_sub(7)).step_by(4) {
                let value = usize::from_le_bytes(
                    buffer[byte_offset..byte_offset + size_of::<usize>()]
                        .try_into()
                        .unwrap(),
                );
                let range = readable_ranges.partition_point(|(base, _)| *base <= value);
                if range > 0 && value < readable_ranges[range - 1].1 {
                    pointers.push((value, address + byte_offset));
                    if pointers.len() >= MAX_POINTERS {
                        break;
                    }
                }
            }
            progress.fetch_add(read, Ordering::Relaxed);
            if pointers.len() >= MAX_POINTERS {
                break;
            }
        }
        if pointers.len() >= MAX_POINTERS {
            break;
        }
    }
    Ok(PointerMap {
        pointers,
        modules: modules.to_vec(),
    })
}

impl PointerMap {
    pub fn paths_to(
        &self,
        target: usize,
        max_offset: usize,
        max_depth: usize,
        result_limit: usize,
    ) -> Vec<PointerPath> {
        find_pointer_paths(
            self.pointers.clone(),
            target,
            &self.modules,
            max_offset,
            max_depth,
            result_limit,
        )
    }
}

pub fn scan_pointer_paths(
    pid: u32,
    target: usize,
    modules: &[(String, usize, usize)],
    max_offset: usize,
    max_depth: usize,
    result_limit: usize,
    progress: Arc<AtomicUsize>,
) -> io::Result<Vec<PointerPath>> {
    // ponytail: these caps keep a pathological process from exhausting app memory; a disk-backed
    // pointer map is the upgrade path for larger scans.
    let map = capture_pointer_map(pid, modules, progress)?;
    Ok(find_pointer_paths(
        map.pointers,
        target,
        modules,
        max_offset,
        max_depth,
        result_limit,
    ))
}

fn find_pointer_paths(
    mut pointers: Vec<(usize, usize)>,
    target: usize,
    modules: &[(String, usize, usize)],
    max_offset: usize,
    max_depth: usize,
    result_limit: usize,
) -> Vec<PointerPath> {
    const MAX_FRONTIER: usize = 50_000;
    pointers.sort_unstable_by_key(|(value, _)| *value);
    let mut results = Vec::new();
    let mut frontier = vec![(target, Vec::<usize>::new())];
    for _ in 0..max_depth.max(1) {
        let mut next = Vec::new();
        for (node, suffix) in frontier {
            let minimum = node.saturating_sub(max_offset);
            let start = pointers.partition_point(|(value, _)| *value < minimum);
            let end = pointers.partition_point(|(value, _)| *value <= node);
            for &(value, location) in &pointers[start..end] {
                let mut reverse_offsets = suffix.clone();
                reverse_offsets.push(node - value);
                if let Some((module, base, _)) = modules
                    .iter()
                    .find(|(_, base, size)| (*base..base.saturating_add(*size)).contains(&location))
                {
                    let mut offsets = reverse_offsets.clone();
                    offsets.reverse();
                    results.push(PointerPath {
                        module: module.clone(),
                        module_offset: location - *base,
                        offsets,
                    });
                    if results.len() >= result_limit.max(1) {
                        return results;
                    }
                }
                if next.len() < MAX_FRONTIER {
                    next.push((location, reverse_offsets));
                }
            }
        }
        if next.is_empty() {
            break;
        }
        next.sort_unstable_by_key(|(address, _)| *address);
        next.dedup_by_key(|(address, _)| *address);
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
}

#[derive(Clone, Copy)]
struct ScanRegion {
    base: usize,
    size: usize,
}

struct ScanProcess {
    handle: *mut c_void,
}

unsafe impl Send for ScanProcess {}

impl Drop for ScanProcess {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

impl ScanProcess {
    fn open(pid: u32, write: bool) -> io::Result<Self> {
        let access = PROCESS_QUERY_LIMITED_INFORMATION
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

    fn read(&self, address: usize, buffer: &mut [u8]) -> io::Result<usize> {
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

pub fn read_scan_value(
    pid: u32,
    address: usize,
    value_type: ScanValueType,
) -> io::Result<ScanValue> {
    let process = ScanProcess::open(pid, false)?;
    let mut bytes = [0; 8];
    let width = value_type.width();
    let read = process.read(address, &mut bytes[..width])?;
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
    let process = ScanProcess::open(pid, false)?;
    let mut bytes = vec![0; length];
    let read = process.read(address, &mut bytes)?;
    bytes.truncate(read);
    Ok(bytes)
}

pub fn write_scan_value(pid: u32, address: usize, value: ScanValue) -> io::Result<()> {
    let process = ScanProcess::open(pid, true)?;
    let bytes = value.bytes();
    let width = value.value_type().width();
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
}

pub fn scan_memory_with_progress(
    pid: u32,
    exact: Option<ScanValue>,
    value_type: ScanValueType,
    result_limit: usize,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<ScanCandidate>> {
    let result_limit = result_limit.max(1);
    let process = ScanProcess::open(pid, false)?;
    let regions = scan_regions_for(&process)
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
    let slots = regions
        .iter()
        .map(|region| region.size / value_type.width())
        .fold(0usize, usize::saturating_add);
    let stride = exact
        .is_none()
        .then(|| slots.div_ceil(result_limit).max(1))
        .unwrap_or(1);
    // ponytail: Memory reads become bandwidth-bound quickly; raise this cap only after profiling.
    let worker_count = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(2)
        .clamp(2, 8)
        .min(regions.len().max(1));
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
    let workers = buckets
        .into_iter()
        .map(|regions| {
            let total = Arc::clone(&total);
            thread::spawn(move || {
                scan_region_bucket(pid, regions, exact, value_type, stride, result_limit, total)
            })
        })
        .collect::<Vec<_>>();
    let mut completed = workers
        .into_iter()
        .filter_map(|worker| worker.join().ok()?.ok());
    let mut found = completed.next().unwrap_or_default();
    for mut bucket in completed {
        found.append(&mut bucket);
    }
    found.truncate(result_limit);
    Ok(found)
}

pub fn filter_scan_candidates(
    pid: u32,
    mut candidates: Vec<ScanCandidate>,
    comparison: ScanComparison,
    exact: Option<ScanValue>,
) -> io::Result<Vec<ScanCandidate>> {
    if candidates.is_empty() {
        return Ok(candidates);
    }
    let worker_count = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(2)
        .clamp(1, 8)
        .min(candidates.len().div_ceil(100_000).max(1));
    let chunk_len = candidates.len().div_ceil(worker_count);
    let kept = thread::scope(|scope| {
        candidates
            .chunks_mut(chunk_len)
            .map(|chunk| scope.spawn(move || filter_candidate_slice(pid, chunk, comparison, exact)))
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
    comparison: ScanComparison,
    exact: Option<ScanValue>,
) -> io::Result<usize> {
    let process = ScanProcess::open(pid, false)?;
    let mut page = [0; PAGE_BYTES];
    let mut index = 0;
    let mut write = 0;
    while index < candidates.len() {
        let page_base = candidates[index].address & !(PAGE_BYTES - 1);
        let mut end = index + 1;
        while end < candidates.len() && candidates[end].address < page_base + PAGE_BYTES {
            end += 1;
        }
        if let Ok(count) = process.read(page_base, &mut page) {
            for read in index..end {
                let candidate = candidates[read];
                let offset = candidate.address - page_base;
                let value_type = candidate.current.value_type();
                if offset + value_type.width() > count {
                    continue;
                }
                let Some(current) = value_type.decode(&page[offset..]) else {
                    continue;
                };
                if scan_value_matches(comparison, current, candidate.previous, exact) {
                    candidates[write] = ScanCandidate {
                        address: candidate.address,
                        previous: current,
                        current,
                    };
                    write += 1;
                }
            }
        }
        index = end;
    }
    Ok(write)
}

pub fn refresh_scan_candidates(pid: u32, candidates: &mut [ScanCandidate]) -> io::Result<()> {
    let process = ScanProcess::open(pid, false)?;
    let mut page = [0; PAGE_BYTES];
    let mut index = 0;
    while index < candidates.len() {
        let page_base = candidates[index].address & !(PAGE_BYTES - 1);
        let mut end = index + 1;
        while end < candidates.len() && candidates[end].address < page_base + PAGE_BYTES {
            end += 1;
        }
        if let Ok(count) = process.read(page_base, &mut page) {
            for candidate in &mut candidates[index..end] {
                let offset = candidate.address - page_base;
                let value_type = candidate.current.value_type();
                if offset + value_type.width() <= count
                    && let Some(current) = value_type.decode(&page[offset..])
                {
                    candidate.current = current;
                }
            }
        }
        index = end;
    }
    Ok(())
}

fn scan_value_matches(
    comparison: ScanComparison,
    current: ScanValue,
    previous: ScanValue,
    exact: Option<ScanValue>,
) -> bool {
    macro_rules! compare {
        ($current:expr, $previous:expr, $variant:path) => {{
            let exact = exact.and_then(|value| match value {
                $variant(value) => Some(value),
                _ => None,
            });
            match comparison {
                ScanComparison::Exact => exact == Some($current),
                ScanComparison::Less => exact.is_some_and(|value| $current < value),
                ScanComparison::Greater => exact.is_some_and(|value| $current > value),
                ScanComparison::Changed => $current != $previous,
                ScanComparison::Unchanged => $current == $previous,
                ScanComparison::Increased => $current > $previous,
                ScanComparison::Decreased => $current < $previous,
            }
        }};
    }
    match (current, previous) {
        (ScanValue::I8(current), ScanValue::I8(previous)) => {
            compare!(current, previous, ScanValue::I8)
        }
        (ScanValue::I16(current), ScanValue::I16(previous)) => {
            compare!(current, previous, ScanValue::I16)
        }
        (ScanValue::I32(current), ScanValue::I32(previous)) => {
            compare!(current, previous, ScanValue::I32)
        }
        (ScanValue::I64(current), ScanValue::I64(previous)) => {
            compare!(current, previous, ScanValue::I64)
        }
        (ScanValue::F32(current), ScanValue::F32(previous)) => match comparison {
            ScanComparison::Changed => current.to_bits() != previous.to_bits(),
            ScanComparison::Unchanged => current.to_bits() == previous.to_bits(),
            _ => compare!(current, previous, ScanValue::F32),
        },
        (ScanValue::F64(current), ScanValue::F64(previous)) => match comparison {
            ScanComparison::Changed => current.to_bits() != previous.to_bits(),
            ScanComparison::Unchanged => current.to_bits() == previous.to_bits(),
            _ => compare!(current, previous, ScanValue::F64),
        },
        _ => false,
    }
}

fn scan_regions_for(process: &ScanProcess) -> Vec<ScanRegion> {
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
            && matches!(information.kind, MEM_PRIVATE | MEM_IMAGE)
            && matches!(
                information.protect & 0xFF,
                PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
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

fn scan_region_bucket(
    pid: u32,
    regions: Vec<ScanRegion>,
    exact: Option<ScanValue>,
    value_type: ScanValueType,
    stride: usize,
    result_limit: usize,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<ScanCandidate>> {
    let process = ScanProcess::open(pid, false)?;
    let mut found = Vec::new();
    let mut buffer = Vec::new();
    'regions: for region in regions {
        let end = region.base.saturating_add(region.size);
        let mut chunk_base = region.base;
        while chunk_base < end {
            let length = (end - chunk_base).min(SCAN_CHUNK_BYTES);
            buffer.resize(length, 0);
            if let Ok(count) = process.read(chunk_base, &mut buffer) {
                let width = value_type.width();
                let step = stride.saturating_mul(width).max(width);
                if count >= width {
                    for offset in (0..=count - width).step_by(step) {
                        let value = value_type.decode(&buffer[offset..]).expect("value width");
                        if exact.is_none() || exact == Some(value) {
                            if total.fetch_add(1, Ordering::Relaxed) >= result_limit {
                                total.fetch_sub(1, Ordering::Relaxed);
                                break 'regions;
                            }
                            found.push(ScanCandidate {
                                address: chunk_base + offset,
                                previous: value,
                                current: value,
                            });
                        }
                    }
                }
            }
            chunk_base = chunk_base.saturating_add(length);
        }
    }
    Ok(found)
}

pub fn read_value(pid: u32, address: usize, value_type: MemoryValueType) -> io::Result<String> {
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }

    let mut bytes = [0u8; 8];
    let width = match value_type {
        MemoryValueType::I8 => 1,
        MemoryValueType::I16 => 2,
        MemoryValueType::I32 | MemoryValueType::F32 => 4,
        MemoryValueType::I64 | MemoryValueType::F64 => 8,
    };
    let mut read = 0;
    let succeeded = unsafe {
        ReadProcessMemory(
            handle,
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            width,
            &mut read,
        )
    } != 0;
    unsafe { CloseHandle(handle) };
    if !succeeded {
        return Err(io::Error::last_os_error());
    }
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

pub fn write_value(
    pid: u32,
    address: usize,
    value_type: MemoryValueType,
    value: &str,
) -> io::Result<()> {
    let mut bytes = [0u8; 8];
    let width = match value_type {
        MemoryValueType::I8 => {
            bytes[0] = value
                .parse::<i8>()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
                .to_le_bytes()[0];
            1
        }
        MemoryValueType::I16 => {
            bytes[..2].copy_from_slice(
                &value
                    .parse::<i16>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
                    .to_le_bytes(),
            );
            2
        }
        MemoryValueType::I32 => {
            bytes[..4].copy_from_slice(
                &value
                    .parse::<i32>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
                    .to_le_bytes(),
            );
            4
        }
        MemoryValueType::F32 => {
            bytes[..4].copy_from_slice(
                &value
                    .parse::<f32>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
                    .to_le_bytes(),
            );
            4
        }
        MemoryValueType::I64 => {
            bytes.copy_from_slice(
                &value
                    .parse::<i64>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
                    .to_le_bytes(),
            );
            8
        }
        MemoryValueType::F64 => {
            bytes.copy_from_slice(
                &value
                    .parse::<f64>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
                    .to_le_bytes(),
            );
            8
        }
    };
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_WRITE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut written = 0;
    let succeeded = unsafe {
        WriteProcessMemory(
            handle,
            address as *mut c_void,
            bytes.as_ptr().cast(),
            width,
            &mut written,
        )
    } != 0;
    unsafe { CloseHandle(handle) };
    if !succeeded {
        return Err(io::Error::last_os_error());
    }
    if written != width {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "partial process-memory write",
        ));
    }
    Ok(())
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
    fn CloseHandle(handle: *mut c_void) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn float_change_comparison_uses_bits() {
        let nan = f32::from_bits(0x7FC0_0001);
        assert!(scan_value_matches(
            ScanComparison::Unchanged,
            ScanValue::F32(nan),
            ScanValue::F32(nan),
            None,
        ));
        assert!(!scan_value_matches(
            ScanComparison::Changed,
            ScanValue::F32(nan),
            ScanValue::F32(nan),
            None,
        ));
    }

    #[test]
    fn pointer_paths_keep_offsets_in_dereference_order() {
        let paths = find_pointer_paths(
            vec![(0x2FE0, 0x2000), (0x1FF0, 0x1010)],
            0x3000,
            &[("game.exe".to_owned(), 0x1000, 0x100)],
            0x100,
            3,
            8,
        );
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].module, "game.exe");
        assert_eq!(paths[0].module_offset, 0x10);
        assert_eq!(paths[0].offsets, vec![0x10, 0x20]);
    }
}
