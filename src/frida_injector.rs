use crossbeam_channel::{Receiver, Sender, unbounded};
use frida::{DeviceManager, Frida, Message, ScriptHandler, ScriptOption};
use std::thread::{self, JoinHandle};

pub(crate) const DEFAULT_NETWORK_SCRIPT: &str = r#"
'use strict';

const MAX_PREVIEW = 16384;
const hooked = [];
const requests = new Map();

function preview(pointer, length) {
    if (pointer.isNull() || length <= 0) return '';
    const size = Math.min(Number(length), MAX_PREVIEW);
    try {
        const bytes = new Uint8Array(pointer.readByteArray(size));
        let text = '';
        for (let i = 0; i < bytes.length; i++) {
            const byte = bytes[i];
            text += byte === 9 || byte === 10 || byte === 13 || (byte >= 32 && byte < 127)
                ? String.fromCharCode(byte) : '.';
        }
        return text;
    } catch (_) { return ''; }
}

function wide(pointer) {
    try { return pointer.isNull() ? '' : pointer.readUtf16String(); } catch (_) { return ''; }
}

function emit(kind, details, data) {
    console.log('[NET] ' + kind + (details ? ' ' + details : '') +
        (data ? '\n' + data : ''));
}

function exportOf(moduleName, name) {
    try {
        const module = Process.findModuleByName(moduleName);
        return module === null ? null : module.findExportByName(name);
    } catch (_) { return null; }
}

function attach(moduleName, name, callbacks) {
    const address = exportOf(moduleName, name);
    if (address === null) return false;
    Interceptor.attach(address, callbacks);
    hooked.push(moduleName + '!' + name);
    return true;
}

// WinHTTP: used by many native Windows applications.
attach('winhttp.dll', 'WinHttpConnect', {
    onEnter(args) { this.host = wide(args[1]); this.port = args[2].toUInt32(); },
    onLeave(retval) { if (!retval.isNull()) requests.set(retval.toString(), { host: this.host + ':' + this.port }); }
});
attach('winhttp.dll', 'WinHttpOpenRequest', {
    onEnter(args) {
        this.parent = args[0].toString(); this.method = wide(args[1]) || 'GET'; this.path = wide(args[2]) || '/';
    },
    onLeave(retval) {
        const parent = requests.get(this.parent) || {};
        if (!retval.isNull()) requests.set(retval.toString(), { host: parent.host || '', method: this.method, path: this.path });
        emit('WinHTTP', this.method + ' https://' + (parent.host || '?') + this.path, '');
    }
});
attach('winhttp.dll', 'WinHttpSendRequest', {
    onEnter(args) {
        const request = requests.get(args[0].toString()) || {};
        const headers = wide(args[1]);
        const body = preview(args[3], args[4].toUInt32());
        emit('WinHTTP SEND', (request.method || '') + ' ' + (request.host || '') + (request.path || ''), headers + (body ? '\n\n' + body : ''));
    }
});

// WinINet: used by older/native desktop applications.
attach('wininet.dll', 'HttpOpenRequestW', {
    onEnter(args) { this.method = wide(args[1]) || 'GET'; this.path = wide(args[2]) || '/'; },
    onLeave(retval) {
        if (!retval.isNull()) requests.set(retval.toString(), { method: this.method, path: this.path });
        emit('WinINet', this.method + ' ' + this.path, '');
    }
});
attach('wininet.dll', 'HttpSendRequestW', {
    onEnter(args) {
        const request = requests.get(args[0].toString()) || {};
        const headers = wide(args[1]);
        const body = preview(args[3], args[4].toUInt32());
        emit('WinINet SEND', (request.method || '') + ' ' + (request.path || ''), headers + (body ? '\n\n' + body : ''));
    }
});

// OpenSSL/BoringSSL when the process exports its SSL API. Data here is plaintext.
for (const module of Process.enumerateModules()) {
    for (const name of ['SSL_write', 'SSL_write_ex']) {
        const address = module.findExportByName(name);
        if (address === null) continue;
        Interceptor.attach(address, {
            onEnter(args) { emit(name, module.name, preview(args[1], args[2].toUInt32())); }
        });
        hooked.push(module.name + '!' + name);
    }
    const read = module.findExportByName('SSL_read');
    if (read !== null) {
        Interceptor.attach(read, {
            onEnter(args) { this.buffer = args[1]; },
            onLeave(retval) { const count = retval.toInt32(); if (count > 0) emit('SSL_read', module.name, preview(this.buffer, count)); }
        });
        hooked.push(module.name + '!SSL_read');
    }
}

emit('READY', 'PID ' + Process.id, hooked.length ? hooked.join('\n') :
    'No supported exported HTTP/TLS API found. Electron/Chromium often links BoringSSL statically, so a generic export hook cannot read its plaintext.');
"#;

pub(crate) enum Event {
    Status(String),
    Log(String),
}

struct Handler(Sender<Event>);

impl ScriptHandler for Handler {
    fn on_message(&mut self, message: Message, _data: Option<Vec<u8>>) {
        let text = match message {
            Message::Log(value) => value.payload,
            Message::Error(value) => format!("{}\n{}", value.description, value.stack),
            other => format!("{other:?}"),
        };
        let _ = self.0.send(Event::Log(text));
    }
}

pub(crate) struct Session {
    stop: Sender<()>,
    pub(crate) events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
}

impl Session {
    pub(crate) fn attach(pid: u32, source: String) -> Self {
        let (events_tx, events) = unbounded();
        let (stop, stop_rx) = unbounded();
        let worker = thread::spawn(move || {
            if let Err(error) = run(pid, &source, &events_tx, &stop_rx) {
                let _ = events_tx.send(Event::Status(format!("Frida error: {error}")));
            }
        });
        Self {
            stop,
            events,
            worker: Some(worker),
        }
    }
}

fn run(pid: u32, source: &str, events: &Sender<Event>, stop: &Receiver<()>) -> Result<(), String> {
    verify_attach_access(pid)?;
    let frida = unsafe { Frida::obtain() };
    let manager = DeviceManager::obtain(&frida);
    let device = manager
        .get_local_device()
        .map_err(|error| format!("local device: {error}"))?;
    let session = device
        .attach(pid)
        .map_err(|error| format!("attach PID {pid}: {error}. The process may block injection; try its grouped window entry or run MacroNest as administrator"))?;
    let mut script = session
        .create_script(source, &mut ScriptOption::default())
        .map_err(|error| format!("create script: {error}"))?;
    script
        .handle_message(Handler(events.clone()))
        .map_err(|error| format!("message handler: {error}"))?;
    script
        .load()
        .map_err(|error| format!("load script: {error}"))?;
    let _ = events.send(Event::Status(format!("Frida attached to PID {pid}")));
    let _ = stop.recv();
    let _ = script.unload();
    let _ = session.detach();
    Ok(())
}

#[cfg(windows)]
fn verify_attach_access(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
            PROCESS_VM_READ, PROCESS_VM_WRITE,
        },
    };
    if pid == std::process::id() {
        return Err("cannot inject Frida into MacroNest itself".to_owned());
    }
    let handle = unsafe {
        OpenProcess(
            PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return Err(format!(
            "Windows denied injection access to PID {pid}: {}. Run MacroNest at the same or higher privilege level",
            std::io::Error::last_os_error()
        ));
    }
    unsafe { CloseHandle(handle) };
    Ok(())
}

#[cfg(not(windows))]
fn verify_attach_access(_pid: u32) -> Result<(), String> { Ok(()) }

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        // ponytail: never join an injector from the UI thread; a target can stall Frida indefinitely.
        self.worker.take();
    }
}
