use crossbeam_channel::{Receiver, Sender, unbounded};
use frida::{DeviceManager, Frida, Message, ScriptHandler, ScriptOption};
use std::thread::{self, JoinHandle};

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
