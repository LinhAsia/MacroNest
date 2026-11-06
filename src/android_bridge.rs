use std::{
    collections::HashMap,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownEmulator {
    pub name: &'static str,
    pub default_port: u16,
}

pub const KNOWN_EMULATOR_PORTS: &[KnownEmulator] = &[
    KnownEmulator {
        name: "MuMu Player 12 / X (Slot 0)",
        default_port: 16384,
    },
    KnownEmulator {
        name: "MuMu Player 12 / X (Slot 1)",
        default_port: 16416,
    },
    KnownEmulator {
        name: "MuMu Player 12 / X (Slot 2)",
        default_port: 16448,
    },
    KnownEmulator {
        name: "MuMu Player 6",
        default_port: 7555,
    },
    KnownEmulator {
        name: "LDPlayer / Standard (Port 5555)",
        default_port: 5555,
    },
    KnownEmulator {
        name: "LDPlayer (Port 5557)",
        default_port: 5557,
    },
    KnownEmulator {
        name: "LDPlayer (Port 5559)",
        default_port: 5559,
    },
    KnownEmulator {
        name: "BlueStacks 5 (Port 5555)",
        default_port: 5555,
    },
    KnownEmulator {
        name: "BlueStacks 5 (Port 5556)",
        default_port: 5556,
    },
    KnownEmulator {
        name: "NoxPlayer (Port 62001)",
        default_port: 62001,
    },
    KnownEmulator {
        name: "NoxPlayer (Port 62025)",
        default_port: 62025,
    },
    KnownEmulator {
        name: "MEmu Player (Port 21503)",
        default_port: 21503,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredEmulator {
    pub label: String,
    pub target: String,
    pub port: u16,
    pub is_online: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidProcess {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub is_user_app: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidMemoryModule {
    pub name: String,
    pub base_address: usize,
    pub end_address: usize,
    pub size: usize,
    pub permissions: String,
    pub pathname: String,
}

#[derive(Clone, Debug, Default)]
pub struct AndroidBridgeState {
    pub is_open: bool,
    pub status: String,
    pub adb_path: Option<PathBuf>,
    pub custom_target: String,
    pub discovered_devices: Vec<DiscoveredEmulator>,
    pub selected_target: String,
    pub is_connected: bool,
    pub connected_device_name: String,
    pub processes: Vec<AndroidProcess>,
    pub process_search: String,
    pub selected_pid: Option<u32>,
    pub selected_process_name: String,
    pub modules: Vec<AndroidMemoryModule>,
    pub module_filter: String,
    // Automated GVA <-> HVA Delta
    pub guest_module_base: Option<usize>,
    pub guest_module_name: String,
    pub host_base_address: Option<usize>,
    pub delta: Option<isize>,
    pub delta_formula: String,
    pub auto_sync_status: String,
    // Value quick probe
    pub read_addr_input: String,
    pub read_value_result: String,
}

impl AndroidBridgeState {
    pub fn new() -> Self {
        Self {
            custom_target: "127.0.0.1:16384".to_string(),
            status: "Ready".to_string(),
            ..Default::default()
        }
    }
}

/// Discovers the path to an adb executable on the system.
pub fn find_adb_executable() -> Option<PathBuf> {
    // 1. Check if adb is in PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("adb.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
            let candidate_bare = dir.join("adb");
            if candidate_bare.is_file() {
                return Some(candidate_bare);
            }
        }
    }

    // 2. Common emulator ADB locations
    let candidates = [
        r"D:\Program Files\Netease\MuMuPlayer\nx_main\adb.exe",
        r"C:\Program Files\Netease\MuMuPlayer\nx_main\adb.exe",
        r"D:\Program Files\Netease\MuMuPlayer\nx_device\15.0\shell\adb.exe",
        r"C:\Program Files\Netease\MuMuPlayer\nx_device\15.0\shell\adb.exe",
        r"C:\LDPlayer\LDPlayer9\adb.exe",
        r"D:\LDPlayer\LDPlayer9\adb.exe",
        r"C:\LDPlayer\LDPlayer4\adb.exe",
        r"D:\LDPlayer\LDPlayer4\adb.exe",
        r"C:\Program Files\BlueStacks_nxt\HD-Adb.exe",
        r"C:\Program Files\Nox\bin\nox_adb.exe",
    ];

    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }

    None
}

/// Probes localhost ports with a fast timeout (40ms) to detect active emulators.
pub fn probe_running_emulators() -> Vec<DiscoveredEmulator> {
    let mut results = Vec::new();
    let mut seen_ports = std::collections::HashSet::new();

    for emu in KNOWN_EMULATOR_PORTS {
        if !seen_ports.insert(emu.default_port) {
            continue;
        }

        let addr = SocketAddr::from(([127, 0, 0, 1], emu.default_port));
        let is_online = TcpStream::connect_timeout(&addr, Duration::from_millis(40)).is_ok();
        if is_online {
            results.push(DiscoveredEmulator {
                label: format!("{} (127.0.0.1:{})", emu.name, emu.default_port),
                target: format!("127.0.0.1:{}", emu.default_port),
                port: emu.default_port,
                is_online: true,
            });
        }
    }

    results
}

/// Executes an ADB command non-interactively with a hidden console window.
fn run_adb_command(adb: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(adb);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to execute adb: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() && !stderr.trim().is_empty() {
        return Err(format!("{} {}", stderr.trim(), stdout.trim()));
    }

    Ok(stdout)
}

/// Connects to an emulator target via ADB (e.g. "127.0.0.1:16384").
pub fn adb_connect(adb: &Path, target: &str) -> Result<String, String> {
    let clean = target.trim();
    if clean.is_empty() {
        return Err("Target cannot be empty".to_string());
    }
    let res = run_adb_command(adb, &["connect", clean])?;
    if res.contains("connected") || res.contains("already connected") {
        // Request root immediately on connection
        let _ = run_adb_command(adb, &["-s", clean, "root"]);
        Ok(res.trim().to_string())
    } else {
        Err(res.trim().to_string())
    }
}

pub fn adb_request_root(adb: &Path, target: &str) -> Result<String, String> {
    let clean = target.trim();
    run_adb_command(adb, &["-s", clean, "root"])
}

/// Lists online ADB devices.
pub fn adb_list_devices(adb: &Path) -> Result<Vec<String>, String> {
    let out = run_adb_command(adb, &["devices"])?;
    let mut devices = Vec::new();
    for line in out.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == "device" {
            devices.push(parts[0].to_string());
        }
    }
    Ok(devices)
}

/// Parses output of `ps -A` or `ps` from Android shell.
pub fn parse_android_ps(output: &str) -> Vec<AndroidProcess> {
    let mut processes = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    if lines.is_empty() {
        return processes;
    }

    // Determine column indices from header
    let header = lines[0];
    let headers: Vec<&str> = header.split_whitespace().collect();
    let pid_col = headers.iter().position(|&h| h == "PID").unwrap_or(1);
    let name_col = headers
        .iter()
        .position(|&h| h == "NAME" || h == "CMD" || h == "COMMAND")
        .unwrap_or(headers.len().saturating_sub(1));
    let user_col = headers.iter().position(|&h| h == "USER").unwrap_or(0);

    for line in lines.iter().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() <= pid_col || parts.len() <= name_col {
            continue;
        }

        let pid: u32 = match parts[pid_col].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let name = parts[name_col].to_string();
        let user = parts.get(user_col).unwrap_or(&"").to_string();

        let is_system = name.starts_with("android.")
            || name.starts_with("com.android.")
            || name.starts_with("vendor.")
            || name.starts_with("system_")
            || name.starts_with('/')
            || name.starts_with('[');

        let is_user_app = !is_system
            && (user.starts_with("u0_a")
                || user.starts_with("u0_i")
                || (name.contains('.') && !name.contains('/')));

        processes.push(AndroidProcess {
            pid,
            name,
            user,
            is_user_app,
        });
    }

    // Sort: user apps first, then alphabetically
    processes.sort_by(|a, b| {
        b.is_user_app
            .cmp(&a.is_user_app)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    processes
}

/// Fetches running processes from the Android device.
pub fn list_android_processes(adb: &Path, target: &str) -> Result<Vec<AndroidProcess>, String> {
    let out = run_adb_command(adb, &["-s", target, "shell", "ps -A || ps"])?;
    Ok(parse_android_ps(&out))
}

/// Parses the contents of `/proc/<pid>/maps` from Android.
pub fn parse_android_maps(maps_content: &str) -> Vec<AndroidMemoryModule> {
    let mut modules_map: HashMap<String, (usize, usize, usize, String, String)> = HashMap::new();
    let mut standalone_sections = Vec::new();

    for line in maps_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Format: 7b120000-7b340000 r-xp 00000000 08:06 12345 /data/app/.../libil2cpp.so
        let mut parts = trimmed.split_whitespace();
        let range = match parts.next() {
            Some(r) => r,
            None => continue,
        };
        let perms = match parts.next() {
            Some(p) => p,
            None => continue,
        };
        let _offset = parts.next();
        let _dev = parts.next();
        let _inode = parts.next();
        let pathname = parts.next().unwrap_or("").trim();

        let (start_str, end_str) = match range.split_once('-') {
            Some((s, e)) => (s, e),
            None => continue,
        };

        let start = match usize::from_str_radix(start_str, 16) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let end = match usize::from_str_radix(end_str, 16) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let clean_name = if pathname.is_empty() {
            "[anonymous]".to_string()
        } else if let Some((_, file)) = pathname.rsplit_once('/') {
            file.to_string()
        } else {
            pathname.to_string()
        };

        // For .so files or known named memory, group contiguous sections by file to identify primary base address
        if clean_name.ends_with(".so")
            || clean_name.ends_with(".apk")
            || clean_name.starts_with('[')
        {
            if let Some(entry) = modules_map.get_mut(&clean_name) {
                if start >= entry.0 && start.saturating_sub(entry.1) < 256 * 1024 * 1024 {
                    if end > entry.1 {
                        entry.1 = end;
                    }
                    entry.2 += end.saturating_sub(start);
                    if perms.contains('x') {
                        entry.3 = perms.to_string();
                    }
                    continue;
                }
            } else {
                modules_map.insert(
                    clean_name.clone(),
                    (start, end, end.saturating_sub(start), perms.to_string(), pathname.to_string()),
                );
                continue;
            }
        }

        standalone_sections.push(AndroidMemoryModule {
            name: clean_name,
            base_address: start,
            end_address: end,
            size: end.saturating_sub(start),
            permissions: perms.to_string(),
            pathname: pathname.to_string(),
        });
    }

    let mut result = Vec::new();
    for (name, (base, end, size, perms, path)) in modules_map {
        result.push(AndroidMemoryModule {
            name,
            base_address: base,
            end_address: end,
            size: if size > 0 { size } else { end.saturating_sub(base) },
            permissions: perms,
            pathname: path,
        });
    }

    result.extend(standalone_sections);

    // Sort: Game critical modules (.so files like libil2cpp, libunity, libneox) first, then by base
    result.sort_by(|a, b| {
        let a_score = module_priority_score(&a.name);
        let b_score = module_priority_score(&b.name);
        b_score
            .cmp(&a_score)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.base_address.cmp(&b.base_address))
    });

    result
}

fn module_priority_score(name: &str) -> u32 {
    let lower = name.to_lowercase();
    if lower.contains("libil2cpp") {
        100
    } else if lower.contains("libunity") {
        90
    } else if lower.contains("libneox") {
        85
    } else if lower.contains("libue4") || lower.contains("libunreal") {
        80
    } else if lower.contains("game") {
        70
    } else if lower.ends_with(".so") {
        50
    } else if lower.starts_with("[heap") || lower.contains("malloc") {
        40
    } else {
        10
    }
}

/// Fetches and parses memory maps of a given Android process PID.
pub fn get_android_memory_maps(
    adb: &Path,
    target: &str,
    pid: u32,
) -> Result<Vec<AndroidMemoryModule>, String> {
    // Try reading with su -c, then su 0, then direct cat
    let commands = [
        format!("su -c 'cat /proc/{pid}/maps'"),
        format!("su 0 cat /proc/{pid}/maps"),
        format!("cat /proc/{pid}/maps"),
    ];

    let mut last_err = String::new();
    for cmd in &commands {
        if let Ok(out) = run_adb_command(adb, &["-s", target, "shell", cmd]) {
            let trimmed = out.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.contains("Permission denied") {
                last_err = trimmed.to_string();
                continue;
            }
            if trimmed.contains("not found") && !trimmed.contains("r-xp") && !trimmed.contains("rw-p") {
                continue;
            }

            let modules = parse_android_maps(trimmed);
            if !modules.is_empty() {
                return Ok(modules);
            }
        }
    }

    if last_err.contains("Permission denied") {
        Err("Permission denied: Bật Root trong Cài đặt giả lập (MuMu Settings -> Root ON)".to_string())
    } else {
        Err(format!("Could not read /proc/{pid}/maps"))
    }
}

/// Reads memory directly from Android `/proc/<pid>/mem`.
pub fn read_android_memory_bytes(
    adb: &Path,
    target: &str,
    pid: u32,
    address: usize,
    size: usize,
) -> Result<Vec<u8>, String> {
    let clamped_size = size.clamp(1, 1024);
    let subcmd = format!(
        "dd if=/proc/{pid}/mem bs=1 skip={address} count={clamped_size} 2>/dev/null | xxd -p || xxd -s {address} -l {clamped_size} -p /proc/{pid}/mem 2>/dev/null"
    );
    let commands = [
        format!("su -c \"{subcmd}\""),
        format!("su 0 sh -c \"{subcmd}\""),
        subcmd,
    ];

    for cmd in &commands {
        if let Ok(out) = run_adb_command(adb, &["-s", target, "shell", cmd]) {
            let hex_clean: String = out.chars().filter(|c| c.is_ascii_hexdigit()).collect();
            if !hex_clean.is_empty() {
                let mut bytes = Vec::new();
                for chunk in hex_clean.as_bytes().chunks(2) {
                    if chunk.len() == 2 {
                        if let Ok(b_str) = std::str::from_utf8(chunk) {
                            if let Ok(byte) = u8::from_str_radix(b_str, 16) {
                                bytes.push(byte);
                            }
                        }
                    }
                }
                if !bytes.is_empty() {
                    return Ok(bytes);
                }
            }
        }
    }

    Err("Could not read memory bytes (permission denied or unmapped address)".to_string())
}

/// Calculates translation delta between Windows Host Address (HVA) and Android Guest Address (GVA).
pub fn calculate_gva_hva_delta(guest_addr: usize, host_addr: usize) -> (isize, String) {
    let delta = host_addr as isize - guest_addr as isize;
    let sign = if delta >= 0 { "+" } else { "-" };
    let abs_delta = delta.unsigned_abs();
    let explanation = format!(
        "Host Base Offset = HVA - GVA: {sign}0x{abs_delta:X}\nFormula: Windows Address = Android Address {sign} 0x{abs_delta:X}"
    );
    (delta, explanation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_android_ps_extracts_user_apps_and_pids() {
        let sample = r#"USER           PID  PPID     VSZ    RSS WCHAN            ADDR S NAME
root             1     0   20480   1230 0                   0 S init
system        1000     1  102400  32100 0                   0 S system_server
system         665     1   50000   1000 0                   0 S android.hardware.audio.service
u0_a123       4567  1000 2048000 150000 0                   0 S com.netease.dwrg
u0_a234       8910  1000 1500000 100000 0                   0 S com.mihoyo.GenshinImpact
"#;
        let procs = parse_android_ps(sample);
        assert_eq!(procs.len(), 5);
        assert_eq!(procs[0].name, "com.mihoyo.GenshinImpact");
        assert_eq!(procs[0].pid, 8910);
        assert!(procs[0].is_user_app);

        assert_eq!(procs[1].name, "com.netease.dwrg");
        assert_eq!(procs[1].pid, 4567);
        assert!(procs[1].is_user_app);

        let audio = procs.iter().find(|p| p.name == "android.hardware.audio.service").unwrap();
        assert!(!audio.is_user_app);

        assert_eq!(procs[3].name, "init");
        assert!(!procs[3].is_user_app);
        assert_eq!(procs[4].name, "system_server");
        assert!(!procs[4].is_user_app);
    }

    #[test]
    fn test_parse_android_maps_groups_modules_and_sorts_priority() {
        let sample = r#"
7a100000-7a200000 r-xp 00000000 08:06 12345 /data/app/com.netease.dwrg/lib/arm64/libil2cpp.so
7a200000-7a250000 r--p 00100000 08:06 12345 /data/app/com.netease.dwrg/lib/arm64/libil2cpp.so
7a250000-7a280000 rw-p 00150000 08:06 12345 /data/app/com.netease.dwrg/lib/arm64/libil2cpp.so
7b000000-7b100000 r-xp 00000000 08:06 12346 /data/app/com.netease.dwrg/lib/arm64/libunity.so
12c00000-13c00000 rw-p 00000000 00:00 0     [anon:libc_malloc]
7ffda000-7ffdb000 rw-p 00000000 00:00 0     [stack]
"#;
        let modules = parse_android_maps(sample);
        assert!(!modules.is_empty());
        // libil2cpp.so should be highest priority (first)
        assert_eq!(modules[0].name, "libil2cpp.so");
        assert_eq!(modules[0].base_address, 0x7a100000);
        assert_eq!(modules[0].end_address, 0x7a280000);
        assert!(modules[0].permissions.contains('x'));

        // libunity.so second
        assert_eq!(modules[1].name, "libunity.so");
        assert_eq!(modules[1].base_address, 0x7b000000);
    }

    #[test]
    fn test_calculate_gva_hva_delta() {
        let guest_addr = 0x7a123000usize;
        let host_addr = 0x24a8b123000usize;
        let (delta, explanation) = calculate_gva_hva_delta(guest_addr, host_addr);
        assert_eq!(delta, (host_addr as isize - guest_addr as isize));
        assert!(explanation.contains("Host Base Offset"));
    }
}
