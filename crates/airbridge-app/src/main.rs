#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

mod connection;
mod design;
mod hardware;
#[cfg(test)]
mod tests;
use design::{Phase, View};

#[derive(Clone, Debug)]
enum Event {
    Connecting,
    Hardware(hardware::Report),
    Line(u64, String),
    Stopped(u64, String),
    FilesSelected(Vec<PathBuf>),
    SelectionFailed(u64, String),
    FolderSelected(PathBuf),
}

struct AirBridgeApp {
    hardware: Option<hardware::Report>,
    hardware_checking: bool,
    shutdown: Arc<AtomicBool>,
    view: View,
    phase: Phase,
    bytes: u64,
    total: Option<u64>,
    incoming_count: usize,
    peer_name: String,
    destination: PathBuf,
    show_folder: bool,
    reduce_motion: bool,
    preview: bool,
    status: String,
    detail: String,
    listening: bool,
    sending: bool,
    received: usize,
    pending_sender: Option<String>,
    files: Vec<PathBuf>,
    send_targets: Vec<(String, String)>,
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
    generation: Arc<AtomicU64>,
    operation: Arc<Mutex<()>>,
    active: Arc<AtomicBool>,
}

impl AirBridgeApp {
    fn new(context: &eframe::CreationContext<'_>) -> Self {
        design::configure(&context.egui_ctx);
        let (destination, reduce_motion) = design::load_preferences();
        let mut app = Self::initial(destination, reduce_motion);
        app.preview = std::env::args().any(|arg| arg == "--preview");
        if !app.preview {
            app.watch_connection();
        }
        app
    }

    fn initial(destination: PathBuf, reduce_motion: bool) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            hardware: None,
            hardware_checking: false,
            shutdown: Arc::new(AtomicBool::new(false)),
            view: View::Home,
            phase: Phase::Idle,
            bytes: 0,
            total: None,
            incoming_count: 0,
            peer_name: String::new(),
            destination,
            show_folder: false,
            reduce_motion,
            preview: false,
            status: "Ready to start".into(),
            detail: "Start the receiver, then open AirDrop on your nearby iPhone.".into(),
            listening: false,
            sending: false,
            received: 0,
            pending_sender: None,
            files: Vec::new(),
            send_targets: Vec::new(),
            events_tx,
            events_rx,
            generation: Arc::new(AtomicU64::new(0)),
            operation: Arc::new(Mutex::new(())),
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    fn start(&mut self) {
        if !self.hardware_ready() {
            return;
        }
        if self.listening || self.sending {
            return;
        }
        self.view = View::Receive;
        self.phase = if self.preview {
            Phase::Ready
        } else {
            Phase::Preparing
        };
        self.bytes = 0;
        self.total = None;
        self.status = "Starting the AirDrop radio…".into();
        self.detail = "Preparing the dedicated Wi‑Fi adapter.".into();
        self.listening = true;
        let Some(destination) = windows_to_wsl(&self.destination.display().to_string()) else {
            self.listening = false;
            self.phase = Phase::Error;
            self.detail = "Choose a save folder on a local Windows drive.".into();
            return;
        };
        self.launch(vec!["receive".into(), destination, "AirBridge".into()]);
    }

    fn stop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.listening = false;
        self.sending = false;
        self.pending_sender = None;
        self.send_targets.clear();
        self.phase = Phase::Idle;
        self.view = View::Home;
        self.status = "Stopped".into();
        self.detail = "Ready to send or receive another transfer.".into();
    }

    fn send_files(&mut self) {
        if !self.hardware_ready() {
            return;
        }
        if self.files.is_empty() || self.sending {
            return;
        }
        if self.listening {
            self.stop();
        }
        let files: Vec<String> = self
            .files
            .iter()
            .filter_map(|path| windows_to_wsl(&path.display().to_string()))
            .collect();
        if files.len() != self.files.len() {
            self.phase = Phase::Error;
            self.status = "Those files could not be opened".into();
            self.detail = "Choose files stored on a local Windows drive.".into();
            return;
        }
        self.view = View::Send;
        self.phase = Phase::Preparing;
        self.sending = true;
        self.send_targets.clear();
        self.status = "Preparing to send…".into();
        self.detail = "Set AirDrop to Everyone for 10 Minutes and keep the iPhone unlocked.".into();
        let mut args = vec!["send".into(), "Windows PC".into()];
        args.extend(files);
        self.launch(args);
    }

    fn select_receiver(&mut self, receiver: &str, name: &str) {
        self.peer_name = name.into();
        self.phase = Phase::Waiting;
        if self.preview {
            self.phase = Phase::Transfer;
            return;
        }
        // IDs come from the current discovery session, never from device names.
        if receiver.is_empty()
            || receiver.len() > 4
            || !receiver.bytes().all(|b| b.is_ascii_digit())
        {
            return;
        }
        let command = format!(
            "umask 077; printf '%s' '{receiver}' > /run/airbridge/send-target.tmp && mv /run/airbridge/send-target.tmp /run/airbridge/send-target"
        );
        let generation = self.generation.clone();
        let id = generation.load(Ordering::SeqCst);
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            if generation.load(Ordering::SeqCst) != id {
                return;
            }
            match bounded_output(
                wsl_command().args(["sh", "-c", &command]),
                Duration::from_secs(5),
            ) {
                Ok(output) if output.status.success() => {}
                _ => {
                    let _ = tx.send(Event::SelectionFailed(
                        id,
                        "Could not select the receiver. Please try again.".into(),
                    ));
                }
            }
        });
        self.send_targets.clear();
        self.status = format!("Connecting to {name}…");
        self.detail = "Accept the AirDrop request on your iPhone when it appears.".into();
    }

    fn launch(&self, args: Vec<String>) {
        if self.preview {
            return;
        }
        let generation = self.generation.clone();
        let id = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let operation = self.operation.clone();
        let active = self.active.clone();
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            // A previous session finishes cleanup before its replacement starts.
            // This lock is never acquired by the UI thread.
            let _guard = operation.lock().unwrap_or_else(|error| error.into_inner());
            if generation.load(Ordering::SeqCst) != id {
                return;
            }
            active.store(true, Ordering::SeqCst);
            if let Err(error) = run_session(args, id, &generation, &tx) {
                let _ = tx.send(Event::Stopped(id, error));
            }
            active.store(false, Ordering::SeqCst);
        });
    }

    fn handle_events(&mut self) {
        while let Ok(event) = self.events_rx.try_recv() {
            let line = match event {
                Event::Connecting => {
                    self.hardware_checking = true;
                    continue;
                }
                Event::Hardware(report) => {
                    self.hardware = Some(report);
                    self.hardware_checking = false;
                    continue;
                }
                Event::FilesSelected(files) => {
                    if self.view == View::Send && !self.sending && !files.is_empty() {
                        self.files = files;
                        self.send_files();
                    }
                    continue;
                }
                Event::FolderSelected(path) => {
                    if !self.listening && !self.sending {
                        if windows_to_wsl(&path.display().to_string()).is_some() {
                            self.destination = path;
                            if let Err(error) = design::save_preferences(&self.destination) {
                                self.phase = Phase::Error;
                                self.detail = format!("Could not remember the folder: {error}");
                            }
                        } else {
                            self.phase = Phase::Error;
                            self.detail = "Choose a folder on a local Windows drive.".into();
                        }
                    }
                    continue;
                }
                Event::SelectionFailed(id, message)
                    if id == self.generation.load(Ordering::SeqCst) =>
                {
                    self.stop();
                    self.phase = Phase::Error;
                    self.detail = message;
                    continue;
                }
                Event::Stopped(id, message) if id == self.generation.load(Ordering::SeqCst) => {
                    self.listening = false;
                    self.sending = false;
                    self.pending_sender = None;
                    self.send_targets.clear();
                    self.phase = Phase::Error;
                    if let Some(report) = &mut self.hardware {
                        report.ready = false;
                        report.summary =
                            "Connection interrupted. AirBridge is checking your adapter.".into();
                        report
                            .details
                            .push_str(&format!("\nSession stopped: {message}"));
                    }
                    self.detail = message;
                    continue;
                }
                Event::Line(id, line) if id == self.generation.load(Ordering::SeqCst) => line,
                _ => continue,
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            match value["event"].as_str().unwrap_or_default() {
                "ready" => {
                    self.phase = Phase::Ready;
                }
                "waking" => {
                    self.phase = Phase::Waking;
                }
                "searching" => {
                    self.phase = Phase::Discovering;
                }
                "send-target" => {
                    if self.phase != Phase::Waiting && self.phase != Phase::Transfer {
                        if let (Some(id), Some(name)) =
                            (value["id"].as_str(), value["name"].as_str())
                        {
                            if !self.send_targets.iter().any(|(known, _)| known == id) {
                                self.send_targets.push((id.into(), name.into()));
                            }
                            self.phase = Phase::Discovering;
                        }
                    }
                }
                "peer" => {
                    self.peer_name = value["name"].as_str().unwrap_or("iPhone").into();
                }
                "asking" => {
                    self.phase = Phase::Waiting;
                }
                "send-accepted" | "sending" => {
                    self.phase = Phase::Transfer;
                    self.send_targets.clear();
                    // The sender reports upload size, not acknowledged byte progress.
                    self.total = value["bytes"].as_u64();
                    self.bytes = 0;
                }
                "approval-needed" => {
                    self.view = View::Receive;
                    self.phase = Phase::Consent;
                    self.bytes = 0;
                    self.total = None;
                    self.incoming_count = value["files"].as_u64().unwrap_or(1) as usize;
                    self.peer_name = value["sender"].as_str().unwrap_or("iPhone").into();
                    self.pending_sender = Some(self.peer_name.clone());
                }
                "accepted" | "upload-started" => {
                    self.pending_sender = None;
                    self.phase = Phase::Transfer;
                }
                "receiving" if self.phase == Phase::Transfer => {
                    self.bytes = value["bytes"].as_u64().unwrap_or(0);
                    self.total = value["total"].as_u64().filter(|n| *n > 0);
                }
                "received" => {
                    self.incoming_count = value["files"].as_array().map_or(1, Vec::len);
                    self.received += self.incoming_count;
                    self.pending_sender = None;
                    self.phase = Phase::Complete;
                }
                "sent" => {
                    self.sending = false;
                    self.send_targets.clear();
                    self.phase = Phase::Complete;
                }
                "declined" => {
                    self.pending_sender = None;
                    self.phase = Phase::Ready;
                }
                "transfer-error" | "error" => {
                    self.pending_sender = None;
                    self.send_targets.clear();
                    self.phase = Phase::Error;
                    self.detail = value["message"]
                        .as_str()
                        .unwrap_or("Please try the transfer again.")
                        .into();
                }
                _ => {}
            }
        }
    }
}

impl Drop for AirBridgeApp {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        if self.active.load(Ordering::SeqCst) {
            // A separate helper survives this UI process exiting and completes
            // bounded runtime shutdown without delaying the window close.
            if let Ok(executable) = std::env::current_exe() {
                let _ = hidden_command(executable)
                    .arg("--stop-runtime")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
        }
    }
}

impl eframe::App for AirBridgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();
        if !self.listening && !self.sending && !self.preview && self.hardware_ready() {
            let dropped: Vec<_> = ctx
                .input(|i| i.raw.dropped_files.clone())
                .into_iter()
                .filter_map(|f| f.path)
                .collect();
            if !dropped.is_empty() {
                self.view = View::Send;
                self.files = dropped;
                self.send_files();
            }
        }
        design::show(self, ctx);
    }
}

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

fn wsl_command() -> Command {
    let mut command = hidden_command("wsl");
    command.args(["-d", "AirBridgeRuntime", "-u", "root", "--exec"]);
    command
}

fn bounded_output(command: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let collect = |stream: Box<dyn Read + Send>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.take(65536).read_to_end(&mut bytes);
            let _ = tx.send(bytes);
        });
        rx
    };
    let stdout = collect(Box::new(child.stdout.take().unwrap()));
    let stderr = collect(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "The helper did not respond in time",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Ok(Output {
        status,
        stdout: stdout
            .recv_timeout(Duration::from_millis(200))
            .unwrap_or_default(),
        stderr: stderr
            .recv_timeout(Duration::from_millis(200))
            .unwrap_or_default(),
    })
}

fn stop_runtime() {
    let _ = bounded_output(
        wsl_command().args(["bash", "/opt/airbridge/bin/airbridge-radio", "stop"]),
        Duration::from_secs(25),
    );
}

fn run_session(
    args: Vec<String>,
    id: u64,
    generation: &AtomicU64,
    tx: &Sender<Event>,
) -> Result<(), String> {
    // The connection worker keeps the runtime alive for the lifetime of the app.
    // Recheck attachment here in case the device was unplugged after its last poll.
    connection::attach_adapter()?;
    if generation.load(Ordering::SeqCst) != id {
        return Ok(());
    }
    let mut child = wsl_command()
        .args(["bash", "/opt/airbridge/bin/airbridge-radio"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start AirBridge: {e}"))?;
    let diagnostics = Arc::new(Mutex::new(VecDeque::new()));
    let completed = Arc::new(AtomicBool::new(false));
    forward(
        child.stdout.take().unwrap(),
        id,
        tx.clone(),
        diagnostics.clone(),
        completed.clone(),
    );
    forward(
        child.stderr.take().unwrap(),
        id,
        tx.clone(),
        diagnostics.clone(),
        completed.clone(),
    );
    loop {
        if generation.load(Ordering::SeqCst) != id {
            stop_runtime();
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            std::thread::sleep(Duration::from_millis(100));
            if status.success() && completed.load(Ordering::SeqCst) {
                return Ok(());
            }
            let detail = diagnostics
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ");
            return Err(if detail.is_empty() {
                format!("The receiver stopped ({status}). Try starting it again.")
            } else {
                detail
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn forward<R: Read + Send + 'static>(
    stream: R,
    id: u64,
    tx: Sender<Event>,
    diagnostics: Arc<Mutex<VecDeque<String>>>,
    completed: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            match line {
                Ok(line) => {
                    if !line.trim().is_empty() {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
                            && value["event"] == "sent"
                        {
                            completed.store(true, Ordering::SeqCst);
                        }
                        let mut recent = diagnostics.lock().unwrap_or_else(|e| e.into_inner());
                        if recent.len() == 4 {
                            recent.pop_front();
                        }
                        recent.push_back(line.chars().take(300).collect());
                    }
                    let _ = tx.send(Event::Line(id, line));
                }
                Err(error) => {
                    let _ = tx.send(Event::Stopped(id, error.to_string()));
                    break;
                }
            }
        }
    });
}

fn windows_to_wsl(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' {
        return None;
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    Some(format!("/mnt/{drive}/{}", path[3..].replace('\\', "/")))
}

fn approve(decision: &str) {
    let command = format!("printf '%s' '{decision}' > /run/airbridge/approval");
    std::thread::spawn(move || {
        let _ = bounded_output(
            wsl_command().args(["sh", "-c", &command]),
            Duration::from_secs(5),
        );
    });
}

fn main() -> eframe::Result {
    if std::env::args().any(|arg| arg == "--compatibility-report") {
        println!("{}", hardware::probe().details);
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--stop-runtime") {
        stop_runtime();
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 650.0])
            .with_icon(design::app_icon())
            .with_min_inner_size([560.0, 580.0])
            .with_title("AirBridge"),
        ..Default::default()
    };
    eframe::run_native(
        "AirBridge",
        options,
        Box::new(|context| {
            let mut app = AirBridgeApp::new(context);
            if std::env::args().any(|a| a == "--preview") {
                app.preview = true;
                design::preview_state(&mut app, "Transfer");
            }
            Ok(Box::new(app))
        }),
    )
}
