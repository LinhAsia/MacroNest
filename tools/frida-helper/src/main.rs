use base64::Engine;
use frida::{DeviceManager, Frida, Message, ScriptHandler, ScriptOption};
use std::{
    io::{self, Read},
    thread,
    time::Duration,
};

fn emit(kind: &str, value: impl AsRef<str>) {
    println!(
        "{kind}\t{}",
        base64::engine::general_purpose::STANDARD.encode(value.as_ref())
    );
}

struct Handler;

impl ScriptHandler for Handler {
    fn on_message(&mut self, message: Message, _data: Option<Vec<u8>>) {
        let text = match message {
            Message::Log(value) => value.payload,
            Message::Error(value) => format!("{}\n{}", value.description, value.stack),
            other => format!("{other:?}"),
        };
        emit("LOG", text);
    }
}

fn main() {
    if let Err(error) = run() {
        emit("STATUS", format!("Frida error: {error}"));
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let pid = std::env::args()
        .nth(1)
        .ok_or("missing target PID")?
        .parse::<u32>()
        .map_err(|error| format!("invalid target PID: {error}"))?;
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .map_err(|error| format!("read hook script: {error}"))?;
    if source.trim().is_empty() {
        return Err("hook script is empty".into());
    }

    let frida = unsafe { Frida::obtain() };
    let manager = DeviceManager::obtain(&frida);
    let device = manager
        .get_local_device()
        .map_err(|error| format!("local device: {error}"))?;
    let session = device.attach(pid).map_err(|error| {
        format!(
            "attach PID {pid}: {error}. The process may block injection; run MacroNest at the same or higher privilege level"
        )
    })?;
    let mut script = session
        .create_script(&source, &mut ScriptOption::default())
        .map_err(|error| format!("create script: {error}"))?;
    script
        .handle_message(Handler)
        .map_err(|error| format!("message handler: {error}"))?;
    script
        .load()
        .map_err(|error| format!("load script: {error}"))?;
    emit("STATUS", format!("Frida attached to PID {pid}"));

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
