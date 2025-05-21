use std::collections::HashMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::{App, egui};
use egui::{Color32, RichText, ScrollArea, Ui}; // Added Ui for convenience
use font_kit::source::SystemSource;
use humansize::{BINARY, format_size};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

static TOKIO_RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create Tokio runtime"));

// --- Data Structures for Clash YAML Configuration (from previous step) ---
#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ClashFullConfig {
    #[serde(rename = "mixed-port")]
    pub mixed_port: Option<u16>,
    #[serde(rename = "redir-port")]
    pub redir_port: Option<u16>,
    pub allow_lan: Option<bool>,
    pub mode: Option<String>,
    pub log_level: Option<String>,
    #[serde(rename = "external-controller")]
    pub external_controller: Option<String>,
    pub secret: Option<String>,
    #[serde(rename = "external-ui")]
    pub external_ui: Option<String>,
    pub dns: Option<DnsConfig>,
    #[serde(default)]
    pub proxies: Vec<ProxyConfig>,
    #[serde(default, rename = "proxy-groups")]
    pub proxy_groups: Vec<ProxyGroupConfig>,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct DnsConfig {
    pub enable: Option<bool>,
    pub ipv6: Option<bool>,
    pub listen: Option<String>,
    pub enhanced_mode: Option<String>,
    pub fake_ip_range: Option<String>,
    #[serde(default)]
    pub fake_ip_filter: Vec<String>,
    #[serde(default)]
    pub default_nameserver: Vec<String>,
    #[serde(default)]
    pub nameserver: Vec<String>,
    #[serde(default)]
    pub fallback: Option<Vec<String>>,
    pub fallback_filter: Option<FallbackFilterConfig>,
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
    #[serde(rename = "type")]
    pub proxy_type: String,
    pub server: String,
    pub port: u16,
    pub password: Option<String>,
    pub udp: Option<bool>,
    pub sni: Option<String>,
    #[serde(rename = "skip-cert-verify")]
    pub skip_cert_verify: Option<bool>,
    pub network: Option<String>,
    pub alpn: Option<Vec<String>>,
    #[serde(rename = "grpc-opts")]
    pub grpc_opts: Option<GrpcOpts>,
    #[serde(rename = "ws-opts")]
    pub ws_opts: Option<WsOpts>,
    pub uuid: Option<String>,
    #[serde(rename = "alterId")]
    pub alter_id: Option<u32>,
    pub cipher: Option<String>,
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
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ProxyGroupConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub group_type: String,
    #[serde(default)]
    pub proxies: Vec<String>,
    pub url: Option<String>,
    pub interval: Option<u32>,
}

// --- Data Structures for Clash API Responses ---
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
struct ClashApiGeneralConfig {
    // port: Option<u16>, // Not strictly needed for this feature
    // socks_port: Option<u16>,
    // redir_port: Option<u16>,
    // mixed_port: Option<u16>,
    // allow_lan: bool,
    mode: String, // "Rule", "Global", "Direct"
                  // log_level: String,
                  // external_controller: String,
}

#[derive(Deserialize, Debug, Clone)]
struct ClashApiSelectorProxyInfo {
    // name: String, // Not strictly needed
    now: Option<String>, // The currently selected proxy in this selector
                         // all: Vec<String>,
                         // #[serde(rename = "type")]
                         // proxy_type: String,
}

#[derive(Deserialize, Debug, Clone)]
struct ClashApiDelayResponse {
    delay: Option<u32>,
    message: Option<String>, // For errors like timeout
}

// --- Application specific dynamic info ---
#[derive(Debug, Clone, Default)]
struct AppDynamicClashInfo {
    mode: String,                              // Current Clash mode (Rule, Global, Direct)
    current_global_proxy_name: Option<String>, // Name of the proxy selected by GLOBAL group
    current_global_proxy_latency: String,      // "N/A", "Testing...", "123 ms", "Error: <reason>"
}

impl AppDynamicClashInfo {
    fn new() -> Self {
        Self {
            mode: "未知".to_string(),
            current_global_proxy_name: None,
            current_global_proxy_latency: "N/A".to_string(),
        }
    }
}

#[derive(Deserialize, Default, Debug)]
struct TrafficInfo {
    up: u64,
    down: u64,
}

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

#[derive(Deserialize, Serialize, Debug, Clone)] // Added Clone
#[serde(default)]
struct AppState {
    config_path: String,
    clash_path: String,
    api_port: String,
    // New: Configuration for latency test
    latency_test_url: String,
    latency_test_timeout_ms: u32,
}

fn extract_port_from_controller_string(controller_addr: &str) -> Option<String> {
    if let Some(port_str) = controller_addr.split(':').last() {
        if !port_str.is_empty() && port_str.chars().all(char::is_numeric) {
            return Some(port_str.to_string());
        } else if port_str.is_empty()
            && controller_addr.starts_with(':')
            && controller_addr.len() > 1
        {
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
            latency_test_url: "http://www.gstatic.com/generate_204".to_string(),
            latency_test_timeout_ms: 5000,
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
    parsed_clash_config: Option<ClashFullConfig>,
    // New fields for dynamic info
    dynamic_clash_info: Arc<Mutex<AppDynamicClashInfo>>,
    is_testing_latency: Arc<Mutex<bool>>, // To disable button during test
}

fn try_parse_clash_config_from_string(
    yaml_string: &str,
) -> Result<ClashFullConfig, serde_yaml::Error> {
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
            if let Ok(font_handle) = system_source.select_best_match(
                &[font_kit::family_name::FamilyName::Title(font_name.clone())],
                &font_kit::properties::Properties::new(),
            ) {
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
            // Use .clone()
            println!("No saved app state found or failed to load, using defaults.");
            AppState::default()
        });
        if app_state_loaded.is_some() {
            println!("Successfully loaded app state {:?}.", app_state);
        }

        let mut initial_config_content = String::new();
        let mut initial_parsed_config: Option<ClashFullConfig> = None;
        let initial_config_status;
        // format!("配置文件 '{}' 内容待加载。", app_state.config_path);

        match fs::read_to_string(&app_state.config_path) {
            Ok(content) => {
                initial_config_content = content;
                match try_parse_clash_config_from_string(&initial_config_content) {
                    Ok(parsed) => {
                        initial_config_status =
                            format!("配置文件 '{}' 加载并解析成功。", app_state.config_path);
                        if let Some(ref ec_str) = parsed.external_controller {
                            if let Some(port_from_parsed_config) =
                                extract_port_from_controller_string(ec_str)
                            {
                                if app_state.api_port != port_from_parsed_config {
                                    app_state.api_port = port_from_parsed_config;
                                }
                            }
                        }
                        initial_parsed_config = Some(parsed);
                    }
                    Err(e) => {
                        initial_config_status = format!(
                            "配置文件 '{}' 加载成功但解析失败: {}",
                            app_state.config_path, e
                        );
                    }
                }
            }
            Err(e) => {
                initial_config_status =
                    format!("加载配置文件 '{}' 失败: {}", app_state.config_path, e);
            }
        }

        if app_state.api_port.is_empty() {
            app_state.api_port = "9090".to_string();
        }

        let stats = Arc::new(Mutex::new(NetworkStats::default()));
        let dynamic_clash_info = Arc::new(Mutex::new(AppDynamicClashInfo::new()));
        let is_testing_latency = Arc::new(Mutex::new(false));

        // --- Monitoring Thread ---
        let stats_clone = Arc::clone(&stats);
        let api_port_for_monitor = Arc::new(Mutex::new(app_state.api_port.clone()));
        let api_port_for_monitor_clone = Arc::clone(&api_port_for_monitor);
        let dynamic_clash_info_clone = Arc::clone(&dynamic_clash_info);

        thread::spawn(move || {
            let client = reqwest::blocking::Client::builder() // Use blocking client in sync thread
                .timeout(Duration::from_secs(2)) // Short timeout for API calls
                .build()
                .expect("Failed to build reqwest client for monitor thread");

            loop {
                thread::sleep(Duration::from_secs(1)); // General polling interval
                let current_api_port = api_port_for_monitor_clone.lock().unwrap().clone();
                if current_api_port.is_empty() {
                    let mut stats_guard = stats_clone.lock().unwrap();
                    stats_guard.api_connected = false; // Also reset traffic API status
                    // Reset dynamic info as well
                    let mut info_guard = dynamic_clash_info_clone.lock().unwrap();
                    *info_guard = AppDynamicClashInfo::new(); // Reset to defaults
                    continue;
                }
                let base_url = format!("http://127.0.0.1:{}", current_api_port);

                // Fetch traffic (existing logic)
                // For brevity, assuming get_traffic is adapted to use the blocking client or remains async with block_on
                // For this example, we'll keep it as is, but in a real app, you'd integrate client usage.
                if let Some(traffic) = get_traffic("127.0.0.1", &current_api_port) {
                    // This uses TOKIO_RUNTIME.block_on internally
                    let mut stats_guard = stats_clone.lock().unwrap();
                    stats_guard.api_connected = true;
                    let elapsed_secs = stats_guard.last_update.elapsed().as_secs_f64();
                    if elapsed_secs >= 0.1 {
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

                // Fetch /configs for mode
                let mut new_mode = "获取失败".to_string();
                let mut new_global_proxy: Option<String> = None;

                match client.get(format!("{}/configs", base_url)).send() {
                    Ok(response) => {
                        if response.status().is_success() {
                            match response.json::<ClashApiGeneralConfig>() {
                                Ok(api_configs) => {
                                    new_mode = api_configs.mode.clone();
                                    if new_mode == "global" {
                                        // Fetch /proxies/GLOBAL if mode is Global
                                        match client
                                            .get(format!("{}/proxies/GLOBAL", base_url))
                                            .send()
                                        {
                                            Ok(proxy_resp) => {
                                                if proxy_resp.status().is_success() {
                                                    match proxy_resp
                                                        .json::<ClashApiSelectorProxyInfo>()
                                                    {
                                                        Ok(selector_info) => {
                                                            new_global_proxy = selector_info.now;
                                                        }
                                                        Err(e) => eprintln!(
                                                            "Failed to parse GLOBAL proxy info: {}",
                                                            e
                                                        ),
                                                    }
                                                } else {
                                                    eprintln!(
                                                        "Failed to get GLOBAL proxy info: HTTP {}",
                                                        proxy_resp.status()
                                                    );
                                                }
                                            }
                                            Err(e) => eprintln!(
                                                "Request to /proxies/GLOBAL failed: {}",
                                                e
                                            ),
                                        }
                                    }
                                }
                                Err(e) => eprintln!("Failed to parse /configs response: {}", e),
                            }
                        } else {
                            eprintln!("Failed to get /configs: HTTP {}", response.status());
                        }
                    }
                    Err(e) => eprintln!("Request to /configs failed: {}", e),
                }

                // Update dynamic info
                let mut info_guard = dynamic_clash_info_clone.lock().unwrap();
                // If mode changed or global proxy changed, reset latency
                if info_guard.mode != new_mode
                    || info_guard.current_global_proxy_name != new_global_proxy
                {
                    info_guard.current_global_proxy_latency = "N/A".to_string();
                }
                info_guard.mode = new_mode;
                info_guard.current_global_proxy_name = new_global_proxy;
                if info_guard.current_global_proxy_name.is_none() && info_guard.mode == "global" {
                    info_guard.current_global_proxy_latency = "GLOBAL组无选择".to_string();
                } else if info_guard.current_global_proxy_name.is_none() {
                    info_guard.current_global_proxy_latency = "N/A".to_string();
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
            dynamic_clash_info,
            is_testing_latency,
        }
    }

    fn process_config_content(&mut self) {
        match try_parse_clash_config_from_string(&self.config_content) {
            Ok(parsed_config) => {
                self.config_editor_status = format!(
                    "配置文件内容解析成功。({})",
                    chrono::Local::now().format("%H:%M:%S")
                );
                if let Some(ref ec_str) = parsed_config.external_controller {
                    if let Some(port_from_parsed) = extract_port_from_controller_string(ec_str) {
                        if self.app_state.api_port != port_from_parsed {
                            self.app_state.api_port = port_from_parsed.clone();
                            *self.api_port_for_monitor.lock().unwrap() = port_from_parsed;
                        }
                    } else if parsed_config.external_controller.is_some() {
                        self.config_editor_status = format!(
                            "配置解析成功，但 external-controller ('{}') 中的端口无效。",
                            ec_str
                        );
                    }
                } else {
                    self.config_editor_status =
                        "配置解析成功，但未找到 external-controller 键。".to_string();
                }
                self.parsed_clash_config = Some(parsed_config);
            }
            Err(e) => {
                self.config_editor_status = format!("配置文件内容解析失败: {}", e);
                self.parsed_clash_config = None;
            }
        }
    }

    fn load_config_from_file(&mut self) {
        match fs::read_to_string(&self.app_state.config_path) {
            Ok(content) => {
                self.config_content = content;
                self.process_config_content();
            }
            Err(e) => {
                self.config_content = String::new();
                self.config_editor_status =
                    format!("加载 '{}' 失败: {}", self.app_state.config_path, e);
                self.parsed_clash_config = None;
            }
        }
    }

    fn save_config_to_file(&mut self) {
        self.process_config_content();
        match fs::write(&self.app_state.config_path, &self.config_content) {
            Ok(_) => {
                self.config_editor_status = format!(
                    "已保存到 '{}'. {}",
                    self.app_state.config_path, self.config_editor_status
                );
            }
            Err(e) => {
                self.config_editor_status =
                    format!("保存 '{}' 失败: {}", self.app_state.config_path, e);
            }
        }
    }

    fn start_clash(&mut self) {
        if self.is_running {
            return;
        }
        let mut port_to_use = self.app_state.api_port.clone();
        if let Some(ref parsed_cfg) = self.parsed_clash_config {
            if let Some(ref ec_str) = parsed_cfg.external_controller {
                if let Some(p) = extract_port_from_controller_string(ec_str) {
                    port_to_use = p;
                }
            }
        }
        if port_to_use != self.app_state.api_port {
            self.app_state.api_port = port_to_use.clone();
            *self.api_port_for_monitor.lock().unwrap() = port_to_use;
        }
        if self.app_state.api_port.is_empty() {
            self.config_editor_status = "错误：API端口为空，无法启动Clash。".to_string();
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
            }
            Err(e) => {
                self.config_editor_status = format!("启动 Clash 失败: {}", e);
            }
        }
    }

    fn stop_clash(&mut self) {
        if let Some(mut child) = self.clash_process.take() {
            match child.kill() {
                Ok(_) => {
                    let _ = child.wait();
                    self.is_running = false;
                    self.config_editor_status = "Clash 已停止。".to_string();
                }
                Err(e) => {
                    self.config_editor_status = format!("停止 Clash 失败: {}", e);
                    self.clash_process = Some(child);
                }
            }
        } else {
            self.is_running = false;
            self.config_editor_status = "Clash 未运行。".to_string();
        }
    }

    // Method to test latency
    fn test_current_proxy_latency(&self) {
        let mut is_testing_lock = self.is_testing_latency.lock().unwrap();
        if *is_testing_lock {
            println!("Latency test already in progress.");
            return;
        }
        *is_testing_lock = true;
        // is_testing_lock 在这里没有 drop，但没关系，因为它只在函数开始时检查和设置

        let dynamic_info_clone = Arc::clone(&self.dynamic_clash_info);
        let api_port = self.app_state.api_port.clone();
        let test_url = self.app_state.latency_test_url.clone();
        let timeout_ms = self.app_state.latency_test_timeout_ms;
        let is_testing_latency_clone = Arc::clone(&self.is_testing_latency);

        // 从 Mutex 中获取需要的数据，然后在 await 之前释放锁
        let proxy_name_to_test: Option<String>;
        {
            let mut info_guard = dynamic_info_clone.lock().unwrap();
            proxy_name_to_test = info_guard.current_global_proxy_name.clone();
            if proxy_name_to_test.is_some() {
                info_guard.current_global_proxy_latency = "测试中...".to_string();
            } else {
                info_guard.current_global_proxy_latency = "无代理可测试".to_string();
                *is_testing_latency_clone.lock().unwrap() = false; // 重置测试状态
                // 不需要 drop(is_testing_lock)，因为它在函数开始时已经处理
                return;
            }
            // info_guard 在这里被 drop，锁被释放
        }

        if let Some(p_name) = proxy_name_to_test {
            if api_port.is_empty() {
                let mut info_guard = dynamic_info_clone.lock().unwrap();
                info_guard.current_global_proxy_latency = "错误: API端口未设置".to_string();
                *is_testing_latency_clone.lock().unwrap() = false;
                return;
            }

            TOKIO_RUNTIME.spawn(async move {
                // async move 捕获 p_name, api_port, test_url, timeout_ms, dynamic_info_clone, is_testing_latency_clone
                let url_encoded_proxy_name = urlencoding::encode(&p_name);
                let request_url = format!(
                    "http://127.0.0.1:{}/proxies/{}/delay?timeout={}&url={}",
                    api_port,
                    url_encoded_proxy_name,
                    timeout_ms,
                    urlencoding::encode(&test_url) // 也对测试URL进行编码
                );

                let client = reqwest::Client::builder()
                    .timeout(Duration::from_millis(timeout_ms as u64 + 1000)) // 稍微增加客户端超时
                    .build()
                    .expect("Failed to build reqwest client for delay test");

                let latency_result_string = match client.get(&request_url).send().await {
                    Ok(response) => {
                        let status = response.status(); // 获取状态
                        if status.is_success() {
                            match response.json::<ClashApiDelayResponse>().await {
                                Ok(delay_info) => {
                                    if let Some(delay) = delay_info.delay {
                                        format!("{} ms", delay)
                                    } else if let Some(msg) = delay_info.message {
                                        format!("超时/错误: {}", msg) // 更明确的错误信息
                                    } else {
                                        "错误: 未知API响应".to_string()
                                    }
                                }
                                Err(e) => {
                                    format!("错误: 解析延迟响应失败 {}", e)
                                }
                            }
                        } else {
                            // 注意：response.text() 消耗 response，所以要先获取 status
                            let err_text = response
                                .text()
                                .await
                                .unwrap_or_else(|_| "无法读取错误信息".to_string());
                            format!("错误: HTTP {} - {}", status, err_text)
                        }
                    }
                    Err(e) => {
                        format!("错误: 请求失败 {}", e)
                    }
                };

                // 现在获取锁来更新UI状态
                {
                    let mut info_guard = dynamic_info_clone.lock().unwrap();
                    // 再次检查当前测试的代理是否仍然是UI上显示的代理，防止过时的更新
                    if info_guard.current_global_proxy_name.as_ref() == Some(&p_name) {
                        info_guard.current_global_proxy_latency = latency_result_string;
                    } else {
                        println!(
                            "Latency test result for '{}' is stale, UI shows different proxy.",
                            p_name
                        );
                    }
                }
                *is_testing_latency_clone.lock().unwrap() = false; // 重置测试状态
            });
        } else {
            // 如果 p_name 是 None，确保 is_testing_latency 被重置
            // 这种情况理论上已经被上面的逻辑处理了，但作为双重保险
            *is_testing_latency_clone.lock().unwrap() = false;
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
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Clash 控制面板");
            ui.add_space(10.0);

            // Section for dynamic info: Mode, Current Proxy, Latency
            // In ClashApp::update method
            ui.collapsing("ℹ️ 当前状态", |ui| {
                let (mode, global_proxy_name, global_proxy_latency, show_test_button_section) = {
                    let info_guard = self.dynamic_clash_info.lock().unwrap();
                    (
                        info_guard.mode.clone(),
                        info_guard.current_global_proxy_name.clone(),
                        info_guard.current_global_proxy_latency.clone(),
                        info_guard.mode == "global", // Determine if we are in a state to show global proxy details
                    )
                }; // Lock on dynamic_clash_info released

                ui.horizontal(|ui| {
                    ui.label("Clash 模式:");
                    ui.label(RichText::new(&mode).strong());
                });

                if show_test_button_section {
                    // Was mode == "global"
                    ui.horizontal(|ui| {
                        ui.label("当前全局代理:");
                        match &global_proxy_name {
                            Some(name) => {
                                ui.label(RichText::new(name).strong().color(Color32::LIGHT_BLUE));
                            }
                            None => {
                                ui.label(RichText::new("未选择").italics());
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("延迟:");
                        ui.label(RichText::new(&global_proxy_latency).strong());

                        if global_proxy_name.is_some() {
                            // Only show button if there's a proxy
                            let is_testing = *self.is_testing_latency.lock().unwrap(); // Lock is_testing_latency briefly
                            if ui
                                .add_enabled(!is_testing, egui::Button::new("⚡ 测试"))
                                .clicked()
                            {
                                // CRITICAL: self.dynamic_clash_info is NOT locked by this thread here.
                                self.test_current_proxy_latency();
                            }
                        }
                    });
                } else if !mode.is_empty() && mode != "未知" {
                    ui.label(format!("当前为 {} 模式，不直接显示全局代理。", mode));
                }
                ui.separator(); // Separator before traffic stats
            });
            // Traffic and API status (existing)
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
                    RichText::new(format!("🔗 API 已连接 ({})", current_monitor_port_str))
                        .color(Color32::GREEN)
                } else {
                    RichText::new(format!(
                        "⚠️ API 未连接 ({})",
                        if current_monitor_port_str.is_empty() {
                            "未设置".to_string()
                        } else {
                            current_monitor_port_str
                        }
                    ))
                    .color(Color32::RED)
                };
                ui.label(api_text);
            });
            ui.horizontal(|ui| {
                ui.label(format!(
                    "⬆️ {}",
                    format_size(stats_guard.upload_speed, BINARY)
                ));
                ui.label(format!(
                    "⬇️ {}",
                    format_size(stats_guard.download_speed, BINARY)
                ));
            });
            ui.horizontal(|ui| {
                ui.label(format!(
                    "总上传: {}",
                    format_size(stats_guard.previous_upload, BINARY)
                ));
                ui.separator();
                ui.label(format!(
                    "总下载: {}",
                    format_size(stats_guard.previous_download, BINARY)
                ));
            });
            drop(stats_guard);
            ui.add_space(10.0);

            // Start/Stop Buttons
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

            // App Settings (collapsible)
            ui.collapsing("⚙️ 应用设置", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Clash 可执行文件路径:");
                    ui.text_edit_singleline(&mut self.app_state.clash_path);
                });
                ui.horizontal(|ui| {
                    ui.label("Clash 配置文件路径:");
                    if ui
                        .text_edit_singleline(&mut self.app_state.config_path)
                        .changed()
                    {
                        self.load_config_from_file();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Clash API 端口 (监控用):");
                    if ui
                        .text_edit_singleline(&mut self.app_state.api_port)
                        .changed()
                    {
                        if !self.app_state.api_port.is_empty()
                            && self.app_state.api_port.chars().all(char::is_numeric)
                        {
                            *self.api_port_for_monitor.lock().unwrap() =
                                self.app_state.api_port.clone();
                            self.config_editor_status =
                                format!("API监控端口已由UI更新为 '{}'。", self.app_state.api_port);
                        } else {
                            self.config_editor_status =
                                format!("警告：API端口 '{}' 无效。", self.app_state.api_port);
                        }
                    }
                });
                if let Some(ref parsed_cfg) = self.parsed_clash_config {
                    if let Some(ref ec) = parsed_cfg.external_controller {
                        ui.label(format!("配置文件中的 API 地址: {}", ec));
                    } else {
                        ui.label("配置文件中未找到 external-controller。");
                    }
                } else {
                    ui.label("配置文件未解析或解析失败。");
                }

                ui.separator();
                ui.label("延迟测试设置:");
                ui.horizontal(|ui| {
                    ui.label("测试URL:");
                    ui.text_edit_singleline(&mut self.app_state.latency_test_url);
                });
                ui.horizontal(|ui| {
                    ui.label("超时 (ms):");
                    let mut timeout_str = self.app_state.latency_test_timeout_ms.to_string();
                    if ui.text_edit_singleline(&mut timeout_str).changed() {
                        if let Ok(val) = timeout_str.parse::<u32>() {
                            self.app_state.latency_test_timeout_ms = val;
                        }
                    }
                });
            });
            ui.add_space(5.0);

            // Config File Editor (collapsible)
            ui.collapsing("📄 配置文件内容", |ui| {
                ui.horizontal(|ui| {
                    if ui.button("🔄 从文件载入").clicked() {
                        self.load_config_from_file();
                    }
                    if ui.button("💾 保存到文件").clicked() {
                        self.save_config_to_file();
                    }
                });
                ui.label(&self.config_editor_status)
                    .on_hover_text("配置文件加载/保存/解析状态");
                ui.add_space(5.0);
                ScrollArea::vertical()
                    .min_scrolled_height(ui.text_style_height(&egui::TextStyle::Body) * 10.0)
                    .show(ui, |ui_inner| {
                        if ui_inner
                            .add(
                                egui::TextEdit::multiline(&mut self.config_content)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(15),
                            )
                            .changed()
                        {
                            self.process_config_content();
                        }
                    });
            });

            ctx.request_repaint_after(Duration::from_millis(500));
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 600.0])
            .with_min_inner_size([500.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Clash 控制面板",
        options,
        Box::new(|cc| Ok(Box::new(ClashApp::new(cc)))),
    )
}
