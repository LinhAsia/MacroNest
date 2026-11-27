#![allow(dead_code)]

use crossbeam_channel::{Receiver, Sender, unbounded};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DEFAULT_FRIDA_VERSION: &str = "17.9.5";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FridaTarget {
    LocalPid(u32),
    RemoteAndroid { host: String, pid: u32 },
}

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
    pub(crate) fn attach(helper: PathBuf, target: FridaTarget, source: String) -> Self {
        let (events_tx, events) = unbounded();
        let child = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_child = Arc::clone(&child);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            if let Err(error) = run(helper, target, source, &events_tx, &worker_child, &worker_stop) {
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
    target: FridaTarget,
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
    let mut cmd = Command::new(&helper);
    match &target {
        FridaTarget::LocalPid(pid) => {
            cmd.arg(pid.to_string());
        }
        FridaTarget::RemoteAndroid { host, pid } => {
            cmd.arg("--remote").arg(host).arg(pid.to_string());
        }
    }
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
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

fn run_adb_text(adb: &Path, target: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(adb);
    if !target.trim().is_empty() {
        cmd.arg("-s").arg(target.trim());
    }
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run adb: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() && stdout.trim().is_empty() {
        return Err(if stderr.trim().is_empty() {
            format!("adb failed with status {}", out.status)
        } else {
            stderr.trim().to_owned()
        });
    }
    Ok(format!("{stdout}\n{stderr}"))
}

pub(crate) fn map_android_uname_to_frida_arch(uname_m: &str, abi: &str) -> &'static str {
    let u = uname_m.trim().to_ascii_lowercase();
    if u.contains("x86_64") || u.contains("amd64") {
        return "android-x86_64";
    }
    if u.contains("aarch64") || u.contains("arm64") {
        return "android-arm64";
    }
    if u.contains("i686") || u.contains("i386") || u == "x86" {
        return "android-x86";
    }
    if u.contains("arm") {
        return "android-arm";
    }
    let a = abi.trim().to_ascii_lowercase();
    if a.contains("x86_64") {
        "android-x86_64"
    } else if a.contains("arm64") {
        "android-arm64"
    } else if a.contains("x86") {
        "android-x86"
    } else {
        "android-arm"
    }
}

fn detect_helper_version(helper: &Path) -> String {
    let mut cmd = Command::new(helper);
    cmd.arg("--version");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    if let Ok(out) = cmd.output()
        && out.status.success()
    {
        let ver = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if !ver.is_empty() && ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return ver;
        }
    }
    DEFAULT_FRIDA_VERSION.to_owned()
}

/// Ensures matching `frida-server` is installed, running on `127.0.0.1:27042`, and ADB-forwarded.
pub(crate) fn setup_android_frida_server(
    adb: &Path,
    target: &str,
    helper: &Path,
    bin_dir: &Path,
    mut on_progress: impl FnMut(&str),
) -> Result<String, String> {
    if !helper.exists() {
        return Err("Frida tool is not installed. Install it in Settings > Downloaded Tools.".into());
    }
    on_progress("Checking ADB root & Android architecture...");
    let _ = run_adb_text(adb, target, &["root"]);
    let uname_out = run_adb_text(adb, target, &["shell", "uname -m"])?;
    let abi_out = run_adb_text(adb, target, &["shell", "getprop ro.product.cpu.abi"]).unwrap_or_default();
    let arch = map_android_uname_to_frida_arch(&uname_out, &abi_out);
    let ver = detect_helper_version(helper);
    let remote_bin = "/data/local/tmp/frida-server";

    let exists_out = run_adb_text(adb, target, &["shell", &format!("ls -l {remote_bin} 2>/dev/null")]).unwrap_or_default();
    if !exists_out.contains("frida-server") {
        let local_bin = bin_dir.join(format!("frida-server-{ver}-{arch}"));
        if !local_bin.is_file() {
            let xz_path = bin_dir.join(format!("frida-server-{ver}-{arch}.xz"));
            let url = format!(
                "https://github.com/frida/frida/releases/download/{ver}/frida-server-{ver}-{arch}.xz"
            );
            on_progress(&format!("Downloading frida-server {ver} ({arch})..."));
            let mut resp = reqwest::blocking::get(&url)
                .and_then(|r| r.error_for_status())
                .map_err(|e| format!("Failed to download {url}: {e}"))?;
            fs::create_dir_all(bin_dir).map_err(|e| format!("mkdir {}: {e}", bin_dir.display()))?;
            let mut f = fs::File::create(&xz_path).map_err(|e| format!("create {}: {e}", xz_path.display()))?;
            std::io::copy(&mut resp, &mut f).map_err(|e| format!("write xz: {e}"))?;
            drop(f);

            on_progress("Decompressing frida-server archive...");
            let mut unxz_cmd = Command::new(helper);
            unxz_cmd.arg("--unxz").arg(&xz_path).arg(&local_bin);
            #[cfg(windows)]
            unxz_cmd.creation_flags(CREATE_NO_WINDOW);
            let status = unxz_cmd.status().map_err(|e| format!("run frida-helper --unxz: {e}"))?;
            let _ = fs::remove_file(&xz_path);
            if !status.success() || !local_bin.is_file() {
                return Err("Failed to decompress frida-server .xz (update Frida tool in Settings or verify frida-helper.exe)".into());
            }
        }
        on_progress(&format!("Pushing frida-server to {remote_bin}..."));
        let local_str = local_bin.to_string_lossy().into_owned();
        run_adb_text(adb, target, &["push", &local_str, remote_bin])?;
        let _ = run_adb_text(adb, target, &["shell", &format!("chmod 755 {remote_bin}")]);
        let _ = run_adb_text(adb, target, &["shell", &format!("su -c \"chmod 755 {remote_bin}\"")]);
    }

    on_progress("Checking frida-server process...");
    let pid_check = run_adb_text(adb, target, &["shell", "pidof frida-server 2>/dev/null || ps -A | grep frida-server | grep -v grep"]).unwrap_or_default();
    if pid_check.trim().is_empty() {
        on_progress("Starting frida-server on 127.0.0.1:27042...");
        let launch_cmds = [
            format!("su -c \"nohup {remote_bin} -l 127.0.0.1:27042 -D >/dev/null 2>&1 &\""),
            format!("su 0 sh -c \"nohup {remote_bin} -l 127.0.0.1:27042 -D >/dev/null 2>&1 &\""),
            format!("nohup {remote_bin} -l 127.0.0.1:27042 -D >/dev/null 2>&1 &"),
        ];
        for c in &launch_cmds {
            let _ = run_adb_text(adb, target, &["shell", c]);
            thread::sleep(Duration::from_millis(550));
            let after = run_adb_text(adb, target, &["shell", "pidof frida-server 2>/dev/null"]).unwrap_or_default();
            if !after.trim().is_empty() {
                break;
            }
        }
    }

    on_progress("Forwarding tcp:27042...");
    run_adb_text(adb, target, &["forward", "tcp:27042", "tcp:27042"])?;
    let final_pid = run_adb_text(adb, target, &["shell", "pidof frida-server 2>/dev/null"]).unwrap_or_default();
    let final_pid = final_pid.trim();
    if final_pid.is_empty() {
        return Err("frida-server did not stay running (device may need root / su)".into());
    }
    Ok(format!("frida-server {ver} ({arch}) running PID {final_pid} -> 127.0.0.1:27042"))
}

#[cfg(test)]
mod tests {
    use super::map_android_uname_to_frida_arch;

    #[test]
    fn maps_android_uname_and_abi_to_frida_server_arch() {
        assert_eq!(map_android_uname_to_frida_arch("x86_64\n", "x86_64"), "android-x86_64");
        assert_eq!(map_android_uname_to_frida_arch("aarch64\n", "arm64-v8a"), "android-arm64");
        assert_eq!(map_android_uname_to_frida_arch("armv7l", "armeabi-v7a"), "android-arm");
        assert_eq!(map_android_uname_to_frida_arch("i686", "x86"), "android-x86");
    }
}
