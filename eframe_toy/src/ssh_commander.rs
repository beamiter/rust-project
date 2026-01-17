use once_cell::sync::Lazy;
use regex::Regex;
use ssh2::Session;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

// --- Custom Error Type ---
#[allow(dead_code)]
#[derive(Debug)]
pub enum SshError {
    ConnectionFailed(String),
    AuthenticationFailed,
    CommandExecutionFailed(String),
    IoError(std::io::Error),
    Ssh2Error(ssh2::Error),
    ChannelSendError(String),
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

// --- Interaction Messages ---
#[derive(Debug)]
pub enum ShellMsg {
    Output(String),
    Error(String),
    Finished(i32),
}

/// SSHCommander App State
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct SSHCommander {
    // Connection
    host: String,
    username: String,
    password: String,
    show_password: bool,
    remember_server: bool,

    // Command params
    ddp_time: String,
    product: String,
    bag: String,
    loop_mode: LoopMode,

    // Parametrized command parts
    container_name: String,
    sim_cmd: String,

    // UI State
    #[serde(skip)]
    output: String, // Log buffer
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
    output_command: String,
    #[serde(skip)]
    need_execute_flag: bool,
    #[serde(skip)]
    force_connect: bool,

    // Async Receivers
    #[serde(skip)]
    dir_list_receiver: Option<Receiver<Result<Vec<String>, SshError>>>,

    // Interactive Execution State
    #[serde(skip)]
    shell_rx: Option<Receiver<ShellMsg>>,
    #[serde(skip)]
    stdin_tx: Option<Sender<String>>,
    #[serde(skip)]
    is_executing_command: bool,
    #[serde(skip)]
    user_input_buffer: String,
}

impl Default for SSHCommander {
    fn default() -> Self {
        Self {
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
            need_execute_flag: false,
            force_connect: false,
            dir_list_receiver: None,

            shell_rx: None,
            stdin_tx: None,
            is_executing_command: false,
            user_input_buffer: String::new(),
        }
    }
}

// --- Helpers ---
fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        "''".to_string()
    } else if !s.contains(
        &[
            '\'', ' ', '\t', '"', '\\', '$', '`', '!', '&', '|', ';', '(', ')', '<', '>', '*', '?',
            '[',
        ][..],
    ) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

fn establish_ssh_session(host: &str, username: &str, password: &str) -> Result<Session, SshError> {
    let tcp = TcpStream::connect(host).map_err(|e| SshError::ConnectionFailed(e.to_string()))?;
    // For interactive sessions, setting a timeout can be tricky.
    // We rely on non-blocking reads in the worker loop.
    tcp.set_read_timeout(Some(Duration::from_secs(60))).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(60))).ok();

    let mut sess = Session::new().map_err(|_| {
        SshError::Ssh2Error(ssh2::Error::new(
            ssh2::ErrorCode::Session(-1),
            "Session::new failed",
        ))
    })?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;
    sess.userauth_password(username, password)?;
    if !sess.authenticated() {
        return Err(SshError::AuthenticationFailed);
    }
    Ok(sess)
}

// --- Regex for Links ---
static URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"https?://[^\s]+").unwrap());

/// Helper to render log lines with clickable links
fn render_log_line(ui: &mut egui::Ui, line: &str) {
    if let Some(mat) = URL_RE.find(line) {
        let url_start = mat.start();
        let url_end = mat.end();
        let url_str = mat.as_str();

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            // Text before URL
            if url_start > 0 {
                ui.label(&line[..url_start]);
            }
            // The URL itself
            ui.hyperlink(url_str);
            // Text after URL
            if url_end < line.len() {
                ui.label(&line[url_end..]);
            }
        });
    } else {
        ui.label(line);
    }
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
        // ls does not need PTY
        let command = format!(
            "docker exec {} ls -1 --indicator-style=slash {}",
            shell_escape(&container_name),
            shell_escape(&path)
        );
        channel.exec(&command)?;
        let mut output_str = String::new();
        channel.read_to_string(&mut output_str)?;

        channel.send_eof()?;
        channel.wait_close()?;

        let mut entries: Vec<String> = output_str
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|name| !name.is_empty() && name != "." && name != "..")
            .collect();
        entries.retain(|e| e.ends_with('/') || e.ends_with(".bag"));
        // Sort: directories first
        entries.sort_by(|a, b| {
            let a_is_dir = a.ends_with('/');
            let b_is_dir = b.ends_with('/');
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });
        Ok(entries)
    }();
    let _ = sender.send(result);
}

// --- Worker: Interactive Command Execution ---
fn interactive_command_worker(
    host: String,
    username: String,
    password: String,
    command_to_execute: String,
    shell_tx: Sender<ShellMsg>,
    stdin_rx: Receiver<String>,
) {
    let run = || -> Result<(), SshError> {
        let session = establish_ssh_session(&host, &username, &password)?;
        let mut channel = session.channel_session()?;

        // 1. Request PTY for interactivity (crucial for docker -it and password prompts)
        channel.request_pty("xterm", None, None)?;

        // 2. Execute command
        channel.exec(&command_to_execute)?;

        // 3. Set non-blocking to handle read/write loop
        session.set_blocking(false);

        let mut buf = [0u8; 2048];
        loop {
            // A. Read from remote
            match channel.read(&mut buf) {
                Ok(n) if n > 0 => {
                    let s = String::from_utf8_lossy(&buf[..n]).to_string();
                    if shell_tx.send(ShellMsg::Output(s)).is_err() {
                        break; // Receiver dropped
                    }
                }
                Ok(_) => { /* EOF or empty read */ }
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::WouldBlock {
                        return Err(e.into());
                    }
                }
            }

            // B. Check if process finished
            if channel.eof() {
                break;
            }

            // C. Write user input to remote
            while let Ok(input_str) = stdin_rx.try_recv() {
                channel.write_all(input_str.as_bytes())?;
                channel.flush()?;
            }

            // Sleep to prevent 100% CPU usage
            std::thread::sleep(Duration::from_millis(10));
        }

        session.set_blocking(true);
        channel.wait_close()?;
        let exit_status = channel.exit_status()?;

        let _ = shell_tx.send(ShellMsg::Finished(exit_status));
        Ok(())
    };

    if let Err(e) = run() {
        let _ = shell_tx.send(ShellMsg::Error(e.to_string()));
    }
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
            "-it".to_string(), // Interactive PTY
            shell_escape(&self.container_name),
            self.sim_cmd.clone(),
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

    fn trigger_load_directory_contents(&mut self) {
        if self.is_loading_directory {
            return;
        }
        self.is_loading_directory = true;
        self.directory_list_error = None;
        self.directory_contents.clear();

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

    fn trigger_execute_command(&mut self) {
        if self.is_executing_command {
            return;
        }
        self.is_executing_command = true;
        self.output.clear();
        self.output
            .push_str("Initializing interactive session...\n");

        let (shell_tx, shell_rx) = std::sync::mpsc::channel();
        let (stdin_tx, stdin_rx) = std::sync::mpsc::channel();

        self.shell_rx = Some(shell_rx);
        self.stdin_tx = Some(stdin_tx);

        self.output_command = self.build_command();
        let host = self.host.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        let command_to_run = self.output_command.clone();

        std::thread::spawn(move || {
            interactive_command_worker(
                host,
                username,
                password,
                command_to_run,
                shell_tx,
                stdin_rx,
            );
        });
    }

    fn send_user_input(&mut self) {
        if self.user_input_buffer.is_empty() {
            return;
        }

        if let Some(tx) = &self.stdin_tx {
            let mut input = self.user_input_buffer.clone();
            // Append newline as if Enter was pressed
            if !input.ends_with('\n') {
                input.push('\n');
            }
            if let Err(e) = tx.send(input) {
                self.output
                    .push_str(&format!("\n[Error sending input: {}]\n", e));
            }
        }
        self.user_input_buffer.clear();
    }

    fn handle_async_shell_msg(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.shell_rx {
            loop {
                match rx.try_recv() {
                    Ok(ShellMsg::Output(data)) => {
                        self.output.push_str(&data);
                    }
                    Ok(ShellMsg::Error(e)) => {
                        self.output
                            .push_str(&format!("\n[Execution Error: {}]\n", e));
                        self.is_executing_command = false;
                        self.shell_rx = None;
                        self.stdin_tx = None;
                        break;
                    }
                    Ok(ShellMsg::Finished(code)) => {
                        self.output
                            .push_str(&format!("\n[Process exited with code: {}]\n", code));
                        self.is_executing_command = false;
                        self.shell_rx = None;
                        self.stdin_tx = None;
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.output.push_str("\n[Disconnected]\n");
                        self.is_executing_command = false;
                        self.shell_rx = None;
                        self.stdin_tx = None;
                        break;
                    }
                }
            }
            if self.is_executing_command {
                ctx.request_repaint();
            }
        }
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
                    self.directory_list_error = Some(e.to_string());
                    self.is_loading_directory = false;
                    self.dir_list_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint();
                }
                Err(_) => {
                    self.is_loading_directory = false;
                    self.dir_list_receiver = None;
                }
            }
        }
    }
}

impl eframe::App for SSHCommander {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let host_bak = self.host.clone();
        let username_bak = self.username.clone();
        if !self.remember_server {
            self.host.clear();
            self.username.clear();
        }
        eframe::set_value(storage, eframe::APP_KEY, self);
        if !self.remember_server {
            self.host = host_bak;
            self.username = username_bak;
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_async_directory_list(ctx);
        self.handle_async_shell_msg(ctx);

        if self.need_execute_flag && !self.is_executing_command {
            self.need_execute_flag = false;
            self.trigger_execute_command();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("SSH Commander");
            ui.separator();

            // --- Connection & Parameters (Collapsible) ---
            egui::CollapsingHeader::new("Connection & Parameters")
                .default_open(true)
                .show(ui, |ui| {
                    // Row 1: Connection Info
                    ui.horizontal(|ui| {
                        ui.label("Host:");
                        ui.add(egui::TextEdit::singleline(&mut self.host).desired_width(150.0));
                        ui.label("User:");
                        ui.add(egui::TextEdit::singleline(&mut self.username).desired_width(100.0));
                        ui.label("Pwd:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.password)
                                .password(!self.show_password)
                                .desired_width(100.0),
                        );
                        ui.checkbox(&mut self.show_password, "👁");
                    });

                    ui.separator();

                    // Row 2: Container (Flexible Width)
                    ui.horizontal(|ui| {
                        ui.label("Container:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.container_name)
                                .desired_width(ui.available_width()),
                        );
                    });

                    // Row 3: Sim Cmd (Flexible Width)
                    ui.horizontal(|ui| {
                        ui.label("Sim Cmd:   ");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.sim_cmd)
                                .desired_width(ui.available_width()),
                        );
                    });

                    // Row 4: Short Params
                    ui.horizontal(|ui| {
                        ui.label("DDP:");
                        ui.add(egui::TextEdit::singleline(&mut self.ddp_time).desired_width(60.));
                        ui.label("Product:");
                        ui.add(egui::TextEdit::singleline(&mut self.product).desired_width(100.));

                        ui.separator();
                        ui.label("Loop:");
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

                    // Row 5: Bag Path (Browse + Flexible Input)
                    ui.horizontal(|ui| {
                        ui.label("Bag Path: ");
                        if ui.button("📂 Browse").clicked() {
                            self.show_file_dialog = true;
                            // Ensure valid start dir
                            if self.current_directory.trim().is_empty() {
                                self.current_directory = "/".to_string();
                            }
                            self.trigger_load_directory_contents();
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.bag)
                                .desired_width(ui.available_width()),
                        );
                    });
                });

            // --- File Dialog Window ---
            if self.show_file_dialog {
                let mut open = true;
                egui::Window::new("Select Bag File")
                    .open(&mut open)
                    .default_size([600.0, 450.0])
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Dir:");
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.current_directory)
                                    .desired_width(ui.available_width() - 50.0),
                            );
                            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                self.trigger_load_directory_contents();
                            }
                            if ui.button("Go").clicked() {
                                self.trigger_load_directory_contents();
                            }
                        });

                        ui.separator();
                        if self.is_loading_directory {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Loading...");
                            });
                        }
                        if let Some(err) = &self.directory_list_error {
                            ui.colored_label(egui::Color32::RED, err);
                        }

                        egui::ScrollArea::vertical()
                            .max_height(350.0)
                            .show(ui, |ui| {
                                // Up Directory Logic
                                if ui.button("⬆️ Up .. (Parent Directory)").clicked() {
                                    let p = Path::new(&self.current_directory);
                                    if let Some(parent) = p.parent() {
                                        let mut s = parent.to_string_lossy().to_string();
                                        if s.is_empty() {
                                            s = "/".to_string();
                                        }
                                        if !s.ends_with('/') {
                                            s.push('/');
                                        }
                                        self.current_directory = s;
                                        self.trigger_load_directory_contents();
                                    }
                                }
                                ui.separator();

                                // Clone to avoid E0502 borrow error
                                let contents = self.directory_contents.clone();
                                for item in &contents {
                                    let is_dir = item.ends_with('/');
                                    let icon = if is_dir { "📁" } else { "📄" };
                                    let label = format!("{} {}", icon, item);

                                    if ui.selectable_label(false, label).clicked() {
                                        if is_dir {
                                            // Handle Path Join safely
                                            let p = Path::new(&self.current_directory).join(item);
                                            self.current_directory =
                                                p.to_string_lossy().to_string();
                                            if !self.current_directory.ends_with('/') {
                                                self.current_directory.push('/');
                                            }
                                            self.trigger_load_directory_contents();
                                        } else {
                                            // Select file
                                            let p = Path::new(&self.current_directory).join(item);
                                            self.bag = p.to_string_lossy().to_string();
                                            self.show_file_dialog = false;
                                        }
                                    }
                                }
                            });

                        ui.separator();
                        if ui.button("Cancel").clicked() {
                            self.show_file_dialog = false;
                        }
                    });
                if !open {
                    self.show_file_dialog = false;
                }
            }

            ui.add_space(8.0);

            // --- Command Preview Section (NEW) ---
            // Construct command live based on current input
            let preview_cmd = self.build_command();
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Command Preview").strong());
                    if ui
                        .button("📋 Copy")
                        .on_hover_text("Copy command to clipboard")
                        .clicked()
                    {
                        // FIX: Use ui.ctx().copy_text() instead of ui.output_mut(...)
                        ui.ctx().copy_text(preview_cmd.clone());
                    }
                });

                ui.add_space(2.0);

                egui::ScrollArea::vertical()
                    .id_salt("cmd_preview_scroll")
                    .max_height(60.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut preview_cmd.clone())
                                .font(egui::TextStyle::Monospace)
                                .desired_width(ui.available_width()),
                        );
                    });
            });

            ui.add_space(8.0);

            // --- Control Bar ---
            ui.horizontal(|ui| {
                if self.is_executing_command {
                    if ui.button("⏹ Stop").clicked() {
                        self.shell_rx = None;
                        self.stdin_tx = None;
                        self.is_executing_command = false;
                        self.output.push_str("\n[Terminated by user]\n");
                    }
                    ui.spinner();
                    ui.label("Running...");
                } else {
                    if ui.button("▶ Execute").clicked() {
                        self.need_execute_flag = true;
                    }
                    if ui.button("🗑 Clear Log").clicked() {
                        self.output.clear();
                    }
                }
            });

            // --- Output Log Area (Rich Text / Hyperlinks) ---
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("log_output")
                .stick_to_bottom(true)
                // Reserve space for bottom input field (approx 40px)
                .max_height(ui.available_height() - 40.0)
                .show(ui, |ui| {
                    for line in self.output.lines() {
                        render_log_line(ui, line);
                    }
                });

            // --- Interactive Input Area ---
            if self.is_executing_command {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Input >");
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.user_input_buffer)
                            .desired_width(ui.available_width() - 60.0)
                            .hint_text("Type password or command..."),
                    );

                    // Send on Enter or Button Click
                    if (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || ui.button("Send").clicked()
                    {
                        self.send_user_input();
                        resp.request_focus();
                    }
                });
            }
        });
    }
}
