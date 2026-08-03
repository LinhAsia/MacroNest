use crossbeam_channel::{Receiver, Sender, unbounded};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

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

pub(crate) struct Session {
    pub(crate) events: Receiver<Event>,
    child: Arc<Mutex<Option<Child>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Session {
    pub(crate) fn attach(helper: PathBuf, pid: u32, source: String) -> Self {
        let (events_tx, events) = unbounded();
        let child = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_child = Arc::clone(&child);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            if let Err(error) = run(helper, pid, source, &events_tx, &worker_child, &worker_stop) {
                let _ = events_tx.send(Event::Status(format!("Frida error: {error}")));
            }
        });
        Self {
            events,
            child,
            stop,
            worker: Some(worker),
        }
    }
}

fn run(
    helper: PathBuf,
    pid: u32,
    source: String,
    events: &Sender<Event>,
    child_slot: &Arc<Mutex<Option<Child>>>,
    stop: &AtomicBool,
) -> Result<(), String> {
    if !helper.exists() {
        return Err(
            "Frida tool is not installed. Install it in Settings > Downloaded Tools.".into(),
        );
    }
    let mut child = Command::new(&helper)
        .arg(pid.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|error| format!("start {}: {error}", helper.display()))?;
    child
        .stdin
        .take()
        .ok_or("Frida helper stdin unavailable")?
        .write_all(source.as_bytes())
        .map_err(|error| format!("send hook script: {error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("Frida helper stdout unavailable")?;
    *child_slot
        .lock()
        .map_err(|_| "Frida helper lock poisoned")? = Some(child);
    if stop.load(Ordering::SeqCst) {
        if let Ok(mut slot) = child_slot.lock()
            && let Some(child) = slot.as_mut()
        {
            let _ = child.kill();
        }
        return Ok(());
    }
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("read helper output: {error}"))?;
        let Some((kind, value)) = line.split_once('\t') else {
            continue;
        };
        let value = String::from_utf8(
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, value)
                .map_err(|error| format!("decode helper output: {error}"))?,
        )
        .map_err(|error| format!("helper returned invalid text: {error}"))?;
        let event = if kind == "STATUS" {
            Event::Status(value)
        } else {
            Event::Log(value)
        };
        let _ = events.send(event);
    }
    Ok(())
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.child.lock()
            && let Some(child) = slot.as_mut()
        {
            let _ = child.kill();
        }
        // ponytail: never join a helper from the UI thread; a target can stall Frida indefinitely.
        self.worker.take();
    }
}
