use std::{
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use serde::{Deserialize, Serialize};

use super::CrosshairApp;

const DEFAULT_PROXY_ADDRESS: &str = "127.0.0.1:8888";
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_CAPTURE_BODY_BYTES: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

#[derive(Clone)]
struct NetworkEntry {
    id: u64,
    time: SystemTime,
    method: String,
    host: String,
    target: String,
    headers: String,
    body: Vec<u8>,
    notes: String,
    secure_tunnel: bool,
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
    Raw,
}

pub(crate) struct NetworkPanelState {
    bind_address: String,
    filter: String,
    entries: Vec<NetworkEntry>,
    selected_id: Option<u64>,
    next_id: u64,
    detail_tab: DetailTab,
    content_tab: ContentTab,
    status: String,
    proxy: Option<NetworkProxy>,
    recovery_file: PathBuf,
}

impl NetworkPanelState {
    pub(crate) fn new(recovery_file: PathBuf) -> Self {
        let recovery_available = recovery_file.exists();
        Self {
            bind_address: DEFAULT_PROXY_ADDRESS.to_owned(),
            filter: String::new(),
            entries: Vec::new(),
            selected_id: None,
            next_id: 1,
            detail_tab: DetailTab::Overview,
            content_tab: ContentTab::Headers,
            status: if recovery_available { "Previous proxy settings can be restored" } else { "Stopped" }.to_owned(),
            proxy: None,
            recovery_file,
        }
    }

    fn drain(&mut self) {
        let Some(proxy) = &self.proxy else { return };
        while let Ok(mut entry) = proxy.events.try_recv() {
            entry.id = self.next_id;
            self.next_id += 1;
            self.entries.push(entry);
        }
        if self.entries.len() > MAX_ENTRIES {
            self.entries.drain(..self.entries.len() - MAX_ENTRIES);
        }
    }

    fn start(&mut self) {
        match NetworkProxy::start(&self.bind_address, self.recovery_file.clone()) {
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
        self.proxy.take();
        self.status = match SystemProxyGuard::restore_file(&self.recovery_file) {
            Ok(true) => "Previous Windows proxy restored".to_owned(),
            Ok(false) => "No saved proxy settings".to_owned(),
            Err(error) => format!("Unable to restore proxy: {error}"),
        };
    }
}

struct NetworkProxy {
    stop: Arc<AtomicBool>,
    events: Receiver<NetworkEntry>,
    thread: Option<JoinHandle<()>>,
    _system_proxy: SystemProxyGuard,
}

impl NetworkProxy {
    fn start(address: &str, recovery_file: PathBuf) -> Result<Self, String> {
        let parsed = address.parse::<SocketAddr>().map_err(|_| "proxy address is invalid".to_owned())?;
        if !parsed.ip().is_loopback() {
            return Err("proxy must use a loopback address".to_owned());
        }
        let listener = TcpListener::bind(parsed).map_err(|error| error.to_string())?;
        listener.set_nonblocking(true).map_err(|error| error.to_string())?;
        let system_proxy = SystemProxyGuard::enable(address, recovery_file)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (tx, events) = unbounded();
        let thread = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let tx = tx.clone();
                        thread::spawn(move || {
                            let _ = proxy_connection(stream, tx);
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self { stop, events, thread: Some(thread), _system_proxy: system_proxy })
    }
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
        if !path.exists() { return Ok(false); }
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let snapshot: ProxySnapshot = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        restore_system_proxy(&snapshot)?;
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
        Ok(true)
    }
}

impl Drop for SystemProxyGuard {
    fn drop(&mut self) {
        let _ = Self::restore_file(&self.recovery_file);
    }
}

fn powershell(script: &str) -> Result<String, String> {
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script])
        .output().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn query_system_proxy() -> Result<ProxySnapshot, String> {
    let output = powershell(
        "$p=Get-ItemProperty -LiteralPath 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'; [pscustomobject]@{proxy_enable=if($null -eq $p.ProxyEnable){$null}else{[uint32]$p.ProxyEnable};proxy_server=if($null -eq $p.ProxyServer){$null}else{[string]$p.ProxyServer}}|ConvertTo-Json -Compress",
    )?;
    serde_json::from_str(&output).map_err(|error| error.to_string())
}

fn set_system_proxy(server: Option<&str>, enabled: bool) -> Result<(), String> {
    let server_command = match server {
        Some(server) => format!("Set-ItemProperty -LiteralPath $k -Name ProxyServer -Type String -Value '{server}';"),
        None => "Remove-ItemProperty -LiteralPath $k -Name ProxyServer -ErrorAction SilentlyContinue;".to_owned(),
    };
    powershell(&format!(
        "$k='HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'; Set-ItemProperty -LiteralPath $k -Name ProxyEnable -Type DWord -Value {}; {} Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class MNWinInet{{[DllImport(\"wininet.dll\")]public static extern bool InternetSetOption(IntPtr h,int o,IntPtr b,int l);}}';[MNWinInet]::InternetSetOption([IntPtr]::Zero,39,[IntPtr]::Zero,0)|Out-Null;[MNWinInet]::InternetSetOption([IntPtr]::Zero,37,[IntPtr]::Zero,0)|Out-Null",
        u8::from(enabled), server_command
    )).map(|_| ())
}

fn restore_system_proxy(snapshot: &ProxySnapshot) -> Result<(), String> {
    set_system_proxy(snapshot.proxy_server.as_deref(), snapshot.proxy_enable.unwrap_or(0) != 0)
}

impl Drop for NetworkProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn proxy_connection(mut client: TcpStream, events: Sender<NetworkEntry>) -> std::io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(10)))?;
    let header = read_header(&mut client)?;
    let header_end = header.windows(4).position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4).unwrap_or(header.len());
    let text = String::from_utf8_lossy(&header[..header_end]).into_owned();
    let mut lines = text.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_owned();
    let target = request_parts.next().unwrap_or_default().to_owned();
    let version = request_parts.next().unwrap_or("HTTP/1.1").to_owned();
    let host_header = lines
        .find_map(|line| line.strip_prefix("Host:").or_else(|| line.strip_prefix("host:")))
        .map(str::trim)
        .unwrap_or_default();

    if method.eq_ignore_ascii_case("CONNECT") {
        let host = target.clone();
        events.send(NetworkEntry {
            id: 0,
            time: SystemTime::now(),
            method,
            host: host.clone(),
            target,
            headers: text,
            body: Vec::new(),
            notes: String::new(),
            secure_tunnel: true,
        }).ok();
        let mut server = TcpStream::connect(&host)?;
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        tunnel(client, server)
    } else {
        let (host, origin_target) = http_destination(&target, host_header);
        let body = header[header_end..].to_vec();
        events.send(NetworkEntry {
            id: 0,
            time: SystemTime::now(),
            method: method.clone(),
            host: host.clone(),
            target: target.clone(),
            headers: text.clone(),
            body,
            notes: String::new(),
            secure_tunnel: false,
        }).ok();
        let address = if host.contains(':') { host.clone() } else { format!("{host}:80") };
        let mut server = TcpStream::connect(address)?;
        let rewritten = rewrite_request(&header, &method, &origin_target, &version);
        server.write_all(&rewritten)?;
        tunnel(client, server)
    }
}

fn read_header(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::with_capacity(2048);
    let mut buffer = [0_u8; 2048];
    while data.len() < MAX_HEADER_BYTES {
        let count = stream.read(&mut buffer)?;
        if count == 0 { break; }
        data.extend_from_slice(&buffer[..count]);
        if let Some(position) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = position + 4;
            let header_text = String::from_utf8_lossy(&data[..header_end]);
            let content_length = header_text.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok()).flatten()
            }).unwrap_or(0).min(MAX_CAPTURE_BODY_BYTES);
            let target_length = header_end + content_length;
            while data.len() < target_length {
                let count = stream.read(&mut buffer)?;
                if count == 0 { break; }
                let remaining = target_length - data.len();
                data.extend_from_slice(&buffer[..count.min(remaining)]);
            }
            return Ok(data);
        }
    }
    if data.len() >= MAX_HEADER_BYTES {
        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "request headers exceed 64 KiB"))
    } else {
        Ok(data)
    }
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
        if running { ui.ctx().request_repaint_after(Duration::from_millis(100)); }

        ui.horizontal(|ui| {
            ui.label(RichText::new("Network").strong().size(17.0));
            ui.separator();
            ui.label(&self.network_panel.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    self.network_panel.entries.clear();
                    self.network_panel.selected_id = None;
                }
                if ui.button("Restore proxy").clicked() {
                    self.network_panel.restore_proxy();
                }
                if running {
                    if ui.button("Stop").clicked() { self.network_panel.stop(); }
                } else if ui.button("Start").clicked() {
                    self.network_panel.start();
                }
            });
        });
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label("Proxy");
            ui.add_enabled(!running, egui::TextEdit::singleline(&mut self.network_panel.bind_address).desired_width(150.0));
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(self.network_panel.bind_address.clone());
            }
            ui.separator();
            ui.label("Filter");
            ui.add(egui::TextEdit::singleline(&mut self.network_panel.filter).desired_width(f32::INFINITY));
        });
        ui.label(RichText::new("Set the target app's HTTP proxy to this address. HTTPS is recorded as an encrypted CONNECT tunnel.").small().weak());
        ui.separator();

        let available = ui.available_size();
        ui.horizontal(|ui| {
            ui.set_height(available.y);
            let list_width = (available.x * 0.48).max(280.0);
            ui.allocate_ui_with_layout(
                egui::vec2(list_width, available.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.render_network_list(ui),
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2((available.x - list_width - 8.0).max(180.0), available.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.render_network_detail(ui),
            );
        });
    }

    fn render_network_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Host").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("{} request(s)", self.network_panel.entries.len()));
            });
        });
        ui.separator();
        let filter = self.network_panel.filter.to_ascii_lowercase();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for entry in self.network_panel.entries.iter().rev() {
                if !filter.is_empty() && !entry.host.to_ascii_lowercase().contains(&filter)
                    && !entry.target.to_ascii_lowercase().contains(&filter) { continue; }
                let selected = self.network_panel.selected_id == Some(entry.id);
                let icon = if entry.secure_tunnel { "[TLS]" } else { "[HTTP]" };
                let label = format!("{icon}  {}  {}", entry.method, entry.host);
                let response = ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::Button::new(RichText::new(label).color(if selected { Color32::WHITE } else { ui.visuals().text_color() }))
                        .selected(selected)
                        .stroke(if selected { Stroke::new(1.0, Color32::from_rgb(92, 190, 225)) } else { Stroke::NONE }),
                );
                if response.clicked() { self.network_panel.selected_id = Some(entry.id); }
            }
        });
    }

    fn render_network_detail(&mut self, ui: &mut egui::Ui) {
        let Some(entry) = self.network_panel.entries.iter().find(|entry| Some(entry.id) == self.network_panel.selected_id).cloned() else {
            ui.centered_and_justified(|ui| { ui.label("Select a request"); });
            return;
        };
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (DetailTab::Overview, "Overview"),
                (DetailTab::Contents, "Contents"),
                (DetailTab::Ssl, "SSL"),
                (DetailTab::Summary, "Summary"),
                (DetailTab::Chart, "Chart"),
                (DetailTab::Notes, "Notes"),
            ] {
                if ui.selectable_label(self.network_panel.detail_tab == tab, label).clicked() {
                    self.network_panel.detail_tab = tab;
                }
            }
        });
        ui.separator();
        match self.network_panel.detail_tab {
            DetailTab::Overview => render_overview(ui, &entry),
            DetailTab::Contents => {
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [
                        (ContentTab::Headers, "Headers"),
                        (ContentTab::Query, "Query String"),
                        (ContentTab::Cookies, "Cookies"),
                        (ContentTab::Text, "Text"),
                        (ContentTab::Hex, "Hex"),
                        (ContentTab::Form, "Form"),
                        (ContentTab::Raw, "Raw"),
                    ] {
                        if ui.selectable_label(self.network_panel.content_tab == tab, label).clicked() {
                            self.network_panel.content_tab = tab;
                        }
                    }
                });
                ui.separator();
                render_contents(ui, &entry, self.network_panel.content_tab);
            }
            DetailTab::Ssl => {
                if entry.secure_tunnel {
                    ui.label(RichText::new("Encrypted TLS tunnel").strong());
                    ui.label("The proxy recorded the destination but did not decrypt this connection.");
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
                if let Some(selected) = self.network_panel.entries.iter_mut().find(|item| item.id == entry.id) {
                    ui.add(egui::TextEdit::multiline(&mut selected.notes).hint_text("Notes for this request...").desired_width(f32::INFINITY).desired_rows(12));
                }
            }
        }
    }
}

fn render_overview(ui: &mut egui::Ui, entry: &NetworkEntry) {
    egui::Grid::new("network-request-summary").num_columns(2).striped(true).show(ui, |ui| {
        ui.label("Method"); ui.label(&entry.method); ui.end_row();
        ui.label("Host"); ui.label(&entry.host); ui.end_row();
        ui.label("Target"); ui.add(egui::Label::new(&entry.target).sense(Sense::click())); ui.end_row();
        ui.label("Transport"); ui.label(if entry.secure_tunnel { "TLS tunnel (encrypted)" } else { "HTTP" }); ui.end_row();
        ui.label("Captured"); ui.label(format!("{:?}", entry.time)); ui.end_row();
        ui.label("Body size"); ui.label(format!("{} byte(s)", entry.body.len())); ui.end_row();
    });
}

fn render_contents(ui: &mut egui::Ui, entry: &NetworkEntry, tab: ContentTab) {
    if entry.secure_tunnel && tab != ContentTab::Headers {
        ui.label("Contents are encrypted inside this TLS tunnel.");
        return;
    }
    match tab {
        ContentTab::Headers => readonly_text(ui, entry.headers.clone()),
        ContentTab::Query => render_pairs(ui, query_pairs(&entry.target)),
        ContentTab::Cookies => {
            let cookies = entry.headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("cookie").then_some(value.trim())
            }).unwrap_or_default();
            render_pairs(ui, cookies.split(';').filter_map(split_pair).collect());
        }
        ContentTab::Text => readonly_text(ui, String::from_utf8_lossy(&entry.body).into_owned()),
        ContentTab::Hex => readonly_text(ui, hex_dump(&entry.body)),
        ContentTab::Form => render_pairs(ui, String::from_utf8_lossy(&entry.body).split('&').filter_map(split_pair).collect()),
        ContentTab::Raw => {
            let mut raw = entry.headers.clone();
            raw.push_str(&String::from_utf8_lossy(&entry.body));
            readonly_text(ui, raw);
        }
    }
}

fn readonly_text(ui: &mut egui::Ui, mut text: String) {
    ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_width(f32::INFINITY).desired_rows(18).interactive(false));
}

fn render_pairs(ui: &mut egui::Ui, pairs: Vec<(String, String)>) {
    if pairs.is_empty() { ui.label("No values"); return; }
    egui::Grid::new("network-content-pairs").num_columns(2).striped(true).show(ui, |ui| {
        ui.label(RichText::new("Name").strong()); ui.label(RichText::new("Value").strong()); ui.end_row();
        for (name, value) in pairs {
            ui.label(name); ui.label(value); ui.end_row();
        }
    });
}

fn query_pairs(target: &str) -> Vec<(String, String)> {
    target.split_once('?').map(|(_, query)| query.split('&').filter_map(split_pair).collect()).unwrap_or_default()
}

fn split_pair(value: &str) -> Option<(String, String)> {
    let value = value.trim();
    if value.is_empty() { return None; }
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
                output.push(byte); index += 3; continue;
            }
        }
        output.push(if bytes[index] == b'+' { b' ' } else { bytes[index] });
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn hex_dump(bytes: &[u8]) -> String {
    bytes.chunks(16).enumerate().map(|(row, chunk)| {
        format!("{:08X}  {}", row * 16, chunk.iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(" "))
    }).collect::<Vec<_>>().join("\n")
}
