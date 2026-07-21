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
    let frida = unsafe { Frida::obtain() };
    let manager = DeviceManager::obtain(&frida);
    let device = manager
        .get_local_device()
        .map_err(|error| format!("local device: {error}"))?;
    let session = device
        .attach(pid)
        .map_err(|error| format!("attach PID {pid}: {error}"))?;
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

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
