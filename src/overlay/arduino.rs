use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::HOOK_STATE;

pub(crate) static ARDUINO_PORT: Lazy<Mutex<Option<Box<dyn serialport::SerialPort>>>> =
    Lazy::new(|| Mutex::new(None));
pub(crate) static CURRENT_ARDUINO_PORT_NAME: Lazy<Mutex<String>> =
    Lazy::new(|| Mutex::new(String::new()));
pub(crate) static ARDUINO_RESPONSIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn close_arduino_runtime() {
    *ARDUINO_PORT.lock() = None;
    CURRENT_ARDUINO_PORT_NAME.lock().clear();
    ARDUINO_RESPONSIVE.store(false, Ordering::Release);
}

pub fn close_arduino_port_for_flash() {
    HOOK_STATE.lock().arduino_flash_in_progress = true;
    close_arduino_runtime();
}

pub fn finish_arduino_flash() {
    HOOK_STATE.lock().arduino_flash_in_progress = false;
}

pub fn arduino_connection_snapshot() -> (bool, String, bool) {
    let flash_in_progress = HOOK_STATE.lock().arduino_flash_in_progress;
    (
        ARDUINO_RESPONSIVE.load(Ordering::Acquire),
        CURRENT_ARDUINO_PORT_NAME.lock().clone(),
        flash_in_progress,
    )
}
