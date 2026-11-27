use base64::Engine;
use frida::{DeviceManager, Frida, Message, ScriptHandler, ScriptOption};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    thread,
    time::Duration,
};
use xz::read::XzDecoder;

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

fn unxz_file(src: &Path, dst: &Path) -> Result<(), String> {
    let in_file = File::open(src).map_err(|e| format!("open {}: {e}", src.display()))?;
    let mut decoder = XzDecoder::new(in_file);
    let mut out_file = File::create(dst).map_err(|e| format!("create {}: {e}", dst.display()))?;
    io::copy(&mut decoder, &mut out_file).map_err(|e| format!("decompress xz: {e}"))?;
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("17.9.5");
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--unxz") {
        let src = args.get(1).ok_or("missing --unxz input path")?;
        let dst = args.get(2).ok_or("missing --unxz output path")?;
        return unxz_file(Path::new(src), Path::new(dst));
    }

    let (remote_host, pid_str) = if args.first().map(String::as_str) == Some("--remote") {
        let host = args.get(1).ok_or("missing --remote host:port")?.clone();
        let pid_s = args.get(2).ok_or("missing target PID")?.clone();
        (Some(host), pid_s)
    } else {
        let pid_s = args.first().ok_or("missing target PID")?.clone();
        (None, pid_s)
    };

    let pid = pid_str
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
    let device = match &remote_host {
        Some(host) => manager
            .get_remote_device(host)
            .map_err(|error| format!("remote device {host}: {error}. Make sure frida-server is running and ADB port is forwarded"))?,
        None => manager
            .get_local_device()
            .map_err(|error| format!("local device: {error}"))?,
    };
    let session = device.attach(pid).map_err(|error| {
        format!(
            "attach PID {pid}: {error}. Check permissions / frida-server status"
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
    let where_txt = remote_host.as_deref().unwrap_or("local");
    emit("STATUS", format!("Frida attached to PID {pid} ({where_txt})"));

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
