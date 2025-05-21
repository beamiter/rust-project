use std::collections::HashMap; // For WsOpts headers, if needed later
use std::fs::{File, self};
use std::io::BufReader;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::{App, egui};
use egui::{Color32, RichText, ScrollArea};
use font_kit::source::SystemSource;
use humansize::{BINARY, format_size};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

static TOKIO_RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create Tokio runtime"));

// --- Data Structures for Clash Configuration ---

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")] // Handles fields like mixed-port
pub struct ClashFullConfig {
    #[serde(rename = "mixed-port")] // Special case if kebab-case doesn't catch it
    pub mixed_port: Option<u16>,
    #[serde(rename = "redir-port")]
    pub redir_port: Option<u16>,
    pub allow_lan: Option<bool>,
    pub mode: Option<String>, // e.g., "Rule", "Global", "Direct"
    pub log_level: Option<String>, // e.g., "info", "warning"
    #[serde(rename = "external-controller")]
    pub external_controller: Option<String>, // e.g., "0.0.0.0:9090" or ":9090"
    pub secret: Option<String>,
    #[serde(rename = "external-ui")]
    pub external_ui: Option<String>, // Path to dashboard
    pub dns: Option<DnsConfig>,
    #[serde(default)] // Ensure empty vec if 'proxies' is missing
    pub proxies: Vec<ProxyConfig>,
    #[serde(default, rename = "proxy-groups")] // Ensure empty vec if 'proxy-groups' is missing
    pub proxy_groups: Vec<ProxyGroupConfig>,
    // Add other top-level fields as needed, e.g., rules, tun, profile etc.
    // rules: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct DnsConfig {
    pub enable: Option<bool>,
    pub ipv6: Option<bool>,
    pub listen: Option<String>,
    pub enhanced_mode: Option<String>, // "fake-ip" or "redir-host"
    pub fake_ip_range: Option<String>, // CIDR, e.g., 198.18.0.1/16
    #[serde(default)]
    pub fake_ip_filter: Vec<String>,
    #[serde(default)]
    pub default_nameserver: Vec<String>,
    #[serde(default)]
    pub nameserver: Vec<String>,
    #[serde(default)]
    pub fallback: Option<Vec<String>>, // Optional field
    pub fallback_filter: Option<FallbackFilterConfig>,
    // Add other DNS fields as needed
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct FallbackFilterConfig {
    pub geoip: Option<bool>,
    pub geoip_code: Option<String>,
    #[serde(default)]
    pub ipcidr: Vec<String>,
    #[serde(default)]
    pub domain: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ProxyConfig {
    pub name: String,
    #[serde(rename = "type")] // 'type' is a reserved keyword in Rust
    pub proxy_type: String, // e.g., "trojan", "vmess", "ss"
    pub server: String,
    pub port: u16,
    pub password: Option<String>, // For Trojan, SS, etc.
    pub udp: Option<bool>,
    pub sni: Option<String>,
    #[serde(rename = "skip-cert-verify")]
    pub skip_cert_verify: Option<bool>,
    pub network: Option<String>, // e.g., "tcp", "ws", "grpc"
    pub alpn: Option<Vec<String>>,
    #[serde(rename = "grpc-opts")]
    pub grpc_opts: Option<GrpcOpts>,
    #[serde(rename = "ws-opts")]
    pub ws_opts: Option<WsOpts>,
    // VMess specific (examples)
    pub uuid: Option<String>,
    #[serde(rename = "alterId")] // Note aLtErId case or use alias
    pub alter_id: Option<u32>, // Or String if it can be non-numeric
    pub cipher: Option<String>, // e.g., "auto"
    // Add other proxy-specific fields as Option<T>
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct GrpcOpts {
    pub grpc_service_name: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct WsOpts {
    pub path: Option<String>,
    pub headers: Option<HashMap<String, String>>, // e.g., {"Host": "example.com"}
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ProxyGroupConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub group_type: String, // e.g., "select", "url-test", "fallback"
    #[serde(default)]
    pub proxies: Vec<String>, // Names of proxies or other groups
    pub url: Option<String>, // For url-test, fallback
    pub interval: Option<u32>, // For url-test, fallback
                               // Add other group-specific fields
}

// --- End of Data Structures for Clash Configuration ---


#[derive(Deserialize, Default, Debug)]
struct TrafficInfo {
    up: u64,
    down: u64,
}

// No longer needed as ClashFullConfig replaces its purpose for API port extraction
// #[derive(Deserialize, Debug)]
// struct ClashConfigYaml {
//     #[serde(rename = "external-controller")]
//     external_controller: Option<String>,
// }

struct NetworkStats {
    upload_speed: u64,
    download_speed: u64,
    previous_upload: u64,
    previous_download: u64,
    last_update: std::time::Instant,
    api_connected: bool,
}

impl Default for NetworkStats {
    fn default() -> Self {
        Self {
            upload_speed: 0,
            download_speed: 0,
            previous_upload: 0,
            previous_download: 0,
            last_update: std::time::Instant::now(),
            api_connected: false,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
struct AppState {
    config_path: String,
    clash_path: String,
    api_port: String,
}

// Helper function to extract port from "host:port" or ":port"
fn extract_port_from_controller_string(controller_addr: &str) -> Option<String> {
    if let Some(port_str) = controller_addr.split(':').last() {
        if !port_str.is_empty() && port_str.chars().all(char::is_numeric) {
            return Some(port_str.to_string());
        } else if port_str.is_empty() && controller_addr.starts_with(':') && controller_addr.len() > 1 {
            let port_only_str = &controller_addr[1..];
            if port_only_str.chars().all(char::is_numeric) {
                return Some(port_only_str.to_string());
            }
        }
    }
    None
}


impl Default for AppState {
    fn default() -> Self {
        let default_config_path = dirs::config_dir()
            .map(|p| p.join("clash/config.yaml"))
            .unwrap_or_else(|| std::path::PathBuf::from("config.yaml"))
            .to_string_lossy()
            .to_string();

        // Try to load full config to get API port, fallback to default "9090"
        let default_api_port = fs::read_to_string(&default_config_path)
            .ok()
            .and_then(|content| try_parse_clash_config_from_string(&content).ok())
            .and_then(|parsed_config| parsed_config.external_controller)
            .and_then(|ec_string| extract_port_from_controller_string(&ec_string))
            .unwrap_or_else(|| "9090".to_string());

        Self {
            config_path: default_config_path,
            clash_path: "clash".to_string(),
            api_port: default_api_port,
        }
    }
}

struct ClashApp {
    clash_process: Option<Child>,
    app_state: AppState,
    is_running: bool,
    stats: Arc<Mutex<NetworkStats>>,
    api_port_for_monitor: Arc<Mutex<String>>,
    config_content: String,
    config_editor_status: String,
    parsed_clash_config: Option<ClashFullConfig>, // 新增：存储完整解析的配置
}

// New function to parse the full config string
fn try_parse_clash_config_from_string(yaml_string: &str) -> Result<ClashFullConfig, serde_yaml::Error> {
    serde_yaml::from_str(yaml_string)
}


impl ClashApp {
    fn configure_fonts_and_style(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        let system_source = SystemSource::new();
        for font_name in [
            "Noto Sans CJK SC".to_string(),
            "Noto Sans CJK TC".to_string(),
            "SauceCodeProNerdFont".to_string(),
            "DejaVuSansMonoNerdFont".to_string(),
            "JetBrainsMonoNerdFont".to_string(),
            "WenQuanYi Micro Hei".to_string(),
            "Microsoft YaHei".to_string(),
        ] {
            match system_source.select_best_match(
                &[font_kit::family_name::FamilyName::Title(font_name.clone())],
                &font_kit::properties::Properties::new(),
            ) {
                Ok(font_handle) => {
                    if let Ok(font) = font_handle.load() {
                        if let Some(font_data) = font.copy_font_data() {
                            fonts.font_data.insert(
                                font_name.clone(),
                                egui::FontData::from_owned(font_data.to_vec()).into(),
                            );
                            fonts
                                .families
                                .entry(egui::FontFamily::Proportional)
                                .or_default()
                                .insert(0, font_name.clone());
                            fonts
                                .families
                                .entry(egui::FontFamily::Monospace)
                                .or_default()
                                .insert(0, font_name);
                        }
                    }
                }
                Err(_) => { /* eprintln!("Font {} not found or failed to load.", font_name); */ }
            }
        }
        ctx.set_fonts(fonts);
        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (
                egui::TextStyle::Heading,
                egui::FontId::new(18.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Body,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Monospace,
                egui::FontId::new(12.0, egui::FontFamily::Monospace),
            ),
            (
                egui::TextStyle::Button,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Small,
                egui::FontId::new(10.0, egui::FontFamily::Proportional),
            ),
        ]
        .into();
        ctx.set_style(style);
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::configure_fonts_and_style(&cc.egui_ctx);

        let app_state_loaded: Option<AppState> = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY)
        } else {
            None
        };

        let mut app_state: AppState = app_state_loaded.clone().unwrap_or_else(|| {
            println!("No saved app state found or failed to load, using defaults.");
            AppState::default() // AppState::default now tries to parse API port
        });
        if app_state_loaded.is_some() {
            println!("Successfully loaded app state {:?}.", app_state);
        }

        // Initial load of config content and attempt to parse it
        let mut initial_config_content = String::new();
        let mut initial_parsed_config: Option<ClashFullConfig> = None;
        let mut initial_config_status = format!("配置文件 '{}' 内容待加载。", app_state.config_path);

        match fs::read_to_string(&app_state.config_path) {
            Ok(content) => {
                initial_config_content = content;
                match try_parse_clash_config_from_string(&initial_config_content) {
                    Ok(parsed) => {
                        initial_config_status = format!("配置文件 '{}' 加载并解析成功。", app_state.config_path);
                        // Override API port from fully parsed config if available
                        if let Some(ref ec_str) = parsed.external_controller {
                            if let Some(port_from_parsed_config) = extract_port_from_controller_string(ec_str) {
                                if app_state.api_port != port_from_parsed_config {
                                     println!(
                                        "API port updated from initial full parse ('{}') over loaded/default state ('{}'). Path: '{}'",
                                        port_from_parsed_config, app_state.api_port, app_state.config_path
                                    );
                                    app_state.api_port = port_from_parsed_config;
                                }
                            }
                        }
                        initial_parsed_config = Some(parsed);
                    }
                    Err(e) => {
                        initial_config_status = format!("配置文件 '{}' 加载成功但解析失败: {}", app_state.config_path, e);
                        eprintln!("Initial parse failed: {}", e);
                    }
                }
            }
            Err(e) => {
                initial_config_status = format!("加载配置文件 '{}' 失败: {}", app_state.config_path, e);
            }
        }

        if app_state.api_port.is_empty() {
            app_state.api_port = "9090".to_string(); // Fallback if still empty
            println!("Warning: API port was empty after all checks, reset to '9090'.");
        }

        let stats = Arc::new(Mutex::new(NetworkStats::default()));
        let stats_clone = Arc::clone(&stats);
        let api_port_for_monitor = Arc::new(Mutex::new(app_state.api_port.clone()));
        let api_port_for_monitor_clone = Arc::clone(&api_port_for_monitor);

        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(1000));
                let current_api_port = api_port_for_monitor_clone.lock().unwrap().clone();
                if current_api_port.is_empty() {
                    let mut stats_guard = stats_clone.lock().unwrap();
                    stats_guard.api_connected = false;
                    stats_guard.upload_speed = 0;
                    stats_guard.download_speed = 0;
                    continue;
                }

                if let Some(traffic) = get_traffic("127.0.0.1", &current_api_port) {
                    let mut stats_guard = stats_clone.lock().unwrap();
                    stats_guard.api_connected = true;
                    let elapsed_secs = stats_guard.last_update.elapsed().as_secs_f64();
                    const MIN_ELAPSED_SECS_FOR_RATE: f64 = 0.1;
                    if elapsed_secs >= MIN_ELAPSED_SECS_FOR_RATE {
                        stats_guard.upload_speed =
                            ((traffic.up.saturating_sub(stats_guard.previous_upload)) as f64
                                / elapsed_secs) as u64;
                        stats_guard.download_speed =
                            ((traffic.down.saturating_sub(stats_guard.previous_download)) as f64
                                / elapsed_secs) as u64;
                    }
                    stats_guard.previous_upload = traffic.up;
                    stats_guard.previous_download = traffic.down;
                    stats_guard.last_update = std::time::Instant::now();
                } else {
                    let mut stats_guard = stats_clone.lock().unwrap();
                    stats_guard.api_connected = false;
                    stats_guard.upload_speed = 0;
                    stats_guard.download_speed = 0;
                }
            }
        });

        Self {
            clash_process: None,
            app_state,
            is_running: false,
            stats,
            api_port_for_monitor,
            config_content: initial_config_content,
            config_editor_status: initial_config_status,
            parsed_clash_config: initial_parsed_config,
        }
    }

    // Renamed and refactored: process current config_content string
    fn process_config_content(&mut self) {
        match try_parse_clash_config_from_string(&self.config_content) {
            Ok(parsed_config) => {
                self.config_editor_status = format!("配置文件内容解析成功。({})", chrono::Local::now().format("%H:%M:%S"));
                if let Some(ref ec_str) = parsed_config.external_controller {
                    if let Some(port_from_parsed) = extract_port_from_controller_string(ec_str) {
                        if self.app_state.api_port != port_from_parsed {
                            println!(
                                "API port updated to '{}' from parsed config content. Previous UI/state value was '{}'.",
                                port_from_parsed, self.app_state.api_port
                            );
                            self.app_state.api_port = port_from_parsed.clone();
                            *self.api_port_for_monitor.lock().unwrap() = port_from_parsed;
                        }
                    } else if parsed_config.external_controller.is_some() { // Has key but port invalid
                         self.config_editor_status = format!("配置解析成功，但 external-controller ('{}') 中的端口无效。", ec_str);
                    }
                } else { // No external-controller key
                    self.config_editor_status = "配置解析成功，但未找到 external-controller 键来更新API端口。".to_string();
                }
                self.parsed_clash_config = Some(parsed_config);
            }
            Err(e) => {
                self.config_editor_status = format!("配置文件内容解析失败: {}", e);
                self.parsed_clash_config = None; // Clear parsed config on error
                eprintln!("Failed to parse config content: {}", e);
            }
        }
    }

    fn load_config_from_file(&mut self) {
        match fs::read_to_string(&self.app_state.config_path) {
            Ok(content) => {
                self.config_content = content;
                // self.config_editor_status = format!("已从 '{}' 加载。", self.app_state.config_path);
                println!("Config content loaded from {}", self.app_state.config_path);
                self.process_config_content(); // Process after loading
            }
            Err(e) => {
                self.config_content = String::new();
                self.config_editor_status = format!("加载 '{}' 失败: {}", self.app_state.config_path, e);
                self.parsed_clash_config = None;
                eprintln!("Failed to load config content from {}: {}", self.app_state.config_path, e);
            }
        }
    }

    fn save_config_to_file(&mut self) {
        // Before saving, process the current editor content to update parsed_clash_config and API port
        self.process_config_content(); // This updates API port based on editor content

        match fs::write(&self.app_state.config_path, &self.config_content) {
            Ok(_) => {
                // Status already updated by process_config_content, can append save status
                self.config_editor_status = format!("已保存到 '{}'. {}", self.app_state.config_path, self.config_editor_status);
                println!("Config content saved to {}", self.app_state.config_path);
                // No need to re-parse here if process_config_content was called before writing
            }
            Err(e) => {
                self.config_editor_status = format!("保存 '{}' 失败: {}", self.app_state.config_path, e);
                eprintln!("Failed to save config content to {}: {}", self.app_state.config_path, e);
            }
        }
    }


    fn start_clash(&mut self) {
        if self.is_running {
            return;
        }

        // Ensure API port is up-to-date from current config before starting
        // This could be from parsed_clash_config or app_state.api_port if parsing failed
        let mut port_to_use = self.app_state.api_port.clone();
        if let Some(ref parsed_cfg) = self.parsed_clash_config {
            if let Some(ref ec_str) = parsed_cfg.external_controller {
                if let Some(p) = extract_port_from_controller_string(ec_str) {
                    port_to_use = p;
                }
            }
        }

        if port_to_use != self.app_state.api_port {
             println!(
                "API port for starting Clash confirmed/updated to '{}' from parsed config. Config path: '{}'. Previous UI/state value was '{}'.",
                port_to_use, self.app_state.config_path, self.app_state.api_port
            );
            self.app_state.api_port = port_to_use.clone();
            *self.api_port_for_monitor.lock().unwrap() = port_to_use.clone();
        }


        if self.app_state.api_port.is_empty() { // Check the final app_state.api_port
            eprintln!("Error: API port is empty. Cannot start Clash. Please set a valid API port.");
            self.config_editor_status = "错误：API端口为空，无法启动Clash。请在上方设置或检查配置文件。".to_string();
            return;
        }

        match Command::new(&self.app_state.clash_path)
            .arg("-f")
            .arg(&self.app_state.config_path)
            .spawn()
        {
            Ok(child) => {
                self.clash_process = Some(child);
                self.is_running = true;
                self.config_editor_status = "Clash 已启动。".to_string();
                println!(
                    "Clash started with config: {}, API port expected by Clash: {}",
                    self.app_state.config_path, self.app_state.api_port // Log the port Clash will use
                );
            }
            Err(e) => {
                eprintln!(
                    "Failed to start Clash (path: '{}', config: '{}'): {}",
                    self.app_state.clash_path, self.app_state.config_path, e
                );
                self.config_editor_status = format!("启动 Clash 失败: {}", e);
            }
        }
    }

    fn stop_clash(&mut self) {
        if let Some(mut child) = self.clash_process.take() {
            match child.kill() {
                Ok(_) => {
                    println!("Clash stop signal sent.");
                    match child.wait() {
                        Ok(status) => println!("Clash process exited with status: {}", status),
                        Err(e) => eprintln!("Error waiting for Clash process to exit: {}", e),
                    }
                    self.is_running = false;
                    self.config_editor_status = "Clash 已停止。".to_string();
                }
                Err(e) => {
                    eprintln!("Failed to stop Clash: {}", e);
                    self.config_editor_status = format!("停止 Clash 失败: {}", e);
                    self.clash_process = Some(child);
                }
            }
        } else {
            self.is_running = false;
            self.config_editor_status = "Clash 未运行或进程句柄丢失。".to_string();
        }
    }
}

async fn get_traffic_async(host: &str, port: &str) -> Option<TrafficInfo> {
    let base_url = format!("http://{}:{}", host, port);
    if let Some(traffic) = try_connections_endpoint(&base_url).await {
        return Some(traffic);
    }
    None
}

async fn try_connections_endpoint(base_url: &str) -> Option<TrafficInfo> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .ok()?;
    let response = client
        .get(&format!("{}/connections", base_url))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let data = response.json::<serde_json::Value>().await.ok()?;
    let mut up = 0;
    let mut down = 0;
    let mut found_connections = false;

    if let Some(connections) = data["connections"].as_array() {
        if !connections.is_empty() {
            found_connections = true;
        }
        for conn in connections {
            up += conn["upload"].as_u64().unwrap_or(0);
            down += conn["download"].as_u64().unwrap_or(0);
        }
    }
    if found_connections || up > 0 || down > 0 {
        Some(TrafficInfo { up, down })
    } else {
        None
    }
}

fn get_traffic(host: &str, port: &str) -> Option<TrafficInfo> {
    TOKIO_RUNTIME.block_on(get_traffic_async(host, port))
}

impl App for ClashApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.app_state);
        println!("App state saved, {:?}.", self.app_state);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Clash 控制面板");
            ui.add_space(10.0);

            ui.collapsing("⚙️ 应用设置", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Clash 可执行文件路径:");
                    ui.text_edit_singleline(&mut self.app_state.clash_path);
                });

                ui.horizontal(|ui| {
                    ui.label("Clash 配置文件路径:");
                    if ui.text_edit_singleline(&mut self.app_state.config_path).changed() {
                        self.load_config_from_file(); // This will also call process_config_content
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Clash API 端口 (监控用):");
                    // This UI field directly updates app_state.api_port and monitor port
                    // It might be overridden if config parsing provides a different port
                    if ui.text_edit_singleline(&mut self.app_state.api_port).changed() {
                        if !self.app_state.api_port.is_empty() && self.app_state.api_port.chars().all(char::is_numeric) {
                             *self.api_port_for_monitor.lock().unwrap() = self.app_state.api_port.clone();
                             println!("API port for monitor updated to '{}' due to UI input.", self.app_state.api_port);
                             self.config_editor_status = format!("API监控端口已由UI更新为 '{}'。", self.app_state.api_port);
                        } else {
                            println!("Warning: Invalid API port entered in UI: '{}'. Monitor port not updated.", self.app_state.api_port);
                            self.config_editor_status = format!("警告：API端口 '{}' 无效，监控端口未更新。", self.app_state.api_port);
                        }
                    }
                });
                 // Display the currently parsed config's external-controller if available
                if let Some(ref parsed_cfg) = self.parsed_clash_config {
                    if let Some(ref ec) = parsed_cfg.external_controller {
                        ui.label(format!("配置文件中的 API 地址: {}", ec));
                    } else {
                        ui.label("配置文件中未找到 external-controller。");
                    }
                } else {
                    ui.label("配置文件未解析或解析失败。");
                }
            });
            ui.add_space(5.0);

            ui.collapsing("📄 配置文件内容", |ui| {
                ui.horizontal(|ui| {
                    if ui.button("🔄 从文件载入").clicked() {
                        self.load_config_from_file();
                    }
                    if ui.button("💾 保存到文件").clicked() {
                        self.save_config_to_file();
                    }
                });
                ui.label(&self.config_editor_status).on_hover_text("配置文件加载/保存/解析状态");
                ui.add_space(5.0);

                ScrollArea::vertical().min_scrolled_height(ui.text_style_height(&egui::TextStyle::Body) * 10.0).show(ui, |ui| {
                    // When text edit changes, immediately try to process it
                    if ui.add(
                        egui::TextEdit::multiline(&mut self.config_content)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(15), // Increased rows
                    ).changed() {
                        self.process_config_content(); // Process on text change
                    }
                });
            });

            ui.add_space(10.0);

            if self.is_running {
                if ui.button("⏹️ 停止 Clash").clicked() {
                    self.stop_clash();
                }
            } else {
                if ui.button("▶️ 启动 Clash").clicked() {
                    self.start_clash();
                }
            }

            ui.add_space(10.0);
            let stats_guard = self.stats.lock().unwrap();
            let current_monitor_port_guard = self.api_port_for_monitor.lock().unwrap();
            let current_monitor_port_str = current_monitor_port_guard.clone();
            drop(current_monitor_port_guard);

            ui.horizontal(|ui| {
                let status_text = if self.is_running {
                    RichText::new("🟢 Clash 运行中").color(Color32::GREEN)
                } else {
                    RichText::new("🔴 Clash 已停止").color(Color32::RED)
                };
                ui.label(status_text);
                ui.separator();
                let api_text = if stats_guard.api_connected {
                    RichText::new(format!("🔗 API 已连接 ({})", current_monitor_port_str)).color(Color32::GREEN)
                } else {
                    let port_display = if current_monitor_port_str.is_empty() { "未设置".to_string() } else { current_monitor_port_str };
                    RichText::new(format!("⚠️ API 未连接 ({})", port_display)).color(Color32::RED)
                };
                ui.label(api_text);
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(format!("⬆️ 上传: {}/s", format_size(stats_guard.upload_speed, BINARY)));
                ui.separator();
                ui.label(format!("⬇️ 下载: {}/s", format_size(stats_guard.download_speed, BINARY)));
            });
            ui.horizontal(|ui| {
                ui.label(format!("总上传: {}", format_size(stats_guard.previous_upload, BINARY)));
                ui.separator();
                ui.label(format!("总下载: {}", format_size(stats_guard.previous_download, BINARY)));
            });
            drop(stats_guard);

            ctx.request_repaint_after(Duration::from_millis(500));
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 550.0]) // Increased height for editor
            .with_min_inner_size([500.0, 450.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Clash 控制面板",
        options,
        Box::new(|cc| Ok(Box::new(ClashApp::new(cc)))),
    )
}

