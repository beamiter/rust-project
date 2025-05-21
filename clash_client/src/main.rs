use std::fs::{self, File}; // 修改：增加了 fs
use std::io::BufReader;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::{App, egui};
use egui::{Color32, RichText, ScrollArea}; // 修改：增加了 ScrollArea
use font_kit::source::SystemSource;
use humansize::{BINARY, format_size};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

static TOKIO_RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create Tokio runtime"));

#[derive(Deserialize, Default, Debug)]
struct TrafficInfo {
    up: u64,
    down: u64,
}

#[derive(Deserialize, Debug)]
struct ClashConfigYaml {
    #[serde(rename = "external-controller")]
    external_controller: Option<String>,
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

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
struct AppState {
    config_path: String,
    clash_path: String,
    api_port: String,
}

impl Default for AppState {
    fn default() -> Self {
        let default_config_path = dirs::config_dir()
            .map(|p| p.join("clash/config.yaml"))
            .unwrap_or_else(|| std::path::PathBuf::from("config.yaml")) // 简化默认回退
            .to_string_lossy()
            .to_string();

        let default_api_port =
            load_api_port_from_config(&default_config_path).unwrap_or_else(|| "9090".to_string());

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
    // 新增字段用于配置文件编辑器
    config_content: String,
    config_editor_status: String,
}

fn load_api_port_from_config(config_path: &str) -> Option<String> {
    let file = File::open(config_path).ok()?;
    let reader = BufReader::new(file);
    let config: ClashConfigYaml = serde_yaml::from_reader(reader).ok()?;

    if let Some(controller_addr) = config.external_controller {
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
                } else {
                    eprintln!(
                        "Warning: Parsed port '{}' from external-controller (format ':port') is not numeric in config: {}",
                        port_only_str, config_path
                    );
                }
            } else {
                eprintln!(
                    "Warning: Parsed port '{}' from external-controller is not numeric in config: {}.",
                    port_str, config_path
                );
            }
        }
    }
    None
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
                egui::FontId::new(12.0, egui::FontFamily::Monospace), // 用于配置编辑器
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
            AppState::default()
        });
        if app_state_loaded.is_some() {
            println!("Successfully loaded app state {:?}.", app_state);
        }

        if let Some(port_from_config) = load_api_port_from_config(&app_state.config_path) {
            if app_state.api_port != port_from_config {
                println!(
                    "API port updated from config file ('{}') over loaded/default state ('{}'). Path: '{}'",
                    port_from_config, app_state.api_port, app_state.config_path
                );
                app_state.api_port = port_from_config;
            }
        } else {
            println!(
                "No API port found in config file '{}'. Using current API port: '{}' (from loaded state or default).",
                app_state.config_path, app_state.api_port
            );
        }
        if app_state.api_port.is_empty() {
            app_state.api_port = "9090".to_string();
            println!("Warning: API port was empty, reset to '9090'.");
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

        let mut new_app = Self {
            clash_process: None,
            app_state,
            is_running: false,
            stats,
            api_port_for_monitor,
            config_content: String::new(), // 新增：初始化
            config_editor_status: String::from("配置文件内容未加载"), // 新增：初始化
        };
        new_app.load_config_content(); // 新增：应用启动时加载一次配置文件内容
        new_app
    }

    // 新增：加载配置文件内容到编辑器
    fn load_config_content(&mut self) {
        match fs::read_to_string(&self.app_state.config_path) {
            Ok(content) => {
                self.config_content = content;
                self.config_editor_status = format!("已从 '{}' 加载。", self.app_state.config_path);
                println!("Config content loaded from {}", self.app_state.config_path);
            }
            Err(e) => {
                self.config_content = String::new(); // 清空内容表示加载失败
                self.config_editor_status =
                    format!("加载 '{}' 失败: {}", self.app_state.config_path, e);
                eprintln!(
                    "Failed to load config content from {}: {}",
                    self.app_state.config_path, e
                );
            }
        }
    }

    // 新增：保存编辑器内容到配置文件
    fn save_config_content(&mut self) {
        match fs::write(&self.app_state.config_path, &self.config_content) {
            Ok(_) => {
                self.config_editor_status = format!("已保存到 '{}'。", self.app_state.config_path);
                println!("Config content saved to {}", self.app_state.config_path);
                // 保存后，重新解析 API 端口，因为用户可能在编辑器中修改了它
                if let Some(parsed_port) = load_api_port_from_config(&self.app_state.config_path) {
                    if self.app_state.api_port != parsed_port {
                        println!(
                            "API port updated to '{}' from saved config file '{}'. Previous UI/state value was '{}'.",
                            parsed_port, self.app_state.config_path, self.app_state.api_port
                        );
                        self.app_state.api_port = parsed_port.clone();
                        *self.api_port_for_monitor.lock().unwrap() = parsed_port;
                    }
                } else {
                    println!(
                        "Failed to parse API port from saved config file: '{}'. Keeping current API port: '{}'",
                        self.app_state.config_path, self.app_state.api_port
                    );
                }
            }
            Err(e) => {
                self.config_editor_status =
                    format!("保存 '{}' 失败: {}", self.app_state.config_path, e);
                eprintln!(
                    "Failed to save config content to {}: {}",
                    self.app_state.config_path, e
                );
            }
        }
    }

    fn start_clash(&mut self) {
        if self.is_running {
            return;
        }

        if let Some(parsed_port_from_config) =
            load_api_port_from_config(&self.app_state.config_path)
        {
            if self.app_state.api_port != parsed_port_from_config {
                println!(
                    "API port for starting Clash updated to '{}' from config file '{}'. Previous UI/state value was '{}'.",
                    parsed_port_from_config, self.app_state.config_path, self.app_state.api_port
                );
                self.app_state.api_port = parsed_port_from_config.clone();
                *self.api_port_for_monitor.lock().unwrap() = parsed_port_from_config;
            }
        } else {
            println!(
                "Could not parse API port from config file '{}' before starting Clash. Will use current API port: '{}'.",
                self.app_state.config_path, self.app_state.api_port
            );
        }
        if self.app_state.api_port.is_empty() {
            eprintln!("Error: API port is empty. Cannot start Clash. Please set a valid API port.");
            self.config_editor_status =
                "错误：API端口为空，无法启动Clash。请在上方设置或检查配置文件。".to_string();
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
                self.config_editor_status = "Clash 已启动。".to_string(); // 更新状态
                println!(
                    "Clash started with config: {}, API port expected by Clash: {}",
                    self.app_state.config_path, self.app_state.api_port
                );
            }
            Err(e) => {
                eprintln!(
                    "Failed to start Clash (path: '{}', config: '{}'): {}",
                    self.app_state.clash_path, self.app_state.config_path, e
                );
                self.config_editor_status = format!("启动 Clash 失败: {}", e); // 更新状态
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
                    self.config_editor_status = "Clash 已停止。".to_string(); // 更新状态
                }
                Err(e) => {
                    eprintln!("Failed to stop Clash: {}", e);
                    self.config_editor_status = format!("停止 Clash 失败: {}", e); // 更新状态
                    self.clash_process = Some(child);
                }
            }
        } else {
            self.is_running = false;
            self.config_editor_status = "Clash 未运行或进程句柄丢失。".to_string(); // 更新状态
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
            ui.add_space(10.0); // 减少一点间距

            ui.collapsing("⚙️ 应用设置", |ui| { // 重命名，使其更通用
                ui.horizontal(|ui| {
                    ui.label("Clash 可执行文件路径:");
                    ui.text_edit_singleline(&mut self.app_state.clash_path);
                });

                ui.horizontal(|ui| {
                    ui.label("Clash 配置文件路径:");
                    // 当配置文件路径改变时，尝试重新加载内容和API端口
                    if ui.text_edit_singleline(&mut self.app_state.config_path).changed() {
                        self.load_config_content(); // 重新加载文件内容到编辑器
                        if let Some(parsed_port) = load_api_port_from_config(&self.app_state.config_path) {
                            if self.app_state.api_port != parsed_port {
                                println!("API port updated to '{}' from config file '{}' due to UI path change.", parsed_port, self.app_state.config_path);
                                self.app_state.api_port = parsed_port.clone();
                                *self.api_port_for_monitor.lock().unwrap() = parsed_port;
                            }
                        } else {
                             println!("Failed to parse API port from new config path: '{}'. Keeping current API port: '{}'", self.app_state.config_path, self.app_state.api_port);
                        }
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Clash API 端口 (监控用):");
                    if ui.text_edit_singleline(&mut self.app_state.api_port).changed() {
                        if !self.app_state.api_port.is_empty() && self.app_state.api_port.chars().all(char::is_numeric) {
                             *self.api_port_for_monitor.lock().unwrap() = self.app_state.api_port.clone();
                             println!("API port for monitor updated to '{}' due to UI input.", self.app_state.api_port);
                        } else {
                            println!("Warning: Invalid API port entered in UI: '{}'. Monitor port not updated.", self.app_state.api_port);
                            // 可以考虑在此处更新 config_editor_status 来给用户反馈
                            self.config_editor_status = format!("警告：API端口 '{}' 无效，监控端口未更新。", self.app_state.api_port);
                        }
                    }
                });
            });
            ui.add_space(5.0);

            // 新增：配置文件编辑器区域
            ui.collapsing("📄 配置文件内容", |ui| {
                ui.horizontal(|ui| {
                    if ui.button("🔄 从文件载入").clicked() {
                        self.load_config_content();
                    }
                    if ui.button("💾 保存到文件").clicked() {
                        self.save_config_content();
                    }
                });
                ui.label(&self.config_editor_status).on_hover_text("配置文件加载/保存状态");
                ui.add_space(5.0);
                // 使用 ScrollArea 包裹 TextEdit，以便内容过长时可以滚动
                // 设置一个最小高度，比如10行
                ScrollArea::vertical().min_scrolled_height(ui.text_style_height(&egui::TextStyle::Body) * 10.0).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.config_content)
                            .font(egui::TextStyle::Monospace) // 使用等宽字体
                            .desired_width(f32::INFINITY) // 占据可用宽度
                            .desired_rows(10), // 建议行数，但ScrollArea会处理实际大小
                    );
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
            let current_monitor_port_str = current_monitor_port_guard.clone(); // 克隆出来用，避免锁占用太久
            drop(current_monitor_port_guard); // 释放锁

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
            .with_inner_size([700.0, 500.0]) // 稍微调大一点窗口
            .with_min_inner_size([500.0, 400.0]), // 最小尺寸也调整下
        ..Default::default()
    };

    eframe::run_native(
        "Clash 控制面板",
        options,
        Box::new(|cc| Ok(Box::new(ClashApp::new(cc)))),
    )
}
