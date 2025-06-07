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
    host: String,
    username: String,
    password: String,
    show_password: bool,
    ddp_time: String,
    product: String,
    bag: String,
    loop_mode: LoopMode, // 新增的循环模式选项

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
    need_execute_flag: bool, // Renamed from need_execute to avoid conflict with method
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

const COMMAND_PREFIX: &str = "docker exec fpp-container-mnt-data-maf_planning ./sim fpp play -v ";

impl Default for SSHCommander {
    fn default() -> Self {
        Self {
            host: "10.21.31.17:22".into(),
            username: "root".into(),
            password: "1".into(),
            show_password: false,
            bag: "/opt/maf_planning/backup/bags".into(),
            ddp_time: "2.0".into(),
            product: "MNP".into(),
            loop_mode: LoopMode::default(), // 添加默认值
            output: String::new(),
            output_command: String::new(),
            show_file_dialog: false,
            current_directory: "/opt/maf_planning/backup/bags".into(),
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

// --- Helper: Establish SSH Session (for worker threads) ---
fn establish_ssh_session(host: &str, username: &str, password: &str) -> Result<Session, SshError> {
    let tcp = TcpStream::connect(host).map_err(|e| SshError::ConnectionFailed(e.to_string()))?;
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
    path: String,
    sender: Sender<Result<Vec<String>, SshError>>,
) {
    let result = || -> Result<Vec<String>, SshError> {
        let session = establish_ssh_session(&host, &username, &password)?;
        let mut channel = session.channel_session()?;
        // Use a more script-friendly ls command
        let command = format!(
            "docker exec fpp-container-mnt-data-maf_planning ls -1 --indicator-style=slash {}",
            path
        );
        channel.exec(&command)?;
        let mut output_str = String::new();
        channel.read_to_string(&mut output_str)?;

        channel.send_eof()?;
        channel.wait_close()?; // Wait for command to finish

        let entries: Vec<String> = output_str
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|name| !name.is_empty() && name != "." && name != "..")
            .collect();
        Ok(entries)
    }();
    if sender.send(result).is_err() {
        eprintln!("Failed to send directory list result to UI thread.");
    }
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
        let mut output_str = String::new();
        channel.read_to_string(&mut output_str)?;

        channel.send_eof()?;
        channel.wait_close()?;
        Ok(output_str)
    }();
    if sender.send(result).is_err() {
        eprintln!("Failed to send command execution result to UI thread.");
    }
}

impl SSHCommander {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }
        Default::default()
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

        std::thread::spawn(move || {
            list_remote_directory_worker(host, username, password, path, sender);
        });
    }

    fn close_ssh_channel(channel: &mut ssh2::Channel) -> Result<(), SshError> {
        channel.send_eof()?;
        // For simple cases like keep-alive, a quick close might be sufficient.
        // For commands expecting full closure, wait_close (with timeout) is better.
        // channel.close()?; // Send close notification
        // channel.wait_close()?; // Wait for remote to acknowledge close
        // The channel will be closed when it's dropped if not explicitly closed.
        // For keep-alive 'echo', just EOF and let it drop might be okay.
        // A more robust solution would be to read output and then close/wait_close.
        let mut buffer = Vec::new();
        let _ = channel.read_to_end(&mut buffer); // Consume any output from echo
        channel.close()?; // Explicitly close
        channel
            .wait_close()
            .map_err(|e| {
                // Non-fatal for keep-alive if wait_close fails slightly
                eprintln!("Keep-alive channel wait_close warning: {}", e);
                SshError::Ssh2Error(e)
            })
            .ok(); // Suppress error for keep-alive, or handle more gracefully
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
                                // Try to gracefully close the keep-alive channel
                                if Self::close_ssh_channel(&mut channel).is_err() {
                                    eprintln!("Failed to gracefully close keep-alive channel. Forcing reconnect.");
                                    self.force_connect = true; // Force reconnect on next attempt if keep-alive fails badly
                                } else {
                                    conn_guard.last_used = Instant::now();
                                    return Ok(conn_arc.clone());
                                }
                            } else {
                                eprintln!("Keep-alive exec failed. Forcing reconnect.");
                                self.force_connect = true; // Force reconnect
                            }
                        }
                        Err(_) => {
                            eprintln!("Keep-alive channel session failed. Forcing reconnect.");
                            self.force_connect = true; // Force reconnect
                        }
                    }
                }
            }
        }

        // Establish new cached connection
        println!("Establishing new cached SSH connection to {}", self.host);
        let tcp = TcpStream::connect(&self.host)
            .map_err(|e| SshError::ConnectionFailed(e.to_string()))?;
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

        // 修改命令构建，包含循环模式选项
        self.output_command = format!(
            "{} --ddp-time {} --product {} {} {}",
            COMMAND_PREFIX,
            self.ddp_time,
            self.product,
            self.loop_mode.as_str(), // 添加循环模式参数
            self.bag
        );
        println!("Executing command: {}", self.output_command);

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
                    self.directory_contents = vec![err_msg.clone()]; // Show error in list
                    self.directory_list_error = Some(err_msg); // Also store for dedicated display
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
                    self.output = err_msg.clone(); // Show error in output area
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

pub fn parse_hyperlink(line: &str) -> Option<String> {
    // Using a lazy_static or once_cell for Regex is more efficient if called frequently,
    // but for this UI, direct creation is fine.
    let url_regex = Regex::new(r"https?://[^\s]+").unwrap();
    url_regex.find(line).map(|m| m.as_str().to_string())
}

impl eframe::App for SSHCommander {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle results from async operations
        self.handle_async_directory_list(ctx);
        self.handle_async_command_execution(ctx);

        // Try to ensure main thread cached connection is alive if it exists
        // This is a non-blocking check/refresh attempt.
        if self.ssh_connection.is_some() || self.force_connect {
            if let Err(e) = self.ensure_ssh_connection_cached() {
                // Log error, maybe display in UI status bar if you add one
                eprintln!("Cached SSH connection error: {}", e);
                self.ssh_connection = None; // Clear bad connection
            }
        }

        if self.need_execute_flag && !self.is_executing_command {
            self.need_execute_flag = false; // Reset flag
                                            // self.force_connect = true; // Worker thread will establish its own connection.
                                            // This flag affects the *cached* connection.
            self.trigger_execute_command();
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }
                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("SSH Commander");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Host: ");
                ui.add(egui::TextEdit::singleline(&mut self.host).desired_width(150.));
                // Optionally display cached connection status or force_connect flag
                // ui.label(format!("Force connect (cache): {}", self.force_connect));
            });
            ui.horizontal(|ui| {
                ui.label("Username: ");
                ui.add(egui::TextEdit::singleline(&mut self.username).desired_width(100.));
                ui.label("Password: ");
                let password_edit = egui::TextEdit::singleline(&mut self.password)
                    .password(!self.show_password)
                    .desired_width(100.);
                ui.add(password_edit);
                if ui
                    .button(if self.show_password { "Hide" } else { "Show" })
                    .clicked()
                {
                    self.show_password = !self.show_password;
                }
            });
            ui.horizontal(|ui| {
                ui.label("ddp_time: ");
                ui.add(egui::TextEdit::singleline(&mut self.ddp_time).desired_width(100.));
                ui.label("product: ");
                ui.add(egui::TextEdit::singleline(&mut self.product).desired_width(100.));
            });

            // 新增的循环模式选择器
            ui.horizontal(|ui| {
                ui.label("Loop Mode: ");
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

                // 可选：添加一个简单的提示或说明
                ui.label(format!("({})", self.loop_mode.as_str()));
            });

            ui.horizontal(|ui| {
                ui.label("bag: ");
                let remaining_width = ui.available_width() - ui.spacing().interact_size.x * 1.5; // Approx button width
                ui.add(
                    egui::TextEdit::singleline(&mut self.bag)
                        .desired_width(remaining_width.max(100.0)),
                );
                if ui.button("Browse...").clicked() {
                    self.show_file_dialog = true;
                    // self.force_connect = true; // Worker thread makes new connection.
                    // This would affect cached connection.
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
                            // Make text edit wider
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
                                if let Some(parent_end) = self.current_directory.rfind('/') {
                                    if parent_end > 0 {
                                        // Avoid going to "" from "/root"
                                        self.current_directory =
                                            self.current_directory[..parent_end].to_string();
                                    } else if &self.current_directory != "/" {
                                        // Go to "/" from "/root"
                                        self.current_directory = "/".to_string();
                                    }
                                    // If already at "/", clicking ".." does nothing or reloads "/"
                                    self.trigger_load_directory_contents();
                                } else if !self.current_directory.is_empty()
                                    && self.current_directory != "/"
                                {
                                    // Handle cases like "mydir" (no slashes) -> effectively go to parent or current dir
                                    // For simplicity, let's just reload current_directory if it's not root
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
                                        let mut new_bag_path = self.current_directory.clone();
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

            if ui.button("Execute").clicked() {
                if !self.is_executing_command {
                    self.need_execute_flag = true;
                }
            }
            if self.is_executing_command {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Executing command...");
                });
            }

            ui.label(&self.output_command); // Show the command that will be/was run

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
