use once_cell::sync::Lazy;
use regex::Regex;
use ssh2::Session;
use std::io::Read;
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// --- Custom Error Type ---
#[allow(dead_code)]
#[derive(Debug)]
pub enum SshError {
    ConnectionFailed(String),
    AuthenticationFailed,
    CommandExecutionFailed(String), // General command failure
    IoError(std::io::Error),
    Ssh2Error(ssh2::Error),
    ChannelSendError(String), // For mpsc send errors
}

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshError::ConnectionFailed(s) => write!(f, "Connection failed: {}", s),
            SshError::AuthenticationFailed => write!(f, "Authentication failed"),
            SshError::CommandExecutionFailed(s) => write!(f, "Command execution failed: {}", s),
            SshError::IoError(e) => write!(f, "IO error: {}", e),
            SshError::Ssh2Error(e) => write!(f, "SSH2 error: {}", e),
            SshError::ChannelSendError(s) => write!(f, "Channel send error: {}", s),
        }
    }
}

impl std::error::Error for SshError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SshError::IoError(e) => Some(e),
            SshError::Ssh2Error(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SshError {
    fn from(err: std::io::Error) -> Self {
        SshError::IoError(err)
    }
}

impl From<ssh2::Error> for SshError {
    fn from(err: ssh2::Error) -> Self {
        SshError::Ssh2Error(err)
    }
}

// --- Loop Mode Enum ---
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum LoopMode {
    OpenLoop,
    CloseLoop,
}

impl LoopMode {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            LoopMode::OpenLoop => "--open-loop",
            LoopMode::CloseLoop => "",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            LoopMode::OpenLoop => "Open Loop",
            LoopMode::CloseLoop => "Close Loop",
        }
    }

    pub fn cli_flag(&self) -> Option<&'static str> {
        match self {
            LoopMode::OpenLoop => Some("--open-loop"),
            LoopMode::CloseLoop => None,
        }
    }
}

impl Default for LoopMode {
    fn default() -> Self {
        LoopMode::OpenLoop
    }
}

// --- SSH Connection Struct (for main thread cache) ---
struct SSHConnection {
    session: Session,
    last_used: Instant,
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct SSHCommander {
    // Connection
    host: String,
    username: String,
    // #[serde(skip)]
    password: String,
    show_password: bool,
    remember_server: bool, // whether to persist host/username

    // Command params
    ddp_time: String,
    product: String,
    bag: String,
    loop_mode: LoopMode,

    // Parametrized command parts (replaces hard-coded prefix)
    container_name: String, // e.g., "my-container"
    sim_cmd: String,        // e.g., "./sim fpp play -v"

    // UI and runtime state
    #[serde(skip)]
    output: String,
    #[serde(skip)]
    show_file_dialog: bool,
    current_directory: String,
    #[serde(skip)]
    directory_contents: Vec<String>,
    #[serde(skip)]
    is_loading_directory: bool,
    #[serde(skip)]
    directory_list_error: Option<String>,

    #[serde(skip)]
    ssh_connection: Option<Arc<Mutex<SSHConnection>>>, // Cached connection for main thread (e.g., keep-alive)
    #[serde(skip)]
    output_command: String,
    #[serde(skip)]
    need_execute_flag: bool,
    #[serde(skip)]
    force_connect: bool, // For the cached connection

    // Receivers for async operations
    #[serde(skip)]
    dir_list_receiver: Option<Receiver<Result<Vec<String>, SshError>>>,
    #[serde(skip)]
    cmd_exec_receiver: Option<Receiver<Result<String, SshError>>>,
    #[serde(skip)]
    is_executing_command: bool,
    #[serde(skip)]
    command_execution_error: Option<String>,
}

impl Default for SSHCommander {
    fn default() -> Self {
        Self {
            // Safe, generic defaults
            host: "127.0.0.1:22".into(),
            username: "user".into(),
            password: String::new(),
            show_password: false,
            remember_server: false,

            ddp_time: "2.0".into(),
            product: "MNP".into(),
            bag: String::new(),
            loop_mode: LoopMode::default(),

            container_name: "my-container".into(),
            sim_cmd: "./sim fpp play -v".into(),

            output: String::new(),
            output_command: String::new(),
            show_file_dialog: false,
            current_directory: "/".into(),
            directory_contents: Vec::new(),
            is_loading_directory: false,
            directory_list_error: None,
            ssh_connection: None,
            need_execute_flag: false,
            force_connect: false,
            dir_list_receiver: None,
            cmd_exec_receiver: None,
            is_executing_command: false,
            command_execution_error: None,
        }
    }
}

// --- Helpers: Shell Escape and Redaction ---
fn shell_escape(s: &str) -> String {
    // Simple POSIX-like single-quote escaping
    if s.is_empty() {
        "''".to_string()
    } else if !s.contains('\'')
        && !s.contains(' ')
        && !s.contains('\t')
        && !s.contains('"')
        && !s.contains('\\')
        && !s.contains('$')
        && !s.contains('`')
        && !s.contains('!')
        && !s.contains('&')
        && !s.contains('|')
        && !s.contains(';')
        && !s.contains('(')
        && !s.contains(')')
        && !s.contains('<')
        && !s.contains('>')
        && !s.contains('*')
        && !s.contains('?')
        && !s.contains('[')
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

fn redact_path(p: &str) -> String {
    // Keep last two components only
    let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        p.to_string()
    } else {
        format!(".../{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    }
}

fn redact_command(cmd: &str) -> String {
    let mut tokens: Vec<String> = cmd.split_whitespace().map(|s| s.to_string()).collect();
    if let Some(last) = tokens.last_mut() {
        if last.contains('/') || last.ends_with(".bag") {
            *last = redact_path(last);
        }
    }
    tokens.join(" ")
}

// --- Helper: Establish SSH Session (for worker threads) ---
fn establish_ssh_session(host: &str, username: &str, password: &str) -> Result<Session, SshError> {
    let tcp = TcpStream::connect(host).map_err(|e| SshError::ConnectionFailed(e.to_string()))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30))).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(30))).ok();

    let mut sess = Session::new().or_else(|_| {
        Err(SshError::Ssh2Error(ssh2::Error::new(
            ssh2::ErrorCode::Session(-1), // Generic session error code
            "Session::new() failed",
        )))
    })?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;
    sess.userauth_password(username, password)?;
    if !sess.authenticated() {
        return Err(SshError::AuthenticationFailed);
    }
    Ok(sess)
}

// --- Worker: List Remote Directory ---
fn list_remote_directory_worker(
    host: String,
    username: String,
    password: String,
    container_name: String,
    path: String,
    sender: Sender<Result<Vec<String>, SshError>>,
) {
    let result = || -> Result<Vec<String>, SshError> {
        let session = establish_ssh_session(&host, &username, &password)?;
        let mut channel = session.channel_session()?;
        let command = format!(
            "docker exec {} ls -1 --indicator-style=slash {}",
            shell_escape(&container_name),
            shell_escape(&path)
        );
        channel.exec(&command)?;
        let mut output_str = String::new();
        channel.read_to_string(&mut output_str)?;

        channel.send_eof()?;
        channel.wait_close()?; // Wait for command to finish

        let mut entries: Vec<String> = output_str
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|name| !name.is_empty() && name != "." && name != "..")
            .collect();

        // Optional: directories and .bag files only
        entries.retain(|e| e.ends_with('/') || e.ends_with(".bag"));

        Ok(entries)
    }();
    let _ = sender.send(result);
}

// --- Worker: Execute Command ---
fn execute_command_worker(
    host: String,
    username: String,
    password: String,
    command_to_execute: String,
    sender: Sender<Result<String, SshError>>,
) {
    let result = || -> Result<String, SshError> {
        let session = establish_ssh_session(&host, &username, &password)?;
        let mut channel = session.channel_session()?;
        channel.exec(&command_to_execute)?;
        let mut out = String::new();
        let mut err = String::new();

        // Read stdout and stderr (stderr best-effort)
        channel.read_to_string(&mut out)?;
        channel.stderr().read_to_string(&mut err).ok();

        channel.send_eof()?;
        channel.wait_close()?;

        if !err.is_empty() {
            Ok(format!("STDOUT:\n{}\n\nSTDERR:\n{}", out, err))
        } else {
            Ok(out)
        }
    }();
    let _ = sender.send(result);
}

impl SSHCommander {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }
        Default::default()
    }

    fn build_command(&self) -> String {
        let mut parts = vec![
            "docker".to_string(),
            "exec".to_string(),
            shell_escape(&self.container_name),
            self.sim_cmd.clone(), // e.g., "./sim fpp play -v"
            "--ddp-time".to_string(),
            shell_escape(&self.ddp_time),
            "--product".to_string(),
            shell_escape(&self.product),
        ];
        if let Some(flag) = self.loop_mode.cli_flag() {
            parts.push(flag.to_string());
        }
        if !self.bag.is_empty() {
            parts.push(shell_escape(&self.bag));
        }
        parts.join(" ")
    }

    // Renamed to avoid conflict with async trigger
    fn trigger_load_directory_contents(&mut self) {
        if self.is_loading_directory {
            return; // Already loading
        }
        self.is_loading_directory = true;
        self.directory_list_error = None; // Clear previous error
        self.directory_contents.clear(); // Clear previous content

        let (sender, receiver) = std::sync::mpsc::channel();
        self.dir_list_receiver = Some(receiver);

        let host = self.host.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        let path = self.current_directory.clone();
        let container_name = self.container_name.clone();

        std::thread::spawn(move || {
            list_remote_directory_worker(host, username, password, container_name, path, sender);
        });
    }

    fn close_ssh_channel(channel: &mut ssh2::Channel) -> Result<(), SshError> {
        channel.send_eof()?;
        let mut buffer = Vec::new();
        let _ = channel.read_to_end(&mut buffer); // consume any remaining output
        channel.close()?;
        // do not propagate wait_close error as fatal for keep-alive
        if let Err(e) = channel.wait_close() {
            eprintln!("Keep-alive channel wait_close warning: {}", e);
        }
        Ok(())
    }

    // Manages the cached SSH connection for the main thread (e.g., keep-alive)
    fn ensure_ssh_connection_cached(&mut self) -> Result<Arc<Mutex<SSHConnection>>, SshError> {
        if !self.force_connect {
            if let Some(conn_arc) = &self.ssh_connection {
                let mut conn_guard = conn_arc.lock().unwrap(); // Handle poisoning better in prod
                if conn_guard.last_used.elapsed() <= Duration::from_secs(300) {
                    conn_guard.last_used = Instant::now();
                    return Ok(conn_arc.clone());
                } else {
                    // Refresh connection (keep-alive)
                    println!("SSH cached connection: sending keep-alive.");
                    match conn_guard.session.channel_session() {
                        Ok(mut channel) => {
                            if channel.exec("echo").is_ok() {
                                if Self::close_ssh_channel(&mut channel).is_err() {
                                    eprintln!("Failed to gracefully close keep-alive channel. Forcing reconnect.");
                                    self.force_connect = true;
                                } else {
                                    conn_guard.last_used = Instant::now();
                                    return Ok(conn_arc.clone());
                                }
                            } else {
                                eprintln!("Keep-alive exec failed. Forcing reconnect.");
                                self.force_connect = true;
                            }
                        }
                        Err(_) => {
                            eprintln!("Keep-alive channel session failed. Forcing reconnect.");
                            self.force_connect = true;
                        }
                    }
                }
            }
        }

        // Establish new cached connection
        println!("Establishing new cached SSH connection to {}", self.host);
        let tcp = TcpStream::connect(&self.host)
            .map_err(|e| SshError::ConnectionFailed(e.to_string()))?;
        tcp.set_read_timeout(Some(Duration::from_secs(30))).ok();
        tcp.set_write_timeout(Some(Duration::from_secs(30))).ok();

        let mut sess = Session::new().or_else(|_| {
            Err(SshError::Ssh2Error(ssh2::Error::new(
                ssh2::ErrorCode::Session(-1),
                "Session::new() failed for cached connection",
            )))
        })?;
        sess.set_tcp_stream(tcp);
        sess.handshake()?;
        sess.userauth_password(&self.username, &self.password)?;
        if !sess.authenticated() {
            return Err(SshError::AuthenticationFailed);
        }
        println!("Cached SSH connection established.");
        let connection = Arc::new(Mutex::new(SSHConnection {
            session: sess,
            last_used: Instant::now(),
        }));
        self.ssh_connection = Some(connection.clone());
        self.force_connect = false;
        Ok(connection)
    }

    fn trigger_execute_command(&mut self) {
        if self.is_executing_command {
            return; // Already executing
        }
        self.is_executing_command = true;
        self.command_execution_error = None;
        self.output = "Executing...".to_string(); // Provide immediate feedback

        let (sender, receiver) = std::sync::mpsc::channel();
        self.cmd_exec_receiver = Some(receiver);

        // Build command with parameterized parts and escaping
        self.output_command = self.build_command();
        println!(
            "Executing command (redacted): {}",
            redact_command(&self.output_command)
        );

        let host = self.host.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        let command_to_run = self.output_command.clone();

        std::thread::spawn(move || {
            execute_command_worker(host, username, password, command_to_run, sender);
        });
    }

    fn handle_async_directory_list(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.dir_list_receiver {
            match receiver.try_recv() {
                Ok(Ok(contents)) => {
                    self.directory_contents = contents;
                    self.is_loading_directory = false;
                    self.dir_list_receiver = None;
                }
                Ok(Err(e)) => {
                    let err_msg = format!("Error listing directory: {}", e);
                    eprintln!("{}", err_msg);
                    self.directory_contents = vec![err_msg.clone()];
                    self.directory_list_error = Some(err_msg);
                    self.is_loading_directory = false;
                    self.dir_list_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint(); // Keep checking
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let err_msg = "Error: Directory listing thread disconnected.".to_string();
                    eprintln!("{}", err_msg);
                    self.directory_contents = vec![err_msg.clone()];
                    self.directory_list_error = Some(err_msg);
                    self.is_loading_directory = false;
                    self.dir_list_receiver = None;
                }
            }
        }
    }

    fn handle_async_command_execution(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.cmd_exec_receiver {
            match receiver.try_recv() {
                Ok(Ok(cmd_output)) => {
                    self.output = cmd_output;
                    self.is_executing_command = false;
                    self.cmd_exec_receiver = None;
                }
                Ok(Err(e)) => {
                    let err_msg = format!("Error executing command: {}", e);
                    eprintln!("{}", err_msg);
                    self.output = err_msg.clone();
                    self.command_execution_error = Some(err_msg);
                    self.is_executing_command = false;
                    self.cmd_exec_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint(); // Keep checking
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let err_msg = "Error: Command execution thread disconnected.".to_string();
                    eprintln!("{}", err_msg);
                    self.output = err_msg.clone();
                    self.command_execution_error = Some(err_msg);
                    self.is_executing_command = false;
                    self.cmd_exec_receiver = None;
                }
            }
        }
    }
}

// URL regex compiled once
static URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"https?://[^\s]+").unwrap());

pub fn parse_hyperlink(line: &str) -> Option<String> {
    URL_RE.find(line).map(|m| m.as_str().to_string())
}

impl eframe::App for SSHCommander {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Only persist host/username if remember_server is true
        let host_bak = self.host.clone();
        let username_bak = self.username.clone();

        if !self.remember_server {
            self.host.clear();
            self.username.clear();
        }

        eframe::set_value(storage, eframe::APP_KEY, self);

        // Restore in-memory values so UI is not affected during runtime
        if !self.remember_server {
            self.host = host_bak;
            self.username = username_bak;
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle async results
        self.handle_async_directory_list(ctx);
        self.handle_async_command_execution(ctx);

        // Try to ensure main thread cached connection is alive if it exists
        if self.ssh_connection.is_some() || self.force_connect {
            if let Err(e) = self.ensure_ssh_connection_cached() {
                eprintln!("Cached SSH connection error: {}", e);
                self.ssh_connection = None; // Clear bad connection
            }
        }

        if self.need_execute_flag && !self.is_executing_command {
            self.need_execute_flag = false;
            self.trigger_execute_command();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("SSH Commander");
            ui.separator();

            egui::CollapsingHeader::new("connection setting")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Host:");
                        ui.add(egui::TextEdit::singleline(&mut self.host).desired_width(200.));
                        ui.label("Username:");
                        ui.add(egui::TextEdit::singleline(&mut self.username).desired_width(120.));
                        ui.label("Password:");
                        let pw = egui::TextEdit::singleline(&mut self.password)
                            .password(!self.show_password)
                            .desired_width(120.);
                        ui.add(pw);
                        if ui
                            .button(if self.show_password { "Hide" } else { "Show" })
                            .clicked()
                        {
                            self.show_password = !self.show_password;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(
                            &mut self.remember_server,
                            "save server info(save Host/Username, password)",
                        );
                    });
                });

            egui::CollapsingHeader::new("command parameter")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Container:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.container_name)
                                .desired_width(200.),
                        );
                        ui.label("Sim Cmd:");
                        ui.add(egui::TextEdit::singleline(&mut self.sim_cmd).desired_width(300.));
                    });
                    ui.horizontal(|ui| {
                        ui.label("ddp_time:");
                        ui.add(egui::TextEdit::singleline(&mut self.ddp_time).desired_width(80.));
                        ui.label("product:");
                        ui.add(egui::TextEdit::singleline(&mut self.product).desired_width(100.));
                        ui.label("Loop Mode:");
                        egui::ComboBox::from_label("")
                            .selected_text(self.loop_mode.display_name())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.loop_mode,
                                    LoopMode::OpenLoop,
                                    LoopMode::OpenLoop.display_name(),
                                );
                                ui.selectable_value(
                                    &mut self.loop_mode,
                                    LoopMode::CloseLoop,
                                    LoopMode::CloseLoop.display_name(),
                                );
                            });
                    });
                });

            egui::CollapsingHeader::new("choose file")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("bag:");
                        let remaining_width =
                            ui.available_width() - ui.spacing().interact_size.x * 1.5; // Approx button width
                        ui.add(
                            egui::TextEdit::singleline(&mut self.bag)
                                .desired_width(remaining_width.max(120.0)),
                        );
                        if ui.button("Browse...").clicked() {
                            self.show_file_dialog = true;
                            self.trigger_load_directory_contents();
                        }
                    });

                    if self.show_file_dialog {
                        let mut open = self.show_file_dialog; // For window's open state
                        egui::Window::new("Select Bag File")
                            .open(&mut open)
                            .fixed_size([450.0, 350.0])
                            .show(ctx, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Current directory: ");
                                    let path_edit_width =
                                        ui.available_width() - ui.spacing().interact_size.x * 1.2;
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.current_directory)
                                            .desired_width(path_edit_width.max(100.0)),
                                    );
                                    if ui.button("Go").clicked() {
                                        self.trigger_load_directory_contents();
                                    }
                                });
                                ui.separator();

                                if self.is_loading_directory {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label("Loading directory contents...");
                                    });
                                } else if let Some(err_msg) = &self.directory_list_error {
                                    ui.colored_label(egui::Color32::RED, err_msg);
                                }

                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    if ui.selectable_label(false, ".. (Up a directory)").clicked() {
                                        if let Some(parent_end) = self.current_directory.rfind('/')
                                        {
                                            if parent_end > 0 {
                                                self.current_directory = self.current_directory
                                                    [..parent_end]
                                                    .to_string();
                                            } else if &self.current_directory != "/" {
                                                self.current_directory = "/".to_string();
                                            }
                                            self.trigger_load_directory_contents();
                                        } else if !self.current_directory.is_empty()
                                            && self.current_directory != "/"
                                        {
                                            self.trigger_load_directory_contents();
                                        }
                                    }
                                    // Display directory contents
                                    for item_name in self.directory_contents.clone() {
                                        let is_dir = item_name.ends_with('/');
                                        let label = if is_dir {
                                            format!("📁 {}", item_name)
                                        } else {
                                            format!("📄 {}", item_name)
                                        };
                                        if ui.selectable_label(false, label).clicked() {
                                            if is_dir {
                                                let mut new_path = self.current_directory.clone();
                                                if !new_path.ends_with('/') {
                                                    new_path.push('/');
                                                }
                                                new_path.push_str(item_name.trim_end_matches('/'));
                                                self.current_directory = new_path;
                                                self.trigger_load_directory_contents();
                                            } else {
                                                let mut new_bag_path =
                                                    self.current_directory.clone();
                                                if !new_bag_path.ends_with('/') {
                                                    new_bag_path.push('/');
                                                }
                                                new_bag_path.push_str(item_name.as_str());
                                                self.bag = new_bag_path;
                                                self.show_file_dialog = false; // Close dialog on selection
                                            }
                                        }
                                    }
                                });
                                ui.separator();
                                ui.horizontal(|ui| {
                                    if ui.button("Cancel").clicked() {
                                        self.show_file_dialog = false;
                                    }
                                });
                            });
                        if !open {
                            // If window was closed by user (e.g. 'x' button)
                            self.show_file_dialog = false;
                        }
                    }
                });

            ui.separator();
            if ui.button("Execute").clicked() && !self.is_executing_command {
                self.need_execute_flag = true;
            }
            if self.is_executing_command {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Executing command...");
                });
            }

            // Show redacted command
            if !self.output_command.is_empty() {
                let to_show = redact_command(&self.output_command);
                ui.label(format!("Command (redacted): {}", to_show));
            }

            if let Some(err_msg) = &self.command_execution_error {
                ui.colored_label(egui::Color32::RED, err_msg);
            }

            ui.separator();
            ui.heading("Output:");
            egui::ScrollArea::vertical().show(ui, |ui| {
                for line in self.output.lines() {
                    if let Some(link) = parse_hyperlink(line) {
                        ui.hyperlink_to(line, link);
                    } else {
                        ui.label(line);
                    }
                }
            });

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                powered_by_egui_and_eframe(ui);
                egui::warn_if_debug_build(ui);
            });
        });

        // Request repaint if there are active async operations
        if self.dir_list_receiver.is_some()
            || self.cmd_exec_receiver.is_some()
            || self.need_execute_flag
        {
            ctx.request_repaint();
        }
    }
}

fn powered_by_egui_and_eframe(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label("Powered by ");
        ui.hyperlink_to("egui", "https://github.com/emilk/egui");
        ui.label(" and ");
        ui.hyperlink_to(
            "eframe",
            "https://github.com/emilk/egui/tree/master/crates/eframe",
        );
        ui.label(".");
    });
}
