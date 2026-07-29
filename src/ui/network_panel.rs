use std::{
    collections::{BTreeMap, HashSet},
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use flate2::read::{DeflateDecoder, GzDecoder};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject},
};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    Networking::WinInet::{
        INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
    },
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_SZ, RegCloseKey,
        RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    },
};

use super::CrosshairApp;

const DEFAULT_PROXY_ADDRESS: &str = "127.0.0.1:8888";
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_CAPTURE_BODY_BYTES: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MAX_STRUCTURED_ROWS: usize = 5_000;

#[derive(Clone)]
struct NetworkEntry {
    id: u64,
    time: SystemTime,
    method: String,
    host: String,
    target: String,
    headers: String,
    body: Vec<u8>,
    response_headers: String,
    response_body: Vec<u8>,
    notes: String,
    secure_tunnel: bool,
}

#[derive(Default)]
struct RequestTree {
    folders: BTreeMap<String, RequestTree>,
    requests: Vec<NetworkEntry>,
}

impl RequestTree {
    fn insert(&mut self, entry: NetworkEntry) {
        let path = request_path(&entry);
        let mut segments = path.split('/').filter(|part| !part.is_empty()).peekable();
        let mut node = self;
        while let Some(segment) = segments.next() {
            if segments.peek().is_none() {
                break;
            }
            node = node.folders.entry(segment.to_owned()).or_default();
        }
        node.requests.push(entry);
    }
}

enum NetworkEvent {
    Entry(NetworkEntry),
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum DetailTab {
    #[default]
    Overview,
    Contents,
    Ssl,
    Summary,
    Chart,
    Notes,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ContentTab {
    #[default]
    Headers,
    Query,
    Cookies,
    Text,
    Hex,
    Form,
    Json,
    Raw,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum MessageSide {
    #[default]
    Request,
    Response,
}

pub(crate) struct NetworkPanelState {
    bind_address: String,
    filter: String,
    entries: Vec<NetworkEntry>,
    selected_id: Option<u64>,
    next_id: u64,
    detail_tab: DetailTab,
    content_tab: ContentTab,
    message_side: MessageSide,
    status: String,
    expanded_hosts: HashSet<String>,
    host_activity: BTreeMap<String, Instant>,
    pinned: bool,
    proxy: Option<NetworkProxy>,
    recovery_file: PathBuf,
    recovery_rx: Option<Receiver<String>>,
    ca_dir: PathBuf,
    ca_installed: bool,
    remove_ca_on_exit: bool,
    decrypt_https: bool,
    frida_processes: Vec<crate::memory_debugger::debugger::ProcessInfo>,
    frida_pid: Option<u32>,
    frida_script: String,
    frida_log: String,
    frida_session: Option<crate::frida_injector::Session>,
}

impl NetworkPanelState {
    pub(crate) fn new(recovery_file: PathBuf, decrypt_https: bool) -> Self {
        let ca_dir = recovery_file
            .parent()
            .unwrap_or(Path::new("."))
            .join("network-ca");
        let ca_installed = ca_dir.join("ca.cer").exists();
        let (recovery_status, recovery_rx) = if recovery_file.exists() {
            let (tx, rx) = unbounded();
            let recovery_path = recovery_file.clone();
            thread::spawn(move || {
                let status = match SystemProxyGuard::restore_file(&recovery_path) {
                    Ok(true) => "Recovered Windows proxy from the previous session".to_owned(),
                    Ok(false) => "Stopped".to_owned(),
                    Err(error) => format!("Unable to recover the previous Windows proxy: {error}"),
                };
                let _ = tx.send(status);
            });
            ("Restoring previous Windows proxy…".to_owned(), Some(rx))
        } else {
            ("Stopped".to_owned(), None)
        };
        Self {
            bind_address: DEFAULT_PROXY_ADDRESS.to_owned(),
            filter: String::new(),
            entries: Vec::new(),
            selected_id: None,
            next_id: 1,
            detail_tab: DetailTab::Overview,
            content_tab: ContentTab::Headers,
            message_side: MessageSide::Request,
            status: recovery_status,
            expanded_hosts: HashSet::new(),
            host_activity: BTreeMap::new(),
            pinned: false,
            proxy: None,
            recovery_file,
            recovery_rx,
            ca_dir,
            ca_installed,
            remove_ca_on_exit: false,
            decrypt_https,
            frida_processes: Vec::new(),
            frida_pid: None,
            frida_script: crate::frida_injector::DEFAULT_NETWORK_SCRIPT.to_owned(),
            frida_log: String::new(),
            frida_session: None,
        }
    }

    fn drain(&mut self) {
        if let Some(rx) = &self.recovery_rx
            && let Ok(status) = rx.try_recv()
        {
            self.status = status;
            self.recovery_rx = None;
        }
        if let Some(session) = &self.frida_session {
            while let Ok(event) = session.events.try_recv() {
                match event {
                    crate::frida_injector::Event::Status(value) => self.status = value,
                    crate::frida_injector::Event::Log(value) => {
                        if !self.frida_log.is_empty() {
                            self.frida_log.push('\n');
                        }
                        self.frida_log.push_str(&value);
                    }
                }
            }
        }
        let Some(proxy) = &self.proxy else { return };
        while let Ok(event) = proxy.events.try_recv() {
            match event {
                NetworkEvent::Entry(mut entry) => {
                    entry.id = self.next_id;
                    self.next_id += 1;
                    self.host_activity
                        .insert(entry.host.clone(), Instant::now());
                    self.entries.push(entry);
                }
                NetworkEvent::Error(error) => self.status = format!("Proxy error: {error}"),
            }
        }
        if self.entries.len() > MAX_ENTRIES {
            self.entries.drain(..self.entries.len() - MAX_ENTRIES);
        }
    }

    pub(crate) fn active_proxy_url(&self) -> Option<String> {
        self.proxy
            .as_ref()
            .map(|_| format!("http://{}", self.bind_address))
    }

    fn start(&mut self) {
        if self.decrypt_https {
            // The certificate file can outlive its Windows trust-store entry.
            // Re-adding the same CA is idempotent and repairs that stale state.
            self.install_ca();
            if !self.ca_installed {
                return;
            }
        }
        let mitm = self
            .decrypt_https
            .then(|| MitmConfig::load(&self.ca_dir))
            .transpose();
        let mitm = match mitm {
            Ok(mitm) => mitm,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        match NetworkProxy::start(&self.bind_address, self.recovery_file.clone(), mitm) {
            Ok(proxy) => {
                self.status = format!("Recording on {}", self.bind_address);
                self.proxy = Some(proxy);
            }
            Err(error) => self.status = format!("Unable to start proxy: {error}"),
        }
    }

    fn stop(&mut self) {
        self.proxy.take();
        self.status = "Stopped".to_owned();
    }

    fn restore_proxy(&mut self) {
        let was_running = self.proxy.is_some();
        self.proxy.take();
        self.status = match SystemProxyGuard::restore_file(&self.recovery_file) {
            Ok(true) => "Previous Windows proxy restored".to_owned(),
            Ok(false) if was_running => "Windows proxy restored".to_owned(),
            Ok(false) => "No saved proxy settings".to_owned(),
            Err(error) => format!("Unable to restore proxy: {error}"),
        };
    }

    pub(crate) fn shutdown(&mut self) {
        self.frida_session.take();
        self.proxy.take();
        if self.recovery_file.exists()
            && SystemProxyGuard::restore_file(&self.recovery_file).is_err()
        {
            let _ = set_system_proxy(None, false);
        }
        if self.remove_ca_on_exit && self.ca_installed {
            spawn_ca_removal();
            self.ca_installed = false;
        }
    }

    fn install_ca(&mut self) {
        self.status = match install_ca(&self.ca_dir) {
            Ok(()) => {
                self.ca_installed = true;
                "MacroNest CA installed for the current Windows user".to_owned()
            }
            Err(error) => format!("Unable to install MacroNest CA: {error}"),
        };
    }

    fn remove_ca(&mut self) {
        self.decrypt_https = false;
        self.status = match remove_ca() {
            Ok(()) => {
                self.ca_installed = false;
                "MacroNest CA removed".to_owned()
            }
            Err(error) => format!("Unable to remove MacroNest CA: {error}"),
        };
    }

    fn refresh_frida_processes(&mut self) {
        match crate::memory_debugger::debugger::list_process_details() {
            Ok(processes) => {
                self.frida_processes = processes;
                if self
                    .frida_pid
                    .is_some_and(|pid| !self.frida_processes.iter().any(|item| item.pid == pid))
                {
                    self.frida_pid = None;
                }
            }
            Err(error) => self.status = format!("Unable to list processes: {error}"),
        }
    }

}

#[derive(Clone)]
struct MitmConfig {
    cert_pem: String,
    key_pem: String,
}

impl MitmConfig {
    fn load(dir: &Path) -> Result<Self, String> {
        let cert_pem = std::fs::read_to_string(dir.join("ca.pem"))
            .map_err(|_| "Install the MacroNest CA before enabling HTTPS decryption".to_owned())?;
        let key_pem =
            std::fs::read_to_string(dir.join("ca-key.pem")).map_err(|error| error.to_string())?;
        Ok(Self { cert_pem, key_pem })
    }
}

struct NetworkProxy {
    stop: Arc<AtomicBool>,
    events: Receiver<NetworkEvent>,
    thread: Option<JoinHandle<()>>,
    system_proxy: Option<SystemProxyGuard>,
}

impl NetworkProxy {
    fn start(
        address: &str,
        recovery_file: PathBuf,
        mitm: Option<MitmConfig>,
    ) -> Result<Self, String> {
        let parsed = address
            .parse::<SocketAddr>()
            .map_err(|_| "proxy address is invalid".to_owned())?;
        if !parsed.ip().is_loopback() {
            return Err("proxy must use a loopback address".to_owned());
        }
        let listener = TcpListener::bind(parsed).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let decrypt_https = mitm.is_some();
        let mitm_bypass_hosts = Arc::new(Mutex::new(HashSet::new()));
        let (tx, events) = unbounded();
        let thread = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let tx = tx.clone();
                        let mitm = mitm.clone();
                        let mitm_bypass_hosts = Arc::clone(&mitm_bypass_hosts);
                        thread::spawn(move || {
                            // Accepted sockets can inherit the listener's non-blocking mode on
                            // Windows; the connection handler intentionally uses blocking I/O.
                            if let Err(error) = stream.set_nonblocking(false) {
                                let _ = tx.send(NetworkEvent::Error(format!(
                                    "configure accepted connection failed: {error}"
                                )));
                                return;
                            }
                            if let Err(error) = proxy_connection(
                                stream,
                                tx.clone(),
                                mitm.as_ref(),
                                &mitm_bypass_hosts,
                            ) {
                                let _ = tx.send(NetworkEvent::Error(error.to_string()));
                            }
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });
        let mut proxy = Self {
            stop,
            events,
            thread: Some(thread),
            system_proxy: None,
        };
        if decrypt_https {
            if let Err(client_error) = verify_local_mitm(address) {
                thread::sleep(Duration::from_millis(25));
                let server_error = proxy.events.try_iter().find_map(|event| match event {
                    NetworkEvent::Error(error) => Some(error),
                    NetworkEvent::Entry(entry) if !entry.notes.is_empty() => Some(entry.notes),
                    _ => None,
                });
                return Err(format!(
                    "HTTPS decryption self-check failed; Windows proxy was not changed: {client_error}{}",
                    server_error
                        .map(|error| format!(" | proxy: {error}"))
                        .unwrap_or_default()
                ));
            }
        }
        proxy.system_proxy = Some(SystemProxyGuard::enable(address, recovery_file)?);
        Ok(proxy)
    }
}

fn verify_local_mitm(address: &str) -> Result<(), String> {
    const CHECK_HOST: &str = "localhost";
    let mut stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
    let timeout = Some(Duration::from_secs(5));
    stream
        .set_read_timeout(timeout)
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(timeout)
        .map_err(|error| error.to_string())?;
    stream
        .write_all(
            format!("CONNECT {CHECK_HOST}:443 HTTP/1.1\r\nHost: {CHECK_HOST}:443\r\n\r\n")
                .as_bytes(),
        )
        .map_err(|error| error.to_string())?;
    let response = read_header(&mut stream).map_err(|error| error.to_string())?;
    if !response.starts_with(b"HTTP/1.1 200") {
        return Err(format!(
            "local proxy rejected CONNECT: {}",
            String::from_utf8_lossy(&response)
                .lines()
                .next()
                .unwrap_or("empty response")
        ));
    }
    let mut tls = native_tls::TlsConnector::new()
        .map_err(|error| error.to_string())?
        .connect(CHECK_HOST, stream)
        .map_err(|error| error.to_string())?;
    tls.shutdown().map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct ProxySnapshot {
    proxy_enable: Option<u32>,
    proxy_server: Option<String>,
}

struct SystemProxyGuard {
    recovery_file: PathBuf,
}

impl SystemProxyGuard {
    fn enable(address: &str, recovery_file: PathBuf) -> Result<Self, String> {
        if recovery_file.exists() {
            Self::restore_file(&recovery_file)?;
        }
        let snapshot = query_system_proxy()?;
        let bytes = serde_json::to_vec(&snapshot).map_err(|error| error.to_string())?;
        std::fs::write(&recovery_file, bytes).map_err(|error| error.to_string())?;
        if let Err(error) = set_system_proxy(Some(address), true) {
            let _ = std::fs::remove_file(&recovery_file);
            return Err(error);
        }
        Ok(Self { recovery_file })
    }

    fn restore_file(path: &Path) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let snapshot: ProxySnapshot =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        restore_system_proxy(&snapshot)?;
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
        Ok(true)
    }
}

impl Drop for SystemProxyGuard {
    fn drop(&mut self) {
        if Self::restore_file(&self.recovery_file).is_err() {
            // Never leave Windows pointing at a proxy process that is about to exit.
            let _ = set_system_proxy(None, false);
        }
    }
}

fn query_system_proxy() -> Result<ProxySnapshot, String> {
    let key = InternetSettingsKey::open()?;
    Ok(ProxySnapshot {
        proxy_enable: key.query_dword("ProxyEnable")?,
        proxy_server: key.query_string("ProxyServer")?,
    })
}

fn set_system_proxy(server: Option<&str>, enabled: bool) -> Result<(), String> {
    let key = InternetSettingsKey::open()?;
    key.set_dword("ProxyEnable", u32::from(enabled))?;
    match server {
        Some(server) => key.set_string("ProxyServer", server)?,
        None => key.delete("ProxyServer")?,
    }
    notify_proxy_changed()?;
    let current = query_system_proxy()?;
    if current.proxy_enable.unwrap_or(0) != u32::from(enabled)
        || server.is_some_and(|server| current.proxy_server.as_deref() != Some(server))
    {
        return Err("Windows did not accept the proxy settings".to_owned());
    }
    Ok(())
}

fn restore_system_proxy(snapshot: &ProxySnapshot) -> Result<(), String> {
    set_system_proxy(
        snapshot.proxy_server.as_deref(),
        snapshot.proxy_enable.unwrap_or(0) != 0,
    )
}

struct InternetSettingsKey(HKEY);

impl InternetSettingsKey {
    fn open() -> Result<Self, String> {
        let path = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings");
        let mut key = std::ptr::null_mut();
        let result = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                &mut key,
            )
        };
        win32_result(result, "open Windows internet settings")?;
        Ok(Self(key))
    }

    fn query_dword(&self, name: &str) -> Result<Option<u32>, String> {
        let name = wide(name);
        let mut value = 0_u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let result = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut size,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        win32_result(result, "read Windows proxy state")?;
        Ok(Some(value))
    }

    fn query_string(&self, name: &str) -> Result<Option<String>, String> {
        let name = wide(name);
        let mut size = 0_u32;
        let result = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        win32_result(result, "read Windows proxy server")?;
        let mut bytes = vec![0_u8; size as usize];
        let result = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                bytes.as_mut_ptr(),
                &mut size,
            )
        };
        win32_result(result, "read Windows proxy server")?;
        let words = bytes[..size as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|word| *word != 0)
            .collect::<Vec<_>>();
        Ok(Some(String::from_utf16_lossy(&words)))
    }

    fn set_dword(&self, name: &str, value: u32) -> Result<(), String> {
        let name = wide(name);
        let bytes = value.to_le_bytes();
        let result = unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                REG_DWORD,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        };
        win32_result(result, "write Windows proxy state")
    }

    fn set_string(&self, name: &str, value: &str) -> Result<(), String> {
        let name = wide(name);
        let words = wide(value);
        let bytes =
            unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), words.len() * 2) };
        let result = unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                REG_SZ,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        };
        win32_result(result, "write Windows proxy server")
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        let name = wide(name);
        let result = unsafe { RegDeleteValueW(self.0, name.as_ptr()) };
        if result == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            win32_result(result, "remove Windows proxy server")
        }
    }
}

impl Drop for InternetSettingsKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn notify_proxy_changed() -> Result<(), String> {
    unsafe {
        if InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            return Err(format!(
                "notify Windows proxy change failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            return Err(format!(
                "refresh Windows proxy failed: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win32_result(code: u32, action: &str) -> Result<(), String> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{action} failed (Windows error {code})"))
    }
}

fn install_ca(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let cert_path = dir.join("ca.cer");
    if !cert_path.exists() {
        let mut params =
            CertificateParams::new(Vec::<String>::new()).map_err(|error| error.to_string())?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(DnType::CommonName, "MacroNest Network CA");
        params.key_usages.extend([
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ]);
        let key = KeyPair::generate().map_err(|error| error.to_string())?;
        let cert = params
            .self_signed(&key)
            .map_err(|error| error.to_string())?;
        std::fs::write(dir.join("ca.pem"), cert.pem()).map_err(|error| error.to_string())?;
        std::fs::write(dir.join("ca-key.pem"), key.serialize_pem())
            .map_err(|error| error.to_string())?;
        std::fs::write(&cert_path, cert.der()).map_err(|error| error.to_string())?;
    }
    let path = cert_path.to_string_lossy().into_owned();
    run_certutil(["-user", "-addstore", "-f", "Root", &path])
}

fn run_certutil<const N: usize>(args: [&str; N]) -> Result<(), String> {
    let mut command = Command::new("certutil.exe");
    command.args(args);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if message.is_empty() {
            format!("certutil exited with {}", output.status)
        } else {
            message
        })
    }
}

fn remove_ca() -> Result<(), String> {
    run_certutil(["-user", "-delstore", "Root", "MacroNest Network CA"])
}

fn spawn_ca_removal() {
    let mut command = Command::new("certutil.exe");
    command.args(["-user", "-delstore", "Root", "MacroNest Network CA"]);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let _ = command.spawn();
}

impl Drop for NetworkProxy {
    fn drop(&mut self) {
        // Restore connectivity before waiting for the proxy worker to exit.
        self.system_proxy.take();
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn proxy_connection(
    mut client: TcpStream,
    events: Sender<NetworkEvent>,
    mitm: Option<&MitmConfig>,
    mitm_bypass_hosts: &Mutex<HashSet<String>>,
) -> std::io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(10)))?;
    let header = read_header(&mut client)?;
    client.set_read_timeout(None)?;
    let header_end = header
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .unwrap_or(header.len());
    let text = String::from_utf8_lossy(&header[..header_end]).into_owned();
    let mut lines = text.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_owned();
    let target = request_parts.next().unwrap_or_default().to_owned();
    let version = request_parts.next().unwrap_or("HTTP/1.1").to_owned();
    let host_header = lines
        .find_map(|line| {
            line.strip_prefix("Host:")
                .or_else(|| line.strip_prefix("host:"))
        })
        .map(str::trim)
        .unwrap_or_default();

    if method.eq_ignore_ascii_case("CONNECT") {
        let host = target.clone();
        let bypass_mitm = mitm_bypass_hosts
            .lock()
            .map(|hosts| hosts.contains(&host))
            .unwrap_or(true);
        if let Some(mitm) = mitm.filter(|_| !bypass_mitm) {
            client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
            if let Err(error) = mitm_connection(client, &host, mitm, events.clone()) {
                if is_unclean_tls_close(&error) {
                    return Ok(());
                }
                if let Ok(mut hosts) = mitm_bypass_hosts.lock() {
                    hosts.insert(host.clone());
                }
                events
                    .send(NetworkEvent::Entry(NetworkEntry {
                        id: 0,
                        time: SystemTime::now(),
                        method: method.clone(),
                        host: host.clone(),
                        target: target.clone(),
                        headers: text.clone(),
                        body: Vec::new(),
                        response_headers: String::new(),
                        response_body: Vec::new(),
                        notes: format!(
                            "HTTPS decryption failed; later connections to this host use a safe encrypted tunnel: {error}"
                        ),
                        secure_tunnel: true,
                    }))
                    .ok();
                return Err(std::io::Error::other(format!(
                    "HTTPS decryption failed for {host}; retrying safely on the next connection: {error}"
                )));
            }
            return Ok(());
        }
        events
            .send(NetworkEvent::Entry(NetworkEntry {
                id: 0,
                time: SystemTime::now(),
                method: method.clone(),
                host: host.clone(),
                target: target.clone(),
                headers: text.clone(),
                body: Vec::new(),
                response_headers: String::new(),
                response_body: Vec::new(),
                notes: String::new(),
                secure_tunnel: true,
            }))
            .ok();
        let mut server = TcpStream::connect(&host).map_err(|error| {
            std::io::Error::new(error.kind(), format!("connect to {host} failed: {error}"))
        })?;
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        server.write_all(&header[header_end..])?;
        tunnel(client, server).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("TLS tunnel for {host} failed: {error}"),
            )
        })
    } else {
        let (host, origin_target) = http_destination(&target, host_header);
        let body = header[header_end..].to_vec();
        events
            .send(NetworkEvent::Entry(NetworkEntry {
                id: 0,
                time: SystemTime::now(),
                method: method.clone(),
                host: host.clone(),
                target: target.clone(),
                headers: text.clone(),
                body,
                response_headers: String::new(),
                response_body: Vec::new(),
                notes: String::new(),
                secure_tunnel: false,
            }))
            .ok();
        let address = if host.contains(':') {
            host.clone()
        } else {
            format!("{host}:80")
        };
        let mut server = TcpStream::connect(address)?;
        let rewritten = rewrite_request(&header, &method, &origin_target, &version);
        server.write_all(&rewritten)?;
        tunnel(client, server)
    }
}

fn read_header(stream: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::with_capacity(2048);
    let mut buffer = [0_u8; 2048];
    while data.len() < MAX_HEADER_BYTES {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..count]);
        if let Some(position) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = position + 4;
            let header_text = String::from_utf8_lossy(&data[..header_end]);
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0)
                .min(MAX_CAPTURE_BODY_BYTES);
            let target_length = header_end + content_length;
            while data.len() < target_length {
                let count = stream.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                let remaining = target_length - data.len();
                data.extend_from_slice(&buffer[..count.min(remaining)]);
            }
            return Ok(data);
        }
    }
    if data.len() >= MAX_HEADER_BYTES {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request headers exceed 64 KiB",
        ))
    } else {
        Ok(data)
    }
}

fn mitm_connection(
    client: TcpStream,
    target: &str,
    mitm: &MitmConfig,
    events: Sender<NetworkEvent>,
) -> std::io::Result<()> {
    let hostname = target
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(target);
    let ca_key = KeyPair::from_pem(&mitm.key_pem).map_err(io_other)?;
    let issuer = Issuer::from_ca_cert_pem(&mitm.cert_pem, ca_key).map_err(io_other)?;
    let leaf_key = KeyPair::generate().map_err(io_other)?;
    let mut params = CertificateParams::new(vec![hostname.to_owned()]).map_err(io_other)?;
    params.distinguished_name.push(DnType::CommonName, hostname);
    params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);
    let leaf = params.signed_by(&leaf_key, &issuer).map_err(io_other)?;
    let ca = CertificateDer::from_pem_slice(mitm.cert_pem.as_bytes()).map_err(io_other)?;
    let config = ServerConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
        .with_safe_default_protocol_versions()
        .map_err(io_other)?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(leaf.der().to_vec()), ca],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        )
        .map_err(io_other)?;
    let mut downstream = StreamOwned::new(
        ServerConnection::new(Arc::new(config)).map_err(io_other)?,
        client,
    );

    let request = read_header(&mut downstream)?;
    if request.is_empty() {
        return Ok(());
    }
    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .unwrap_or(request.len());
    let text = String::from_utf8_lossy(&request[..header_end]).into_owned();
    let mut parts = text.lines().next().unwrap_or_default().split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or("/").to_owned();
    let server = TcpStream::connect(target)?;
    let connector = native_tls::TlsConnector::new().map_err(io_other)?;
    let mut upstream = connector.connect(hostname, server).map_err(io_other)?;
    let websocket_upgrade = text.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("upgrade") && value.trim().eq_ignore_ascii_case("websocket")
        })
    });
    if websocket_upgrade {
        upstream.write_all(&request)?;
    } else {
        upstream.write_all(&force_connection_close(&request, header_end))?;
    }
    let response = read_header(&mut upstream)?;
    let response_header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .unwrap_or(response.len());
    let response_headers =
        String::from_utf8_lossy(&response[..response_header_end]).into_owned();
    let switching_protocols = response_headers
        .lines()
        .next()
        .is_some_and(|line| line.split_whitespace().nth(1) == Some("101"));
    downstream.write_all(&response)?;
    let mut response_body = response[response_header_end..].to_vec();
    if switching_protocols {
        emit_http_entry(
            &events,
            method,
            hostname,
            path.clone(),
            text,
            request[header_end..].to_vec(),
            response_headers,
            response_body,
        );
        relay_websocket(&mut downstream, &mut upstream, hostname, &path, events)?;
    } else {
        copy_and_capture(&mut upstream, &mut downstream, &mut response_body)?;
        emit_http_entry(
            &events,
            method,
            hostname,
            path,
            text,
            request[header_end..].to_vec(),
            response_headers,
            response_body,
        );
    }
    downstream.flush()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_http_entry(
    events: &Sender<NetworkEvent>,
    method: String,
    host: &str,
    target: String,
    headers: String,
    body: Vec<u8>,
    response_headers: String,
    response_body: Vec<u8>,
) {
    events
        .send(NetworkEvent::Entry(NetworkEntry {
            id: 0,
            time: SystemTime::now(),
            method,
            host: host.to_owned(),
            target,
            headers,
            body,
            response_headers,
            response_body,
            notes: String::new(),
            secure_tunnel: false,
        }))
        .ok();
}

fn copy_and_capture(
    reader: &mut impl Read,
    writer: &mut impl Write,
    captured: &mut Vec<u8>,
) -> std::io::Result<()> {
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        writer.write_all(&buffer[..count])?;
        if captured.len() < MAX_CAPTURE_BODY_BYTES {
            let remaining = MAX_CAPTURE_BODY_BYTES - captured.len();
            captured.extend_from_slice(&buffer[..count.min(remaining)]);
        }
    }
}

fn relay_websocket(
    downstream: &mut StreamOwned<ServerConnection, TcpStream>,
    upstream: &mut native_tls::TlsStream<TcpStream>,
    host: &str,
    path: &str,
    events: Sender<NetworkEvent>,
) -> std::io::Result<()> {
    downstream
        .sock
        .set_read_timeout(Some(Duration::from_millis(25)))?;
    upstream
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(25)))?;
    let mut client_frames = Vec::new();
    let mut server_frames = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let mut progressed = false;
        match downstream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                progressed = true;
                upstream.write_all(&buffer[..count])?;
                client_frames.extend_from_slice(&buffer[..count]);
                emit_websocket_frames(&mut client_frames, "WS SEND", host, path, &events);
            }
            Err(error) if is_retryable_io(&error) => {}
            Err(error) => return Err(error),
        }
        match upstream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                progressed = true;
                downstream.write_all(&buffer[..count])?;
                downstream.flush()?;
                server_frames.extend_from_slice(&buffer[..count]);
                emit_websocket_frames(&mut server_frames, "WS RECEIVE", host, path, &events);
            }
            Err(error) if is_retryable_io(&error) => {}
            Err(error) => return Err(error),
        }
        if !progressed {
            thread::yield_now();
        }
    }
}

fn is_retryable_io(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn is_unclean_tls_close(error: &std::io::Error) -> bool {
    let message = error.to_string();
    message.contains("peer closed connection without sending TLS close_notify")
        || message.contains("unexpected-eof")
}

fn emit_websocket_frames(
    buffer: &mut Vec<u8>,
    method: &str,
    host: &str,
    path: &str,
    events: &Sender<NetworkEvent>,
) {
    loop {
        if buffer.len() < 2 {
            return;
        }
        let masked = buffer[1] & 0x80 != 0;
        let mut payload_len = usize::from(buffer[1] & 0x7f);
        let mut offset = 2;
        if payload_len == 126 {
            if buffer.len() < 4 {
                return;
            }
            payload_len = usize::from(u16::from_be_bytes([buffer[2], buffer[3]]));
            offset = 4;
        } else if payload_len == 127 {
            if buffer.len() < 10 {
                return;
            }
            let length = u64::from_be_bytes(buffer[2..10].try_into().unwrap_or([0; 8]));
            let Ok(length) = usize::try_from(length) else {
                buffer.clear();
                return;
            };
            payload_len = length;
            offset = 10;
        }
        let mask = if masked {
            if buffer.len() < offset + 4 {
                return;
            }
            let value = [
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
            ];
            offset += 4;
            Some(value)
        } else {
            None
        };
        if buffer.len() < offset.saturating_add(payload_len) {
            return;
        }
        let opcode = buffer[0] & 0x0f;
        let mut payload = buffer[offset..offset + payload_len].to_vec();
        if let Some(mask) = mask {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }
        buffer.drain(..offset + payload_len);
        if !matches!(opcode, 1 | 2) {
            continue;
        }
        events
            .send(NetworkEvent::Entry(NetworkEntry {
                id: 0,
                time: SystemTime::now(),
                method: method.to_owned(),
                host: host.to_owned(),
                target: path.to_owned(),
                headers: format!(
                    "WebSocket opcode: {opcode}\nPayload length: {}",
                    payload.len()
                ),
                body: payload,
                response_headers: String::new(),
                response_body: Vec::new(),
                notes: String::new(),
                secure_tunnel: false,
            }))
            .ok();
    }
}

fn force_connection_close(request: &[u8], header_end: usize) -> Vec<u8> {
    let header = String::from_utf8_lossy(&request[..header_end]);
    let mut output = String::new();
    for line in header.lines() {
        if !line.to_ascii_lowercase().starts_with("connection:")
            && !line.to_ascii_lowercase().starts_with("proxy-connection:")
        {
            output.push_str(line);
            output.push_str("\r\n");
        }
    }
    output.push_str("Connection: close\r\n\r\n");
    let mut bytes = output.into_bytes();
    bytes.extend_from_slice(&request[header_end..]);
    bytes
}

fn io_other(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

fn http_destination(target: &str, host_header: &str) -> (String, String) {
    if let Some(rest) = target.strip_prefix("http://") {
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        (host.to_owned(), format!("/{path}"))
    } else {
        (host_header.to_owned(), target.to_owned())
    }
}

fn rewrite_request(header: &[u8], method: &str, target: &str, version: &str) -> Vec<u8> {
    let Some(line_end) = header.windows(2).position(|window| window == b"\r\n") else {
        return header.to_vec();
    };
    let mut output = format!("{method} {target} {version}").into_bytes();
    output.extend_from_slice(&header[line_end..]);
    output
}

fn tunnel(mut left: TcpStream, mut right: TcpStream) -> std::io::Result<()> {
    let mut left_read = left.try_clone()?;
    let mut right_write = right.try_clone()?;
    let forward = thread::spawn(move || {
        let result = std::io::copy(&mut left_read, &mut right_write);
        let _ = right_write.shutdown(Shutdown::Write);
        result
    });
    let result = std::io::copy(&mut right, &mut left);
    let _ = left.shutdown(Shutdown::Write);
    let _ = forward.join();
    result.map(|_| ())
}

impl CrosshairApp {
    pub(crate) fn render_network_panel(&mut self, ui: &mut egui::Ui) {
        self.network_panel.drain();
        let running = self.network_panel.proxy.is_some();
        let host_flash_active = self
            .network_panel
            .host_activity
            .values()
            .any(|updated| updated.elapsed() < Duration::from_millis(700));
        if running || host_flash_active {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new(self.tr("Network", "Network")).strong().size(17.0));
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(if self.network_panel.pinned {
                        self.tr("Unpin hosts", "Unpin hosts")
                    } else {
                        self.tr("Pin hosts", "Pin hosts")
                    })
                    .clicked()
                {
                    self.network_panel.pinned = !self.network_panel.pinned;
                }
                if ui.button(self.tr("Clear", "Clear")).clicked() {
                    self.network_panel.entries.clear();
                    self.network_panel.host_activity.clear();
                    self.network_panel.selected_id = None;
                }
                if ui.button(self.tr("Restore proxy", "Restore proxy")).clicked() {
                    self.network_panel.restore_proxy();
                }
                if running {
                    if ui.button(self.tr("Stop", "Stop")).clicked() {
                        self.network_panel.stop();
                    }
                } else if ui.button(self.tr("Start", "Start")).clicked() {
                    self.network_panel.start();
                }
            });
        });
        ui.horizontal(|ui| {
            let status_text = if self.network_panel.status == "Stopped" {
                self.tr("Stopped", "Stopped")
            } else if self.network_panel.status == "Ready" {
                self.tr("Ready", "Ready")
            } else {
                &self.network_panel.status
            };
            ui.add(egui::Label::new(status_text).wrap());
            if self.network_panel.status.contains("failed")
                || self.network_panel.status.contains("error")
            {
                if ui.small_button(self.tr("Copy error", "Copy error")).clicked() {
                    ui.ctx().copy_text(self.network_panel.status.clone());
                }
            }
        });
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label(self.tr("Proxy", "Proxy"));
            let bind_resp = ui.add_enabled(
                !running,
                egui::TextEdit::singleline(&mut self.network_panel.bind_address)
                    .desired_width(150.0),
            );
            Self::apply_vietnamese_input_if_changed(
                &bind_resp,
                self.state.vietnamese_input_enabled,
                self.state.vietnamese_input_mode,
                &mut self.network_panel.bind_address,
            );
            if ui.button(self.tr("Copy", "Copy")).clicked() {
                ui.ctx().copy_text(self.network_panel.bind_address.clone());
            }
            ui.separator();
            if ui.add_enabled(!self.network_panel.ca_installed, egui::Button::new(self.tr("Install CA", "Install CA"))).clicked() {
                self.network_panel.install_ca();
            }
            if ui.add_enabled(self.network_panel.ca_installed, egui::Button::new(self.tr("Remove CA", "Remove CA"))).clicked() {
                self.network_panel.remove_ca();
                self.state.network_decrypt_https = false;
                self.persist_deferred(ui.ctx());
            }
            let decrypt_label = self.tr("Decrypt HTTPS", "Decrypt HTTPS");
            if ui.add_enabled(
                !running && self.network_panel.ca_installed,
                egui::Checkbox::new(&mut self.network_panel.decrypt_https, decrypt_label),
            ).changed() {
                self.state.network_decrypt_https = self.network_panel.decrypt_https;
                self.persist_deferred(ui.ctx());
            }
            let remove_ca_label = self.tr("Remove CA on exit", "Remove CA on exit");
            ui.checkbox(&mut self.network_panel.remove_ca_on_exit, remove_ca_label);
            ui.separator();
            ui.label(self.tr("Filter", "Filter"));
            let filter_resp = ui.add(
                egui::TextEdit::singleline(&mut self.network_panel.filter)
                    .desired_width(f32::INFINITY),
            );
            Self::apply_vietnamese_input_if_changed(
                &filter_resp,
                self.state.vietnamese_input_enabled,
                self.state.vietnamese_input_mode,
                &mut self.network_panel.filter,
            );
        });
        let proxy_hint = self.tr("Press Start to capture domains safely. Enable Decrypt HTTPS only when the target accepts the MacroNest CA; the CA is installed once and reused.", "Press Start to capture domains safely. Enable Decrypt HTTPS only when the target accepts the MacroNest CA; the CA is installed once and reused.");
        ui.label(RichText::new(proxy_hint).small().weak());
        ui.group(|ui| {
            ui.label(RichText::new(self.tr("Frida injection (certificate pinning only)", "Frida injection (certificate pinning only)")).strong());
            ui.label(RichText::new(self.tr("Attaching Frida alone does not bypass TLS. Use this only with a hook script for the target app's TLS stack.", "Attaching Frida alone does not bypass TLS. Use this only with a hook script for the target app's TLS stack.")).small().weak());
            let selected = self
                .network_panel
                .frida_pid
                .and_then(|pid| {
                    self.network_panel
                        .frida_processes
                        .iter()
                        .find(|item| item.pid == pid)
                })
                .map(|process| format!("{} — PID {}", process.name, process.pid))
                .unwrap_or_else(|| self.tr("Select process", "Select process").to_owned());
            let process_picker = egui::ComboBox::from_id_salt("network-frida-process")
                .height(720.0)
                .selected_text(Self::truncate_window_title(&selected, 52))
                .show_ui(ui, |ui| {
                    ui.set_min_height(480.0);
                    ui.label(RichText::new(self.tr("Window processes (grouped)", "Window processes (grouped)")).strong());
                    for window in self.open_window_infos.clone() {
                        let Some(pid) = crate::window_list::process_id_for_window(Some(&window.selector)) else { continue };
                        if pid == std::process::id() { continue; }
                        if Self::selectable_process_row(
                            ui,
                            self.network_panel.frida_pid == Some(pid),
                            Self::truncate_window_title(&Self::simplify_window_title(&window.title), 70),
                            window.process_id,
                            &window.process_path,
                        ).clicked() {
                            self.network_panel.frida_pid = Some(pid);
                        }
                    }
                    ui.separator();
                    ui.label(RichText::new(self.tr("All processes (individual PID)", "All processes (individual PID)")).strong());
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.add_sized([190.0, 18.0], egui::Label::new(RichText::new("Name").strong()));
                        ui.add_sized([70.0, 18.0], egui::Label::new(RichText::new("PID").strong()));
                        ui.label(RichText::new("Path").strong());
                    });
                    let count = self.network_panel.frida_processes.len();
                    egui::ScrollArea::vertical().max_height(620.0).show_rows(ui, 22.0, count, |ui, rows| {
                        for index in rows {
                            let process = &mut self.network_panel.frida_processes[index];
                            if process.pid == std::process::id() { continue; }
                            if process.path.is_empty() {
                                process.path = crate::memory_debugger::debugger::process_path(process.pid);
                            }
                            if Self::selectable_process_detail_row(ui, self.network_panel.frida_pid == Some(process.pid), &process.name, process.pid, &process.path).clicked() {
                                self.network_panel.frida_pid = Some(process.pid);
                            }
                        }
                    });
                });
            if process_picker.response.clicked() { self.network_panel.refresh_frida_processes(); }
            if process_picker.response.clicked() { self.ensure_open_windows_ready(true); }
            ui.horizontal(|ui| {
                if self.network_panel.frida_session.is_some() {
                    if ui.button(self.tr("Detach", "Detach")).clicked() {
                        self.network_panel.frida_session.take();
                    }
                } else if ui
                    .add_enabled(
                        self.network_panel.frida_pid.is_some(),
                        egui::Button::new(self.tr("Attach Frida agent", "Attach Frida agent")),
                    )
                    .clicked()
                {
                    let pid = self.network_panel.frida_pid.unwrap();
                    self.network_panel.frida_log.clear();
                    self.network_panel.status = format!("Attaching Frida to PID {pid}...");
                    self.network_panel.frida_session =
                        Some(crate::frida_injector::Session::attach(
                            pid,
                            self.network_panel.frida_script.clone(),
                        ));
                }
            });
            ui.collapsing(self.tr("Advanced options", "Advanced options"), |ui| {
                ui.label(self.tr("Custom Frida JavaScript", "Custom Frida JavaScript"));
                let script_resp = ui.add(
                    egui::TextEdit::multiline(&mut self.network_panel.frida_script)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(7),
                );
                Self::apply_vietnamese_input_if_changed(
                    &script_resp,
                    self.state.vietnamese_input_enabled,
                    self.state.vietnamese_input_mode,
                    &mut self.network_panel.frida_script,
                );
                if ui.button(self.tr("Clear log", "Clear log")).clicked() { self.network_panel.frida_log.clear(); }
                if !self.network_panel.frida_log.is_empty() { readonly_text(ui, self.network_panel.frida_log.clone()); }
            });
        });
        ui.separator();

        self.render_network_capture(ui);
    }

    fn render_network_capture(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        ui.horizontal(|ui| {
            ui.set_height(available.y);
            let list_width = (available.x * 0.32).clamp(220.0, 360.0);
            ui.allocate_ui_with_layout(
                egui::vec2(list_width, available.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.render_network_list(ui),
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2((available.x - list_width - 8.0).max(180.0), available.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("network-detail-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            self.render_network_detail(ui);
                        });
                },
            );
        });
    }

    fn render_network_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.tr("Host", "Host")).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let req_text = self.tr("request(s)", "request(s)");
                ui.label(format!("{} {req_text}", self.network_panel.entries.len()));
            });
        });
        ui.separator();
        let filter = self.network_panel.filter.to_ascii_lowercase();
        let mut hosts = Vec::<String>::new();
        for entry in self.network_panel.entries.iter().rev() {
            if (filter.is_empty()
                || entry.host.to_ascii_lowercase().contains(&filter)
                || entry.target.to_ascii_lowercase().contains(&filter))
                && !hosts.contains(&entry.host)
            {
                hosts.push(entry.host.clone());
            }
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for host in hosts {
                    let matching = self
                        .network_panel
                        .entries
                        .iter()
                        .rev()
                        .filter(|entry| {
                            entry.host == host
                                && (filter.is_empty()
                                    || entry.host.to_ascii_lowercase().contains(&filter)
                                    || entry.target.to_ascii_lowercase().contains(&filter))
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    let expanded = self.network_panel.expanded_hosts.contains(&host);
                    let arrow = if expanded { "-" } else { "+" };
                    let flash = self
                        .network_panel
                        .host_activity
                        .get(&host)
                        .map(|updated| {
                            1.0 - (updated.elapsed().as_secs_f32() / 0.7).clamp(0.0, 1.0)
                        })
                        .unwrap_or(0.0);
                    if left_row_button(
                        ui,
                        format!("{arrow}  {host}  ({})", matching.len()),
                        24.0,
                        blend_color(
                            Color32::from_rgb(126, 82, 24),
                            Color32::from_rgb(255, 190, 62),
                            flash,
                        ),
                        Stroke::new(1.0, Color32::from_rgb(218, 145, 42)),
                    )
                    .clicked()
                    {
                        if expanded {
                            self.network_panel.expanded_hosts.remove(&host);
                        } else {
                            self.network_panel.expanded_hosts.insert(host.clone());
                        }
                    }
                    if !expanded {
                        continue;
                    }
                    let mut tree = RequestTree::default();
                    for entry in matching {
                        tree.insert(entry);
                    }
                    if let Some(id) =
                        render_request_tree(ui, &tree, self.network_panel.selected_id, &host)
                    {
                        self.network_panel.selected_id = Some(id);
                    }
                }
            });
    }

    pub(crate) fn render_network_pinned_viewport(&mut self, ctx: &egui::Context) {
        if !self.network_panel.pinned {
            return;
        }
        self.network_panel.drain();
        ctx.request_repaint_after(Duration::from_millis(100));
        let builder = egui::ViewportBuilder::default()
            .with_title("MacroNest - Network hosts")
            .with_position(egui::pos2(0.0, 0.0))
            .with_inner_size(egui::vec2(480.0, 620.0))
            .with_min_inner_size(egui::vec2(320.0, 260.0))
            .with_clamp_size_to_monitor_size(true)
            .with_decorations(false)
            .with_resizable(true)
            .with_always_on_top();
        let mut unpin = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("network-pinned"),
            builder,
            |ctx, _| {
                if ctx.input(|input| input.viewport().close_requested()) {
                    unpin = true;
                }
                egui::TopBottomPanel::top("network-pinned-title")
                    .exact_height(38.0)
                    .show(ctx, |ui| {
                        let response = ui
                            .horizontal(|ui| {
                                ui.label(Self::material_icon_text(0xe30c, 17.0));
                                ui.label(RichText::new("MacroNest").strong());
                                ui.label(RichText::new("Network hosts").weak());
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_sized(
                                                [32.0, 28.0],
                                                egui::Button::new(Self::material_icon_text(
                                                    0xe5cd, 17.0,
                                                )),
                                            )
                                            .on_hover_text("Unpin")
                                            .clicked()
                                        {
                                            unpin = true;
                                        }
                                    },
                                );
                            })
                            .response
                            .interact(Sense::drag());
                        if response.dragged() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                    });
                egui::CentralPanel::default().show(ctx, |ui| self.render_network_list(ui));
                render_network_popup_resize_handles(ctx);
            },
        );
        if unpin {
            self.network_panel.pinned = false;
        }
    }

    fn render_network_detail(&mut self, ui: &mut egui::Ui) {
        let Some(entry) = self
            .network_panel
            .entries
            .iter()
            .find(|entry| Some(entry.id) == self.network_panel.selected_id)
            .cloned()
        else {
            ui.centered_and_justified(|ui| {
                ui.label(self.tr("Select a request", "Select a request"));
            });
            return;
        };
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (DetailTab::Overview, self.tr("Overview", "Overview")),
                (DetailTab::Contents, self.tr("Contents", "Contents")),
                (DetailTab::Ssl, self.tr("SSL", "SSL")),
                (DetailTab::Summary, self.tr("Summary", "Summary")),
                (DetailTab::Chart, self.tr("Chart", "Chart")),
                (DetailTab::Notes, self.tr("Notes", "Notes")),
            ] {
                if ui
                    .selectable_label(self.network_panel.detail_tab == tab, label)
                    .clicked()
                {
                    self.network_panel.detail_tab = tab;
                }
            }
        });
        ui.separator();
        match self.network_panel.detail_tab {
            DetailTab::Overview => render_overview(ui, &entry),
            DetailTab::Contents => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.tr("Direction", "Direction")).strong());
                    if ui
                        .selectable_label(
                            self.network_panel.message_side == MessageSide::Request,
                            self.tr("Sent (request)", "Sent (request)"),
                        )
                        .clicked()
                    {
                        self.network_panel.message_side = MessageSide::Request;
                    }
                    let has_response = !entry.response_headers.is_empty();
                    if ui
                        .add_enabled(
                            has_response,
                            egui::Button::selectable(
                                self.network_panel.message_side == MessageSide::Response,
                                self.tr("Received (response)", "Received (response)"),
                            ),
                        )
                        .clicked()
                    {
                        self.network_panel.message_side = MessageSide::Response;
                    }
                    if !has_response && !entry.secure_tunnel {
                        ui.label(RichText::new("Response was not captured").small().weak());
                    }
                });
                ui.label(
                    RichText::new(
                        "Data shows JSON/form field names, paths, types and values. Application class names are only available when the protocol actually sends them.",
                    )
                    .small()
                    .weak(),
                );
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [
                        (ContentTab::Headers, self.tr("Headers", "Headers")),
                        (ContentTab::Query, self.tr("Query", "Query")),
                        (ContentTab::Cookies, self.tr("Cookies", "Cookies")),
                        (ContentTab::Text, self.tr("Text", "Text")),
                        (ContentTab::Hex, self.tr("Hex", "Hex")),
                        (ContentTab::Form, self.tr("Form", "Form")),
                        (ContentTab::Json, self.tr("Data", "Data")),
                        (ContentTab::Raw, self.tr("Raw", "Raw")),
                    ] {
                        ui.add_enabled_ui(
                            !entry.secure_tunnel || tab == ContentTab::Headers,
                            |ui| {
                                if ui
                                    .selectable_label(self.network_panel.content_tab == tab, label)
                                    .clicked()
                                {
                                    self.network_panel.content_tab = tab;
                                }
                            },
                        );
                    }
                });
                ui.separator();
                render_contents(
                    ui,
                    &entry,
                    self.network_panel.content_tab,
                    self.network_panel.message_side,
                );
            }
            DetailTab::Ssl => {
                if entry.secure_tunnel {
                    ui.label(RichText::new("Encrypted TLS tunnel").strong());
                    ui.add(
                        egui::Label::new(
                            "Only the destination is visible. Stop capture, install the MacroNest CA, enable Decrypt HTTPS, then start again to inspect sent and received data. Apps with certificate pinning may still reject decryption.",
                        )
                        .wrap(),
                    );
                } else {
                    ui.label("This request used plain HTTP; SSL does not apply.");
                }
            }
            DetailTab::Summary => render_overview(ui, &entry),
            DetailTab::Chart => {
                ui.label("Timing begins when the request reaches MacroNest.");
                ui.add(egui::ProgressBar::new(1.0).text("Request captured"));
            }
            DetailTab::Notes => {
                if let Some(selected) = self
                    .network_panel
                    .entries
                    .iter_mut()
                    .find(|item| item.id == entry.id)
                {
                    let notes_resp = ui.add(
                        egui::TextEdit::multiline(&mut selected.notes)
                            .hint_text("Notes for this request...")
                            .desired_width(f32::INFINITY)
                            .desired_rows(12),
                    );
                    Self::apply_vietnamese_input_if_changed(
                        &notes_resp,
                        self.state.vietnamese_input_enabled,
                        self.state.vietnamese_input_mode,
                        &mut selected.notes,
                    );
                }
            }
        }
    }
}

fn request_path(entry: &NetworkEntry) -> &str {
    if entry.secure_tunnel {
        return &entry.target;
    }
    entry
        .target
        .strip_prefix("http://")
        .and_then(|value| value.split_once('/').map(|(_, path)| path))
        .or_else(|| {
            entry
                .target
                .strip_prefix("https://")
                .and_then(|value| value.split_once('/').map(|(_, path)| path))
        })
        .unwrap_or(&entry.target)
}

fn render_request_tree(
    ui: &mut egui::Ui,
    tree: &RequestTree,
    selected_id: Option<u64>,
    id_path: &str,
) -> Option<u64> {
    let mut clicked = None;
    ui.indent(id_path, |ui| {
        for (folder, child) in &tree.folders {
            egui::CollapsingHeader::new(format!("[folder] {folder}"))
                .id_salt((id_path, folder))
                .show(ui, |ui| {
                    clicked = clicked.or_else(|| {
                        render_request_tree(ui, child, selected_id, &format!("{id_path}/{folder}"))
                    });
                });
        }
        for entry in &tree.requests {
            let selected = selected_id == Some(entry.id);
            let icon = if entry.secure_tunnel { "TLS" } else { "HTTP" };
            let leaf = request_path(entry)
                .rsplit('/')
                .next()
                .filter(|part| !part.is_empty())
                .unwrap_or(&entry.target);
            let response = left_row_button(
                ui,
                format!("[{icon}] {}  {leaf}", entry.method),
                22.0,
                if selected {
                    Color32::from_rgb(35, 116, 148)
                } else {
                    Color32::from_rgb(58, 58, 58)
                },
                if selected {
                    Stroke::new(1.0, Color32::from_rgb(92, 190, 225))
                } else {
                    Stroke::NONE
                },
            );
            if response.clicked() {
                clicked = Some(entry.id);
            }
        }
    });
    clicked
}

fn left_row_button(
    ui: &mut egui::Ui,
    text: String,
    height: f32,
    fill: Color32,
    stroke: Stroke,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
    let fill = if response.hovered() {
        fill.gamma_multiply(1.2)
    } else {
        fill
    };
    ui.painter().rect_filled(rect, 2.0, fill);
    if stroke != Stroke::NONE {
        ui.painter()
            .line_segment([rect.left_top(), rect.right_top()], stroke);
        ui.painter()
            .line_segment([rect.left_bottom(), rect.right_bottom()], stroke);
    }
    ui.painter().with_clip_rect(rect).text(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::TextStyle::Button.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    response
}

fn blend_color(from: Color32, to: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |from: u8, to: u8| {
        (f32::from(from) + (f32::from(to) - f32::from(from)) * amount).round() as u8
    };
    Color32::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

fn render_overview(ui: &mut egui::Ui, entry: &NetworkEntry) {
    egui::Grid::new("network-request-summary")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Method");
            ui.label(&entry.method);
            ui.end_row();
            ui.label("Host");
            ui.label(&entry.host);
            ui.end_row();
            ui.label("Target");
            ui.add(egui::Label::new(&entry.target).sense(Sense::click()));
            ui.end_row();
            ui.label("Transport");
            ui.label(if entry.secure_tunnel {
                "TLS tunnel (encrypted)"
            } else {
                "HTTP"
            });
            ui.end_row();
            ui.label("Captured");
            ui.label(format!("{:?}", entry.time));
            ui.end_row();
            ui.label("Body size");
            ui.label(format!("{} byte(s)", entry.body.len()));
            ui.end_row();
            ui.label("Response");
            ui.label(
                entry
                    .response_headers
                    .lines()
                    .next()
                    .filter(|line| !line.is_empty())
                    .unwrap_or("Not captured"),
            );
            ui.end_row();
            ui.label("Response body size");
            ui.label(format!("{} byte(s)", entry.response_body.len()));
            ui.end_row();
        });
}

fn render_contents(
    ui: &mut egui::Ui,
    entry: &NetworkEntry,
    tab: ContentTab,
    side: MessageSide,
) {
    if entry.secure_tunnel && tab != ContentTab::Headers {
        ui.label(
            "No payload is available for this encrypted tunnel. Enable Decrypt HTTPS before capture.",
        );
        return;
    }
    let (headers, body) = match side {
        MessageSide::Request => (&entry.headers, entry.body.as_slice()),
        MessageSide::Response => (&entry.response_headers, entry.response_body.as_slice()),
    };
    let body = decoded_body(headers, body);
    match tab {
        ContentTab::Headers => readonly_text(ui, headers.clone()),
        ContentTab::Query => {
            if side == MessageSide::Request {
                render_pairs(ui, query_pairs(&entry.target));
            } else {
                ui.label("Query parameters belong to the request.");
            }
        }
        ContentTab::Cookies => {
            let cookie_name = if side == MessageSide::Request {
                "cookie"
            } else {
                "set-cookie"
            };
            let cookies = headers
                .lines()
                .filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case(cookie_name)
                        .then_some(value.trim())
                })
                .flat_map(|value| value.split(';'))
                .filter_map(split_pair)
                .collect();
            render_pairs(ui, cookies);
        }
        ContentTab::Text => readonly_text(ui, String::from_utf8_lossy(&body).into_owned()),
        ContentTab::Hex => readonly_text(ui, hex_dump(&body)),
        ContentTab::Form => render_form(ui, headers, &body),
        ContentTab::Json => {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) {
                render_json_data(ui, &value);
            } else {
                let pairs = form_pairs(headers, &body);
                if pairs.is_empty() {
                    ui.label("No JSON or form fields found in this message.");
                } else {
                    render_pairs(ui, pairs);
                }
            }
        }
        ContentTab::Raw => {
            let mut raw = headers.clone();
            raw.push_str(&String::from_utf8_lossy(&body));
            readonly_text(ui, raw);
        }
    }
}

fn render_form(ui: &mut egui::Ui, headers: &str, body: &[u8]) {
    let content_type = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-type")
                .then_some(value.trim().to_ascii_lowercase())
        })
        .unwrap_or_default();
    if content_type.starts_with("application/x-www-form-urlencoded") {
        render_pairs(ui, form_pairs(headers, body));
    } else if content_type.starts_with("multipart/form-data") {
        ui.label("Multipart form body captured; field parsing is not implemented yet.");
    } else {
        ui.label("This request is not form data.");
    }
}

fn form_pairs(headers: &str, body: &[u8]) -> Vec<(String, String)> {
    let is_form = headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("content-type")
                && value
                    .trim()
                    .to_ascii_lowercase()
                    .starts_with("application/x-www-form-urlencoded")
        })
    });
    if !is_form {
        return Vec::new();
    }
    String::from_utf8_lossy(body)
        .split('&')
        .filter_map(split_pair)
        .collect()
}

fn decoded_body(headers: &str, body: &[u8]) -> Vec<u8> {
    let mut decoded = if header_contains(headers, "transfer-encoding", "chunked") {
        decode_chunked(body).unwrap_or_else(|| body.to_vec())
    } else {
        body.to_vec()
    };
    let content_encoding = header_value(headers, "content-encoding")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut output = Vec::new();
    let result = if content_encoding.contains("gzip") {
        GzDecoder::new(decoded.as_slice()).read_to_end(&mut output)
    } else if content_encoding.contains("deflate") {
        DeflateDecoder::new(decoded.as_slice()).read_to_end(&mut output)
    } else {
        return decoded;
    };
    if result.is_ok() {
        output
    } else {
        std::mem::take(&mut decoded)
    }
}

fn header_value<'a>(headers: &'a str, wanted: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(wanted).then_some(value.trim())
    })
}

fn header_contains(headers: &str, name: &str, value: &str) -> bool {
    header_value(headers, name).is_some_and(|current| {
        current
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case(value))
    })
}

fn decode_chunked(body: &[u8]) -> Option<Vec<u8>> {
    let mut offset = 0;
    let mut decoded = Vec::new();
    loop {
        let line_end = body[offset..]
            .windows(2)
            .position(|window| window == b"\r\n")?
            + offset;
        let size_text = std::str::from_utf8(&body[offset..line_end])
            .ok()?
            .split(';')
            .next()?;
        let size = usize::from_str_radix(size_text.trim(), 16).ok()?;
        offset = line_end + 2;
        if size == 0 {
            return Some(decoded);
        }
        let data_end = offset.checked_add(size)?;
        if data_end + 2 > body.len() || &body[data_end..data_end + 2] != b"\r\n" {
            return None;
        }
        decoded.extend_from_slice(&body[offset..data_end]);
        offset = data_end + 2;
    }
}

fn render_json_data(ui: &mut egui::Ui, value: &serde_json::Value) {
    let mut rows = Vec::new();
    collect_json_rows(value, "", &mut rows);
    if rows.len() == MAX_STRUCTURED_ROWS {
        ui.label(
            RichText::new("Showing the first 5,000 structured fields.")
                .small()
                .weak(),
        );
    }
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("network-json-data")
            .num_columns(3)
            .striped(true)
            .spacing([18.0, 4.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Name / path").strong());
                ui.label(RichText::new("Type").strong());
                ui.label(RichText::new("Value").strong());
                ui.end_row();
                for (path, kind, value) in rows {
                    ui.label(path);
                    ui.label(kind);
                    ui.label(value);
                    ui.end_row();
                }
            });
    });
}

fn collect_json_rows(
    value: &serde_json::Value,
    path: &str,
    rows: &mut Vec<(String, &'static str, String)>,
) {
    // ponytail: cap pathological payloads here; virtualize the table before raising this ceiling.
    if rows.len() >= MAX_STRUCTURED_ROWS {
        return;
    }
    match value {
        serde_json::Value::Object(fields) => {
            if !path.is_empty() {
                rows.push((
                    path.to_owned(),
                    "Object",
                    format!("{} field(s)", fields.len()),
                ));
            }
            for (name, value) in fields {
                let child = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}.{name}")
                };
                collect_json_rows(value, &child, rows);
            }
        }
        serde_json::Value::Array(values) => {
            rows.push((
                path.to_owned(),
                "Array",
                format!("{} item(s)", values.len()),
            ));
            for (index, value) in values.iter().enumerate() {
                collect_json_rows(value, &format!("{path}[{index}]"), rows);
            }
        }
        serde_json::Value::String(value) => {
            rows.push((path.to_owned(), "String", value.clone()));
        }
        serde_json::Value::Number(value) => {
            rows.push((path.to_owned(), "Number", value.to_string()));
        }
        serde_json::Value::Bool(value) => {
            rows.push((path.to_owned(), "Boolean", value.to_string()));
        }
        serde_json::Value::Null => rows.push((path.to_owned(), "Null", String::new())),
    }
}

fn readonly_text(ui: &mut egui::Ui, mut text: String) {
    let available_width = ui.available_width().max(1.0);
    ui.add(
        egui::TextEdit::multiline(&mut text)
            .code_editor()
            .desired_width(available_width)
            .desired_rows(18)
            .interactive(false),
    );
}

fn render_pairs(ui: &mut egui::Ui, pairs: Vec<(String, String)>) {
    if pairs.is_empty() {
        ui.label("No values");
        return;
    }
    egui::Grid::new("network-content-pairs")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new("Name").strong());
            ui.label(RichText::new("Value").strong());
            ui.end_row();
            for (name, value) in pairs {
                ui.label(name);
                ui.label(value);
                ui.end_row();
            }
        });
}

fn query_pairs(target: &str) -> Vec<(String, String)> {
    target
        .split_once('?')
        .map(|(_, query)| query.split('&').filter_map(split_pair).collect())
        .unwrap_or_default()
}

fn split_pair(value: &str) -> Option<(String, String)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (name, value) = value.split_once('=').unwrap_or((value, ""));
    Some((percent_decode(name), percent_decode(value)))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                output.push(byte);
                index += 3;
                continue;
            }
        }
        output.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn hex_dump(bytes: &[u8]) -> String {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(row, chunk)| {
            format!(
                "{:08X}  {}",
                row * 16,
                chunk
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_network_popup_resize_handles(ctx: &egui::Context) {
    let rect = ctx.content_rect();
    let edge = 6.0;
    let corner = 20.0;
    let handles = [
        (
            "n",
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + edge)),
            egui::viewport::ResizeDirection::North,
            egui::CursorIcon::ResizeVertical,
        ),
        (
            "s",
            egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - edge), rect.max),
            egui::viewport::ResizeDirection::South,
            egui::CursorIcon::ResizeVertical,
        ),
        (
            "w",
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + edge, rect.max.y)),
            egui::viewport::ResizeDirection::West,
            egui::CursorIcon::ResizeHorizontal,
        ),
        (
            "e",
            egui::Rect::from_min_max(egui::pos2(rect.max.x - edge, rect.min.y), rect.max),
            egui::viewport::ResizeDirection::East,
            egui::CursorIcon::ResizeHorizontal,
        ),
        (
            "nw",
            egui::Rect::from_min_size(rect.min, egui::vec2(corner, corner)),
            egui::viewport::ResizeDirection::NorthWest,
            egui::CursorIcon::ResizeNwSe,
        ),
        (
            "ne",
            egui::Rect::from_min_max(
                egui::pos2(rect.max.x - corner, rect.min.y),
                egui::pos2(rect.max.x, rect.min.y + corner),
            ),
            egui::viewport::ResizeDirection::NorthEast,
            egui::CursorIcon::ResizeNeSw,
        ),
        (
            "sw",
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x, rect.max.y - corner),
                egui::pos2(rect.min.x + corner, rect.max.y),
            ),
            egui::viewport::ResizeDirection::SouthWest,
            egui::CursorIcon::ResizeNeSw,
        ),
        (
            "se",
            egui::Rect::from_min_max(
                egui::pos2(rect.max.x - corner, rect.max.y - corner),
                rect.max,
            ),
            egui::viewport::ResizeDirection::SouthEast,
            egui::CursorIcon::ResizeNwSe,
        ),
    ];

    for (id, handle_rect, direction, cursor) in handles {
        egui::Area::new(egui::Id::new(("network-popup-resize", id)))
            .order(egui::Order::Foreground)
            .fixed_pos(handle_rect.min)
            .interactable(true)
            .show(ctx, |ui| {
                let (_, response) =
                    ui.allocate_exact_size(handle_rect.size(), Sense::click_and_drag());
                if response.hovered() {
                    ui.ctx().set_cursor_icon(cursor);
                }
                if response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
            });
    }
}
