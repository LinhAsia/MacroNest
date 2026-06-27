use anyhow::{Context, Result, bail};
use hidapi::HidApi;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::ffi::CString;
use std::time::Instant;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::core::PCSTR;

use super::HOOK_STATE;
use crate::model::ArduinoTransport;

pub(crate) struct ArduinoRawHidRuntime {
    pub(crate) handle: HANDLE,
    pub(crate) path: String,
}

unsafe impl Send for ArduinoRawHidRuntime {}

impl Drop for ArduinoRawHidRuntime {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

pub(crate) static ARDUINO_PORT: Lazy<Mutex<Option<Box<dyn serialport::SerialPort>>>> =
    Lazy::new(|| Mutex::new(None));
pub(crate) static CURRENT_ARDUINO_PORT_NAME: Lazy<Mutex<String>> =
    Lazy::new(|| Mutex::new(String::new()));
pub(crate) static ARDUINO_HID_DEVICE: Lazy<Mutex<Option<ArduinoRawHidRuntime>>> =
    Lazy::new(|| Mutex::new(None));
pub(crate) static CURRENT_ARDUINO_HID_NAME: Lazy<Mutex<String>> =
    Lazy::new(|| Mutex::new(String::new()));
pub(crate) static LAST_ARDUINO_OPEN_ATTEMPT: Lazy<Mutex<Option<Instant>>> =
    Lazy::new(|| Mutex::new(None));
pub(crate) static LAST_ARDUINO_HID_WRITE_AT: Lazy<Mutex<Option<Instant>>> =
    Lazy::new(|| Mutex::new(None));

pub(crate) fn close_arduino_runtime_handles() {
    let mut port_guard = ARDUINO_PORT.lock();
    let mut port_name_guard = CURRENT_ARDUINO_PORT_NAME.lock();
    let mut hid_guard = ARDUINO_HID_DEVICE.lock();
    let mut hid_name_guard = CURRENT_ARDUINO_HID_NAME.lock();
    let mut hid_write_guard = LAST_ARDUINO_HID_WRITE_AT.lock();
    *port_guard = None;
    *port_name_guard = String::new();
    *hid_guard = None;
    *hid_name_guard = String::new();
    *hid_write_guard = None;
}

pub fn close_arduino_port_for_flash() {
    HOOK_STATE.lock().arduino_flash_in_progress = true;
    close_arduino_runtime_handles();
}

pub fn finish_arduino_flash() {
    HOOK_STATE.lock().arduino_flash_in_progress = false;
}

pub fn arduino_connection_snapshot() -> (bool, String, bool) {
    let flash_in_progress = HOOK_STATE.lock().arduino_flash_in_progress;
    let current_serial = CURRENT_ARDUINO_PORT_NAME.lock().clone();
    let current_hid = CURRENT_ARDUINO_HID_NAME.lock().clone();
    let serial_connected = ARDUINO_PORT.lock().is_some();
    let hid_connected = ARDUINO_HID_DEVICE.lock().is_some();
    if hid_connected {
        (true, current_hid, flash_in_progress)
    } else {
        (serial_connected, current_serial, flash_in_progress)
    }
}

pub(crate) fn parse_hex_u16_runtime(value: &str, fallback: u16) -> u16 {
    let cleaned = value
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    u16::from_str_radix(cleaned, 16).unwrap_or(fallback)
}

pub(crate) fn open_arduino_hid_device(vid: u16, pid: u16) -> Result<ArduinoRawHidRuntime> {
    let api = HidApi::new().context("Failed to enumerate HID devices")?;
    let mut fallback_path: Option<String> = None;
    let mut selected_path: Option<String> = None;

    for device in api.device_list() {
        if device.vendor_id() != vid || device.product_id() != pid {
            continue;
        }

        let path = device.path().to_string_lossy().into_owned();
        if fallback_path.is_none() {
            fallback_path = Some(path.clone());
        }

        let usage_page = device.usage_page();
        let interface_number = device.interface_number();
        let is_vendor_hid = usage_page == 0xFF00 || usage_page == 0xFFC0;
        let looks_like_standard_mouse = interface_number == 0;

        if is_vendor_hid && !looks_like_standard_mouse {
            selected_path = Some(path);
            break;
        }
    }

    let path = if let Some(path) = selected_path {
        path
    } else if let Some(path) = fallback_path {
        path
    } else {
        bail!("No HID device found for VID 0x{vid:04X} PID 0x{pid:04X}");
    };

    let c_path = CString::new(path.clone()).context("Invalid HID device path")?;
    let handle = unsafe {
        CreateFileA(
            PCSTR(c_path.as_ptr() as *const u8),
            (windows::Win32::Storage::FileSystem::FILE_GENERIC_READ
                | windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE)
                .0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .with_context(|| format!("Failed to open HID path {path}"))?;

    Ok(ArduinoRawHidRuntime { handle, path })
}
