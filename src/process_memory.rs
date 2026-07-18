use std::{ffi::c_void, io};

use crate::model::MemoryValueType;

const PROCESS_VM_READ: u32 = 0x0010;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

pub fn read_value(pid: u32, address: usize, value_type: MemoryValueType) -> io::Result<String> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }

    let mut bytes = [0u8; 8];
    let width = match value_type {
        MemoryValueType::I32 | MemoryValueType::F32 => 4,
        MemoryValueType::I64 => 8,
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
        MemoryValueType::I32 => i32::from_le_bytes(bytes[..4].try_into().unwrap()).to_string(),
        MemoryValueType::F32 => f32::from_le_bytes(bytes[..4].try_into().unwrap()).to_string(),
        MemoryValueType::I64 => i64::from_le_bytes(bytes).to_string(),
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
    fn CloseHandle(handle: *mut c_void) -> i32;
}
