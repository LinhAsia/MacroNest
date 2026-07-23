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

#[repr(C)]
struct WorkingSetExInformation {
    virtual_address: *mut c_void,
    virtual_attributes: usize,
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

#[derive(Clone, Copy)]
pub struct MemoryScanOptions {
    pub writable: bool,
    pub executable: bool,
    pub copy_on_write: bool,
    pub active_memory_only: bool,
    pub mem_private: bool,
    pub mem_image: bool,
    pub mem_mapped: bool,
    pub alignment: Option<usize>,
}

impl Default for MemoryScanOptions {
    fn default() -> Self {
        Self {
            writable: true,
            executable: false,
            copy_on_write: false,
            active_memory_only: true,
            mem_private: true,
            mem_image: false,
            mem_mapped: false,
            alignment: Some(4),
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
    current: u64,
}

impl ScanCandidate {
    fn new(address: usize, current: ScanValue) -> Self {
        Self {
            address,
            current: u64::from_le_bytes(current.bytes()),
        }
    }

    pub fn current(self, value_type: ScanValueType) -> ScanValue {
        value_type
            .decode(&self.current.to_le_bytes())
            .expect("stored scan value width")
    }

    fn set_current(&mut self, current: ScanValue) {
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

pub struct PointerMap {
    pointers: Vec<(usize, usize)>,
    modules: Vec<(String, usize, usize)>,
}

pub fn capture_pointer_map(
    pid: u32,
    modules: &[(String, usize, usize)],
    pointer_width: usize,
    progress: Arc<AtomicUsize>,
) -> io::Result<PointerMap> {
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
    let mut pointers = Vec::new();
    pointers
        .try_reserve_exact(1_000_000)
        .map_err(|_| io::Error::other("not enough memory to start pointer map"))?;
    let mut buffer = vec![0u8; SCAN_CHUNK_BYTES];
    for region in regions {
        for offset in (0..region.size).step_by(SCAN_CHUNK_BYTES) {
            let address = region.base + offset;
            let wanted = (region.size - offset).min(SCAN_CHUNK_BYTES);
            let Ok(read) = process.read(address, &mut buffer[..wanted]) else {
                continue;
            };
            if read < pointer_width {
                continue;
            }
            for byte_offset in (0..=read - pointer_width).step_by(pointer_width) {
                let value = if pointer_width == 4 {
                    u32::from_le_bytes(
                        buffer[byte_offset..byte_offset + 4].try_into().unwrap(),
                    ) as usize
                } else {
                    u64::from_le_bytes(
                        buffer[byte_offset..byte_offset + 8].try_into().unwrap(),
                    ) as usize
                };
                let range = readable_ranges.partition_point(|(base, _)| *base <= value);
                if range > 0 && value < readable_ranges[range - 1].1 {
                    if pointers.len() == pointers.capacity() {
                        pointers.try_reserve_exact(1_000_000).map_err(|_| {
                            io::Error::other(format!(
                                "not enough memory after capturing {} pointers",
                                pointers.len()
                            ))
                        })?;
                    }
                    pointers.push((value, address + byte_offset));
                }
            }
            progress.fetch_add(read, Ordering::Relaxed);
        }
    }
    pointers.sort_unstable();
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
            &self.pointers,
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
    pointer_width: usize,
    max_offset: usize,
    max_depth: usize,
    result_limit: usize,
    progress: Arc<AtomicUsize>,
) -> io::Result<Vec<PointerPath>> {
    // ponytail: these caps keep a pathological process from exhausting app memory; a disk-backed
    // pointer map is the upgrade path for larger scans.
    let map = capture_pointer_map(pid, modules, pointer_width, progress)?;
    Ok(find_pointer_paths(
        &map.pointers,
        target,
        modules,
        max_offset,
        max_depth,
        result_limit,
    ))
}

fn find_pointer_paths(
    pointers: &[(usize, usize)],
    target: usize,
    modules: &[(String, usize, usize)],
    max_offset: usize,
    max_depth: usize,
    result_limit: usize,
) -> Vec<PointerPath> {
    const MAX_FRONTIER: usize = 50_000;
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
    Between,
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
            use windows_sys::Win32::{
                Foundation::CloseHandle,
                System::Threading::ResumeThread,
            };
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
    let process = ScanProcess::open(pid, true)?;
    let mut bytes = vec![0; capacity];
    bytes[..encoded.len()].copy_from_slice(&encoded);
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
}

pub fn scan_memory_with_progress(
    pid: u32,
    exact: Option<ScanValue>,
    value_type: ScanValueType,
    result_limit: usize,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<ScanCandidate>> {
    scan_memory_range_with_progress(
        pid,
        exact,
        None,
        value_type,
        result_limit,
        MemoryScanOptions::default(),
        total,
    )
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
    let regions = scan_regions_for(&process, options)
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
    let alignment = options.alignment.unwrap_or(1).max(1);
    // ponytail: merging worker Vecs temporarily raises peak RAM, so only parallelize unknown
    // scans whose result storage remains modest; a chunked result store is the upgrade path.
    const MAX_PARALLEL_UNKNOWN_BYTES: usize = 512 * 1024 * 1024;
    let estimated_result_bytes = slots.saturating_mul(std::mem::size_of::<ScanCandidate>());
    let worker_count = if exact.is_none() && estimated_result_bytes > MAX_PARALLEL_UNKNOWN_BYTES {
        1
    } else {
        thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(2)
            .clamp(2, 8)
            .min(regions.len().max(1))
    };
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
                scan_region_bucket(pid, regions, exact, range, value_type, alignment, result_limit, total)
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

pub fn filter_scan_candidates(
    pid: u32,
    mut candidates: Vec<ScanCandidate>,
    value_type: ScanValueType,
    comparison: ScanComparison,
    exact: Option<ScanValue>,
    range: Option<(ScanValue, ScanValue)>,
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
            .map(|chunk| scope.spawn(move || filter_candidate_slice(pid, chunk, value_type, comparison, exact, range)))
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
                if offset + value_type.width() > count {
                    continue;
                }
                let Some(current) = value_type.decode(&page[offset..]) else {
                    continue;
                };
                let matches = if comparison == ScanComparison::Between {
                    range.is_some_and(|(min, max)| scan_value_between(current, min, max))
                } else {
                    scan_value_matches(comparison, current, candidate.current(value_type), exact)
                };
                if matches {
                    candidates[write] = ScanCandidate::new(candidate.address, current);
                    write += 1;
                }
            }
        }
        index = end;
    }
    Ok(write)
}

pub fn refresh_scan_candidates(
    pid: u32,
    candidates: &mut [ScanCandidate],
    value_type: ScanValueType,
) -> io::Result<()> {
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
                if offset + value_type.width() <= count
                    && let Some(current) = value_type.decode(&page[offset..])
                {
                    candidate.set_current(current);
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
                ScanComparison::Exact => exact.is_some_and(|expected| {
                    scan_exact_matches($variant($current), $variant(expected))
                }),
                ScanComparison::Less => exact.is_some_and(|value| $current < value),
                ScanComparison::Greater => exact.is_some_and(|value| $current > value),
                ScanComparison::Changed => $current != $previous,
                ScanComparison::Unchanged => $current == $previous,
                ScanComparison::Increased => $current > $previous,
                ScanComparison::Decreased => $current < $previous,
                ScanComparison::Between => false,
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

fn scan_exact_matches(current: ScanValue, expected: ScanValue) -> bool {
    match (current, expected) {
        (ScanValue::F32(current), ScanValue::F32(expected)) => {
            current.is_finite()
                && expected.is_finite()
                && (current - expected).abs() <= (expected.abs() * 1e-6).max(1e-5)
        }
        (ScanValue::F64(current), ScanValue::F64(expected)) => {
            current.is_finite()
                && expected.is_finite()
                && (current - expected).abs() <= (expected.abs() * 1e-12).max(1e-9)
        }
        _ => current == expected,
    }
}

fn is_valid_unknown_scan_value(value: ScanValue) -> bool {
    match value {
        ScanValue::F32(v) => v.is_finite() && (v == 0.0 || (v.abs() >= 1e-30 && v.abs() <= 1e30)),
        ScanValue::F64(v) => v.is_finite() && (v == 0.0 || (v.abs() >= 1e-300 && v.abs() <= 1e300)),
        _ => true,
    }
}

fn scan_regions_for(process: &ScanProcess, options: MemoryScanOptions) -> Vec<ScanRegion> {
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
            if options.active_memory_only {
                regions.extend(active_scan_regions(process, region));
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

fn active_scan_regions(process: &ScanProcess, region: ScanRegion) -> Vec<ScanRegion> {
    const QUERY_PAGES: usize = 4096;
    let end = region.base.saturating_add(region.size);
    let mut page = region.base;
    let mut active_start = None;
    let mut active = Vec::new();
    while page < end {
        let count = (end - page).div_ceil(PAGE_BYTES).min(QUERY_PAGES);
        let mut information = (0..count)
            .map(|index| WorkingSetExInformation {
                virtual_address: page.saturating_add(index * PAGE_BYTES) as *mut c_void,
                virtual_attributes: 0,
            })
            .collect::<Vec<_>>();
        let queried = unsafe {
            K32QueryWorkingSetEx(
                process.handle,
                information.as_mut_ptr().cast(),
                (information.len() * size_of::<WorkingSetExInformation>()) as u32,
            )
        } != 0;
        for (index, entry) in information.iter().enumerate() {
            let address = page.saturating_add(index * PAGE_BYTES);
            let resident = queried && entry.virtual_attributes & 1 != 0;
            if resident {
                active_start.get_or_insert(address);
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
            size: end - start,
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

fn scan_region_bucket(
    pid: u32,
    regions: Vec<ScanRegion>,
    exact: Option<ScanValue>,
    range: Option<(ScanValue, ScanValue)>,
    value_type: ScanValueType,
    alignment: usize,
    result_limit: usize,
    total: Arc<AtomicUsize>,
) -> io::Result<Vec<ScanCandidate>> {
    let process = ScanProcess::open(pid, false)?;
    let mut found = Vec::new();
    let expected = regions
        .iter()
        .map(|region| region.size / alignment)
        .fold(0usize, usize::saturating_add)
        .min(result_limit);
    found.try_reserve_exact(expected).map_err(|_| {
        io::Error::other(format!(
            "not enough memory for {expected} scan results ({} bytes each)",
            std::mem::size_of::<ScanCandidate>()
        ))
    })?;
    let mut buffer = Vec::new();
    'regions: for region in regions {
        let end = region.base.saturating_add(region.size);
        let mut chunk_base = region.base;
        while chunk_base < end {
            let length = (end - chunk_base).min(SCAN_CHUNK_BYTES);
            buffer.resize(length, 0);
            if let Ok(count) = process.read(chunk_base, &mut buffer) {
                let width = value_type.width();
                let step = alignment.max(1);
                let chunk_start = found.len();
                if count >= width {
                    for offset in (0..=count - width).step_by(step) {
                        let value = value_type.decode(&buffer[offset..]).expect("value width");
                        if exact.is_none_or(|expected| scan_exact_matches(value, expected))
                            && range.is_none_or(|(min, max)| scan_value_between(value, min, max))
                            && (exact.is_some() || range.is_some() || is_valid_unknown_scan_value(value))
                        {
                            found.push(ScanCandidate::new(chunk_base + offset, value));
                        }
                    }
                }
                let chunk_matches = found.len() - chunk_start;
                if chunk_matches > 0 {
                    let allowed = claim_result_slots(&total, chunk_matches, result_limit);
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

fn scan_value_between(value: ScanValue, min: ScanValue, max: ScanValue) -> bool {
    macro_rules! between {
        ($variant:path) => {
            if let ($variant(value), $variant(min), $variant(max)) = (value, min, max) {
                return value >= min && value <= max;
            }
        };
    }
    between!(ScanValue::I8);
    between!(ScanValue::I16);
    between!(ScanValue::I32);
    between!(ScanValue::F32);
    between!(ScanValue::I64);
    between!(ScanValue::F64);
    false
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
    fn K32QueryWorkingSetEx(process: *mut c_void, information: *mut c_void, length: u32) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

#[cfg(test)]
mod tests {
    #[test]
    fn result_slots_are_claimed_per_chunk_without_exceeding_limit() {
        let total = AtomicUsize::new(0);
        assert_eq!(claim_result_slots(&total, 8, 10), 8);
        assert_eq!(claim_result_slots(&total, 8, 10), 2);
        assert_eq!(claim_result_slots(&total, 1, 10), 0);
        assert_eq!(total.load(Ordering::Relaxed), 10);
    }

    #[test]
    fn between_includes_both_bounds() {
        assert!(scan_value_between(ScanValue::I32(10), ScanValue::I32(10), ScanValue::I32(20)));
        assert!(scan_value_between(ScanValue::F32(20.0), ScanValue::F32(10.0), ScanValue::F32(20.0)));
        assert!(!scan_value_between(ScanValue::I32(21), ScanValue::I32(10), ScanValue::I32(20)));
    }

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
    fn exact_float_scan_accepts_display_rounding() {
        assert!(scan_exact_matches(
            ScanValue::F32(123.39999),
            ScanValue::F32(123.4),
        ));
        assert!(!scan_exact_matches(
            ScanValue::F32(123.39),
            ScanValue::F32(123.4),
        ));
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
            &[(0x1FF0, 0x1010), (0x2FE0, 0x2000)],
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
