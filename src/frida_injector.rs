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
